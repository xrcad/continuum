use std::{
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

use uuid::Uuid;

/// A UUID-backed, phantom-typed identifier for a B-Rep element.
///
/// Contains no Bevy types — the mapping to a Bevy `Entity` is managed
/// separately by [`super::BRepRegistry`].
///
/// All trait impls are written manually so that `T` is unconstrained;
/// the marker types used as `T` are never compared themselves.
pub struct Id<T>(Uuid, PhantomData<fn() -> T>);

impl<T> Id<T> {
    /// Allocate a new random ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4(), PhantomData)
    }

    /// Wrap an existing UUID (e.g. when deserialising).
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid, PhantomData)
    }

    /// Return the underlying UUID.
    pub fn uuid(self) -> Uuid {
        self.0
    }
}

// Manual impls — no `T: Trait` bounds, since T is only a phantom.

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for Id<T> {}

impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.0)
    }
}

impl<T> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
