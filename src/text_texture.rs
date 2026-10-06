//! Text drawn into a texture on the CPU, for signs, labels and monitor
//! overlays that sit on a mesh in the 3D world.
//!
//! Glyphs come from egui's built-in fonts, so this needs the `ui` feature.
//! The result is an ordinary [`TextureAsset`]: insert it into the texture
//! assets and use it as a material's base color, and the text tilts, lights
//! and fogs with the mesh like any other texture.

use crate::assets::{
    TextureAsset, TextureColorSpace, TextureFilter, TextureSampler, TextureWrap,
};

/// How [`text_texture`] draws a block of text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Glyph height in texels.
    pub size: f32,
    /// Text color as sRGB with alpha.
    pub color: [u8; 4],
    /// Fill behind the text as sRGB with alpha.
    pub background: [u8; 4],
    /// Empty texels around the text on every side.
    pub padding: u32,
    /// Monospace instead of the proportional font.
    pub monospace: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: 32.0,
            color: [255, 255, 255, 255],
            background: [0, 0, 0, 255],
            padding: 8,
            monospace: false,
        }
    }
}

/// Draws `text` (lines split on `\n`, no wrapping) into a new sRGB texture
/// just large enough to hold it plus the padding.
pub fn text_texture(text: &str, style: TextStyle) -> TextureAsset {
    let ctx = egui::Context::default();
    let font = if style.monospace {
        egui::FontId::monospace(style.size)
    } else {
        egui::FontId::proportional(style.size)
    };
    let mut galley = None;
    // Fonts exist only inside a pass; one empty pass at one pixel per point
    // lays the text out with atlas texels equal to texture texels.
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        galley = Some(ctx.fonts(|fonts| {
            fonts.layout_no_wrap(
                text.to_owned(),
                font.clone(),
                egui::Color32::WHITE,
            )
        }));
    });
    let galley = galley.expect("egui ran the pass");
    let atlas = ctx.fonts(|fonts| fonts.image());

    let pad = style.padding;
    let width = galley.size().x.ceil() as u32 + 2 * pad;
    let height = galley.size().y.ceil() as u32 + 2 * pad;
    let (width, height) = (width.max(1), height.max(1));
    let mut rgba8 = style.background.repeat((width * height) as usize);

    for row in &galley.rows {
        for glyph in &row.glyphs {
            let uv = glyph.uv_rect;
            if uv.is_nothing() {
                continue;
            }
            let left = (glyph.pos.x + uv.offset.x).round() as i64 + pad as i64;
            let top = (glyph.pos.y + uv.offset.y).round() as i64 + pad as i64;
            for v in uv.min[1]..uv.max[1] {
                for u in uv.min[0]..uv.max[0] {
                    let x = left + (u - uv.min[0]) as i64;
                    let y = top + (v - uv.min[1]) as i64;
                    if x < 0 || y < 0 || x >= width as i64 || y >= height as i64
                    {
                        continue;
                    }
                    let coverage =
                        atlas.pixels[v as usize * atlas.size[0] + u as usize];
                    let at = (y as usize * width as usize + x as usize) * 4;
                    for (channel, fg) in style.color.iter().enumerate() {
                        let bg = rgba8[at + channel] as f32;
                        let blend = coverage * style.color[3] as f32 / 255.0;
                        rgba8[at + channel] =
                            (bg + (*fg as f32 - bg) * blend).round() as u8;
                    }
                }
            }
        }
    }

    TextureAsset {
        size: [width, height],
        rgba8,
        color_space: TextureColorSpace::Srgb,
        sampler: TextureSampler {
            wrap: [TextureWrap::ClampToEdge; 2],
            mipmap_filter: TextureFilter::Linear,
            ..TextureSampler::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(texture: &TextureAsset) -> usize {
        texture
            .rgba8
            .chunks(4)
            .filter(|pixel| pixel[0] > 128)
            .count()
    }

    #[test]
    fn text_draws_ink_inside_its_padding() {
        let style = TextStyle {
            padding: 4,
            ..TextStyle::default()
        };
        let texture = text_texture("CAM 3", style);
        let [width, height] = texture.size;
        assert!(width > 60 && height >= 32, "{width}x{height}");
        assert_eq!(texture.rgba8.len(), (width * height * 4) as usize);
        assert!(ink(&texture) > 100, "{} lit texels", ink(&texture));
        // The padding border stays background.
        for x in 0..width as usize {
            assert_eq!(&texture.rgba8[x * 4..x * 4 + 4], &[0, 0, 0, 255]);
        }
    }

    #[test]
    fn longer_and_multiline_text_grows_the_texture() {
        let style = TextStyle::default();
        let one = text_texture("REC", style);
        let wide = text_texture("REC REC REC", style);
        let tall = text_texture("REC\nREC", style);
        assert!(wide.size[0] > one.size[0] * 2);
        assert!(tall.size[1] > one.size[1] + 20);
        assert_eq!(text_texture("REC", style), one, "repeatable");
        assert_eq!(ink(&text_texture("", style)), 0);
    }
}
