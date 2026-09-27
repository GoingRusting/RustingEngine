use crate::assets::MaterialModel;
use crate::core::material::Material;

#[test]
fn material_default_has_pbr_model() {
    assert_eq!(Material::default().model, MaterialModel::Pbr);
}

#[test]
fn material_builder_defaults_match_material_defaults() {
    let expected = Material::default();
    let actual = Material::standard().build();

    assert_eq!(actual.color, expected.color);
    assert_eq!(actual.emissive, expected.emissive);
    assert_eq!(actual.roughness, expected.roughness);
    assert_eq!(actual.metalness, expected.metalness);
    assert_eq!(actual.model, expected.model);
    assert_eq!(actual.base_color_texture, expected.base_color_texture);
    assert_eq!(
        actual.metallic_roughness_texture,
        expected.metallic_roughness_texture
    );
}

#[test]
fn material_builder_chaining_preserves_model() {
    let mat = Material::standard()
        .color([1.0, 0.0, 0.0])
        .roughness(0.8)
        .metalness(0.5)
        .model(MaterialModel::Unlit)
        .emissive(0.0)
        .build();

    assert_eq!(mat.model, MaterialModel::Unlit);
    assert_eq!(mat.color, [1.0, 0.0, 0.0]);
    assert_eq!(mat.roughness, 0.8);
    assert_eq!(mat.metalness, 0.5);
}

#[test]
fn material_builder_model_can_be_overridden() {
    let mat = Material::standard()
        .model(MaterialModel::Unlit)
        .model(MaterialModel::Pbr)
        .build();
    assert_eq!(mat.model, MaterialModel::Pbr);
}
