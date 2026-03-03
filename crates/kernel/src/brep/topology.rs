use super::id::Id;

// ---------------------------------------------------------------------------
// Phantom marker types — one per B-Rep topological level.
// These are never instantiated; they exist only to make `Id<T>` type-safe.
// ---------------------------------------------------------------------------

pub struct Vertex;
pub struct Edge;
pub struct Loop;
pub struct Face;
pub struct Shell;
pub struct Solid;

// ---------------------------------------------------------------------------
// Convenient type aliases.
// ---------------------------------------------------------------------------

pub type VertexId = Id<Vertex>;
pub type EdgeId = Id<Edge>;
pub type LoopId = Id<Loop>;
pub type FaceId = Id<Face>;
pub type ShellId = Id<Shell>;
pub type SolidId = Id<Solid>;
