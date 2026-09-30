#!/usr/bin/env python3
"""Extract the authored speedometer/tachometer pointers into named glTF nodes."""

from __future__ import annotations

import argparse
import copy
import json
from pathlib import Path

from split_combined_wheels import append_indices, read_accessor, transform_point, world_matrices
from split_steering_wheel import triangle_components


PREFIX = "InstrumentNeedle_"


def append_part(
    document: dict,
    binary: bytearray,
    source_node: dict,
    source_mesh: dict,
    source_primitive: dict,
    indices: list[int],
    name: str,
) -> int:
    primitive = copy.deepcopy(source_primitive)
    primitive["indices"] = append_indices(document, binary, indices, name + "_indices")
    mesh = copy.deepcopy(source_mesh)
    mesh["name"] = name
    mesh["primitives"] = [primitive]
    mesh_index = len(document["meshes"])
    document["meshes"].append(mesh)
    node = copy.deepcopy(source_node)
    node.pop("children", None)
    node["name"] = name
    node["mesh"] = mesh_index
    node_index = len(document["nodes"])
    document["nodes"].append(node)
    return node_index


def parent_of(document: dict, node_index: int) -> int:
    parents = [
        index
        for index, node in enumerate(document["nodes"])
        if node_index in node.get("children", [])
    ]
    if len(parents) != 1:
        raise ValueError(f"node {node_index}: expected exactly one parent, got {parents}")
    return parents[0]


def split_sls(document: dict, binary: bytearray, matrices: list, node_index: int) -> int:
    node = document["nodes"][node_index]
    mesh = document["meshes"][node["mesh"]]
    if len(mesh["primitives"]) != 1:
        raise ValueError("SLS needle mesh must contain one primitive")
    primitive = mesh["primitives"][0]
    positions = read_accessor(document, binary, primitive["attributes"]["POSITION"])
    indices = [value[0] for value in read_accessor(document, binary, primitive["indices"])]
    parts = {"Tachometer": [], "Speedometer": []}
    for offset in range(0, len(indices), 3):
        triangle = indices[offset : offset + 3]
        points = [transform_point(matrices[node_index], positions[index]) for index in triangle]
        x = sum(point[0] for point in points) / 3.0
        parts["Tachometer" if x < 0.414 else "Speedometer"].extend(triangle)

    replacements = [
        append_part(
            document,
            binary,
            node,
            mesh,
            primitive,
            part_indices,
            f"{PREFIX}{kind}_SLS",
        )
        for kind, part_indices in parts.items()
    ]
    parent = document["nodes"][parent_of(document, node_index)]
    children: list[int] = []
    for child in parent["children"]:
        children.extend(replacements if child == node_index else [child])
    parent["children"] = children
    print(
        "  SLS: "
        + ", ".join(f"{kind} {len(part) // 3} triangles" for kind, part in parts.items())
    )
    return sum(len(part) // 3 for part in parts.values())


def split_zonda(document: dict, binary: bytearray, matrices: list, node_index: int) -> int:
    node = document["nodes"][node_index]
    mesh = document["meshes"][node["mesh"]]
    if len(mesh["primitives"]) != 1:
        raise ValueError("Zonda gauge mesh must contain one primitive")
    primitive = mesh["primitives"][0]
    positions = read_accessor(document, binary, primitive["attributes"]["POSITION"])
    indices = [value[0] for value in read_accessor(document, binary, primitive["indices"])]
    parts = {"Tachometer": [], "Speedometer": [], "Body": []}
    for triangles in triangle_components(indices):
        unique = set(index for triangle in triangles for index in triangle)
        points = [transform_point(matrices[node_index], positions[index]) for index in unique]
        minimum = tuple(min(point[axis] for point in points) for axis in range(3))
        maximum = tuple(max(point[axis] for point in points) for axis in range(3))
        centre = tuple((minimum[axis] + maximum[axis]) * 0.5 for axis in range(3))
        vertical_pointer = maximum[1] - minimum[1] > 0.025 and 0.62 < centre[1] < 0.67
        if vertical_pointer and abs(centre[0] - 0.3985) < 0.015:
            kind = "Tachometer"
        elif vertical_pointer and abs(centre[0] - 0.2295) < 0.015:
            kind = "Speedometer"
        else:
            kind = "Body"
        parts[kind].extend(index for triangle in triangles for index in triangle)

    if not parts["Tachometer"] or not parts["Speedometer"]:
        raise ValueError("Zonda analogue pointers were not found")
    body_primitive = copy.deepcopy(primitive)
    body_primitive["indices"] = append_indices(
        document, binary, parts["Body"], "ZondaInstrumentBody_indices"
    )
    body_mesh = copy.deepcopy(mesh)
    body_mesh["name"] = mesh.get("name", "ZondaInstrumentBody") + "_body"
    body_mesh["primitives"] = [body_primitive]
    node["mesh"] = len(document["meshes"])
    document["meshes"].append(body_mesh)

    parent = document["nodes"][parent_of(document, node_index)]
    insertion = parent["children"].index(node_index) + 1
    for kind in ("Tachometer", "Speedometer"):
        child = append_part(
            document,
            binary,
            node,
            mesh,
            primitive,
            parts[kind],
            f"{PREFIX}{kind}_Zonda",
        )
        parent["children"].insert(insertion, child)
        insertion += 1
    print(
        "  Zonda: "
        + ", ".join(
            f"{kind} {len(parts[kind]) // 3} triangles"
            for kind in ("Tachometer", "Speedometer")
        )
    )
    return (len(parts["Tachometer"]) + len(parts["Speedometer"])) // 3


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("gltf", type=Path)
    parser.add_argument("--vehicle", choices=["sls", "zonda"], required=True)
    parser.add_argument("--node", type=int, required=True)
    args = parser.parse_args()

    document = json.loads(args.gltf.read_text())
    if any(node.get("name", "").startswith(PREFIX) for node in document["nodes"]):
        print(f"{args.gltf}: instrument needles already split")
        return
    binary_path = args.gltf.with_name(document["buffers"][0]["uri"])
    binary = bytearray(binary_path.read_bytes())
    matrices = world_matrices(document)
    count = (
        split_sls(document, binary, matrices, args.node)
        if args.vehicle == "sls"
        else split_zonda(document, binary, matrices, args.node)
    )
    document["buffers"][0]["byteLength"] = len(binary)
    binary_path.write_bytes(binary)
    args.gltf.write_text(json.dumps(document, ensure_ascii=False, separators=(",", ":")))
    print(f"{args.gltf}: extracted {count} analogue-pointer triangles")


if __name__ == "__main__":
    main()
