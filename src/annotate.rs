//! Annotated captures: boxes and short IDs drawn over a frame, with a
//! legend, so a vision model can name what it sees by persistent ID.

use bevy_ecs::prelude::{Entity, World};
use nalgebra::Vector3;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::runtime::{picking, GlobalTransform, MeshRenderer, Name, SceneId};
use crate::scenario::CameraView;
use crate::AssetServer;

/// One mesh entity as it appears in a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    /// First four hex digits of `id`; the label drawn on the frame.
    pub short_id: String,
    pub id: Uuid,
    pub name: Option<String>,
    /// Pixel box `[left, top, right, bottom]`, clipped to the frame.
    pub rect: [i32; 4],
}

impl Annotation {
    #[must_use]
    pub fn data(&self) -> Value {
        json!({"short_id": self.short_id, "id": self.id, "name": self.name, "rect": self.rect})
    }
}

/// Every mesh entity with a persistent ID whose projected bounds touch the
/// frame, ordered by ID. Bounds are the mesh's local box, so a rotated
/// object gets a looser box; hidden-behind-other-objects entities are kept.
pub fn annotations(world: &mut World, extent: [u32; 2]) -> Vec<Annotation> {
    match CameraView::active(world, extent) {
        Some(view) => annotations_from(world, &view),
        None => Vec::new(),
    }
}

/// [`annotations`] seen through one camera view.
pub fn annotations_from(
    world: &mut World,
    view: &CameraView,
) -> Vec<Annotation> {
    let extent = view.extent();
    let mut query =
        world.query::<(Entity, &SceneId, &MeshRenderer, &GlobalTransform)>();
    let found: Vec<_> = query
        .iter(world)
        .map(|(entity, id, mesh, transform)| {
            (entity, id.0, mesh.mesh, *transform)
        })
        .collect();
    let assets = world.resource::<AssetServer>();
    let mut notes: Vec<Annotation> = found
        .into_iter()
        .filter_map(|(entity, id, handle, transform)| {
            let (min, max) = assets.mesh_bounds(handle)?;
            let matrix = picking::matrix_from_array(transform.matrix);
            let mut rect = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            let mut any = false;
            for corner in 0..8 {
                let local = Vector3::new(
                    if corner & 1 == 0 { min[0] } else { max[0] },
                    if corner & 2 == 0 { min[1] } else { max[1] },
                    if corner & 4 == 0 { min[2] } else { max[2] },
                );
                let world_point = matrix * local.push(1.0);
                if let Some(p) =
                    view.project([world_point.x, world_point.y, world_point.z])
                {
                    any = true;
                    rect = [
                        rect[0].min(p[0]),
                        rect[1].min(p[1]),
                        rect[2].max(p[0]),
                        rect[3].max(p[1]),
                    ];
                }
            }
            let (w, h) = (extent[0] as f32, extent[1] as f32);
            if !any
                || rect[2] < 0.0
                || rect[3] < 0.0
                || rect[0] > w
                || rect[1] > h
            {
                return None;
            }
            Some(Annotation {
                short_id: id.simple().to_string()[..4].to_owned(),
                id,
                name: world.get::<Name>(entity).map(|name| name.0.clone()),
                rect: [
                    rect[0].max(0.0) as i32,
                    rect[1].max(0.0) as i32,
                    (rect[2].min(w - 1.0)) as i32,
                    (rect[3].min(h - 1.0)) as i32,
                ],
            })
        })
        .collect();
    notes.sort_by_key(|note| note.id);
    notes
}

/// 3x5 glyphs for `0-9a-f`, one row per `u8`, top bit left of three.
const GLYPHS: [[u8; 5]; 16] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
    [7, 5, 7, 5, 5],
    [6, 5, 6, 5, 6],
    [7, 4, 4, 4, 7],
    [6, 5, 5, 5, 6],
    [7, 4, 7, 4, 7],
    [7, 4, 7, 4, 4],
];

fn put(rgba: &mut [u8], extent: [u32; 2], x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= extent[0] as i32 || y >= extent[1] as i32 {
        return;
    }
    let at = (y as usize * extent[0] as usize + x as usize) * 4;
    rgba[at..at + 4].copy_from_slice(&color);
}

/// A bright color per ID, so neighbouring boxes differ.
fn color_of(id: Uuid) -> [u8; 4] {
    let bytes = id.as_bytes();
    let pick = |byte: u8| 80 + (byte % 4) * 58;
    [pick(bytes[0]), pick(bytes[1]), pick(bytes[2]), 255]
}

/// Draws each box one pixel wide and its short ID in a 2x scaled font on
/// a black strip at the box's top-left corner.
pub fn draw(rgba: &mut [u8], extent: [u32; 2], notes: &[Annotation]) {
    for note in notes {
        let color = color_of(note.id);
        let [left, top, right, bottom] = note.rect;
        for x in left..=right {
            put(rgba, extent, x, top, color);
            put(rgba, extent, x, bottom, color);
        }
        for y in top..=bottom {
            put(rgba, extent, left, y, color);
            put(rgba, extent, right, y, color);
        }
        let strip_width = note.short_id.len() as i32 * 8 + 1;
        for y in 0..12 {
            for x in 0..strip_width {
                put(rgba, extent, left + x, top + y, [0, 0, 0, 255]);
            }
        }
        text(rgba, extent, left + 1, top + 1, &note.short_id);
    }
}

/// Draws `label` (characters `0-9a-f`; others are skipped) in white at 2x
/// scale, 8 pixels per character.
pub fn text(rgba: &mut [u8], extent: [u32; 2], x: i32, y: i32, label: &str) {
    for (index, digit) in label.chars().enumerate() {
        let Some(glyph) = digit.to_digit(16).map(|d| GLYPHS[d as usize]) else {
            continue;
        };
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..3 {
                if bits >> (2 - column) & 1 == 1 {
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        put(
                            rgba,
                            extent,
                            x + index as i32 * 8 + column * 2 + dx,
                            y + row as i32 * 2 + dy,
                            [255, 255, 255, 255],
                        );
                    }
                }
            }
        }
    }
}

/// Frames from several ticks in one grid image, each labelled with its
/// tick on a black strip. Tiles are `tile_width` wide (nearest-neighbour
/// downscale) and the grid is as square as the frame count allows.
/// Returns the pixels and the image size.
#[must_use]
pub fn contact_sheet(
    frames: &[(u32, Vec<u8>)],
    extent: [u32; 2],
    tile_width: u32,
) -> (Vec<u8>, [u32; 2]) {
    let tile_width = tile_width.clamp(1, extent[0]);
    let tile_height = (extent[1] * tile_width / extent[0]).max(1);
    let columns = (frames.len() as f32).sqrt().ceil().max(1.0) as u32;
    let rows = (frames.len() as u32).div_ceil(columns).max(1);
    let size = [columns * tile_width, rows * tile_height];
    let mut sheet = [0u8, 0, 0, 255].repeat((size[0] * size[1]) as usize);
    for (index, (tick, rgba)) in frames.iter().enumerate() {
        let (left, top) = (
            (index as u32 % columns) * tile_width,
            (index as u32 / columns) * tile_height,
        );
        for y in 0..tile_height {
            for x in 0..tile_width {
                let from = ((y * extent[1] / tile_height) * extent[0]
                    + x * extent[0] / tile_width)
                    as usize
                    * 4;
                let to = (((top + y) * size[0]) + left + x) as usize * 4;
                sheet[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
            }
        }
        let label = tick.to_string();
        for y in 0..12 {
            for x in 0..label.len() as i32 * 8 + 1 {
                put(
                    &mut sheet,
                    size,
                    left as i32 + x,
                    top as i32 + y,
                    [0, 0, 0, 255],
                );
            }
        }
        text(&mut sheet, size, left as i32 + 1, top as i32 + 1, &label);
    }
    (sheet, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_outlines_the_box_and_writes_the_short_id() {
        let extent = [64, 64];
        let mut rgba = vec![9u8; 64 * 64 * 4];
        let id =
            Uuid::parse_str("1234abcd-0000-0000-0000-000000000000").unwrap();
        let note = Annotation {
            short_id: "1234".into(),
            id,
            name: None,
            rect: [10, 20, 50, 60],
        };
        draw(&mut rgba, extent, &[note]);
        let pixel = |x: usize, y: usize| rgba[(y * 64 + x) * 4..][..4].to_vec();
        assert_eq!(pixel(30, 60)[3], 255, "bottom edge");
        assert_eq!(pixel(50, 40), pixel(30, 60), "right edge, same color");
        assert_eq!(pixel(30, 40), vec![9; 4], "inside is untouched");
        // The `1` glyph's stem is white on the black strip.
        assert_eq!(pixel(10 + 1 + 2, 20 + 1), vec![255; 4]);
        assert_eq!(pixel(10 + 1, 20 + 1), vec![0, 0, 0, 255]);
    }

    #[test]
    fn annotations_box_a_visible_mesh_around_the_frame_center() {
        use crate::runtime::{Camera, Projection, RenderExtractPlugin};
        use crate::{App, Transform};
        use std::time::Duration;

        let mut app = App::new();
        app.add_plugin(crate::assets::AssetPlugin).unwrap();
        app.add_plugin(RenderExtractPlugin).unwrap();
        let (mesh, material) = {
            let assets = app.world().resource::<AssetServer>();
            (assets.fallback_mesh, assets.fallback_material)
        };
        app.spawn((
            Transform::default().with_position(0.0, 0.0, 5.0),
            Camera {
                projection: Projection::Perspective {
                    vertical_fov_radians: 1.0,
                    near: 0.1,
                    far: 100.0,
                },
                active: true,
                priority: 0,
                viewport: None,
            },
        ));
        let id = Uuid::new_v4();
        app.spawn((
            SceneId(id),
            Name("Crate".into()),
            Transform::default(),
            MeshRenderer {
                mesh,
                material,
                cast_shadows: true,
                receive_shadows: true,
            },
        ));
        app.update(Duration::ZERO).unwrap();
        let notes = annotations(app.world_mut(), [800, 600]);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, id);
        assert_eq!(notes[0].name.as_deref(), Some("Crate"));
        let [left, top, right, bottom] = notes[0].rect;
        assert!(left < 400 && right > 400 && top < 300 && bottom > 300);
        assert!(right - left < 400, "{:?}", notes[0].rect);
    }

    #[test]
    fn a_contact_sheet_tiles_frames_in_a_grid_with_tick_labels() {
        let extent = [40, 20];
        let red = [255u8, 0, 0, 255].repeat(40 * 20);
        let blue = [0u8, 0, 255, 255].repeat(40 * 20);
        let frames = [(1, red.clone()), (2, blue.clone()), (3, red)];
        let (sheet, size) = contact_sheet(&frames, extent, 20);
        // Three frames: two columns, two rows of 20x10 tiles.
        assert_eq!(size, [40, 20]);
        let pixel =
            |x: usize, y: usize| sheet[(y * 40 + x) * 4..][..4].to_vec();
        assert_eq!(pixel(35, 8), vec![0, 0, 255, 255], "second tile is blue");
        assert_eq!(pixel(15, 19), vec![255, 0, 0, 255], "third tile is red");
        assert_eq!(
            pixel(35, 18),
            vec![0, 0, 0, 255],
            "the empty slot stays blank"
        );
        assert_eq!(pixel(0, 0), vec![0, 0, 0, 255], "label strip");
    }
}
