"""Rebuild the small skinned glTF bar fixture with Python's standard library.

A thin bar two meters tall, skinned to two joints: Root at its foot and Tip
one meter up. The middle ring of vertices is split evenly between them. The
"bend" animation turns Tip a quarter turn around Z in one second, so the top
half folds over. `rusting scene add-model` keeps the skin as `rusting.skin`
and the clip as `rusting.animation`. Run this file from any directory.
"""

import base64
import json
import math
import struct
from pathlib import Path

DESTINATION = Path(__file__).with_name("bending_bar.gltf")

# Three rings of four corners at heights 0, 1 and 2; flat normals come from
# the importer.
square = [(-0.1, -0.1), (0.1, -0.1), (0.1, 0.1), (-0.1, 0.1)]
positions = [c for y in (0, 1, 2) for x, z in square for c in (x, y, z)]
indices = []
for ring in (0, 1):
    for side in range(4):
        a, b = ring * 4 + side, ring * 4 + (side + 1) % 4
        indices += [a, b, b + 4, a, b + 4, a + 4]
indices += [0, 2, 1, 0, 3, 2, 8, 9, 10, 8, 10, 11]
joints = [j for ring in (0, 1, 2) for _ in square for j in (0, 1, 0, 0)]
weights = [
    w
    for ring_weights in ((1, 0), (0.5, 0.5), (0, 1))
    for _ in square
    for w in (*ring_weights, 0, 0)
]
# Column-major: Root binds at the origin, Tip one meter up.
inverse_bind = [
    *(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1),
    *(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, -1, 0, 1),
]
times = [0.0, 1.0]
rotations = [0, 0, 0, 1, 0, 0, math.sin(math.pi / 4), math.cos(math.pi / 4)]

buffer = bytearray()
views, accessors = [], []
for values, fmt, component, count, shape, extra in (
    (positions, "f", 5126, 12, "VEC3", {"min": [-0.1, 0, -0.1], "max": [0.1, 2, 0.1]}),
    (indices, "H", 5123, len(indices), "SCALAR", {}),
    (joints, "B", 5121, 12, "VEC4", {}),
    (weights, "f", 5126, 12, "VEC4", {}),
    (inverse_bind, "f", 5126, 2, "MAT4", {}),
    (times, "f", 5126, 2, "SCALAR", {"min": [0], "max": [1]}),
    (rotations, "f", 5126, 2, "VEC4", {}),
):
    while len(buffer) % 4:
        buffer.append(0)
    offset = len(buffer)
    buffer.extend(struct.pack("<" + str(len(values)) + fmt, *values))
    views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(buffer) - offset})
    accessors.append({
        "bufferView": len(views) - 1,
        "componentType": component,
        "count": count,
        "type": shape,
        **extra,
    })

document = {
    "asset": {"version": "2.0", "generator": "RustingEngine bending bar fixture"},
    "scene": 0,
    "scenes": [{"name": "Bar", "nodes": [0]}],
    "nodes": [
        {"name": "Rig", "children": [1, 2]},
        {"name": "Body", "mesh": 0, "skin": 0},
        {"name": "Root", "children": [3]},
        {"name": "Tip", "translation": [0, 1, 0]},
    ],
    "meshes": [{"name": "Bar", "primitives": [{
        "attributes": {"POSITION": 0, "JOINTS_0": 2, "WEIGHTS_0": 3},
        "indices": 1,
    }]}],
    "skins": [{"joints": [2, 3], "inverseBindMatrices": 4, "skeleton": 2}],
    "animations": [{
        "name": "bend",
        "samplers": [{"input": 5, "output": 6, "interpolation": "LINEAR"}],
        "channels": [{"sampler": 0, "target": {"node": 3, "path": "rotation"}}],
    }],
    "buffers": [{
        "uri": "data:application/octet-stream;base64," + base64.b64encode(buffer).decode("ascii"),
        "byteLength": len(buffer),
    }],
    "bufferViews": views,
    "accessors": accessors,
}

DESTINATION.write_text(json.dumps(document, indent=2) + "\n")
print(DESTINATION)
