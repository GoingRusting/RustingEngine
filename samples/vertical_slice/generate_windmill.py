"""Rebuild the small animated glTF windmill fixture with Python's standard library.

A tower with a rotor that turns once every four seconds (linear rotation keys)
and a flag that bobs (cubic spline translation keys). `rusting scene add-model`
turns the "spin" animation into a `rusting.animation` clip. Run this file from
any directory.
"""

import base64
import json
import math
import struct
from pathlib import Path

DESTINATION = Path(__file__).with_name("windmill.gltf")

# A unit cube with shared corners; flat normals come from the importer.
positions = [
    coordinate
    for x in (-1, 1)
    for y in (-1, 1)
    for z in (-1, 1)
    for coordinate in (x, y, z)
]
indices = [
    0, 1, 3, 0, 3, 2,  4, 6, 7, 4, 7, 5,  0, 4, 5, 0, 5, 1,
    2, 3, 7, 2, 7, 6,  0, 2, 6, 0, 6, 4,  1, 5, 7, 1, 7, 3,
]
times = [0.0, 1.0, 2.0, 3.0, 4.0]
# Quarter turns around Z: [x, y, z, w].
rotations = [
    value
    for step in range(5)
    for value in (0, 0, math.sin(step * math.pi / 4), math.cos(step * math.pi / 4))
]
flag_times = [0.0, 2.0, 4.0]
# (in-tangent, value, out-tangent) per key.
flag = [
    value
    for height in (1.6, 2.0, 1.6)
    for value in (0, 0, 0, 0.5, height, 0, 0, 0, 0)
]

buffer = bytearray()
views, accessors = [], []
for values, fmt, count, shape, extra in (
    (positions, "f", 8, "VEC3", {"min": [-1, -1, -1], "max": [1, 1, 1]}),
    (indices, "H", 36, "SCALAR", {}),
    (times, "f", 5, "SCALAR", {"min": [0], "max": [4]}),
    (rotations, "f", 5, "VEC4", {}),
    (flag_times, "f", 3, "SCALAR", {"min": [0], "max": [4]}),
    (flag, "f", 9, "VEC3", {}),
):
    while len(buffer) % 4:
        buffer.append(0)
    offset = len(buffer)
    buffer.extend(struct.pack("<" + str(len(values)) + fmt, *values))
    views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(buffer) - offset})
    accessors.append({
        "bufferView": len(views) - 1,
        "componentType": 5126 if fmt == "f" else 5123,
        "count": count,
        "type": shape,
        **extra,
    })

document = {
    "asset": {"version": "2.0", "generator": "RustingEngine windmill fixture"},
    "scene": 0,
    "scenes": [{"name": "Windmill", "nodes": [0]}],
    "nodes": [
        {"name": "Tower", "mesh": 0, "scale": [0.4, 1.5, 0.4], "children": [1, 2]},
        {"name": "Rotor", "mesh": 0, "translation": [0, 1.2, 1.2], "scale": [2.5, 0.15, 0.1]},
        {"name": "Flag", "mesh": 0, "translation": [0.5, 1.6, 0], "scale": [0.3, 0.1, 0.05]},
    ],
    "meshes": [{"name": "Cube", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
    "animations": [{
        "name": "spin",
        "samplers": [
            {"input": 2, "output": 3, "interpolation": "LINEAR"},
            {"input": 4, "output": 5, "interpolation": "CUBICSPLINE"},
        ],
        "channels": [
            {"sampler": 0, "target": {"node": 1, "path": "rotation"}},
            {"sampler": 1, "target": {"node": 2, "path": "translation"}},
        ],
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
