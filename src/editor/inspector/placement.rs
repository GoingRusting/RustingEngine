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
    FOG_COMPONENT, HUD_ELEMENT_COMPONENT, SKY_LIGHT_COMPONENT,
    TILE_MAP_COMPONENT, TONE_MAPPING_COMPONENT,
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
