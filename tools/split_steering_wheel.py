#!/usr/bin/env python3
"""Extract steering-wheel geometry into semantic glTF nodes.

Some supplied vehicle exports merge the wheel with the complete interior.  This
utility partitions indexed triangles around the measured steering plane while
preserving every vertex attribute and material.  Exports with useful shared
indices are split by connected component; fully de-indexed exports can be split
triangle-by-triangle.
"""

from __future__ import annotations

import argparse
import copy
import json
from pathlib import Path

from split_combined_wheels import append_indices, read_accessor, transform_point, world_matrices


def vector_length(value: tuple[float, float, float]) -> float:
    return sum(component * component for component in value) ** 0.5


def triangle_components(indices: list[int]) -> list[list[list[int]]]:
    parent: dict[int, int] = {}

    def find(value: int) -> int:
        parent.setdefault(value, value)
        while parent[value] != value:
            parent[value] = parent[parent[value]]
            value = parent[value]
        return value

    def union(left: int, right: int) -> None:
        left_root = find(left)
        right_root = find(right)
        if left_root != right_root:
            parent[right_root] = left_root

    triangles = [indices[offset : offset + 3] for offset in range(0, len(indices), 3)]
    for triangle in triangles:
        union(triangle[0], triangle[1])
        union(triangle[1], triangle[2])

    grouped: dict[int, list[list[int]]] = {}
    for triangle in triangles:
        grouped.setdefault(find(triangle[0]), []).append(triangle)
    return list(grouped.values())


def disc_coordinates(
    point: tuple[float, float, float],
    centre: tuple[float, float, float],
    normal: tuple[float, float, float],
    radii: tuple[float, float],
) -> tuple[float, float]:
    relative = tuple(point[axis] - centre[axis] for axis in range(3))
    plane_distance = sum(relative[axis] * normal[axis] for axis in range(3))
    radial = (relative[0] / radii[0]) ** 2 + (relative[1] / radii[1]) ** 2
    return plane_distance, radial


def group_centre(
    triangles: list[list[int]],
    positions: list[tuple],
    matrix: list[list[float]],
) -> tuple[tuple[float, float, float], tuple[float, float, float]]:
    unique_indices = set(index for triangle in triangles for index in triangle)
    points = [transform_point(matrix, positions[index]) for index in unique_indices]
    minimum = tuple(min(point[axis] for point in points) for axis in range(3))
    maximum = tuple(max(point[axis] for point in points) for axis in range(3))
    centre = tuple((minimum[axis] + maximum[axis]) * 0.5 for axis in range(3))
    span = tuple(maximum[axis] - minimum[axis] for axis in range(3))
    return centre, span


def split_node(
    document: dict,
    binary: bytearray,
    matrices: list[list[list[float]]],
    node_index: int,
    mode: str,
    centre: tuple[float, float, float],
    normal: tuple[float, float, float],
    radii: tuple[float, float],
    thickness: float,
    radial_limit: float,
    max_component_span: float,
) -> int:
    node = document["nodes"][node_index]
    if "mesh" not in node or node.get("name", "").startswith("SteeringWheel_"):
        return 0
    source_mesh = document["meshes"][node["mesh"]]
    body_primitives: list[dict] = []
    steering_primitives: list[dict] = []
    selected_triangles = 0

    for primitive_index, primitive in enumerate(source_mesh["primitives"]):
        if primitive.get("mode", 4) != 4 or "indices" not in primitive:
            body_primitives.append(copy.deepcopy(primitive))
            continue
        positions = read_accessor(document, binary, primitive["attributes"]["POSITION"])
        indices = [value[0] for value in read_accessor(document, binary, primitive["indices"])]
        if len(indices) % 3:
            raise ValueError(f"{node.get('name')}: index count is not divisible by three")

        groups = (
            [[indices[offset : offset + 3]] for offset in range(0, len(indices), 3)]
            if mode == "triangle"
            else triangle_components(indices)
        )
        selected: list[int] = []
        remaining: list[int] = []
        for triangles in groups:
            candidate_centre, span = group_centre(triangles, positions, matrices[node_index])
            plane_distance, radial = disc_coordinates(candidate_centre, centre, normal, radii)
            compact_enough = mode == "triangle" or max(span) <= max_component_span
            target = (
                compact_enough
                and abs(plane_distance) <= thickness
                and radial <= radial_limit
            )
            destination = selected if target else remaining
            destination.extend(index for triangle in triangles for index in triangle)

        if not selected:
            body_primitives.append(copy.deepcopy(primitive))
            continue
        semantic_name = f"SteeringWheel_{node.get('name', node_index)}_{primitive_index}"
        if remaining:
            body_primitive = copy.deepcopy(primitive)
            body_primitive["indices"] = append_indices(
                document, binary, remaining, semantic_name + "_body_indices"
            )
            body_primitives.append(body_primitive)
        steering_primitive = copy.deepcopy(primitive)
        steering_primitive["indices"] = append_indices(
            document, binary, selected, semantic_name + "_indices"
        )
        steering_primitives.append(steering_primitive)
        selected_triangles += len(selected) // 3

    if not steering_primitives:
        return 0

    semantic_name = f"SteeringWheel_{node.get('name', node_index)}"
    if not body_primitives:
        steering_mesh = copy.deepcopy(source_mesh)
        steering_mesh["name"] = semantic_name
        steering_mesh["primitives"] = steering_primitives
        steering_mesh_index = len(document["meshes"])
        document["meshes"].append(steering_mesh)
        node["name"] = semantic_name
        node["mesh"] = steering_mesh_index
        print(f"  {node_index:3d} {semantic_name}: {selected_triangles} steering triangles")
        return selected_triangles

    body_mesh = copy.deepcopy(source_mesh)
    body_mesh["name"] = source_mesh.get("name", node.get("name", "mesh")) + "_body"
    body_mesh["primitives"] = body_primitives
    body_mesh_index = len(document["meshes"])
    document["meshes"].append(body_mesh)
    node["mesh"] = body_mesh_index

    steering_mesh = copy.deepcopy(source_mesh)
    steering_mesh["name"] = semantic_name
    steering_mesh["primitives"] = steering_primitives
    steering_mesh_index = len(document["meshes"])
    document["meshes"].append(steering_mesh)
    steering_node = copy.deepcopy(node)
    steering_node.pop("children", None)
    steering_node["name"] = semantic_name
    steering_node["mesh"] = steering_mesh_index
    steering_node_index = len(document["nodes"])
    document["nodes"].append(steering_node)

    parents = [
        index
        for index, candidate in enumerate(document["nodes"][:steering_node_index])
        if node_index in candidate.get("children", [])
    ]
    if len(parents) != 1:
        raise ValueError(f"{node.get('name')}: expected exactly one parent, got {parents}")
    children = document["nodes"][parents[0]]["children"]
    insertion = children.index(node_index) + 1
    children.insert(insertion, steering_node_index)
    print(f"  {node_index:3d} {node.get('name')}: {selected_triangles} steering triangles")
    return selected_triangles


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("gltf", type=Path)
    parser.add_argument("--centre", type=float, nargs=3, required=True)
    parser.add_argument("--normal", type=float, nargs=3, required=True)
    parser.add_argument("--radii", type=float, nargs=2, required=True)
    parser.add_argument("--thickness", type=float, default=0.08)
    parser.add_argument("--radial-limit", type=float, default=1.35)
    parser.add_argument("--max-component-span", type=float, default=0.55)
    parser.add_argument("--triangle-nodes", type=int, nargs="*", default=[])
    parser.add_argument("--component-nodes", type=int, nargs="*", default=[])
    args = parser.parse_args()

    document = json.loads(args.gltf.read_text())
    if any(node.get("name", "").startswith("SteeringWheel_") for node in document["nodes"]):
        print(f"{args.gltf}: steering wheel already split")
        return
    binary_path = args.gltf.with_name(document["buffers"][0]["uri"])
    binary = bytearray(binary_path.read_bytes())
    matrices = world_matrices(document)
    normal_length = vector_length(tuple(args.normal))
    if normal_length <= 1.0e-6:
        raise ValueError("steering plane normal cannot be zero")
    normal = tuple(component / normal_length for component in args.normal)
    total = 0
    for node_index in args.triangle_nodes:
        total += split_node(
            document,
            binary,
            matrices,
            node_index,
            "triangle",
            tuple(args.centre),
            normal,
            tuple(args.radii),
            args.thickness,
            args.radial_limit,
            args.max_component_span,
        )
    for node_index in args.component_nodes:
        total += split_node(
            document,
            binary,
            matrices,
            node_index,
            "component",
            tuple(args.centre),
            normal,
            tuple(args.radii),
            args.thickness,
            args.radial_limit,
            args.max_component_span,
        )
    if not total:
        raise ValueError("no steering-wheel triangles matched")
    document["buffers"][0]["byteLength"] = len(binary)
    binary_path.write_bytes(binary)
    args.gltf.write_text(json.dumps(document, ensure_ascii=False, separators=(",", ":")))
    print(f"{args.gltf}: extracted {total} steering triangles; binary is {len(binary)} bytes")


if __name__ == "__main__":
    main()
