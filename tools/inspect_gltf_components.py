#!/usr/bin/env python3
"""List disconnected triangle components near a point in a glTF scene.

This is a read-only diagnostic for vehicle exports whose named mesh nodes contain
several independent interior parts. Coordinates are reported in scene space.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from split_combined_wheels import read_accessor, transform_point, world_matrices


def components(indices: list[int]) -> list[list[int]]:
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

    for offset in range(0, len(indices), 3):
        triangle = indices[offset : offset + 3]
        union(triangle[0], triangle[1])
        union(triangle[1], triangle[2])

    grouped: dict[int, list[int]] = {}
    for value in parent:
        grouped.setdefault(find(value), []).append(value)
    return list(grouped.values())


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("gltf", type=Path)
    parser.add_argument("--nodes", type=int, nargs="*", default=[])
    parser.add_argument("--near", type=float, nargs=3, required=True)
    parser.add_argument("--radius", type=float, default=0.65)
    parser.add_argument("--minimum-vertices", type=int, default=3)
    parser.add_argument("--limit", type=int, default=0)
    args = parser.parse_args()

    document = json.loads(args.gltf.read_text())
    binary = args.gltf.with_name(document["buffers"][0]["uri"]).read_bytes()
    matrices = world_matrices(document)
    target = tuple(args.near)
    node_indices = args.nodes or [
        index for index, node in enumerate(document["nodes"]) if "mesh" in node
    ]

    for node_index in node_indices:
        node = document["nodes"][node_index]
        mesh = document["meshes"][node["mesh"]]
        for primitive_index, primitive in enumerate(mesh["primitives"]):
            if primitive.get("mode", 4) != 4 or "indices" not in primitive:
                continue
            positions = read_accessor(
                document, binary, primitive["attributes"]["POSITION"]
            )
            indices = [
                value[0] for value in read_accessor(document, binary, primitive["indices"])
            ]
            found = []
            for component_index, vertex_indices in enumerate(components(indices)):
                if len(vertex_indices) < args.minimum_vertices:
                    continue
                points = [
                    transform_point(matrices[node_index], positions[index])
                    for index in vertex_indices
                ]
                minimum = tuple(min(point[axis] for point in points) for axis in range(3))
                maximum = tuple(max(point[axis] for point in points) for axis in range(3))
                centre = tuple((minimum[axis] + maximum[axis]) * 0.5 for axis in range(3))
                distance = sum(
                    (centre[axis] - target[axis]) ** 2 for axis in range(3)
                ) ** 0.5
                if distance <= args.radius:
                    found.append(
                        (distance, component_index, len(vertex_indices), minimum, maximum)
                    )
            if found:
                material = document["materials"][primitive.get("material", 0)].get(
                    "name", "unknown"
                )
                print(
                    f"node {node_index} {node.get('name')} primitive {primitive_index} "
                    f"material {material}"
                )
                ordered = sorted(found)
                if args.limit:
                    ordered = ordered[: args.limit]
                for distance, component_index, count, minimum, maximum in ordered:
                    print(
                        f"  component {component_index:4d} vertices {count:6d} "
                        f"distance {distance:.3f} "
                        f"min {tuple(round(value, 3) for value in minimum)} "
                        f"max {tuple(round(value, 3) for value in maximum)}"
                    )


if __name__ == "__main__":
    main()
