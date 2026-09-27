//! Starter art-direction presets. A preset is a scene patch: it writes a
//! sun, ambient and sky light, tone mapping, a background color, the field
//! of view of perspective cameras, and the size and color of HUD text as
//! ordinary scene data. Nothing is baked into the engine; every value stays
//! editable after the preset is applied, and applying goes through
//! `scene patch`, so revisions, dry runs and editor undo work as usual.

use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::runtime::{
    SceneDocument, SceneProjection, AMBIENT_LIGHT_COMPONENT,
    BACKGROUND_COMPONENT, HUD_ELEMENT_COMPONENT, SKY_LIGHT_COMPONENT,
    TONE_MAPPING_COMPONENT,
};
use crate::scene_patch::{PatchOperation, ScenePatch};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ArtPreset {
    pub name: &'static str,
    pub summary: &'static str,
    pub sun_color: [f32; 3],
    /// Lux.
    pub sun_illuminance: f32,
    pub sun_shadows: bool,
    /// Euler angles in radians for the sun entity.
    pub sun_rotation: [f32; 3],
    pub ambient_color: [f32; 3],
    pub ambient_intensity: f32,
    pub sky_color: [f32; 3],
    pub ground_color: [f32; 3],
    pub sky_intensity: f32,
    /// `Linear`, `Reinhard` or `Aces`.
    pub tone_mapper: &'static str,
    pub exposure: f32,
    /// Linear RGBA.
    pub background: [f32; 4],
    pub field_of_view_degrees: f32,
    /// HUD text size in logical pixels.
    pub font_size: f32,
    /// sRGBA HUD text color.
    pub text_color: [f32; 4],
}

pub const PRESETS: &[ArtPreset] = &[
    ArtPreset {
        name: "daylight",
        summary: "Neutral noon sun, blue sky fill, filmic contrast, 60 degree view.",
        sun_color: [1.0, 0.96, 0.9],
        sun_illuminance: 100_000.0,
        sun_shadows: true,
        sun_rotation: [-0.9, 0.5, 0.0],
        ambient_color: [0.8, 0.85, 1.0],
        ambient_intensity: 0.1,
        sky_color: [0.55, 0.7, 1.0],
        ground_color: [0.35, 0.3, 0.25],
        sky_intensity: 0.35,
        tone_mapper: "Aces",
        exposure: 1.0,
        background: [0.16, 0.35, 0.72, 1.0],
        field_of_view_degrees: 60.0,
        font_size: 20.0,
        text_color: [1.0, 1.0, 1.0, 1.0],
    },
    ArtPreset {
        name: "golden_hour",
        summary: "Low orange sun, warm fill, long shadows, slightly tighter 50 degree view.",
        sun_color: [1.0, 0.68, 0.42],
        sun_illuminance: 70_000.0,
        sun_shadows: true,
        sun_rotation: [-0.3, 0.9, 0.0],
        ambient_color: [1.0, 0.8, 0.7],
        ambient_intensity: 0.08,
        sky_color: [0.95, 0.7, 0.5],
        ground_color: [0.3, 0.2, 0.15],
        sky_intensity: 0.3,
        tone_mapper: "Aces",
        exposure: 1.2,
        background: [0.79, 0.35, 0.16, 1.0],
        field_of_view_degrees: 50.0,
        font_size: 22.0,
        text_color: [1.0, 0.93, 0.8, 1.0],
    },
    ArtPreset {
        name: "night",
        summary: "Dim blue moonlight, dark sky, raised exposure, pale blue text.",
        sun_color: [0.6, 0.7, 1.0],
        sun_illuminance: 20_000.0,
        sun_shadows: true,
        sun_rotation: [-1.1, -0.4, 0.0],
        ambient_color: [0.3, 0.35, 0.6],
        ambient_intensity: 0.06,
        sky_color: [0.1, 0.15, 0.3],
        ground_color: [0.04, 0.04, 0.06],
        sky_intensity: 0.2,
        tone_mapper: "Aces",
        exposure: 1.6,
        background: [0.01, 0.015, 0.04, 1.0],
        field_of_view_degrees: 55.0,
        font_size: 20.0,
        text_color: [0.75, 0.85, 1.0, 1.0],
    },
    ArtPreset {
        name: "flat_toy",
        summary: "Soft shadowless light, bright fill, linear colors, large dark text for a toy-like look.",
        sun_color: [1.0, 1.0, 1.0],
        sun_illuminance: 60_000.0,
        sun_shadows: false,
        sun_rotation: [-0.8, 0.3, 0.0],
        ambient_color: [1.0, 1.0, 1.0],
        ambient_intensity: 0.35,
        sky_color: [0.9, 0.95, 1.0],
        ground_color: [0.8, 0.75, 0.7],
        sky_intensity: 0.35,
        tone_mapper: "Linear",
        exposure: 1.0,
        background: [0.83, 0.77, 0.68, 1.0],
        field_of_view_degrees: 45.0,
        font_size: 26.0,
        text_color: [0.15, 0.15, 0.2, 1.0],
    },
];

#[must_use]
pub fn preset(name: &str) -> Option<&'static ArtPreset> {
    PRESETS.iter().find(|preset| preset.name == name)
}

/// The patch that applies `preset` to `document`. The sun goes on the first
/// entity with a directional light; ambient, sky, tone mapping and
/// background go on the first entity with any of them, else on the sun.
/// Missing entities are created as `Sun` and `Environment`.
#[must_use]
pub fn preset_patch(
    document: &SceneDocument,
    preset: &ArtPreset,
) -> ScenePatch {
    let mut operations = Vec::new();
    let environment_keys = [
        AMBIENT_LIGHT_COMPONENT,
        SKY_LIGHT_COMPONENT,
        TONE_MAPPING_COMPONENT,
        BACKGROUND_COMPONENT,
    ];
    let sun = document
        .entities
        .iter()
        .find(|entity| entity.directional_light.is_some())
        .map(|entity| entity.id);
    let environment = document
        .entities
        .iter()
        .find(|entity| {
            environment_keys
                .iter()
                .any(|key| entity.components.contains_key(*key))
        })
        .map(|entity| entity.id);
    let light = json!({
        "color": preset.sun_color,
        "illuminance": preset.sun_illuminance,
        "shadows": preset.sun_shadows,
    });
    let sun = sun.unwrap_or_else(|| {
        let id = Uuid::new_v4();
        operations.push(PatchOperation::Create {
            entity: json!({
                "id": id,
                "name": unused_name(document, "Sun"),
                "transform": {"position": [0.0, 10.0, 0.0], "rotation": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]},
                "directional_light": light,
                "components": {},
            }),
        });
        id
    });
    let mut set = |id: Uuid, path: &str, value: Value| {
        operations.push(PatchOperation::Set {
            id,
            path: path.into(),
            value,
            expected: None,
        });
    };
    set(sun, "/directional_light", light);
    set(sun, "/transform/rotation", json!(preset.sun_rotation));
    let environment = environment.unwrap_or(sun);
    let component = |key: &str| format!("/components/{key}");
    set(
        environment,
        &component(AMBIENT_LIGHT_COMPONENT),
        json!({"color": preset.ambient_color, "intensity": preset.ambient_intensity}),
    );
    set(
        environment,
        &component(SKY_LIGHT_COMPONENT),
        json!({"sky_color": preset.sky_color, "ground_color": preset.ground_color, "intensity": preset.sky_intensity}),
    );
    set(
        environment,
        &component(TONE_MAPPING_COMPONENT),
        json!({"mapper": preset.tone_mapper, "exposure": preset.exposure}),
    );
    set(
        environment,
        &component(BACKGROUND_COMPONENT),
        json!({"color": preset.background}),
    );
    for entity in &document.entities {
        if let Some(camera) = &entity.camera {
            if matches!(camera.projection, SceneProjection::Perspective { .. })
            {
                set(
                    entity.id,
                    "/camera/projection/Perspective/vertical_fov_radians",
                    json!(preset.field_of_view_degrees.to_radians()),
                );
            }
        }
        if entity.components.contains_key(HUD_ELEMENT_COMPONENT) {
            let hud = component(HUD_ELEMENT_COMPONENT);
            set(
                entity.id,
                &format!("{hud}/font_size"),
                json!(preset.font_size),
            );
            set(entity.id, &format!("{hud}/color"), json!(preset.text_color));
        }
    }
    ScenePatch {
        expected_revision: None,
        operations,
    }
}

/// Scene names are unique, so a created `Sun` becomes `Sun 2` when needed.
fn unused_name(document: &SceneDocument, base: &str) -> String {
    let taken = |name: &str| {
        document
            .entities
            .iter()
            .any(|entity| entity.name.as_deref() == Some(name))
    };
    (1..)
        .map(|index| {
            if index == 1 {
                base.to_owned()
            } else {
                format!("{base} {index}")
            }
        })
        .find(|name| !taken(name))
        .expect("some name is free")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{
        read_scene_document, AmbientLight, App, SceneBackground, ToneMapper,
        ToneMapping,
    };
    use crate::scene_patch::patch_scene_file;

    #[test]
    fn presets_patch_lighting_camera_text_and_background_and_reapply_in_place()
    {
        let folder = std::env::temp_dir()
            .join(format!("rusting-art-preset-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        let project = crate::project::create_project_from(
            &folder,
            "Look",
            crate::project::ProjectTemplate::Platformer2d,
        )
        .unwrap();
        let scene = project.scene_path;
        let before = read_scene_document(&scene).unwrap();

        let night = preset("night").unwrap();
        let dry = patch_scene_file(&scene, &preset_patch(&before, night), true)
            .unwrap();
        assert!(!dry.written);
        patch_scene_file(&scene, &preset_patch(&before, night), false).unwrap();
        let after = read_scene_document(&scene).unwrap();
        assert_eq!(
            after.entities.len(),
            before.entities.len() + 1,
            "one Sun added"
        );
        let sun = after
            .entities
            .iter()
            .find(|entity| entity.directional_light.is_some())
            .unwrap();
        assert_eq!(sun.name.as_deref(), Some("Sun"));
        let hud: crate::runtime::HudElement = serde_json::from_str(
            after
                .entities
                .iter()
                .find_map(|entity| entity.components.get(HUD_ELEMENT_COMPONENT))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(hud.color, night.text_color);
        assert_eq!(hud.font_size, night.font_size);

        // A second preset edits the same entities instead of adding more.
        let toy = preset("flat_toy").unwrap();
        patch_scene_file(&scene, &preset_patch(&after, toy), false).unwrap();
        let again = read_scene_document(&scene).unwrap();
        assert_eq!(again.entities.len(), after.entities.len());

        // The values load as ordinary components and set the background.
        let mut app = App::new();
        app.add_plugin(crate::assets::AssetPlugin).unwrap();
        crate::runtime::load_scene(
            app.world_mut(),
            &scene,
            crate::runtime::SceneLoadMode::Replace,
        )
        .unwrap();
        app.update(std::time::Duration::from_secs_f64(1.0 / 60.0))
            .unwrap();
        let world = app.world_mut();
        let tone = *world.query::<&ToneMapping>().single(world).unwrap();
        assert_eq!(tone.mapper, ToneMapper::Linear);
        let ambient = *world.query::<&AmbientLight>().single(world).unwrap();
        assert_eq!(ambient.intensity, toy.ambient_intensity);
        assert_eq!(world.query::<&SceneBackground>().iter(world).count(), 1);
        assert_eq!(
            world
                .resource::<crate::runtime::RenderSettings>()
                .background_color,
            toy.background
        );
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn every_preset_has_a_unique_name_and_sane_values() {
        for (index, preset) in PRESETS.iter().enumerate() {
            assert!(PRESETS[..index]
                .iter()
                .all(|other| other.name != preset.name));
            assert!(
                ["Linear", "Reinhard", "Aces"].contains(&preset.tone_mapper)
            );
            assert!(preset.exposure > 0.0 && preset.font_size > 0.0);
            assert!((1.0..179.0).contains(&preset.field_of_view_degrees));
        }
    }
}
