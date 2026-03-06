//! Operation log: causal broadcast, buffering, and application of [`DocOp`]s.

use std::collections::VecDeque;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use xrcad_net::{PeerId, PeerMessageReceived};

use crate::{OpApplied, doc_op::DocOp, vector_clock::VectorClock};

// ─────────────────────────────────────────────────────────────────────────────
// OpEnvelope
// ─────────────────────────────────────────────────────────────────────────────

/// A [`DocOp`] with its causal metadata. This is the unit of exchange on the
/// reliable channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpEnvelope {
    /// Peer that generated this op.
    pub peer_id: PeerId,
    /// Monotonically increasing per-peer sequence number.
    pub seq: u64,
    /// The sender's vector clock at the time of generation.
    /// All ops from peers listed in `deps` must be applied before this op.
    pub deps: VectorClock,
    /// Wall-clock timestamp — informational only, never used for ordering.
    pub timestamp_ms: i64,
    /// The operation payload.
    pub op: DocOp,
}

impl OpEnvelope {
    pub fn summary(&self) -> String {
        format!("{}/{} {}", self.peer_id, self.seq, self.op.summary())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Wire message wrapper
// ─────────────────────────────────────────────────────────────────────────────

/// The message type sent on the reliable channel.
/// Wraps an [`OpEnvelope`] for wire serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ColabMsg {
    Op(OpEnvelope),
    /// Sent on reconnect: "here is my current vector clock; send me anything I'm missing."
    SyncRequest(VectorClock),
    /// Response to a SyncRequest: a batch of missing ops in causal order.
    SyncResponse(Vec<OpEnvelope>),
}

// ─────────────────────────────────────────────────────────────────────────────
// OpLog resource
// ─────────────────────────────────────────────────────────────────────────────

/// The in-memory operation log. Holds the local vector clock, applied ops, and
/// ops waiting for their causal dependencies to arrive.
#[derive(Resource, Default)]
pub struct OpLog {
    /// Our current view of what has been applied across all peers.
    pub clock: VectorClock,
    /// Our own sequence counter (also stored in `clock[local_peer_id]`).
    pub local_seq: u64,
    /// Ops waiting for causal dependencies. Drained by `apply_ready_ops`.
    pub pending: VecDeque<OpEnvelope>,
    /// Complete applied history (for git commit batching).
    pub applied: Vec<OpEnvelope>,
}

impl OpLog {
    /// Record a locally generated op: assign a sequence number, update the local clock,
    /// and return the envelope ready for broadcast.
    pub fn seal_local(&mut self, peer_id: PeerId, op: DocOp) -> OpEnvelope {
        self.local_seq += 1;
        self.clock.observe(&peer_id, self.local_seq);
        OpEnvelope {
            peer_id,
            seq: self.local_seq,
            deps: self.clock.clone(),
            timestamp_ms: chrono_millis(),
            op,
        }
    }

    /// Enqueue a received envelope for causal ordering.
    pub fn enqueue(&mut self, env: OpEnvelope) {
        self.pending.push_back(env);
    }

    /// Drain and return all ops whose causal dependencies are satisfied.
    pub fn drain_ready(&mut self) -> Vec<OpEnvelope> {
        let mut ready = Vec::new();
        let mut i = 0;
        while i < self.pending.len() {
            if self.clock.satisfies_deps(&self.pending[i].deps) {
                let env = self.pending.remove(i).unwrap();
                self.clock.observe(&env.peer_id, env.seq);
                ready.push(env);
            } else {
                i += 1;
            }
        }
        ready
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Bevy systems
// ─────────────────────────────────────────────────────────────────────────────

/// Receive raw reliable-channel messages and enqueue any [`ColabMsg::Op`]s.
pub fn receive_ops(mut messages: MessageReader<PeerMessageReceived>, mut log: ResMut<OpLog>) {
    for PeerMessageReceived(raw) in messages.read() {
        if raw.channel != xrcad_net::Channel::Reliable {
            continue;
        }
        match postcard::from_bytes::<ColabMsg>(&raw.payload) {
            Ok(ColabMsg::Op(env)) => log.enqueue(env),
            Ok(_) => { /* sync messages handled separately */ }
            Err(e) => tracing::warn!("ColabMsg decode error: {e}"),
        }
    }
}

/// Apply any ops whose causal dependencies are now satisfied.
/// Fires an [`OpApplied`] event for each one so other systems can react.
pub fn apply_ready_ops(mut log: ResMut<OpLog>, mut applied: MessageWriter<OpApplied>) {
    for env in log.drain_ready() {
        tracing::debug!("applying op: {}", env.summary());
        let event_env = env.clone();
        log.applied.push(env);
        applied.write(OpApplied {
            envelope: event_env,
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn chrono_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
