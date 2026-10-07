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
    ColorGrading, SceneDocument, SceneProjection, AMBIENT_LIGHT_COMPONENT,
    BACKGROUND_COMPONENT, COLOR_GRADING_COMPONENT, HUD_ELEMENT_COMPONENT,
    SKY_LIGHT_COMPONENT, TONE_MAPPING_COMPONENT,
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
    /// Written as `rusting.color_grading`.
    pub grading: ColorGrading,
    /// Linear RGB base colors that suit the light: dark, mid, light, then
    /// two accents. Use them for materials so the scene stays in one palette.
    pub palette: [[f32; 3]; 5],
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
        grading: ColorGrading { contrast: 1.05, saturation: 1.05, shadows: [0.97, 0.99, 1.04], highlights: [1.03, 1.01, 0.97], vignette: 0.15, ..ColorGrading::DEFAULT },
        palette: [[0.08, 0.1, 0.12], [0.35, 0.45, 0.3], [0.85, 0.82, 0.75], [0.9, 0.35, 0.1], [0.1, 0.4, 0.8]],
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
        grading: ColorGrading { contrast: 1.1, saturation: 1.1, shadows: [0.9, 0.92, 1.1], highlights: [1.08, 1.0, 0.88], vignette: 0.25, ..ColorGrading::DEFAULT },
        palette: [[0.12, 0.06, 0.08], [0.6, 0.3, 0.18], [0.95, 0.8, 0.6], [0.95, 0.5, 0.1], [0.25, 0.2, 0.5]],
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
        grading: ColorGrading { contrast: 1.15, saturation: 0.9, shadows: [0.85, 0.9, 1.15], highlights: [1.1, 1.0, 0.85], vignette: 0.35, ..ColorGrading::DEFAULT },
        palette: [[0.02, 0.025, 0.05], [0.12, 0.15, 0.25], [0.45, 0.5, 0.6], [1.0, 0.6, 0.2], [0.3, 0.8, 0.9]],
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
        grading: ColorGrading { contrast: 1.0, saturation: 1.15, shadows: [1.0, 1.0, 1.0], highlights: [1.0, 1.0, 1.0], vignette: 0.0, ..ColorGrading::DEFAULT },
        palette: [[0.15, 0.15, 0.2], [0.85, 0.3, 0.3], [0.95, 0.9, 0.8], [0.2, 0.55, 0.9], [0.95, 0.75, 0.15]],
    },
    ArtPreset {
        name: "dark_interior",
        summary: "Almost no ambient, warm practical lights carry the scene, heavy vignette: horror, night shifts, dungeons. Add point lights.",
        sun_color: [0.5, 0.6, 0.9],
        // Moonlight through windows: bright enough to throw readable
        // shafts, while the room around stays dark.
        sun_illuminance: 30_000.0,
        sun_shadows: true,
        sun_rotation: [-1.0, 0.6, 0.0],
        ambient_color: [0.35, 0.4, 0.6],
        ambient_intensity: 0.02,
        sky_color: [0.05, 0.06, 0.1],
        ground_color: [0.02, 0.02, 0.02],
        sky_intensity: 0.05,
        tone_mapper: "Aces",
        exposure: 1.4,
        background: [0.005, 0.005, 0.01, 1.0],
        field_of_view_degrees: 65.0,
        font_size: 20.0,
        text_color: [0.95, 0.85, 0.65, 1.0],
        grading: ColorGrading { contrast: 1.2, saturation: 0.85, shadows: [0.85, 0.92, 1.15], highlights: [1.12, 1.0, 0.82], vignette: 0.45, ..ColorGrading::DEFAULT },
        palette: [[0.03, 0.03, 0.04], [0.18, 0.15, 0.13], [0.55, 0.5, 0.42], [1.0, 0.55, 0.15], [0.8, 0.1, 0.08]],
    },
    ArtPreset {
        name: "bright_stylized",
        summary: "Strong sun with soft shadows, saturated colors and gentle contrast: cartoon, casual and party games.",
        sun_color: [1.0, 0.97, 0.9],
        sun_illuminance: 90_000.0,
        sun_shadows: true,
        sun_rotation: [-0.95, 0.6, 0.0],
        ambient_color: [0.85, 0.9, 1.0],
        ambient_intensity: 0.2,
        sky_color: [0.6, 0.8, 1.0],
        ground_color: [0.5, 0.45, 0.35],
        sky_intensity: 0.45,
        tone_mapper: "Aces",
        exposure: 1.1,
        background: [0.35, 0.65, 0.95, 1.0],
        field_of_view_degrees: 50.0,
        font_size: 26.0,
        text_color: [1.0, 1.0, 1.0, 1.0],
        grading: ColorGrading { contrast: 1.05, saturation: 1.25, shadows: [0.92, 0.95, 1.1], highlights: [1.04, 1.02, 0.96], vignette: 0.1, ..ColorGrading::DEFAULT },
        palette: [[0.12, 0.1, 0.2], [0.25, 0.65, 0.3], [0.98, 0.95, 0.88], [1.0, 0.4, 0.3], [0.2, 0.5, 1.0]],
    },
];

/// What `preset apply --only` can limit a preset to: sun plus environment;
/// ambient, sky, tone mapping, grading and background without the sun;
/// perspective field of view; HUD text.
pub const PRESET_SCOPES: [&str; 4] =
    ["lighting", "environment", "camera", "text"];

#[must_use]
pub fn preset(name: &str) -> Option<&'static ArtPreset> {
    PRESETS.iter().find(|preset| preset.name == name)
}

/// The patch that applies `preset` to `document`. The sun goes on the first
/// entity with a directional light; ambient, sky, tone mapping and
/// background go on the first entity with any of them. Missing entities
/// are created as `Sun` and `Environment`. `only` limits
/// the patch to some of [`PRESET_SCOPES`]; empty means every scope.
#[must_use]
pub fn preset_patch(
    document: &SceneDocument,
    preset: &ArtPreset,
    only: &[&str],
) -> ScenePatch {
    let wants = |scope: &str| only.is_empty() || only.contains(&scope);
    let mut operations = Vec::new();
    let wants_sun = wants("lighting");
    if !wants_sun && !wants("environment") {
        preset_camera_and_text(document, preset, &wants, &mut operations);
        return ScenePatch {
            expected_revision: None,
            operations,
        };
    }
    let environment_keys = [
        AMBIENT_LIGHT_COMPONENT,
        SKY_LIGHT_COMPONENT,
        TONE_MAPPING_COMPONENT,
        COLOR_GRADING_COMPONENT,
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
    let mut create = |name: &str, extra: Value| {
        let id = Uuid::new_v4();
        let mut entity = json!({
            "id": id,
            "name": unused_name(document, name),
            "transform": {"position": [0.0, 10.0, 0.0], "rotation": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]},
            "components": {},
        });
        if let (Some(entity), Value::Object(extra)) =
            (entity.as_object_mut(), extra)
        {
            entity.extend(extra);
        }
        operations.push(PatchOperation::Create { entity });
        id
    };
    let sun = wants_sun.then(|| {
        sun.unwrap_or_else(|| {
            create("Sun", json!({"directional_light": light.clone()}))
        })
    });
    let environment =
        environment.unwrap_or_else(|| create("Environment", json!({})));
    let mut set = |id: Uuid, path: &str, value: Value| {
        operations.push(PatchOperation::Set {
            id: id.into(),
            path: path.into(),
            value,
            expected: None,
        });
    };
    if let Some(sun) = sun {
        set(sun, "/directional_light", light);
        set(sun, "/transform/rotation", json!(preset.sun_rotation));
    }
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
        &component(COLOR_GRADING_COMPONENT),
        json!(preset.grading),
    );
    set(
        environment,
        &component(BACKGROUND_COMPONENT),
        json!({"color": preset.background}),
    );
    preset_camera_and_text(document, preset, &wants, &mut operations);
    ScenePatch {
        expected_revision: None,
        operations,
    }
}

fn preset_camera_and_text(
    document: &SceneDocument,
    preset: &ArtPreset,
    wants: &dyn Fn(&str) -> bool,
    operations: &mut Vec<PatchOperation>,
) {
    let mut set = |id: Uuid, path: &str, value: Value| {
        operations.push(PatchOperation::Set {
            id: id.into(),
            path: path.into(),
            value,
            expected: None,
        });
    };
    for entity in &document.entities {
        if let Some(camera) = entity.camera.as_ref().filter(|_| wants("camera"))
        {
            if matches!(camera.projection, SceneProjection::Perspective { .. })
            {
                set(
                    entity.id,
                    "/camera/projection/Perspective/vertical_fov_radians",
                    json!(preset.field_of_view_degrees.to_radians()),
                );
            }
        }
        if wants("text")
            && entity.components.contains_key(HUD_ELEMENT_COMPONENT)
        {
            let hud = format!("/components/{HUD_ELEMENT_COMPONENT}");
            set(
                entity.id,
                &format!("{hud}/font_size"),
                json!(preset.font_size),
            );
            set(entity.id, &format!("{hud}/color"), json!(preset.text_color));
        }
    }
}

/// Scene names are unique, so a created `Sun` becomes `Sun 2` when needed.
pub(crate) fn unused_name(document: &SceneDocument, base: &str) -> String {
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
    fn every_preset_sun_lights_a_surface_visibly() {
        for preset in PRESETS {
            let lit = preset.sun_illuminance
                / crate::runtime::DirectionalLight::LUX_PER_UNIT
                * preset.exposure;
            assert!(lit >= 0.25, "{}: the sun gives {lit}", preset.name);
        }
    }

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
        let lighting_only = preset_patch(&before, night, &["lighting"]);
        assert!(lighting_only.operations.iter().all(|operation| !matches!(
            operation,
            crate::scene_patch::PatchOperation::Set { path, .. }
                if path.contains(HUD_ELEMENT_COMPONENT) || path.starts_with("/camera")
        )));
        let text_only = preset_patch(&before, night, &["text"]);
        assert!(!text_only.operations.is_empty());
        assert!(text_only.operations.iter().all(|operation| matches!(
            operation,
            crate::scene_patch::PatchOperation::Set { path, .. }
                if path.contains(HUD_ELEMENT_COMPONENT)
        )));
        let environment_only = preset_patch(&before, night, &["environment"]);
        assert!(environment_only.operations.iter().all(|operation| {
            match operation {
                crate::scene_patch::PatchOperation::Set { path, .. } => {
                    path.starts_with("/components/")
                }
                crate::scene_patch::PatchOperation::Create { entity } => {
                    entity["directional_light"].is_null()
                }
                _ => false,
            }
        }));
        let dry =
            patch_scene_file(&scene, &preset_patch(&before, night, &[]), true)
                .unwrap();
        assert!(!dry.written);
        patch_scene_file(&scene, &preset_patch(&before, night, &[]), false)
            .unwrap();
        let after = read_scene_document(&scene).unwrap();
        assert_eq!(
            after.entities.len(),
            before.entities.len() + 2,
            "a Sun and an Environment added"
        );
        let sun = after
            .entities
            .iter()
            .find(|entity| entity.directional_light.is_some())
            .unwrap();
        assert_eq!(sun.name.as_deref(), Some("Sun"));
        assert!(
            !sun.components.contains_key(AMBIENT_LIGHT_COMPONENT),
            "the environment is its own entity, not the sun"
        );
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
        patch_scene_file(&scene, &preset_patch(&after, toy, &[]), false)
            .unwrap();
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
        let grading = *world.query::<&ColorGrading>().single(world).unwrap();
        assert_eq!(grading, toy.grading);
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
