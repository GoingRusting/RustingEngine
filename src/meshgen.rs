//! Deterministic placeholder meshes: a preset name and a seed give the same
//! low-poly binary glTF on every machine, in the texture palette, so
//! `rusting asset generate <root> mesh "barrel 3"` puts a prop in a scene
//! that is not a grey cube. Meshes carry no normals; the importer shades
//! them flat, which is the style.

/// Preset names `synth` accepts.
pub const PRESETS: [&str; 5] = ["crate", "barrel", "rock", "tree", "gem"];

/// One coloured part of a mesh: indexed triangles, counter-clockwise seen
/// from outside, y up, metres.
struct Part {
    color: [f32; 3],
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

/// A closed surface of revolution: `profile` runs bottom to top as
/// `(radius, height)`, both ends on the axis. `sides` vertices per ring,
/// the first at `turn` of a full circle; `radius_at` may move each vertex
/// in or out (the rock's lumps).
fn lathe(
    profile: &[(f32, f32)],
    sides: u32,
    turn: f32,
    radius_at: impl Fn(u32, u32) -> f32,
) -> (Vec<[f32; 3]>, Vec<u32>) {
    let last = profile.len() as u32 - 1;
    let mut positions = vec![[0.0, profile[0].1, 0.0]];
    for (ring, &(radius, height)) in profile.iter().enumerate().skip(1) {
        let ring = ring as u32;
        if ring == last {
            break;
        }
        for side in 0..sides {
            let angle =
                std::f32::consts::TAU * (side as f32 / sides as f32 + turn);
            let radius = radius * radius_at(ring, side);
            positions.push([
                radius * angle.cos(),
                height,
                -radius * angle.sin(),
            ]);
        }
    }
    positions.push([0.0, profile[last as usize].1, 0.0]);
    let top = positions.len() as u32 - 1;
    let at = |ring: u32, side: u32| 1 + (ring - 1) * sides + side % sides;
    let mut indices = Vec::new();
    for side in 0..sides {
        indices.extend([0, at(1, side + 1), at(1, side)]);
        for ring in 1..last - 1 {
            let (a, b) = (at(ring, side), at(ring, side + 1));
            let (c, d) = (at(ring + 1, side), at(ring + 1, side + 1));
            indices.extend([a, b, d, a, d, c]);
        }
        indices.extend([at(last - 1, side), at(last - 1, side + 1), top]);
    }
    (positions, indices)
}

/// Seeded splitmix64 value in 0..1; never the global RNG.
fn hash(seed: u64, a: u32, b: u32) -> f32 {
    let mut z = seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(u64::from(a) << 32 | u64::from(b));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

fn parts(preset: &str, seed: u64) -> Option<Vec<Part>> {
    let color = crate::texgen::palette(seed);
    let part = |color, (positions, indices)| Part {
        color,
        positions,
        indices,
    };
    let round = |_, _| 1.0;
    let wood = [0.42, 0.29, 0.18];
    Some(match preset {
        // 1 m box on the ground.
        "crate" => {
            let r = std::f32::consts::FRAC_1_SQRT_2;
            let profile = [(0.0, 0.0), (r, 0.0), (r, 1.0), (0.0, 1.0)];
            vec![part(color, lathe(&profile, 4, 0.125, round))]
        }
        // Bulging twelve-sided barrel, 1.1 m tall.
        "barrel" => {
            let profile = [
                (0.0, 0.0),
                (0.36, 0.0),
                (0.42, 0.3),
                (0.44, 0.55),
                (0.42, 0.8),
                (0.36, 1.1),
                (0.0, 1.1),
            ];
            vec![part(color, lathe(&profile, 12, 0.0, round))]
        }
        // Lumpy boulder about 1 m across; the seed moves every lump.
        "rock" => {
            let profile = [
                (0.0, 0.0),
                (0.45, 0.05),
                (0.55, 0.3),
                (0.5, 0.55),
                (0.3, 0.75),
                (0.0, 0.8),
            ];
            let lump = |ring, side| 0.75 + 0.5 * hash(seed, ring, side);
            vec![part([0.5, 0.5, 0.52], lathe(&profile, 7, 0.0, lump))]
        }
        // Wooden trunk under two stacked cones, 3 m tall.
        "tree" => {
            let trunk = [(0.0, 0.0), (0.15, 0.0), (0.12, 1.0), (0.0, 1.0)];
            let lower = [(0.0, 0.8), (0.9, 0.8), (0.0, 2.2)];
            let upper = [(0.0, 1.7), (0.65, 1.7), (0.0, 3.0)];
            vec![
                part(wood, lathe(&trunk, 6, 0.0, round)),
                part(color, lathe(&lower, 7, 0.0, round)),
                part(color, lathe(&upper, 7, 0.5, round)),
            ]
        }
        // Six-sided pickup gem, 0.6 m tall, its base on the ground.
        "gem" => {
            let profile = [(0.0, 0.0), (0.25, 0.25), (0.15, 0.6), (0.0, 0.6)];
            vec![part(color, lathe(&profile, 6, 0.0, round))]
        }
        _ => return None,
    })
}

/// The binary glTF (`.glb`) for `preset`: one mesh with a primitive and an
/// unlit-looking rough material per part. `None` for an unknown preset.
#[must_use]
pub fn synth(preset: &str, seed: u64) -> Option<Vec<u8>> {
    let parts = parts(preset, seed)?;
    let mut bin = Vec::new();
    let (mut views, mut accessors, mut materials, mut primitives) =
        (vec![], vec![], vec![], vec![]);
    for (index, part) in parts.iter().enumerate() {
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        let start = bin.len();
        for position in &part.positions {
            for axis in 0..3 {
                min[axis] = min[axis].min(position[axis]);
                max[axis] = max[axis].max(position[axis]);
                bin.extend(position[axis].to_le_bytes());
            }
        }
        views.push(serde_json::json!({"buffer": 0, "byteOffset": start,
            "byteLength": bin.len() - start, "target": 34962}));
        let indices_start = bin.len();
        for index in &part.indices {
            bin.extend(index.to_le_bytes());
        }
        views
            .push(serde_json::json!({"buffer": 0, "byteOffset": indices_start,
            "byteLength": bin.len() - indices_start, "target": 34963}));
        accessors.push(serde_json::json!({"bufferView": 2 * index,
            "componentType": 5126, "count": part.positions.len(),
            "type": "VEC3", "min": min, "max": max}));
        accessors.push(serde_json::json!({"bufferView": 2 * index + 1,
            "componentType": 5125, "count": part.indices.len(),
            "type": "SCALAR"}));
        // glTF colour factors are linear; the palette is sRGB.
        let linear = part.color.map(|channel| channel.powf(2.2));
        materials.push(serde_json::json!({"pbrMetallicRoughness": {
            "baseColorFactor": [linear[0], linear[1], linear[2], 1.0],
            "metallicFactor": 0.0, "roughnessFactor": 0.85}}));
        primitives.push(serde_json::json!({
            "attributes": {"POSITION": 2 * index},
            "indices": 2 * index + 1, "material": index}));
    }
    let json = serde_json::json!({
        "asset": {"version": "2.0", "generator": "rusting meshgen"},
        "scene": 0, "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": preset}],
        "meshes": [{"name": preset, "primitives": primitives}],
        "materials": materials, "accessors": accessors,
        "bufferViews": views, "buffers": [{"byteLength": bin.len()}],
    });
    let mut json = serde_json::to_vec(&json).ok()?;
    // Chunks are 4-byte aligned: JSON pads with spaces, BIN with zeros.
    json.resize(json.len().next_multiple_of(4), b' ');
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut glb = Vec::with_capacity(total);
    glb.extend(b"glTF");
    glb.extend(2u32.to_le_bytes());
    glb.extend((total as u32).to_le_bytes());
    glb.extend((json.len() as u32).to_le_bytes());
    glb.extend(b"JSON");
    glb.extend(json);
    glb.extend((bin.len() as u32).to_le_bytes());
    glb.extend(b"BIN\0");
    glb.extend(bin);
    Some(glb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_closed_outward_seeded_meshes() {
        for preset in PRESETS {
            for part in parts(preset, 1).unwrap() {
                // Closed: every edge is used once in each direction.
                let mut edges = std::collections::HashMap::new();
                let mut volume = 0.0;
                for corner in part.indices.chunks(3) {
                    for k in 0..3 {
                        let edge = (corner[k], corner[(k + 1) % 3]);
                        *edges.entry(edge).or_insert(0) += 1;
                    }
                    let [a, b, c] = [0, 1, 2].map(|k| {
                        nalgebra::Vector3::from(
                            part.positions[corner[k] as usize],
                        )
                    });
                    volume += a.dot(&b.cross(&c)) / 6.0;
                }
                for (&(a, b), &count) in &edges {
                    assert_eq!(count, 1, "{preset}: edge {a}-{b} repeats");
                    assert!(edges.contains_key(&(b, a)), "{preset}: hole");
                }
                // Outward winding gives a positive volume.
                assert!(volume > 0.001, "{preset} is inside out: {volume}");
            }
            let one = synth(preset, 1).unwrap();
            assert_eq!(one, synth(preset, 1).unwrap(), "{preset} repeats");
            assert_ne!(one, synth(preset, 2).unwrap(), "{preset} seeds differ");
            assert_eq!(one.len() % 4, 0);
        }
        assert!(synth("teapot", 1).is_none());
    }

    #[cfg(feature = "gltf")]
    #[test]
    fn generated_meshes_load_as_gltf() {
        for preset in PRESETS {
            let glb = synth(preset, 3).unwrap();
            let (document, buffers, _) = gltf::import_slice(&glb).unwrap();
            let mesh = document.meshes().next().unwrap();
            for primitive in mesh.primitives() {
                let reader =
                    primitive.reader(|buffer| Some(&buffers[buffer.index()]));
                let count = reader.read_positions().unwrap().count();
                assert!(reader
                    .read_indices()
                    .unwrap()
                    .into_u32()
                    .all(|index| (index as usize) < count));
            }
        }
    }
}
