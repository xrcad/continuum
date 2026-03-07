{
  description = "xrcad — collaborative CAD in the browser";

  inputs = {
    # Pinned to last nixos-unstable commit before glibc 2.40→2.42 bump that
    # broke tcgetattr/PTY on Android (nix-community/nix-on-droid#495).
    nixpkgs.url     = "github:NixOS/nixpkgs/88d3861";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        xrcad-server = pkgs.rustPlatform.buildRustPackage {
          pname   = "xrcad-server";
          version = "0.1.0";
          src     = ./.;

          cargoLock.lockFile = ./Cargo.lock;

          # Build only the server crate — no WASM, no GUI deps.
          cargoBuildFlags = [ "-p" "xrcad-server" ];
          doCheck         = false;

          # Android kernels don't support the Linux namespaces nix uses for
          # its build sandbox.  This per-derivation flag lets the package build
          # on nix-on-droid without requiring a daemon restart to pick up
          # sandbox = false from nix.conf.
          __noChroot = true;
        };

        # Script that downloads the pre-built WASM app from the gh-pages branch
        # and drops it in ~/.local/share/xrcad-server/www
        xrcad-fetch-wasm = pkgs.writeShellScriptBin "xrcad-fetch-wasm" ''
          set -euo pipefail
          DEST="''${XDG_DATA_HOME:-$HOME/.local/share}/xrcad-server/www"
          REPO="''${1:-https://github.com/xrcad/continuum}"
          echo "Fetching WASM app from $REPO (gh-pages) → $DEST"
          rm -rf "$DEST"
          ${pkgs.git}/bin/git clone --depth 1 --branch gh-pages "$REPO" "$DEST"
          echo "Done. Start the server with:  xrcad-server --dir $DEST"
        '';

      in {
        packages = {
          xrcad-server    = xrcad-server;
          xrcad-fetch-wasm = xrcad-fetch-wasm;
          default         = xrcad-server;
        };

        devShells.default = pkgs.mkShell {
          packages = [ pkgs.rustup pkgs.pkg-config xrcad-fetch-wasm ];
        };
      }
    ) // {

    # ── NixOS module (for a NixOS machine on your LAN / Tailscale) ─────────────
    nixosModules.xrcad-server = { config, lib, pkgs, ... }:
      let cfg = config.services.xrcad-server; in {
        options.services.xrcad-server = {
          enable  = lib.mkEnableOption "xrcad WebSocket relay + static file server";
          port    = lib.mkOption {
            type    = lib.types.port;
            default = 8080;
            description = "TCP port to listen on.";
          };
          wasmDir = lib.mkOption {
            type        = lib.types.str;
            description = ''
              Directory containing the pre-built WASM app files.
              Run  xrcad-fetch-wasm  to populate this directory.
            '';
          };
          openFirewall = lib.mkOption {
            type    = lib.types.bool;
            default = false;
            description = "Open the firewall port so LAN / Tailscale devices can connect.";
          };
        };

        config = lib.mkIf cfg.enable {
          systemd.services.xrcad-server = {
            description = "xrcad relay server";
            wantedBy    = [ "multi-user.target" ];
            after       = [ "network.target" ];
            serviceConfig = {
              ExecStart    = "${self.packages.${pkgs.system}.xrcad-server}/bin/xrcad-server --port ${toString cfg.port} --dir ${cfg.wasmDir}";
              Restart      = "on-failure";
              DynamicUser  = true;
              ReadOnlyPaths = [ cfg.wasmDir ];
            };
          };

          networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.port ];
        };
      };

    # ── nix-on-droid module ─────────────────────────────────────────────────────
    # Import the home-manager module into the home-manager sub-scope that
    # nix-on-droid exposes.  In nix-on-droid top-level modules, home-manager
    # options live under home-manager.config, so we bridge them here.
    # Usage: add xrcad.nixOnDroidModules.xrcad-server to your modules list,
    # then configure via home-manager.config.services.xrcad-server.*.
    nixOnDroidModules.xrcad-server = {
      # Android kernels don't support the Linux namespacing that Nix uses for
      # its build sandbox, so builds fail with "Permission denied" on PTY.
      # Disabling the sandbox and syscall filter is standard for nix-on-droid.
      nix.extraOptions = ''
        sandbox = false
        filter-syscalls = false
      '';

      home-manager.config.imports = [ self.homeManagerModules.xrcad-server ];
    };

    # ── home-manager module (standalone HM or imported via nixOnDroidModules) ──
    homeManagerModules.xrcad-server = { config, lib, pkgs, ... }:
      let cfg = config.services.xrcad-server; in {
        options.services.xrcad-server = {
          enable  = lib.mkEnableOption "xrcad WebSocket relay + static file server";
          port    = lib.mkOption {
            type    = lib.types.port;
            default = 8080;
            description = "TCP port to listen on.";
          };
          wasmDir = lib.mkOption {
            type    = lib.types.str;
            default = "$HOME/.local/share/xrcad-server/www";
            description = "Directory containing the pre-built WASM app files.";
          };
          useTermuxBoot = lib.mkOption {
            type    = lib.types.bool;
            default = false;
            description = ''
              Install a Termux:Boot script so xrcad-server starts automatically
              when the Android device boots.  Requires the Termux:Boot app.
              Enable this on nix-on-droid instead of the systemd user service.
            '';
          };
          useSystemd = lib.mkOption {
            type    = lib.types.bool;
            default = true;
            description = ''
              Register a systemd user service.  Disable on nix-on-droid
              (which has no systemd) and enable useTermuxBoot instead.
            '';
          };
        };

        config = lib.mkIf cfg.enable {
          home.packages = [
            self.packages.${pkgs.system}.xrcad-server
            self.packages.${pkgs.system}.xrcad-fetch-wasm
          ];

          # Auto-fetch WASM files on every `nix-on-droid switch` / `home-manager switch`
          # if the directory doesn't exist yet.  Re-run xrcad-fetch-wasm manually to update.
          home.activation.xrcad-fetch-wasm = {
            after = [ "writeBoundary" ];
            before = [];
            data = ''
              if [ ! -d "${cfg.wasmDir}" ]; then
                $DRY_RUN_CMD ${self.packages.${pkgs.system}.xrcad-fetch-wasm}/bin/xrcad-fetch-wasm
              fi
            '';
          };

          # systemd user service — for regular Linux / NixOS home-manager.
          systemd.user.services.xrcad-server = lib.mkIf cfg.useSystemd {
            Unit.Description = "xrcad relay server";
            Install.WantedBy = [ "default.target" ];
            Service = {
              # Fetch WASM on first start if directory is missing, then start server.
              ExecStartPre = pkgs.writeShellScript "xrcad-ensure-wasm" ''
                [ -d "${cfg.wasmDir}" ] || ${self.packages.${pkgs.system}.xrcad-fetch-wasm}/bin/xrcad-fetch-wasm
              '';
              ExecStart = "${self.packages.${pkgs.system}.xrcad-server}/bin/xrcad-server --port ${toString cfg.port} --dir ${cfg.wasmDir}";
              Restart    = "on-failure";
            };
          };

          # Termux:Boot script — for nix-on-droid (Android, no systemd).
          # Install the "Termux:Boot" app from F-Droid, then enable this option.
          # Fetches WASM automatically if missing, then starts the server.
          home.file.".termux/boot/xrcad-server" = lib.mkIf cfg.useTermuxBoot {
            executable = true;
            text = ''
              #!/data/data/com.termux/files/usr/bin/sh
              export HOME=/data/data/com.termux.nix/files/home
              LOG="$HOME/.local/share/xrcad-server/server.log"
              mkdir -p "$(dirname "$LOG")"

              # Fetch WASM app on first boot (or if directory was lost).
              if [ ! -d "${cfg.wasmDir}" ]; then
                ${self.packages.${pkgs.system}.xrcad-fetch-wasm}/bin/xrcad-fetch-wasm \
                  >> "$LOG" 2>&1
              fi

              ${self.packages.${pkgs.system}.xrcad-server}/bin/xrcad-server \
                --port ${toString cfg.port} \
                --dir ${cfg.wasmDir} \
                >> "$LOG" 2>&1 &
            '';
          };
        };
      };
  };
}
