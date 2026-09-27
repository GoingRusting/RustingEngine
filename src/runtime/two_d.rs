//! First slice of the 2D path: tile maps and a side-view platformer
//! controller. Sprites are `MeshRenderer`s with the `Quad` primitive and an
//! unlit material; a 2D camera is a `Camera` with an orthographic
//! projection looking down -Z; screen-space UI is `HudElement`.
//!
//! 2D scenes live in the XY plane: +X is right, +Y is up, and the camera
//! sits at +Z. CPU physics stays 3D; 2D colliders are boxes and capsules
//! centered on z = 0.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{
    Commands, Component, DetectChanges, Query, Ref, RemovedComponents, Res,
    ResMut,
};
use serde::{Deserialize, Serialize};

use super::{
    ActionMap, Collider, ColliderShape, FrameTime, MeshRenderer, PhysicsBody,
    PhysicsWorld, RigidBody, RigidBodyKind, RuntimeInput, PLAYER_JUMP,
    PLAYER_LEFT, PLAYER_RIGHT,
};
use crate::assets::{
    AlphaMode, AssetServer, Handle, MaterialAsset, MaterialModel,
    PrimitiveShape,
};
use crate::Transform;

/// How one tile character draws and collides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileKind {
    /// Linear RGBA; multiplies the texture.
    pub color: [f32; 4],
    /// Image path relative to the project's `assets` folder. Transparent
    /// pixels (alpha below 0.5) are cut out.
    pub texture: Option<PathBuf>,
    /// Blocks bodies with a CPU collider.
    pub solid: bool,
}

impl Default for TileKind {
    fn default() -> Self {
        Self {
            color: [1.0; 4],
            texture: None,
            solid: true,
        }
    }
}

/// A grid of square tiles written as text rows, top row first. Each
/// character looks up its [`TileKind`] in `tiles`; characters without one
/// (such as space or `.`) are empty. The entity's `Transform` position is
/// the top-left corner of the grid; rotation and scale are not applied.
///
/// Tiles are spawned when the map is added or changed, as entities without
/// a `SceneId`, so they are never saved. Each run of solid tiles in a row
/// gets one fixed box collider.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileMap {
    /// Tile edge length in metres.
    pub tile_size: f32,
    pub rows: Vec<String>,
    /// Keyed by a one-character string.
    pub tiles: BTreeMap<String, TileKind>,
}

impl Default for TileMap {
    fn default() -> Self {
        Self {
            tile_size: 1.0,
            rows: vec!["#...#".into(), "#####".into()],
            tiles: BTreeMap::from([("#".into(), TileKind::default())]),
        }
    }
}

impl TileMap {
    /// Center of the cell at `column`, `row` relative to the map origin.
    #[must_use]
    pub fn cell_center(&self, column: usize, row: usize) -> [f32; 2] {
        [
            (column as f32 + 0.5) * self.tile_size,
            -(row as f32 + 0.5) * self.tile_size,
        ]
    }

    /// The kind at `column`, `row`, if the cell is not empty.
    #[must_use]
    pub fn kind_at(&self, column: usize, row: usize) -> Option<&TileKind> {
        let character = self.rows.get(row)?.chars().nth(column)?;
        self.tiles.get(character.encode_utf8(&mut [0; 4]) as &str)
    }
}

/// Marks an entity spawned for the [`TileMap`] on the given entity.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileOf(pub Entity);

/// Per frame: respawns the tiles of every added or changed map and removes
/// the tiles of removed maps.
pub(super) fn build_tile_maps(
    mut commands: Commands,
    assets: Option<ResMut<AssetServer>>,
    maps: Query<(Entity, Ref<TileMap>, Ref<Transform>)>,
    mut removed: RemovedComponents<TileMap>,
    tiles: Query<(Entity, &TileOf)>,
) {
    let mut stale: BTreeSet<Entity> = removed.read().collect();
    let changed: Vec<_> = maps
        .iter()
        .filter(|(_, map, transform)| {
            map.is_changed() || transform.is_changed()
        })
        .collect();
    stale.extend(changed.iter().map(|(entity, ..)| *entity));
    if stale.is_empty() {
        return;
    }
    for (tile, owner) in &tiles {
        if stale.contains(&owner.0) {
            commands.entity(tile).despawn();
        }
    }
    let Some(mut assets) = assets else {
        return;
    };
    let quad = assets.builtin_primitive(PrimitiveShape::Quad);
    for (entity, map, transform) in changed {
        let origin = transform.position;
        let size = map.tile_size;
        let mut materials = BTreeMap::new();
        for (row, text) in map.rows.iter().enumerate() {
            let mut run: Option<usize> = None;
            let columns = text.chars().count();
            for column in 0..=columns {
                let kind = map.kind_at(column, row);
                if let Some(kind) = kind {
                    let key = text.chars().nth(column).unwrap_or(' ');
                    let material = *materials
                        .entry(key)
                        .or_insert_with(|| tile_material(&mut assets, kind));
                    let [x, y] = map.cell_center(column, row);
                    commands.spawn((
                        Transform {
                            position: [origin[0] + x, origin[1] + y, origin[2]],
                            scale: [size, size, 1.0],
                            ..Transform::default()
                        },
                        MeshRenderer {
                            mesh: quad,
                            material,
                            cast_shadows: false,
                            receive_shadows: false,
                        },
                        TileOf(entity),
                    ));
                }
                let solid = kind.is_some_and(|kind| kind.solid);
                match (solid, run) {
                    (true, None) => run = Some(column),
                    (false, Some(start)) => {
                        run = None;
                        let width = (column - start) as f32 * size;
                        let [x, y] = map.cell_center(start, row);
                        commands.spawn((
                            Transform::new([
                                origin[0] + x - size / 2.0 + width / 2.0,
                                origin[1] + y,
                                origin[2],
                            ]),
                            PhysicsBody::default(),
                            RigidBody {
                                kind: RigidBodyKind::Fixed,
                                ..RigidBody::default()
                            },
                            Collider {
                                shape: ColliderShape::Box {
                                    half_extents: [
                                        width / 2.0,
                                        size / 2.0,
                                        size / 2.0,
                                    ],
                                },
                                ..Collider::default()
                            },
                            TileOf(entity),
                        ));
                    }
                    _ => {}
                }
            }
        }
    }
}

fn tile_material(
    assets: &mut AssetServer,
    kind: &TileKind,
) -> Handle<MaterialAsset> {
    let texture = kind.texture.as_ref().and_then(|path| {
        assets
            .textures
            .handle_for_path(path)
            .map(Ok)
            .unwrap_or_else(|| assets.load_texture(path))
            .map_err(|error| eprintln!("tile texture: {error}"))
            .ok()
    });
    let material = MaterialAsset {
        model: MaterialModel::Unlit,
        alpha_mode: if texture.is_some() {
            AlphaMode::Mask { cutoff: 0.5 }
        } else {
            AlphaMode::Opaque
        },
        base_color: kind.color,
        base_color_texture: texture,
        ..MaterialAsset::default()
    };
    let existing = assets.materials.iter().find_map(|(handle, existing)| {
        (*existing == material).then_some(handle)
    });
    existing.unwrap_or_else(|| assets.materials.insert(material))
}

/// Default body of a [`PlatformerController`] without a `Collider`: 1 m
/// tall and 0.6 m wide.
pub const DEFAULT_PLATFORMER_SHAPE: ColliderShape = ColliderShape::Capsule {
    half_height: 0.2,
    radius: 0.3,
};

/// Side-view run and jump in the XY plane. Reads `player.left`,
/// `player.right`, and `player.jump` from the [`ActionMap`]; speeds are in
/// metres per second and `gravity` in metres per second squared. Parent an
/// orthographic camera to the body to follow it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformerController {
    pub run_speed: f32,
    pub jump_speed: f32,
    pub gravity: f32,
    /// Collision layers the body stops against.
    pub collision_mask: u32,
    #[serde(skip)]
    pub vertical_speed: f32,
    #[serde(skip)]
    pub grounded: bool,
    /// Seconds a jump press stays pending; set to [`JUMP_GRACE`] on press.
    #[serde(skip)]
    pub jump_buffer: f32,
    /// Seconds since the body was last grounded.
    #[serde(skip)]
    pub air_time: f32,
}

/// A jump pressed up to this long before landing, or this long after
/// running off a ledge, still jumps.
pub const JUMP_GRACE: f32 = 0.1;

impl Default for PlatformerController {
    fn default() -> Self {
        Self {
            run_speed: 5.0,
            jump_speed: 8.0,
            gravity: 20.0,
            collision_mask: u32::MAX,
            vertical_speed: 0.0,
            grounded: false,
            jump_buffer: 0.0,
            air_time: 0.0,
        }
    }
}

/// Per rendered frame: remembers jump presses for the next fixed step.
pub(super) fn platformer_jump(
    input: Res<RuntimeInput>,
    actions: Res<ActionMap>,
    mut players: Query<&mut PlatformerController>,
) {
    if actions.just_pressed(&input, PLAYER_JUMP) {
        for mut player in &mut players {
            player.jump_buffer = JUMP_GRACE;
        }
    }
}

/// Per fixed step: running, gravity, jumping, and collision sliding.
pub(super) fn platformer_move(
    time: Res<FrameTime>,
    input: Res<RuntimeInput>,
    actions: Res<ActionMap>,
    physics: Res<PhysicsWorld>,
    mut players: Query<(
        Entity,
        &mut PlatformerController,
        &mut Transform,
        Option<&Collider>,
    )>,
) {
    let dt = time.fixed_delta.as_secs_f32();
    let run = f32::from(u8::from(actions.held(&input, PLAYER_RIGHT)))
        - f32::from(u8::from(actions.held(&input, PLAYER_LEFT)));
    for (entity, mut player, mut transform, collider) in &mut players {
        if player.jump_buffer > 0.0 && player.air_time <= JUMP_GRACE {
            player.vertical_speed = player.jump_speed;
            player.jump_buffer = 0.0;
            // No second jump from the same grace window.
            player.air_time = f32::INFINITY;
        }
        player.jump_buffer = (player.jump_buffer - dt).max(0.0);
        player.vertical_speed -= player.gravity * dt;
        let moved = physics.move_character(
            collider
                .map_or(DEFAULT_PLATFORMER_SHAPE, |collider| collider.shape),
            transform.position,
            [run * player.run_speed * dt, player.vertical_speed * dt, 0.0],
            player.collision_mask,
            Some(entity),
        );
        player.grounded = moved.grounded;
        if moved.grounded && player.vertical_speed < 0.0 {
            player.vertical_speed = 0.0;
            player.air_time = 0.0;
        } else {
            player.air_time += dt;
        }
        transform.position = moved.position;
    }
}
