#!/usr/bin/env python3
"""Split disconnected four-corner wheel meshes in a Sketchfab glTF.

The supplied Marussia and Pagani assets pack all four copies of each tyre/rim
layer into one primitive.  Bevy cannot give those copies independent pivots.
This utility keeps every original vertex attribute and material, but writes four
small index accessors and four semantic nodes per layer.  It is intentionally
idempotent so the source archive can be re-imported and processed again.
"""

from __future__ import annotations

import argparse
import copy
import json
import struct
from pathlib import Path


COMPONENT_FORMAT = {5121: "B", 5123: "H", 5125: "I", 5126: "f"}
COMPONENT_SIZE = {key: struct.calcsize("<" + value) for key, value in COMPONENT_FORMAT.items()}


def matrix_multiply(a: list[list[float]], b: list[list[float]]) -> list[list[float]]:
    return [
        [sum(a[row][k] * b[k][column] for k in range(4)) for column in range(4)]
        for row in range(4)
    ]


def node_matrix(node: dict) -> list[list[float]]:
    if "matrix" in node:
        values = node["matrix"]
        return [[values[column * 4 + row] for column in range(4)] for row in range(4)]

    translation = node.get("translation", [0.0, 0.0, 0.0])
    scale = node.get("scale", [1.0, 1.0, 1.0])
    x, y, z, w = node.get("rotation", [0.0, 0.0, 0.0, 1.0])
    result = [
        [1.0 - 2.0 * y * y - 2.0 * z * z, 2.0 * x * y - 2.0 * z * w, 2.0 * x * z + 2.0 * y * w, 0.0],
        [2.0 * x * y + 2.0 * z * w, 1.0 - 2.0 * x * x - 2.0 * z * z, 2.0 * y * z - 2.0 * x * w, 0.0],
        [2.0 * x * z - 2.0 * y * w, 2.0 * y * z + 2.0 * x * w, 1.0 - 2.0 * x * x - 2.0 * y * y, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
    for row in range(3):
        for column in range(3):
            result[row][column] *= scale[column]
        result[row][3] = translation[row]
    return result


def transform_point(matrix: list[list[float]], point: tuple[float, float, float]) -> tuple[float, float, float]:
    value = (*point, 1.0)
    return tuple(sum(matrix[row][column] * value[column] for column in range(4)) for row in range(3))


def read_accessor(document: dict, binary: bytes, accessor_index: int) -> list[tuple]:
    accessor = document["accessors"][accessor_index]
    view = document["bufferViews"][accessor["bufferView"]]
    components = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[accessor["type"]]
    component_type = accessor["componentType"]
    format_string = "<" + COMPONENT_FORMAT[component_type] * components
    stride = view.get("byteStride", COMPONENT_SIZE[component_type] * components)
    offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    return [struct.unpack_from(format_string, binary, offset + index * stride) for index in range(accessor["count"])]


def world_matrices(document: dict) -> list[list[list[float]]]:
    parents: dict[int, int] = {}
    for parent, node in enumerate(document["nodes"]):
        for child in node.get("children", []):
            parents[child] = parent
    cache: dict[int, list[list[float]]] = {}

    def world(index: int) -> list[list[float]]:
        if index not in cache:
            local = node_matrix(document["nodes"][index])
            cache[index] = matrix_multiply(world(parents[index]), local) if index in parents else local
        return cache[index]

    return [world(index) for index in range(len(document["nodes"]))]


def append_indices(document: dict, binary: bytearray, indices: list[int], name: str) -> int:
    while len(binary) % 4:
        binary.append(0)
    offset = len(binary)
    binary.extend(struct.pack("<" + "I" * len(indices), *indices))
    view_index = len(document["bufferViews"])
    document["bufferViews"].append(
        {"buffer": 0, "byteOffset": offset, "byteLength": len(indices) * 4, "target": 34963, "name": name}
    )
    accessor_index = len(document["accessors"])
    document["accessors"].append(
        {
            "bufferView": view_index,
            "componentType": 5125,
            "count": len(indices),
            "type": "SCALAR",
            "min": [min(indices)],
            "max": [max(indices)],
            "name": name,
        }
    )
    return accessor_index


def split_node(
    document: dict,
    binary: bytearray,
    matrices: list[list[list[float]]],
    node_index: int,
    axle_midpoint_z: float,
    slot: str,
) -> bool:
    node = document["nodes"][node_index]
    mesh = document["meshes"][node["mesh"]]
    groups: dict[str, list[int]] = {corner: [] for corner in ("FL", "FR", "RL", "RR")}

    # The supplied targets contain one triangle primitive per material layer.
    if len(mesh["primitives"]) != 1 or mesh["primitives"][0].get("mode", 4) != 4:
        raise ValueError(f"{node.get('name')}: expected one indexed triangle primitive")
    primitive = mesh["primitives"][0]
    positions = read_accessor(document, binary, primitive["attributes"]["POSITION"])
    indices = [value[0] for value in read_accessor(document, binary, primitive["indices"])]
    if len(indices) % 3:
        raise ValueError(f"{node.get('name')}: index count is not divisible by three")

    matrix = matrices[node_index]
    for offset in range(0, len(indices), 3):
        triangle = indices[offset : offset + 3]
        points = [transform_point(matrix, positions[index]) for index in triangle]
        x = sum(point[0] for point in points) / 3.0
        z = sum(point[2] for point in points) / 3.0
        # Source models use +X for the driver's left and +Z forward. The model
        # root's 180-degree yaw converts those to the simulation frame later.
        corner = ("F" if z >= axle_midpoint_z else "R") + ("L" if x >= 0.0 else "R")
        groups[corner].extend(triangle)

    missing = [corner for corner, corner_indices in groups.items() if not corner_indices]
    if missing:
        print(
            f"{node.get('name')}: skipped (does not contain "
            f"{', '.join(missing)} corner geometry)"
        )
        return False

    parents = [index for index, candidate in enumerate(document["nodes"]) if node_index in candidate.get("children", [])]
    if len(parents) != 1:
        raise ValueError(f"{node.get('name')}: expected exactly one parent")
    parent = document["nodes"][parents[0]]
    replacement_nodes: list[int] = []
    for corner in ("FL", "FR", "RL", "RR"):
        corner_indices = groups[corner]
        semantic_name = f"Wheel{slot}_{corner}_{node.get('name', node_index)}"
        accessor = append_indices(document, binary, corner_indices, semantic_name + "_indices")
        corner_primitive = copy.deepcopy(primitive)
        corner_primitive["indices"] = accessor
        corner_mesh = copy.deepcopy(mesh)
        corner_mesh["name"] = semantic_name
        corner_mesh["primitives"] = [corner_primitive]
        mesh_index = len(document["meshes"])
        document["meshes"].append(corner_mesh)
        corner_node = copy.deepcopy(node)
        corner_node["name"] = semantic_name
        corner_node["mesh"] = mesh_index
        replacement_nodes.append(len(document["nodes"]))
        document["nodes"].append(corner_node)

    replaced: list[int] = []
    for child in parent["children"]:
        replaced.extend(replacement_nodes if child == node_index else [child])
    parent["children"] = replaced
    return True


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("gltf", type=Path)
    parser.add_argument("--axle-midpoint-z", type=float, required=True)
    parser.add_argument("--spin-nodes", type=int, nargs="*", default=[])
    parser.add_argument("--hub-nodes", type=int, nargs="*", default=[])
    args = parser.parse_args()

    document = json.loads(args.gltf.read_text())
    if any(node.get("name", "").startswith("WheelSpin_") for node in document["nodes"]):
        print(f"{args.gltf}: wheel meshes already split")
        return
    binary_path = args.gltf.with_name(document["buffers"][0]["uri"])
    binary = bytearray(binary_path.read_bytes())
    matrices = world_matrices(document)
    split_spin = sum(
        split_node(document, binary, matrices, node_index, args.axle_midpoint_z, "Spin")
        for node_index in args.spin_nodes
    )
    split_hub = sum(
        split_node(document, binary, matrices, node_index, args.axle_midpoint_z, "Hub")
        for node_index in args.hub_nodes
    )
    document["buffers"][0]["byteLength"] = len(binary)
    binary_path.write_bytes(binary)
    args.gltf.write_text(json.dumps(document, ensure_ascii=False, separators=(",", ":")))
    print(
        f"{args.gltf}: split {split_spin} spinning and {split_hub} hub layers; "
        f"binary is {len(binary)} bytes"
    )


if __name__ == "__main__":
    main()
