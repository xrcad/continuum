//! Vector clock implementation for causal ordering of [`DocOp`]s.
//!
//! Each peer maintains a `VectorClock` — a map from [`PeerId`] to a monotonically
//! increasing sequence counter. Every [`OpEnvelope`] carries the sender's clock at
//! the time the op was generated. Receivers buffer ops until all causally prior ops
//! have been applied, then apply in causal order.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use xrcad_net::PeerId;

/// A snapshot of one peer's knowledge of all peers' sequence numbers.
///
/// `clock[peer]` means "I have seen all ops from `peer` up to and including sequence
/// number `clock[peer]`."
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock(pub HashMap<PeerId, u64>);

impl VectorClock {
    /// Return this peer's current sequence number, or 0 if not seen.
    pub fn get(&self, peer: &PeerId) -> u64 {
        *self.0.get(peer).unwrap_or(&0)
    }

    /// Increment and return the next sequence number for `peer`.
    pub fn increment(&mut self, peer: &PeerId) -> u64 {
        let entry = self.0.entry(*peer).or_insert(0);
        *entry += 1;
        *entry
    }

    /// Record that we have seen op `seq` from `peer`.
    pub fn observe(&mut self, peer: &PeerId, seq: u64) {
        let entry = self.0.entry(*peer).or_insert(0);
        if seq > *entry {
            *entry = seq;
        }
    }

    /// Returns `true` if this clock is causally at or after `other` for every peer —
    /// i.e. we have seen everything `other` has seen.
    pub fn dominates(&self, other: &VectorClock) -> bool {
        other.0.iter().all(|(peer, &seq)| self.get(peer) >= seq)
    }

    /// Returns `true` if we have seen all the deps declared in `deps`.
    pub fn satisfies_deps(&self, deps: &VectorClock) -> bool {
        self.dominates(deps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid() -> PeerId {
        PeerId(uuid::Uuid::new_v4())
    }

    #[test]
    fn dominates_empty() {
        let a = VectorClock::default();
        let b = VectorClock::default();
        assert!(a.dominates(&b));
    }

    #[test]
    fn dominates_partial() {
        let p = pid();
        let mut a = VectorClock::default();
        let mut b = VectorClock::default();
        a.observe(&p, 5);
        b.observe(&p, 3);
        assert!(a.dominates(&b));
        assert!(!b.dominates(&a));
    }

    #[test]
    fn increment_is_monotone() {
        let p = pid();
        let mut clock = VectorClock::default();
        assert_eq!(clock.increment(&p), 1);
        assert_eq!(clock.increment(&p), 2);
        assert_eq!(clock.increment(&p), 3);
    }
}
