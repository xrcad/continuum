//! Coordinator: mDNS registration/browse + TCP listener + peer routing.
//!
//! One long-lived tokio task. Communicates with Bevy via the `NetInbound` /
//! `NetOutbound` unbounded channels owned by [`super::NetBridge`].

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};

use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::framing::make_frame;
use super::peer::{PeerCmd, PeerEvent, WireMsg, run_peer};
use super::{NetInbound, NetOutbound};
use crate::{Channel, PeerId, SessionId};

const SERVICE_TYPE: &str = "_xrcad._tcp.local.";

// ─────────────────────────────────────────────────────────────────────────────

pub(super) async fn run_coordinator(
    local_peer_id: PeerId,
    local_display_name: String,
    session_id: SessionId,
    inbound_tx: mpsc::UnboundedSender<NetInbound>,
    mut outbound_rx: mpsc::UnboundedReceiver<NetOutbound>,
) {
    // ── TCP listener ──────────────────────────────────────────────────────────
    let listener = match TcpListener::bind("0.0.0.0:0").await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("xrcad-net: bind: {e}");
            return;
        }
    };
    let port = listener.local_addr().map_or(0, |a| a.port());
    tracing::info!("xrcad-net: TCP listener on port {port}");

    // ── mDNS ──────────────────────────────────────────────────────────────────
    let mdns = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("xrcad-net: mDNS daemon: {e}");
            return;
        }
    };

    let instance_name = local_peer_id.0.to_string();
    let hostname = format!("{}.local.", instance_name);
    let peer_id_str = local_peer_id.0.to_string();
    let session_id_str = session_id.0.to_string();
    let properties = [
        ("peer_id", peer_id_str.as_str()),
        ("display", local_display_name.as_str()),
        ("session_id", session_id_str.as_str()),
    ];

    match ServiceInfo::new(
        SERVICE_TYPE,
        &instance_name,
        &hostname,
        "",
        port,
        &properties[..],
    ) {
        Ok(svc) => {
            if let Err(e) = mdns.register(svc.enable_addr_auto()) {
                tracing::error!("xrcad-net: mDNS register: {e}");
            } else {
                tracing::info!("xrcad-net: mDNS registered as {instance_name}");
            }
        }
        Err(e) => tracing::error!("xrcad-net: ServiceInfo: {e}"),
    }

    let browse_rx = match mdns.browse(SERVICE_TYPE) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("xrcad-net: mDNS browse: {e}");
            return;
        }
    };

    // ── Internal channels ─────────────────────────────────────────────────────
    let (peer_event_tx, mut peer_event_rx) = mpsc::unbounded_channel::<PeerEvent>();
    let (listener_tx, mut listener_rx) = mpsc::unbounded_channel::<TcpStream>();
    let (mdns_event_tx, mut mdns_event_rx) = mpsc::unbounded_channel::<ServiceEvent>();

    // ── Listener task ─────────────────────────────────────────────────────────
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    tracing::debug!("xrcad-net: incoming from {addr}");
                    if listener_tx.send(stream).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("xrcad-net: accept: {e}");
                    break;
                }
            }
        }
    });

    // ── mDNS bridge: flume Receiver → tokio mpsc ──────────────────────────────
    std::thread::spawn(move || {
        while let Ok(event) = browse_rx.recv() {
            if mdns_event_tx.send(event).is_err() {
                break;
            }
        }
    });

    // ── Coordinator state ─────────────────────────────────────────────────────
    let mut active_peers: HashMap<PeerId, mpsc::UnboundedSender<PeerCmd>> = HashMap::new();
    let mut connecting: HashSet<PeerId> = HashSet::new();
    // fullname (e.g. "uuid._xrcad._tcp.local.") → PeerId, for ServiceRemoved lookup.
    let mut fullname_map: HashMap<String, PeerId> = HashMap::new();

    // ── Main loop ─────────────────────────────────────────────────────────────
    loop {
        tokio::select! {
            // Commands from Bevy → route to peer write channels.
            Some(cmd) = outbound_rx.recv() => {
                match cmd {
                    NetOutbound::Broadcast { channel, payload } => {
                        let frame = encode_payload(channel, &payload);
                        for tx in active_peers.values() {
                            tx.send(PeerCmd::Send(frame.clone())).ok();
                        }
                    }
                    NetOutbound::SendTo { peer_id, channel, payload } => {
                        if let Some(tx) = active_peers.get(&peer_id) {
                            tx.send(PeerCmd::Send(encode_payload(channel, &payload))).ok();
                        }
                    }
                }
            }

            // Events from peer tasks → update state and forward to Bevy.
            Some(event) = peer_event_rx.recv() => {
                match event {
                    PeerEvent::Connected { peer_id, display_name, session_id: peer_sess, writer } => {
                        connecting.remove(&peer_id);
                        if let std::collections::hash_map::Entry::Vacant(e) =
                            active_peers.entry(peer_id)
                        {
                            tracing::info!("xrcad-net: peer connected: {peer_id}");
                            e.insert(writer);
                            inbound_tx
                                .send(NetInbound::PeerConnected {
                                    peer_id,
                                    display_name: Some(display_name),
                                    session_id: peer_sess,
                                })
                                .ok();
                        } else {
                            tracing::debug!(
                                "xrcad-net: duplicate connection from {peer_id}, dropping"
                            );
                            drop(writer); // writer task exits when its rx is dropped
                        }
                    }
                    PeerEvent::Disconnected { peer_id, graceful } => {
                        connecting.remove(&peer_id);
                        if active_peers.remove(&peer_id).is_some() {
                            tracing::info!("xrcad-net: peer disconnected: {peer_id}");
                            inbound_tx.send(NetInbound::PeerDisconnected { peer_id, graceful }).ok();
                        }
                    }
                    PeerEvent::Message { from, channel, payload } => {
                        if active_peers.contains_key(&from) {
                            inbound_tx.send(NetInbound::Message { from, channel, payload }).ok();
                        }
                    }
                }
            }

            // mDNS events → discover or lose peers.
            Some(mdns_event) = mdns_event_rx.recv() => {
                match mdns_event {
                    ServiceEvent::ServiceResolved(info) => {
                        handle_resolved(
                            &info,
                            local_peer_id,
                            &local_display_name,
                            session_id,
                            &active_peers,
                            &mut connecting,
                            &mut fullname_map,
                            &inbound_tx,
                            &peer_event_tx,
                        );
                    }
                    ServiceEvent::ServiceRemoved(_ty, fullname) => {
                        if let Some(peer_id) = fullname_map.remove(&fullname) {
                            tracing::info!("xrcad-net: mDNS lost {peer_id}");
                            inbound_tx.send(NetInbound::PeerLost { peer_id }).ok();
                        }
                    }
                    _ => {}
                }
            }

            // Incoming TCP connection → spawn peer task.
            Some(stream) = listener_rx.recv() => {
                let evt_tx = peer_event_tx.clone();
                let name = local_display_name.clone();
                tokio::spawn(async move {
                    run_peer(stream, local_peer_id, name, session_id, evt_tx).await;
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Encode a `WireMsg::Payload` into a pre-assembled length-prefixed frame.
fn encode_payload(channel: Channel, payload: &[u8]) -> Vec<u8> {
    let msg = WireMsg::Payload {
        channel,
        payload: payload.to_vec(),
    };
    let bytes = postcard::to_allocvec(&msg).unwrap_or_default();
    make_frame(&bytes)
}

/// Handle a `ServiceResolved` mDNS event: parse TXT properties, store the
/// fullname mapping, notify Bevy, and spawn an outbound connection if needed.
fn handle_resolved(
    info: &ResolvedService,
    local_peer_id: PeerId,
    local_display_name: &str,
    session_id: SessionId,
    active_peers: &HashMap<PeerId, mpsc::UnboundedSender<PeerCmd>>,
    connecting: &mut HashSet<PeerId>,
    fullname_map: &mut HashMap<String, PeerId>,
    inbound_tx: &mpsc::UnboundedSender<NetInbound>,
    peer_event_tx: &mpsc::UnboundedSender<PeerEvent>,
) {
    // Parse peer_id from TXT record.
    let Some(pid_str) = info.get_property_val_str("peer_id") else {
        return;
    };
    let Ok(uuid) = Uuid::parse_str(pid_str) else {
        return;
    };
    let peer_id = PeerId(uuid);

    // Skip our own advertisement.
    if peer_id == local_peer_id {
        return;
    }

    let display = info
        .get_property_val_str("display")
        .unwrap_or("Unknown")
        .to_string();
    let disc_sess = info
        .get_property_val_str("session_id")
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(SessionId);

    fullname_map.insert(info.get_fullname().to_string(), peer_id);

    // Pick the first IPv4 address resolved by mDNS.
    let Some(addr_v4) = info.get_addresses_v4().into_iter().next() else {
        return;
    };
    let sock_addr = SocketAddr::new(IpAddr::V4(addr_v4), info.get_port());

    tracing::info!("xrcad-net: discovered {peer_id} at {sock_addr}");

    inbound_tx
        .send(NetInbound::PeerDiscovered {
            peer_id,
            display_name: Some(display),
            session_id: disc_sess,
        })
        .ok();

    // Auto-connect if not already connected or attempting connection.
    if active_peers.contains_key(&peer_id) || connecting.contains(&peer_id) {
        return;
    }

    connecting.insert(peer_id);
    let evt_tx = peer_event_tx.clone();
    let name = local_display_name.to_string();

    tokio::spawn(async move {
        match TcpStream::connect(sock_addr).await {
            Ok(stream) => {
                run_peer(stream, local_peer_id, name, session_id, evt_tx).await;
            }
            Err(e) => {
                tracing::warn!("xrcad-net: connect to {peer_id} at {sock_addr}: {e}");
                // Remove from `connecting` so a future mDNS re-resolve can retry.
                evt_tx
                    .send(PeerEvent::Disconnected {
                        peer_id,
                        graceful: false,
                    })
                    .ok();
            }
        }
    });
}
