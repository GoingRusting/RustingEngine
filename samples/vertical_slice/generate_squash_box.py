"""Rebuild the small morph target glTF fixture with Python's standard library.

A one meter box with a flat hat on top, as two primitives of one mesh. Both
have two blend shapes: "squash" halves the height and widens the sides, and
"lean" slides the top two thirds of a meter along +X. The "wobble" animation
keys the weights [0, 0], [1, 0], [0, 1], [0, 0] one second apart.
`rusting scene add-model` keeps the weights as `rusting.morph` and the clip as
`rusting.animation`. Run this file from any directory.
"""

import base64
import json
import struct
from pathlib import Path

DESTINATION = Path(__file__).with_name("squash_box.gltf")


def box(low, high):
    """Positions, normals and indices of an axis-aligned box, four corners
    per face so the faces keep flat normals."""
    positions, normals, indices = [], [], []
    for axis in range(3):
        for sign in (-1, 1):
            u, v = [a for a in range(3) if a != axis]
            if sign < 0:
                u, v = v, u
            base = len(positions)
            for du, dv in ((0, 0), (1, 0), (1, 1), (0, 1)):
                corner = [0.0, 0.0, 0.0]
                corner[axis] = high[axis] if sign > 0 else low[axis]
                corner[u] = high[u] if du else low[u]
                corner[v] = high[v] if dv else low[v]
                positions.append(corner)
                normal = [0.0, 0.0, 0.0]
                normal[axis] = float(sign)
                normals.append(normal)
            indices += [base, base + 1, base + 2, base, base + 2, base + 3]
    return positions, normals, indices


def shapes(positions):
    """Offsets of the squash and lean shapes for each vertex."""
    squash = [[x * 0.3, -y * 0.5, z * 0.3] for x, y, z in positions]
    lean = [[y * 2 / 3, 0.0, 0.0] for _, y, _ in positions]
    return squash, lean


primitives = [box((-0.5, 0, -0.5), (0.5, 1, 0.5)), box((-0.3, 1, -0.3), (0.3, 1.1, 0.3))]
times = [0.0, 1.0, 2.0, 3.0]
weights = [0, 0, 1, 0, 0, 1, 0, 0]

buffer = bytearray()
views, accessors = [], []


def accessor(values, fmt, component, count, shape, extra=None):
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
        **(extra or {}),
    })
    return len(accessors) - 1


def vec3(points):
    flat = [c for point in points for c in point]
    bounds = {
        "min": [min(p[i] for p in points) for i in range(3)],
        "max": [max(p[i] for p in points) for i in range(3)],
    }
    return accessor(flat, "f", 5126, len(points), "VEC3", bounds)


mesh_primitives = []
for positions, normals, indices in primitives:
    squash, lean = shapes(positions)
    mesh_primitives.append({
        "attributes": {"POSITION": vec3(positions), "NORMAL": vec3(normals)},
        "indices": accessor(indices, "H", 5123, len(indices), "SCALAR"),
        "targets": [{"POSITION": vec3(squash)}, {"POSITION": vec3(lean)}],
    })
time_accessor = accessor(times, "f", 5126, len(times), "SCALAR", {"min": [0], "max": [3]})
weight_accessor = accessor(weights, "f", 5126, len(weights), "SCALAR")

document = {
    "asset": {"version": "2.0", "generator": "RustingEngine squash box fixture"},
    "scene": 0,
    "scenes": [{"name": "Box", "nodes": [0]}],
    "nodes": [{"name": "Box", "mesh": 0}],
    "meshes": [{
        "name": "Box",
        "primitives": mesh_primitives,
        "weights": [0, 0],
        "extras": {"targetNames": ["squash", "lean"]},
    }],
    "animations": [{
        "name": "wobble",
        "samplers": [{"input": time_accessor, "output": weight_accessor, "interpolation": "LINEAR"}],
        "channels": [{"sampler": 0, "target": {"node": 0, "path": "weights"}}],
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
