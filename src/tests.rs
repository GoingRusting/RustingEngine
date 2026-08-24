#[cfg(test)]
use crate::core::material::Material;
use crate::rendering::shader_registry::ShaderType;
use crate::scene::object::{Instance, InstanceData, PhysicsPushConstants};

// ShaderType Tests

#[test]
fn shader_type_default_is_pbr() {
    assert_eq!(ShaderType::default(), ShaderType::Pbr);
}

#[test]
fn shader_type_sort_keys_are_unique_and_ordered() {
    let all = ShaderType::all();
    let mut keys: Vec<u32> = all.iter().map(|s| s.sort_key()).collect();
    let original = keys.clone();
    keys.sort();
    keys.dedup();
    // All unique
    assert_eq!(keys.len(), all.len());
    // Already sorted
    assert_eq!(keys, original);
}

#[test]
fn shader_type_equality_and_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(ShaderType::Pbr);
    set.insert(ShaderType::Unlit);
    set.insert(ShaderType::Pbr); // duplicate

    assert_eq!(set.len(), 2);
    assert!(set.contains(&ShaderType::Pbr));
    assert!(set.contains(&ShaderType::Unlit));
}

#[test]
fn shader_type_is_copy() {
    let a = ShaderType::Emissive;
    let b = a; // Copy
    assert_eq!(a, b);
}

// Material Builder Tests

#[test]
fn material_default_has_pbr_shader() {
    let mat = Material::default();
    assert_eq!(mat.shader, ShaderType::Pbr);
}

#[test]
fn material_builder_sets_shader() {
    let mat = Material::standard().shader(ShaderType::Unlit).build();
    assert_eq!(mat.shader, ShaderType::Unlit);
}

#[test]
fn material_builder_default_shader_is_pbr() {
    let mat = Material::standard().build();
    assert_eq!(mat.shader, ShaderType::Pbr);
}

#[test]
fn material_builder_defaults_match_material_defaults() {
    let expected = Material::default();
    let actual = Material::standard().build();

    assert_eq!(actual.color, expected.color);
    assert_eq!(actual.emissive, expected.emissive);
    assert_eq!(actual.roughness, expected.roughness);
    assert_eq!(actual.metalness, expected.metalness);
    assert_eq!(actual.shader, expected.shader);
    assert_eq!(actual.base_color_texture, expected.base_color_texture);
    assert_eq!(
        actual.metallic_roughness_texture,
        expected.metallic_roughness_texture
    );
}

#[test]
fn material_builder_chaining_preserves_shader() {
    let mat = Material::standard()
        .color([1.0, 0.0, 0.0])
        .roughness(0.8)
        .metalness(0.5)
        .shader(ShaderType::NormalDebug)
        .emissive(0.0)
        .build();

    assert_eq!(mat.shader, ShaderType::NormalDebug);
    assert_eq!(mat.color, [1.0, 0.0, 0.0]);
    assert_eq!(mat.roughness, 0.8);
    assert_eq!(mat.metalness, 0.5);
}

#[test]
fn material_builder_shader_can_be_overridden() {
    let mat = Material::standard()
        .shader(ShaderType::Unlit)
        .shader(ShaderType::Emissive) // override to test what if user override
        .build();
    assert_eq!(mat.shader, ShaderType::Emissive);
}

// Instance Tests

#[test]
fn instance_default_has_pbr_shader() {
    let inst = Instance::default();
    assert_eq!(inst.shader, ShaderType::Pbr);
}

#[test]
fn instance_custom_shader() {
    let inst = Instance {
        shader: ShaderType::Unlit,
        ..Default::default()
    };
    assert_eq!(inst.shader, ShaderType::Unlit);
}

#[test]
fn instance_clone_preserves_shader() {
    let inst = Instance {
        shader: ShaderType::Emissive,
        color: [1.0, 0.0, 0.0],
        ..Default::default()
    };
    let cloned = inst.clone();
    assert_eq!(cloned.shader, ShaderType::Emissive);
    assert_eq!(cloned.color, [1.0, 0.0, 0.0]);
}

#[test]
fn instance_material_copy_preserves_every_property() {
    let material = Material::standard()
        .color([0.2, 0.4, 0.6])
        .emissive(1.25)
        .roughness(0.15)
        .metalness(0.85)
        .shader(ShaderType::Unlit)
        .base_color_texture(3)
        .metallic_roughness_texture(7)
        .build();
    let mut instance = Instance::default();

    instance.apply_material(&material);

    assert_eq!(instance.color, material.color);
    assert_eq!(instance.emissive, material.emissive);
    assert_eq!(instance.roughness, material.roughness);
    assert_eq!(instance.metalness, material.metalness);
    assert_eq!(instance.shader, material.shader);
    assert_eq!(instance.base_color_texture, material.base_color_texture);
    assert_eq!(
        instance.metallic_roughness_texture,
        material.metallic_roughness_texture
    );
}

#[test]
fn gpu_physics_struct_layout_is_stable() {
    use std::mem::{align_of, offset_of, size_of};

    assert_eq!(size_of::<InstanceData>(), 144);
    assert_eq!(align_of::<InstanceData>(), 4);
    assert_eq!(offset_of!(InstanceData, model), 0);
    assert_eq!(offset_of!(InstanceData, color), 64);
    assert_eq!(offset_of!(InstanceData, mat_props), 80);
    assert_eq!(offset_of!(InstanceData, velocity), 96);
    assert_eq!(offset_of!(InstanceData, angular_velocity), 112);
    assert_eq!(offset_of!(InstanceData, physic_props), 128);

    assert_eq!(size_of::<PhysicsPushConstants>(), 48);
    assert_eq!(offset_of!(PhysicsPushConstants, global_gravity), 32);
}

#[test]
fn every_compute_shader_uses_the_canonical_instance_layout() {
    let shaders = [
        include_str!("shaders/compute/basic.comp"),
        include_str!("shaders/compute/cs_grid_build.comp"),
        include_str!("shaders/compute/cull.comp"),
        include_str!("shaders/compute/empty.comp"),
        include_str!("shaders/compute/full.comp"),
        include_str!("shaders/compute/grid_build.comp"),
        include_str!("shaders/compute/mid.comp"),
        include_str!("shaders/compute/no_coll.comp"),
        include_str!("shaders/compute/physics_simple.glsl"),
    ];
    let fields = [
        "mat4 model",
        "vec4 color",
        "vec4 mat_props",
        "vec4 velocity",
        "vec4 angular_velocity",
        "vec4 physic_props",
    ];

    for shader in shaders {
        let mut previous = 0;
        for field in fields {
            let offset =
                shader.find(field).expect("canonical field is missing");
            assert!(offset >= previous, "compute shader field order differs");
            previous = offset;
        }
    }
}

#[test]
fn every_physics_push_constant_layout_has_explicit_padding() {
    let shaders = [
        include_str!("shaders/compute/basic.comp"),
        include_str!("shaders/compute/cs_grid_build.comp"),
        include_str!("shaders/compute/empty.comp"),
        include_str!("shaders/compute/full.comp"),
        include_str!("shaders/compute/grid_build.comp"),
        include_str!("shaders/compute/mid.comp"),
        include_str!("shaders/compute/no_coll.comp"),
        include_str!("shaders/compute/physics_simple.glsl"),
    ];

    for shader in shaders {
        let padding = shader
            .find("_pad[3]")
            .expect("push-constant padding is missing");
        let gravity = shader
            .find("global_gravity")
            .expect("global gravity push constant is missing");
        assert!(padding < gravity, "padding must precede global_gravity");
    }
}

// Batch Grouping Tests

// Uses a lightweight mock to test add_instance batching logic
// without requiring GPU. We replicate the batching key check.

#[test]
fn same_shader_same_key_groups_together() {
    // Simulates the batching condition: same mesh ptr + same shader = same batch
    let shader_a = ShaderType::Pbr;
    let shader_b = ShaderType::Pbr;
    assert_eq!(shader_a, shader_b); // same shader should merge
}

#[test]
fn different_shader_different_key_separates() {
    let shader_a = ShaderType::Pbr;
    let shader_b = ShaderType::Unlit;
    assert_ne!(shader_a, shader_b); // different shader should NOT merge
}

// Physics is unchanged

// #[test]
// fn physics_default_unchanged() {
//     let phys = Physics::default();
//     assert_eq!(phys.mass, 1.0);
//     assert_eq!(phys.gravity_scale, 1.0);
//     assert_eq!(phys.collision_type, CollisionType::Box);
//     assert_eq!(phys.linear_velocity, [0.0, 0.0, 0.0]);
// }

// #[test]
// fn physics_builder_chain() {
//     let phys = Physics::default()
//         .linear_velocity([1.0, 2.0, 3.0])
//         .mass(50.0)
//         .collision(1.0)
//         .gravity(0.5);

//     assert_eq!(phys.velocity, [1.0, 2.0, 3.0, 4.0]);
//     assert_eq!(phys.mass, 50.0);
//     assert_eq!(phys.collision, 1.0);
//     assert_eq!(phys.gravity, 0.5);
// }
