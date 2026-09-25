//! Material system for creating visually appealing objects without textures.
//!
//! Materials control the surface appearance of objects including color,
//! roughness, metalness, and lighting model.

use crate::assets::MaterialModel;

/// Core material definition that determines how an object appears when rendered.
///
/// Materials are lightweight and cheap to clone. They can be shared between
/// multiple objects.
///
/// # Examples
///
/// ```
/// use rusting_engine::{Material, MaterialModel};
///
/// // Create a red, metallic material
/// let metal = Material::standard()
///     .color([1.0, 0.0, 0.0])
///     .metalness(0.9)
///     .roughness(0.3)
///     .build();
///
/// // Create an emissive glowing material
/// let glow = Material::standard()
///     .color([0.0, 1.0, 0.0])
///     .emissive(2.0)
///     .model(MaterialModel::Unlit)
///     .build();
/// ```
#[derive(Clone, Debug)]
pub struct Material {
    /// RGB color values in linear space (0.0 - 1.0 range)
    pub color: [f32; 3],

    /// Self-illumination strength. Values > 0.0 make the material glow.
    pub emissive: f32,

    /// Surface micro-roughness (0.0 = mirror smooth, 1.0 = completely rough)
    pub roughness: f32,

    /// How metallic the surface appears (0.0 = dielectric, 1.0 = pure metal)
    pub metalness: f32,

    /// Lighting model; the renderer picks the shader variant from it.
    pub model: MaterialModel,

    /// Optional texture ID for the base color/albedo map
    pub base_color_texture: Option<usize>,

    /// Optional texture ID for the metallic/roughness map
    pub metallic_roughness_texture: Option<usize>,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            color: [1.0, 1.0, 1.0],
            emissive: 0.0,
            roughness: 0.5,
            metalness: 0.0,
            model: MaterialModel::Pbr,
            base_color_texture: None,
            metallic_roughness_texture: None,
        }
    }
}

impl Material {
    /// Creates a new [`MaterialBuilder`] with default values.
    ///
    /// This is the recommended way to create custom materials.
    ///
    /// # Example
    ///
    /// ```
    /// # use rusting_engine::Material;
    /// let material = Material::standard()
    ///     .color([0.2, 0.5, 1.0])
    ///     .roughness(0.2)
    ///     .build();
    /// ```
    pub fn standard() -> MaterialBuilder {
        MaterialBuilder::default()
    }
}

/// Builder pattern for creating [`Material`] instances.
///
/// Provides a fluent interface for setting material properties before
/// finalizing with [`build()`](MaterialBuilder::build).
///
/// # Example
///
/// ```
/// # use rusting_engine::Material;
/// let shiny_plastic = Material::standard()
///     .color([0.8, 0.1, 0.8])
///     .roughness(0.4)
///     .metalness(0.0)
///     .build();
/// ```
pub struct MaterialBuilder {
    color: [f32; 3],
    emissive: f32,
    roughness: f32,
    metalness: f32,
    model: MaterialModel,
    base_color_texture: Option<usize>,
    metallic_roughness_texture: Option<usize>,
}

impl Default for MaterialBuilder {
    fn default() -> Self {
        let material = Material::default();
        Self {
            color: material.color,
            emissive: material.emissive,
            roughness: material.roughness,
            metalness: material.metalness,
            model: material.model,
            base_color_texture: material.base_color_texture,
            metallic_roughness_texture: material.metallic_roughness_texture,
        }
    }
}

impl MaterialBuilder {
    /// Sets the base color of the material.
    ///
    /// Values are in linear RGB space, typically ranging from 0.0 to 1.0.
    ///
    /// # Arguments
    ///
    /// * `c` - RGB color as `[red, green, blue]` where each component is 0.0-1.0
    ///
    /// # Example
    ///
    /// ```
    /// # use rusting_engine::Material;
    /// let red = Material::standard()
    ///     .color([1.0, 0.0, 0.0])  // Pure red
    ///     .build();
    ///
    /// let cyan = Material::standard()
    ///     .color([0.0, 1.0, 1.0])  // Cyan
    ///     .build();
    /// ```
    pub fn color(mut self, c: [f32; 3]) -> Self {
        self.color = c;
        self
    }

    /// Sets the emissive (self-illumination) strength.
    ///
    /// Values > 0.0 cause the material to appear to emit light.
    /// Common range: 0.0 (no emission) to 2.0 (strong glow).
    ///
    /// # Example
    ///
    /// ```
    /// # use rusting_engine::Material;
    /// let neon_sign = Material::standard()
    ///     .color([1.0, 0.0, 0.0])
    ///     .emissive(1.5)  // Glowing red
    ///     .build();
    /// ```
    pub fn emissive(mut self, e: f32) -> Self {
        self.emissive = e;
        self
    }

    /// Sets the surface roughness (micro-scale surface variation).
    ///
    /// * `0.0` - Perfect mirror/smooth surface (sharp reflections)
    /// * `0.5` - Slightly rough (blurry reflections)
    /// * `1.0` - Completely rough (diffuse appearance)
    ///
    /// # Example
    ///
    /// ```
    /// # use rusting_engine::Material;
    /// let mirror = Material::standard()
    ///     .roughness(0.0)   // Perfect reflection
    ///     .build();
    ///
    /// let matte = Material::standard()
    ///     .roughness(0.9)   // Almost no reflection
    ///     .build();
    /// ```
    pub fn roughness(mut self, r: f32) -> Self {
        self.roughness = r;
        self
    }

    /// Sets the metalness of the material.
    ///
    /// * `0.0` - Dielectric (plastic, wood, stone) - uses diffuse color
    /// * `0.5` - Partially metallic
    /// * `1.0` - Pure metal (reflects specular color)
    ///
    /// # Example
    ///
    /// ```
    /// # use rusting_engine::Material;
    /// let gold = Material::standard()
    ///     .color([1.0, 0.8, 0.0])
    ///     .metalness(1.0)   // Pure metal
    ///     .roughness(0.3)   // Polished
    ///     .build();
    /// ```
    pub fn metalness(mut self, m: f32) -> Self {
        self.metalness = m;
        self
    }

    /// Sets the lighting model.
    ///
    /// # Example
    ///
    /// ```
    /// # use rusting_engine::{Material, MaterialModel};
    /// let unlit = Material::standard()
    ///     .model(MaterialModel::Unlit)  // No lighting calculations
    ///     .build();
    /// ```
    pub fn model(mut self, model: MaterialModel) -> Self {
        self.model = model;
        self
    }

    /// Sets the base color texture ID.
    pub fn base_color_texture(mut self, tex_id: usize) -> Self {
        self.base_color_texture = Some(tex_id);
        self
    }

    /// Sets the metallic/roughness texture ID.
    pub fn metallic_roughness_texture(mut self, tex_id: usize) -> Self {
        self.metallic_roughness_texture = Some(tex_id);
        self
    }

    /// Finalizes the builder and creates the [`Material`].
    ///
    /// # Example
    ///
    /// ```
    /// # use rusting_engine::Material;
    /// let material = Material::standard()
    ///     .color([0.2, 0.5, 0.8])
    ///     .build();  // Consumes the builder
    /// ```
    pub fn build(self) -> Material {
        Material {
            color: self.color,
            emissive: self.emissive,
            roughness: self.roughness,
            metalness: self.metalness,
            model: self.model,
            base_color_texture: self.base_color_texture,
            metallic_roughness_texture: self.metallic_roughness_texture,
        }
    }
}
