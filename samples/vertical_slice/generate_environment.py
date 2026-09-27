"""Rebuild the small glTF environment fixture using Python's standard library.

The source geometry and two 2x2 PBR maps are generated here so the sample
has no download or external art dependency. Run this file from any directory.
"""

import base64
import json
import struct
import zlib
from pathlib import Path

DESTINATION = Path(__file__).with_name("environment.gltf")


def png(rows):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(
            ">I", zlib.crc32(kind + data)
        )

    raw = b"".join(b"\0" + b"".join(bytes(pixel) for pixel in row) for row in rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">2I5B", 2, 2, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def data_uri(mime, data):
    return f"data:{mime};base64,{base64.b64encode(data).decode('ascii')}"


# Each face has its own vertices for hard normals and full UV coordinates.
faces = [
    ((1, 0, 0), ((1, -1, -1), (1, -1, 1), (1, 1, 1), (1, 1, -1))),
    ((-1, 0, 0), ((-1, -1, 1), (-1, -1, -1), (-1, 1, -1), (-1, 1, 1))),
    ((0, 1, 0), ((-1, 1, -1), (1, 1, -1), (1, 1, 1), (-1, 1, 1))),
    ((0, -1, 0), ((-1, -1, 1), (1, -1, 1), (1, -1, -1), (-1, -1, -1))),
    ((0, 0, 1), ((1, -1, 1), (-1, -1, 1), (-1, 1, 1), (1, 1, 1))),
    ((0, 0, -1), ((-1, -1, -1), (1, -1, -1), (1, 1, -1), (-1, 1, -1))),
]
positions, normals, uvs, indices = [], [], [], []
for normal, corners in faces:
    first = len(positions) // 3
    for corner, uv in zip(corners, ((0, 0), (1, 0), (1, 1), (0, 1))):
        positions.extend(corner)
        normals.extend(normal)
        uvs.extend(uv)
    indices.extend(first + index for index in (0, 1, 2, 0, 2, 3))

buffer = bytearray()
views, accessors = [], []
for values, fmt, count, shape, minimum, maximum in (
    (positions, "f", 24, "VEC3", [-1, -1, -1], [1, 1, 1]),
    (normals, "f", 24, "VEC3", None, None),
    (uvs, "f", 24, "VEC2", None, None),
    (indices, "H", 36, "SCALAR", None, None),
):
    while len(buffer) % 4:
        buffer.append(0)
    offset = len(buffer)
    buffer.extend(struct.pack("<" + str(len(values)) + fmt, *values))
    views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(buffer) - offset})
    accessor = {
        "bufferView": len(views) - 1,
        "componentType": 5126 if fmt == "f" else 5123,
        "count": count,
        "type": shape,
    }
    if minimum is not None:
        accessor["min"], accessor["max"] = minimum, maximum
    accessors.append(accessor)

albedo = png([
    ((176, 184, 185, 255), (159, 171, 173, 255)),
    ((159, 171, 173, 255), (176, 184, 185, 255)),
])
surface = png([
    ((255, 195, 15, 255), (255, 205, 15, 255)),
    ((255, 205, 15, 255), (255, 195, 15, 255)),
])

materials = [
    {
        "name": "Textured concrete",
        "pbrMetallicRoughness": {
            "baseColorTexture": {"index": 0},
            "metallicRoughnessTexture": {"index": 1},
            "metallicFactor": 0.1,
            "roughnessFactor": 0.9,
        },
    },
    {
        "name": "Brushed steel",
        "pbrMetallicRoughness": {
            "baseColorFactor": [0.37, 0.48, 0.55, 1],
            "metallicFactor": 0.85,
            "roughnessFactor": 0.28,
        },
    },
    {
        "name": "Signal orange",
        "pbrMetallicRoughness": {
            "baseColorFactor": [0.95, 0.34, 0.08, 1],
            "metallicFactor": 0.18,
            "roughnessFactor": 0.42,
        },
    },
]

nodes = [
    {"name": "Floor", "mesh": 0, "translation": [0, -0.3, 0], "scale": [12, 0.25, 12]},
    {"name": "North wall", "mesh": 0, "translation": [0, 2, -12], "scale": [12, 2, 0.2]},
    {"name": "West wall", "mesh": 0, "translation": [-12, 2, 0], "scale": [0.2, 2, 12]},
    {"name": "Walkway", "mesh": 1, "translation": [0, 0.12, 0], "scale": [1.2, 0.12, 10]},
    {"name": "Signal block", "mesh": 2, "translation": [0, 1.3, -7], "scale": [1.5, 1.5, 0.6]},
]
for x in (-8, 8):
    for z in (-8, 0, 8):
        nodes.append({
            "name": f"Steel pillar {x} {z}", "mesh": 1,
            "translation": [x, 2, z], "scale": [0.35, 2, 0.35],
        })

document = {
    "asset": {"version": "2.0", "generator": "RustingEngine vertical slice fixture"},
    "scene": 0,
    "scenes": [{"name": "PBR courtyard", "nodes": list(range(len(nodes)))}],
    "nodes": nodes,
    "meshes": [
        {"name": material["name"], "primitives": [{
            "attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
            "indices": 3, "material": index,
        }]}
        for index, material in enumerate(materials)
    ],
    "materials": materials,
    "images": [
        {"uri": data_uri("image/png", albedo)},
        {"uri": data_uri("image/png", surface)},
    ],
    "textures": [{"source": 0}, {"source": 1}],
    "samplers": [{}],
    "buffers": [{"uri": data_uri("application/octet-stream", buffer), "byteLength": len(buffer)}],
    "bufferViews": views,
    "accessors": accessors,
}

DESTINATION.write_text(json.dumps(document, indent=2) + "\n")
print(DESTINATION)
