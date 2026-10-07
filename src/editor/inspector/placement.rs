//! Which registered components an object can take, and the names the
//! Inspector shows for them.
//!
//! Most components add behavior to any object. A few make an object into
//! something of its own, like Godot's `WorldEnvironment` or `Control`
//! nodes: scene-wide environment settings, a HUD element or a tile map.
//! Those are offered only on objects that are not already a mesh, camera,
//! light or another such kind, and are created from Add Object.

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::World;

use crate::runtime::{
    AmbientLight, AmbientOcclusion, Bloom, Camera, DirectionalLight, Fog,
    MeshRenderer, Name, PointLight, SceneBackground, SkyLight, SpotLight,
    ToneMapping, AMBIENT_LIGHT_COMPONENT, AMBIENT_OCCLUSION_COMPONENT,
    BACKGROUND_COMPONENT, BLOOM_COMPONENT, ENVIRONMENT_MAP_COMPONENT,
    FOG_COMPONENT, HUD_ELEMENT_COMPONENT, POST_VOLUME_COMPONENT,
    SKY_LIGHT_COMPONENT, TILE_MAP_COMPONENT, TONE_MAPPING_COMPONENT,
};
use crate::runtime::{
    ANIMATION_COMPONENT, ARTICULATION_COMPONENT, AUTO_SIMULATION_COMPONENT,
    BURST_EMITTER_COMPONENT, CONNECTIONS_COMPONENT, COUNTER_COMPONENT,
    FLUID_BLOCK_COMPONENT, IK_COMPONENT, INPUT_ACTION_COMPONENT,
    JOINT_COMPONENT, MORPH_COMPONENT, PARTICLE_EMITTER_COMPONENT,
    PHYSICS_SYNC_COMPONENT, PICKUP_COMPONENT, PLATFORMER_CONTROLLER_COMPONENT,
    PLAYER_CONTROLLER_COMPONENT, RAGDOLL_COMPONENT, REFLECTION_PROBE_COMPONENT,
    RENDER_BOUNDS_COMPONENT, SCENE_INSTANCE_COMPONENT, SKIN_COMPONENT,
    SOUND_CUE_COMPONENT, TWEEN_COMPONENT, WATER_COMPONENT,
};
use crate::runtime::{
    CAMERA_SCREEN_COMPONENT, CAMERA_SHAKE_COMPONENT, COLOR_GRADING_COMPONENT,
    SQUASH_COMPONENT,
};

/// Components that together make a World Environment object.
pub(in crate::editor) const ENVIRONMENT_COMPONENTS: [&str; 3] = [
    SKY_LIGHT_COMPONENT,
    AMBIENT_LIGHT_COMPONENT,
    TONE_MAPPING_COMPONENT,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Behavior,
    Environment,
    Hud,
    TileMap,
}

fn kind(name: &str) -> Kind {
    match name {
        AMBIENT_LIGHT_COMPONENT
        | SKY_LIGHT_COMPONENT
        | TONE_MAPPING_COMPONENT
        | BACKGROUND_COMPONENT
        | ENVIRONMENT_MAP_COMPONENT
        | FOG_COMPONENT
        | BLOOM_COMPONENT
        | COLOR_GRADING_COMPONENT
        | POST_VOLUME_COMPONENT
        | AMBIENT_OCCLUSION_COMPONENT => Kind::Environment,
        HUD_ELEMENT_COMPONENT => Kind::Hud,
        TILE_MAP_COMPONENT => Kind::TileMap,
        _ => Kind::Behavior,
    }
}

/// How the Add Component menu offers a component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::editor) enum Placement {
    Allowed,
    /// Shown greyed out, with the reason on hover.
    Blocked(String),
    /// Not listed: it does not belong on this kind of object.
    Hidden,
}

/// How `name` may be added to `entity`, which already has the registered
/// components in `present`.
pub(in crate::editor) fn placement(
    world: &World,
    entity: Entity,
    name: &str,
    present: &[&str],
) -> Placement {
    let wanted = kind(name);
    if wanted == Kind::Behavior {
        return Placement::Allowed;
    }
    let object = world.entity(entity);
    let built_in = object.contains::<MeshRenderer>()
        || object.contains::<Camera>()
        || object.contains::<DirectionalLight>()
        || object.contains::<PointLight>()
        || object.contains::<SpotLight>();
    let other_kind = present.iter().any(|present| {
        let present = kind(present);
        present != Kind::Behavior && present != wanted
    });
    if built_in || other_kind {
        return Placement::Hidden;
    }
    // A post volume's own fog and grading apply only in its area.
    if object.contains::<crate::runtime::PostVolume>()
        && matches!(name, FOG_COMPONENT | COLOR_GRADING_COMPONENT)
    {
        return Placement::Allowed;
    }
    // The renderer reads the first one it finds, so a second copy would
    // silently do nothing.
    let holder = match name {
        SKY_LIGHT_COMPONENT => first_with::<SkyLight>(world),
        AMBIENT_LIGHT_COMPONENT => first_with::<AmbientLight>(world),
        TONE_MAPPING_COMPONENT => first_with::<ToneMapping>(world),
        BACKGROUND_COMPONENT => first_with::<SceneBackground>(world),
        ENVIRONMENT_MAP_COMPONENT => {
            first_with::<crate::runtime::EnvironmentMap>(world)
        }
        FOG_COMPONENT => first_with::<Fog>(world),
        BLOOM_COMPONENT => first_with::<Bloom>(world),
        COLOR_GRADING_COMPONENT => {
            first_with::<crate::runtime::ColorGrading>(world)
        }
        AMBIENT_OCCLUSION_COMPONENT => first_with::<AmbientOcclusion>(world),
        _ => None,
    };
    match holder {
        Some(holder) if holder != entity => {
            let holder = world
                .get::<Name>(holder)
                .map_or("another object", |name| name.0.as_str());
            Placement::Blocked(format!(
                "The scene already has a {} on {holder}",
                component_label(name)
            ))
        }
        _ => Placement::Allowed,
    }
}

/// True when some object already sets the scene's sky.
pub(in crate::editor) fn scene_has_environment(world: &World) -> bool {
    first_with::<SkyLight>(world).is_some()
}

fn first_with<T: Component>(world: &World) -> Option<Entity> {
    let mut query = world.try_query::<(Entity, &T)>()?;
    query
        .iter(world)
        .map(|(entity, _)| entity)
        .min_by_key(|entity| entity.index_u32())
}

/// Plain-English help for the Add Component picker: what the component does,
/// and when to add it. Game components get a generic line.
pub(in crate::editor) fn component_help(
    name: &str,
) -> (&'static str, &'static str) {
    match name {
        "physics" => (
            "Makes the object solid: it gets a collider, a body type and a simulation class.",
            "Walls, floors, crates, balls: anything that should be hit, pushed or stood on.",
        ),
        PLAYER_CONTROLLER_COMPONENT => (
            "First-person walking, jumping and mouse look, with collision against CPU and static colliders.",
            "On the object that is your player. Put a Camera on a child to see through it.",
        ),
        PLATFORMER_CONTROLLER_COMPONENT => (
            "Side-view running and jumping for 2D levels.",
            "On the hero of a 2D platformer.",
        ),
        TWEEN_COMPONENT => (
            "Moves, rotates or scales the object smoothly over time.",
            "Doors, lifts, spinning pickups, simple animation without code.",
        ),
        SQUASH_COMPONENT => (
            "Wobbles the object's scale back to rest after a squash.",
            "Landings, hits, bouncy pickups; game code calls squash.",
        ),
        CAMERA_SHAKE_COMPONENT => (
            "Shakes the camera's view while trauma lasts.",
            "Hits, explosions, landings; game code calls add_trauma.",
        ),
        SOUND_CUE_COMPONENT => (
            "Plays a sound when its trigger happens.",
            "Footsteps, pickups, hits, ambient loops.",
        ),
        BURST_EMITTER_COMPONENT => (
            "Fires a burst of short-lived particles.",
            "Sparks, dust, explosions, hit effects.",
        ),
        PARTICLE_EMITTER_COMPONENT => (
            "A full particle effect: emission shape, random ranges, forces, size and color over life, glow and blending.",
            "Fire, smoke, snow, rain, leaves, magic. Start from `rusting effect list`.",
        ),
        SKIN_COMPONENT => (
            "Joints that bend the object's mesh, with weights from the imported model.",
            "Characters and creatures imported from glTF. Animate the joints with Animation.",
        ),
        MORPH_COMPONENT => (
            "Blend shape weights that reshape the object's mesh, from the imported model.",
            "Faces, blinking eyes, squash and stretch. Animate the weights with Animation.",
        ),
        RAGDOLL_COMPONENT => (
            "Lets a character go limp when hit or told to, then blend back to its animation.",
            "Knockdowns, falls, deaths and stumbles of animated characters.",
        ),
        IK_COMPONENT => (
            "Turns this object toward a target, or bends its parent and grandparent so it reaches one.",
            "Heads that track the player, hands on levers, feet on uneven ground.",
        ),
        ANIMATION_COMPONENT => (
            "Keyframe clips that move, turn, scale, recolor, show or hide the object and its children, with events for game code.",
            "Doors, platforms, blinking lights, idle bobbing, cutscene moves.",
        ),
        FLUID_BLOCK_COMPONENT => (
            "Fills the object's box with fluid particles that splash and settle.",
            "Small splashing volumes such as a bucket. For seas and rivers use Water.",
        ),
        WATER_COMPONENT => (
            "A rectangle of animated waves that light objects float in, with an optional current.",
            "Seas, lakes and rivers.",
        ),
        COUNTER_COMPONENT => (
            "A named number the scene can change and react to.",
            "Score, lives, coins collected, keys held.",
        ),
        PICKUP_COMPONENT => (
            "Collected when the player touches it; it can add to a counter.",
            "Coins, health, keys.",
        ),
        CONNECTIONS_COMPONENT => (
            "Wires events from this object to actions on other objects.",
            "Button opens door, trigger plays sound, without writing code.",
        ),
        JOINT_COMPONENT => (
            "Links this body to another one with a hinge, slider or spring.",
            "Doors, swings, chains, suspensions.",
        ),
        ARTICULATION_COMPONENT => (
            "A chain of joints driven as one system.",
            "Robot arms, ragdolls, rigged mechanisms.",
        ),
        INPUT_ACTION_COMPONENT => (
            "Names a keyboard, mouse or gamepad input so scripts and connections can use it.",
            "Custom controls such as Fire or Interact.",
        ),
        PHYSICS_SYNC_COMPONENT => (
            "Chooses how a GPU body's pose is read back to the CPU.",
            "Only when game code or events need the exact position of a GPU body.",
        ),
        AUTO_SIMULATION_COMPONENT => (
            "Lets the engine pick CPU or GPU physics for this body.",
            "When you do not want to choose the simulation class yourself.",
        ),
        RENDER_BOUNDS_COMPONENT => (
            "Overrides the box used to decide if the object is on screen.",
            "Meshes that animate outside their box and pop out of view.",
        ),
        SKY_LIGHT_COMPONENT => (
            "Sets the sky colors and sun-like ambient light for the scene.",
            "Once per scene, on an Environment object.",
        ),
        AMBIENT_LIGHT_COMPONENT => (
            "Adds a flat light that reaches every surface, including shadows.",
            "Lift dark shadows in interiors.",
        ),
        TONE_MAPPING_COMPONENT => (
            "Maps bright scene colors to the screen: exposure and contrast.",
            "When the picture is too bright, too dark or washed out.",
        ),
        BACKGROUND_COMPONENT => (
            "Fills the screen behind everything with one color.",
            "2D games or scenes with no sky.",
        ),
        ENVIRONMENT_MAP_COMPONENT => (
            "Uses an image of the surroundings for reflections and sky light.",
            "Shiny materials that need realistic reflections.",
        ),
        REFLECTION_PROBE_COMPONENT => (
            "Captures reflections of a local area.",
            "Indoor rooms with shiny floors or glass.",
        ),
        FOG_COMPONENT => (
            "Fades distant objects into a color.",
            "Mood, depth, hiding the far clip plane.",
        ),
        BLOOM_COMPONENT => (
            "Makes very bright areas glow.",
            "Lamps, sun, magic, neon.",
        ),
        AMBIENT_OCCLUSION_COMPONENT => (
            "Darkens creases and contact points for depth.",
            "Make objects look grounded. Costs some GPU time.",
        ),
        COLOR_GRADING_COMPONENT => (
            "Adjusts contrast, saturation, shadow and highlight tints, and vignette.",
            "Give the whole picture one mood, like a film look.",
        ),
        POST_VOLUME_COMPONENT => (
            "Limits this object's fog and color grading to a box around it.",
            "A foggy hall next to a warm office.",
        ),
        CAMERA_SCREEN_COMPONENT => (
            "Shows what another camera sees on this mesh.",
            "CCTV monitors and rear-view screens.",
        ),
        HUD_ELEMENT_COMPONENT => (
            "A piece of screen UI: text, bar or image at a screen position.",
            "Score, health bar, crosshair.",
        ),
        TILE_MAP_COMPONENT => (
            "A grid of tiles drawn and collided as one 2D object.",
            "2D levels.",
        ),
        SCENE_INSTANCE_COMPONENT => (
            "Places another scene file inside this one.",
            "Reusable prefabs such as a room, enemy or pickup.",
        ),
        _ if name.starts_with("rusting.") => (
            "Built-in engine component.",
            "See the Inspector section after adding it.",
        ),
        _ => (
            "A component from your game's code.",
            "Added with default values; its behavior comes from your game.",
        ),
    }
}

/// Display name for a registered component: the part after the last `.`,
/// in title case, so `rusting.sky_light` reads "Sky Light" and a game's
/// `my_game.health` reads "Health".
/// Group heading the Add Component picker lists `name` under.
pub(in crate::editor) fn component_group(name: &str) -> &'static str {
    match kind(name) {
        Kind::Environment => "Environment",
        Kind::Hud => "User Interface",
        Kind::TileMap => "2D",
        Kind::Behavior if name.starts_with("rusting.") => "Engine",
        Kind::Behavior => "Game",
    }
}

pub(in crate::editor) fn component_label(name: &str) -> String {
    match name {
        HUD_ELEMENT_COMPONENT => return "HUD Element".to_owned(),
        BACKGROUND_COMPONENT => return "Background Color".to_owned(),
        _ => {}
    }
    let short = name.rsplit('.').next().unwrap_or(name);
    short
        .split(['_', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::App;
    use crate::runtime::{
        PICKUP_COMPONENT, SCENE_INSTANCE_COMPONENT, TWEEN_COMPONENT,
    };
    use crate::Transform;

    #[test]
    fn every_engine_component_has_specific_help() {
        let app = App::new();
        for name in crate::runtime::registered_component_names(app.world())
            .iter()
            .filter(|name| name.starts_with("rusting."))
        {
            let (what, _) = component_help(name);
            assert_ne!(what, "Built-in engine component.", "{name}");
        }
    }

    #[test]
    fn component_labels_drop_the_namespace() {
        assert_eq!(component_label(SKY_LIGHT_COMPONENT), "Sky Light");
        assert_eq!(component_label(HUD_ELEMENT_COMPONENT), "HUD Element");
        assert_eq!(component_label(SCENE_INSTANCE_COMPONENT), "Scene Instance");
        assert_eq!(component_label("my_game.health"), "Health");
        assert_eq!(component_label("score"), "Score");
    }

    #[test]
    fn environment_and_hud_stay_off_meshes_and_each_other() {
        let mut app = App::new();
        let world = app.world_mut();
        // A light stands in for every built-in kind: mesh, camera, light.
        let lamp = world
            .spawn((Transform::default(), PointLight::default()))
            .id();
        let empty = world.spawn(Transform::default()).id();
        let sky = world
            .spawn((Name("Environment".into()), SkyLight::default()))
            .id();
        let world = app.world();

        for name in [SKY_LIGHT_COMPONENT, HUD_ELEMENT_COMPONENT] {
            assert_eq!(placement(world, lamp, name, &[]), Placement::Hidden);
        }
        for name in [TWEEN_COMPONENT, PICKUP_COMPONENT] {
            assert_eq!(placement(world, lamp, name, &[]), Placement::Allowed);
        }
        assert_eq!(
            placement(world, empty, HUD_ELEMENT_COMPONENT, &[]),
            Placement::Allowed
        );
        assert_eq!(
            placement(world, empty, TONE_MAPPING_COMPONENT, &[]),
            Placement::Allowed
        );
        // One sky per scene: the renderer reads only the first.
        assert_eq!(
            placement(world, empty, SKY_LIGHT_COMPONENT, &[]),
            Placement::Blocked(
                "The scene already has a Sky Light on Environment".into()
            )
        );
        // The environment object takes more environment settings, not HUD.
        let present = [SKY_LIGHT_COMPONENT];
        assert_eq!(
            placement(world, sky, AMBIENT_LIGHT_COMPONENT, &present),
            Placement::Allowed
        );
        assert_eq!(
            placement(world, sky, HUD_ELEMENT_COMPONENT, &present),
            Placement::Hidden
        );
    }

    #[test]
    fn atmosphere_settings_go_on_one_environment_object() {
        let mut app = App::new();
        let world = app.world_mut();
        let lamp = world
            .spawn((Transform::default(), PointLight::default()))
            .id();
        let sky = world
            .spawn((
                Name("Environment".into()),
                SkyLight::default(),
                Fog::default(),
            ))
            .id();
        let empty = world.spawn(Transform::default()).id();
        let world = app.world();
        let present = [SKY_LIGHT_COMPONENT, FOG_COMPONENT];
        for name in
            [FOG_COMPONENT, BLOOM_COMPONENT, AMBIENT_OCCLUSION_COMPONENT]
        {
            assert_eq!(placement(world, lamp, name, &[]), Placement::Hidden);
            assert_eq!(
                placement(world, sky, name, &present),
                Placement::Allowed
            );
        }
        assert_eq!(
            placement(world, empty, FOG_COMPONENT, &[]),
            Placement::Blocked(
                "The scene already has a Fog on Environment".into()
            )
        );
        assert_eq!(
            placement(world, empty, BLOOM_COMPONENT, &[]),
            Placement::Allowed
        );
    }
}
