//! Reflected descriptions of the engine's own components, resources, and
//! assets. Their hints and docs are what `rusting schema` prints.

use std::path::PathBuf;

use bevy_ecs::entity::Entity;

use super::TypeRegistry;
use crate::assets::{AlphaMode, MaterialAsset, MaterialModel};
use crate::runtime::{
    AmbientLight, Antialiasing, Articulation, AutoSimulation, AxisMotion,
    BurstEmitter, Connection, Connections, Counter, CullingMode,
    DeterminismMode, Easing, HudAnchor, HudElement, Joint, JointAxis,
    JointKind, JointMotor, JointSpring, PhysicsSettings, PhysicsSyncMode,
    Pickup, PlatformerController, PlayerController, QualityProfile, RandomSeed,
    RenderBounds, RenderSettings, SceneBackground, SceneInstance,
    ShadowQuality, SkyLight, SoundCue, TileKind, TileMap, ToneMapper,
    ToneMapping, Tween, TweenProperty, TweenRepeat,
};

crate::reflect! {
    struct AmbientLight {
        color: [f32; 3] { unit: "linear RGB", min: 0.0, max: 1.0, color: true },
        intensity: f32 { unit: "factor", min: 0.0 },
    }
}

crate::reflect! {
    struct SkyLight {
        sky_color: [f32; 3] {
            unit: "linear RGB", min: 0.0, max: 1.0, color: true,
            doc: "seen by up-facing surfaces",
        },
        ground_color: [f32; 3] {
            unit: "linear RGB", min: 0.0, max: 1.0, color: true,
            doc: "seen by down-facing surfaces",
        },
        intensity: f32 { unit: "factor", min: 0.0 },
    }
}

crate::reflect! {
    enum ToneMapper { Linear, Reinhard, Aces }
}

crate::reflect! {
    struct ToneMapping {
        mapper: ToneMapper,
        exposure: f32 { unit: "factor", min: 0.001 },
    }
}

crate::reflect! {
    enum RenderBounds {
        Sphere {
            center: [f32; 3] { unit: "m" },
            radius: f32 { unit: "m", min: 0.0 },
        },
        Aabb {
            min: [f32; 3] { unit: "m", doc: "<= max" },
            max: [f32; 3] { unit: "m", doc: ">= min" },
        },
    }
}

crate::reflect! {
    enum PhysicsSyncMode { None, Events, SelectedState, FullState }
}

crate::reflect! {
    struct AutoSimulation {
        #[skip] decision: Option<crate::runtime::AllocationDecision>,
    }
}

crate::reflect! {
    struct PlayerController {
        walk_speed: f32 { unit: "m/s", min: 0.0 },
        sprint_multiplier: f32 { unit: "factor", min: 1.0 },
        jump_speed: f32 { unit: "m/s", min: 0.0 },
        gravity: f32 { unit: "m/s²", min: 0.0 },
        look_sensitivity: f32 { unit: "rad/pixel", min: 0.0 },
        collision_mask: u32 {
            unit: "bitmask", doc: "layers the body stops against",
        },
        yaw: f32 { unit: "rad", doc: "0 looks toward -Z" },
        pitch: f32 { unit: "rad", min: -1.55, max: 1.55 },
        camera_distance: f32 {
            unit: "m", min: 0.0,
            doc: "0 is first person; above 0 the camera orbits behind",
        },
        camera_height: f32 {
            unit: "m", doc: "third-person orbit center above the body",
        },
        #[skip] vertical_speed: f32,
        #[skip] grounded: bool,
        #[skip] jump_requested: bool,
    }
}

crate::reflect! {
    struct JointSpring {
        target: f32 { unit: "m or rad" },
        stiffness: f32 { unit: "N/m or N·m/rad", min: 0.0 },
        damping: f32 { unit: "N·s/m or N·m·s/rad", min: 0.0 },
    }
}

crate::reflect! {
    struct JointMotor {
        speed: f32 { unit: "m/s or rad/s" },
        max_force: f32 { unit: "N or N·m", min: 0.0 },
    }
}

crate::reflect! {
    enum AxisMotion {
        Locked,
        Free,
        Limited {
            min: f32 { unit: "m or rad" },
            max: f32 { unit: "m or rad" },
        },
    }
}

crate::reflect! {
    struct JointAxis {
        motion: AxisMotion,
        spring: Option<JointSpring>,
        motor: Option<JointMotor>,
    }
}

crate::reflect! {
    enum JointKind {
        Fixed,
        Hinge {
            limit: Option<[f32; 2]> { unit: "rad", doc: "min, max" },
            spring: Option<JointSpring>,
            motor: Option<JointMotor>,
        },
        Slider {
            limit: Option<[f32; 2]> { unit: "m", doc: "min, max" },
            spring: Option<JointSpring>,
            motor: Option<JointMotor>,
        },
        BallSocket,
        ConeTwist {
            swing: f32 { unit: "rad", min: 0.0 },
            twist: [f32; 2] { unit: "rad", doc: "min, max about X" },
        },
        Distance {
            min: f32 { unit: "m", min: 0.0 },
            max: f32 { unit: "m", min: 0.0 },
        },
        Spring {
            rest_length: f32 { unit: "m", min: 0.0 },
            stiffness: f32 { unit: "N/m", min: 0.0 },
            damping: f32 { unit: "N·s/m", min: 0.0 },
        },
        Generic {
            linear: [JointAxis; 3] { doc: "X, Y, Z along the target frame" },
            angular: [JointAxis; 3] { doc: "twist about X, swing about Y, Z" },
        },
    }
}

crate::reflect! {
    struct Joint {
        target: Entity { doc: "body this one hangs from; null is the world" },
        kind: JointKind,
        anchor: [f32; 3] { unit: "m", doc: "in this body's local space" },
        frame: [f32; 3] {
            unit: "rad", doc: "joint axes in this body's local space; X is the hinge, slider, and twist axis",
        },
        target_anchor: [f32; 3] {
            unit: "m", doc: "in the target's local space, or world space",
        },
        target_frame: [f32; 3] { unit: "rad", doc: "in the target's local space" },
        collide_connected: bool { doc: "false keeps the two bodies from colliding" },
        break_force: f32 { unit: "N", min: 0.0, doc: "0 never breaks" },
        break_torque: f32 { unit: "N·m", min: 0.0, doc: "0 never breaks" },
    }
}

crate::reflect! {
    struct Articulation {}
}

crate::reflect! {
    enum Easing {
        Linear, QuadIn, QuadOut, QuadInOut, CubicOut, SineInOut, BackOut,
        BounceOut,
    }
}

crate::reflect! {
    enum TweenProperty { Position, Rotation, Scale }
}

crate::reflect! {
    enum TweenRepeat { Once, Loop, PingPong }
}

crate::reflect! {
    struct Tween {
        property: TweenProperty { doc: "rotation is radians" },
        from: [f32; 3] { unit: "property unit" },
        to: [f32; 3] { unit: "property unit" },
        duration: f32 { unit: "s", min: 0.0, doc: "0 jumps to `to`" },
        delay: f32 { unit: "s", min: 0.0, doc: "holds `from` first" },
        easing: Easing,
        repeat: TweenRepeat,
        #[skip] elapsed: f32,
    }
}

crate::reflect! {
    struct SoundCue {
        clip: String { unit: "asset path", doc: "relative to assets/" },
        volume: f32 { unit: "linear gain", min: 0.0 },
        on_collision: bool { doc: "CPU collider contacts only" },
        #[skip] triggered: bool,
        #[skip] touching: bool,
    }
}

crate::reflect! {
    struct BurstEmitter {
        count: u32 { unit: "particles", doc: "each is one CPU entity" },
        speed: f32 { unit: "m/s", min: 0.0 },
        lifetime: f32 { unit: "s", min: 0.0 },
        particle_scale: f32 {
            unit: "factor", min: 0.0, doc: "of the emitter's scale",
        },
        gravity: f32 { unit: "m/s²" },
        on_collision: bool { doc: "CPU collider contacts only" },
        #[skip] triggered: bool,
        #[skip] touching: bool,
    }
}

crate::reflect! {
    enum HudAnchor {
        TopLeft, Top, TopRight, Center, BottomLeft, Bottom, BottomRight,
    }
}

crate::reflect! {
    struct HudElement {
        text: String { doc: "{name} shows the counter with that name" },
        anchor: HudAnchor,
        offset: [f32; 2] {
            unit: "logical pixels",
            doc: "from the anchor toward the screen center",
        },
        font_size: f32 { unit: "logical pixels", min: 1.0 },
        color: [f32; 4] { unit: "sRGBA", min: 0.0, max: 1.0, color: true },
        button: bool { doc: "a click sends HudButtonPressed" },
        requires: Option<String> {
            unit: "counter name",
            doc: "null, or shown only once that counter is complete",
        },
    }
}

crate::reflect! {
    struct Counter {
        name: String { doc: "the lowest-ID counter wins when names repeat" },
        value: i32,
        target: Option<i32> {
            doc: "null, or the value that completes the counter",
        },
    }
}

crate::reflect! {
    struct Pickup {
        counter: String { unit: "counter name" },
        value: i32 { doc: "added on collect" },
        requires: Option<String> {
            unit: "counter name",
            doc: "null, or collectable only once that counter is complete",
        },
        collected: bool { doc: "set by the engine; scenarios can check it" },
    }
}

crate::reflect! {
    struct Connection {
        handler: String {
            doc: "name the game registered with App::add_signal_handler",
        },
        target: Entity { doc: "object passed to the handler as the receiver" },
    }
}

crate::reflect! {
    struct Connections {
        list: Vec<Connection> {
            doc: "signal handlers run, in order, when this object emits",
        },
    }
}

crate::reflect! {
    struct SceneInstance {
        source: PathBuf {
            unit: "scene file",
            doc: "placed under this object on load, relative to this scene; \
                  empty places nothing",
        },
    }
}

crate::reflect! {
    struct SceneBackground {
        color: [f32; 4] {
            unit: "linear RGBA", min: 0.0, max: 1.0, color: true,
        },
    }
}

crate::reflect! {
    struct TileKind {
        color: [f32; 4] {
            unit: "linear RGBA", min: 0.0, max: 1.0, color: true,
            doc: "multiplies the texture",
        },
        texture: Option<PathBuf> {
            unit: "asset path",
            doc: "null or an image; alpha below 0.5 is cut out",
        },
        solid: bool { doc: "blocks CPU colliders" },
    }
}

crate::reflect! {
    struct TileMap {
        tile_size: f32 { unit: "m", min: 0.001 },
        rows: Vec<String> {
            doc: "one string per row, top first; characters without a kind are empty",
        },
        tiles: std::collections::BTreeMap<String, TileKind> {
            doc: "maps a one-character string to its kind",
        },
    }
}

crate::reflect! {
    struct PlatformerController {
        run_speed: f32 { unit: "m/s", min: 0.0 },
        jump_speed: f32 { unit: "m/s", min: 0.0 },
        gravity: f32 { unit: "m/s²", min: 0.0 },
        collision_mask: u32 {
            unit: "bitmask", doc: "layers the body stops against",
        },
        #[skip] vertical_speed: f32,
        #[skip] grounded: bool,
        #[skip] jump_buffer: f32,
        #[skip] air_time: f32,
    }
}

crate::reflect! {
    enum QualityProfile { Auto, Eco, Balanced, High }
}

crate::reflect! {
    enum CullingMode { Auto, Disabled, Frustum, FrustumAndOcclusion }
}

crate::reflect! {
    enum Antialiasing { Auto, Off, Msaa2, Msaa4 }
}

crate::reflect! {
    enum ShadowQuality { Auto, Low, Medium, High }
}

crate::reflect! {
    struct RenderSettings {
        quality: QualityProfile,
        vsync: bool,
        limit_fps: bool,
        max_fps: u32 { unit: "frames/s", min: 1.0 },
        render_scale: f32 { unit: "factor", min: 0.25, max: 2.0 },
        background_color: [f32; 4] {
            unit: "linear RGBA", min: 0.0, max: 1.0, color: true,
        },
        culling: CullingMode,
        antialiasing: Antialiasing,
        shadows: ShadowQuality,
    }
}

crate::reflect! {
    struct PhysicsSettings {
        gravity: [f32; 3] { unit: "m/s²" },
        enabled: bool,
    }
}

crate::reflect! {
    struct RandomSeed(u64)
}

crate::reflect! {
    enum DeterminismMode { Off, Local, CrossPlatform }
}

crate::reflect! {
    enum MaterialModel { Pbr, Unlit }
}

crate::reflect! {
    enum AlphaMode {
        Opaque,
        Mask { cutoff: f32 { min: 0.0, max: 1.0 } },
        Blend,
    }
}

crate::reflect! {
    struct MaterialAsset {
        model: MaterialModel,
        alpha_mode: AlphaMode,
        base_color: [f32; 4] {
            unit: "linear RGBA", min: 0.0, max: 1.0, color: true,
        },
        emissive: [f32; 3] { unit: "linear RGB", min: 0.0, color: true },
        metallic: f32 { min: 0.0, max: 1.0 },
        roughness: f32 { min: 0.0, max: 1.0 },
        base_color_texture: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
        normal_texture: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
        metallic_roughness_texture: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
        occlusion_texture: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
        emissive_texture: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
    }
}

pub(super) fn register(registry: &mut TypeRegistry) {
    registry.register_resource::<RenderSettings>("rusting.render_settings");
    registry.register_resource::<PhysicsSettings>("rusting.physics_settings");
    registry.register_resource::<RandomSeed>("rusting.random_seed");
    registry.register_resource::<DeterminismMode>("rusting.determinism");
    registry.register_asset::<MaterialAsset>("rusting.material");
}
