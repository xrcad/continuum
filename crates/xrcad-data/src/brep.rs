use bevy::prelude::*;
use std::collections::HashMap;
use xrcad_kernel::brep::Id;

/// Attaches a B-Rep `Id<T>` to a Bevy entity as a component.
#[derive(Component)]
pub struct BRepId<T: Send + Sync + 'static>(pub Id<T>);

/// Bidirectional `Id<T>` ↔ `Entity` map, stored as a Bevy resource.
///
/// Insert one `BRepRegistry<T>` per element type that needs lookup:
/// ```rust,ignore
/// app.init_resource::<BRepRegistry<Vertex>>();
/// app.init_resource::<BRepRegistry<Edge>>();
/// // …
/// ```
#[derive(Resource)]
pub struct BRepRegistry<T: Send + Sync + 'static> {
    by_id: HashMap<Id<T>, Entity>,
    by_entity: HashMap<Entity, Id<T>>,
}

impl<T: Send + Sync + 'static> BRepRegistry<T> {
    pub fn insert(&mut self, id: Id<T>, entity: Entity) {
        self.by_id.insert(id, entity);
        self.by_entity.insert(entity, id);
    }

    pub fn entity(&self, id: Id<T>) -> Option<Entity> {
        self.by_id.get(&id).copied()
    }

    pub fn id(&self, entity: Entity) -> Option<Id<T>> {
        self.by_entity.get(&entity).copied()
    }

    pub fn remove_by_id(&mut self, id: Id<T>) -> Option<Entity> {
        let entity = self.by_id.remove(&id)?;
        self.by_entity.remove(&entity);
        Some(entity)
    }

    pub fn remove_by_entity(&mut self, entity: Entity) -> Option<Id<T>> {
        let id = self.by_entity.remove(&entity)?;
        self.by_id.remove(&id);
        Some(id)
    }
}

impl<T: Send + Sync + 'static> Default for BRepRegistry<T> {
    fn default() -> Self {
        Self {
            by_id: HashMap::new(),
            by_entity: HashMap::new(),
        }
    }
}
