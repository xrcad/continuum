{
  description = "xrcad — collaborative CAD in the browser";

  inputs = {
    nixpkgs.url     = "github:NixOS/nixpkgs/nixos-unstable";
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

    # ── home-manager module (nix-on-droid, or regular HM on any machine) ───────
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
            type        = lib.types.str;
            default     = "\${XDG_DATA_HOME:-\${HOME}/.local/share}/xrcad-server/www";
            description = "Directory containing the pre-built WASM app files.";
          };
        };

        config = lib.mkIf cfg.enable {
          home.packages = [
            self.packages.${pkgs.system}.xrcad-server
            self.packages.${pkgs.system}.xrcad-fetch-wasm
          ];

          # systemd user service — works on Linux with systemd-user (NixOS HM).
          # On nix-on-droid (no systemd) the packages are still installed;
          # start manually with:  xrcad-server --port 8080 --dir <wasmDir>
          systemd.user.services.xrcad-server = lib.mkIf (builtins.currentSystem != "aarch64-linux") {
            Unit.Description = "xrcad relay server";
            Install.WantedBy = [ "default.target" ];
            Service = {
              ExecStart = "${self.packages.${pkgs.system}.xrcad-server}/bin/xrcad-server --port ${toString cfg.port} --dir ${cfg.wasmDir}";
              Restart   = "on-failure";
            };
          };
        };
      };
  };
}
