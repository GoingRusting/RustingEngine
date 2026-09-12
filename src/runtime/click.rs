//! Click events: gameplay picking against the active render camera.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::World;
use nalgebra::Vector3;

use crate::assets::AssetServer;
use crate::runtime::picking::{ray_mesh_bounds, scene_ray, Ray};
use crate::runtime::{
    Camera, EventQueue, GlobalTransform, MeshRenderer, MouseButton,
    RenderWorld, RuntimeInput,
};

/// Fired the frame a mouse button is pressed over a renderable object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickEvent {
    pub entity: Entity,
    pub button: MouseButton,
    pub world_position: [f32; 3],
}

/// Buttons checked for click events. `MouseButton::Other` codes are ignored;
/// most games only care about these three.
const CLICK_BUTTONS: [MouseButton; 3] =
    [MouseButton::Left, MouseButton::Right, MouseButton::Middle];

/// Picks the object under the cursor and emits one [`ClickEvent`] per mouse
/// button pressed this frame. Does nothing when no camera, cursor position,
/// or renderable object is available, which keeps this safe to run even
/// before the renderer's first frame.
pub(super) fn route_click_events(world: &mut World) {
    let Some(active_camera) = world
        .get_resource::<RenderWorld>()
        .and_then(|render_world| render_world.active_camera)
    else {
        return;
    };
    let input = world.resource::<RuntimeInput>();
    let pressed_buttons: Vec<MouseButton> = CLICK_BUTTONS
        .into_iter()
        .filter(|button| input.mouse_just_pressed(*button))
        .collect();
    if pressed_buttons.is_empty() {
        return;
    }
    let Some(cursor) = input.cursor_position() else {
        return;
    };
    let viewport_size = input.viewport_size();
    if viewport_size[0] <= 0.0 || viewport_size[1] <= 0.0 {
        return;
    }

    let camera = Camera {
        projection: active_camera.projection,
        active: true,
        priority: active_camera.priority,
    };
    let Some(ray) = scene_ray(
        cursor,
        [0.0, 0.0],
        viewport_size,
        camera,
        active_camera.transform,
    ) else {
        return;
    };
    let Some((entity, world_position)) = pick_nearest_mesh(world, ray) else {
        return;
    };

    let mut events = world.resource_mut::<EventQueue<ClickEvent>>();
    for button in pressed_buttons {
        events.send(ClickEvent {
            entity,
            button,
            world_position: [
                world_position.x,
                world_position.y,
                world_position.z,
            ],
        });
    }
}

/// Finds the nearest renderable mesh under a world-space ray.
fn pick_nearest_mesh(
    world: &mut World,
    ray: Ray,
) -> Option<(Entity, Vector3<f32>)> {
    let candidates = {
        let mut query =
            world.query::<(Entity, &MeshRenderer, &GlobalTransform)>();
        query
            .iter(world)
            .map(|(entity, mesh, transform)| (entity, mesh.mesh, *transform))
            .collect::<Vec<_>>()
    };
    let assets = world.resource::<AssetServer>();
    candidates
        .into_iter()
        .filter_map(|(entity, handle, transform)| {
            let mesh = assets.meshes.get(handle)?;
            let distance = ray_mesh_bounds(ray, transform, mesh)?;
            Some((distance, entity))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(distance, entity)| {
            (entity, ray.origin + ray.direction * distance)
        })
}
