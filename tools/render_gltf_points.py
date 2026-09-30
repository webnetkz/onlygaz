#!/usr/bin/env python3
"""Render a coloured orthographic point cloud from selected glTF nodes."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from split_combined_wheels import read_accessor, transform_point, world_matrices


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("gltf", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--nodes", type=int, nargs="+", required=True)
    parser.add_argument("--horizontal", choices="xyz", default="x")
    parser.add_argument("--vertical", choices="xyz", default="y")
    parser.add_argument("--depth", choices="xyz", default="z")
    parser.add_argument("--horizontal-range", type=float, nargs=2, required=True)
    parser.add_argument("--vertical-range", type=float, nargs=2, required=True)
    parser.add_argument("--depth-range", type=float, nargs=2, required=True)
    parser.add_argument("--size", type=int, nargs=2, default=[1200, 800])
    args = parser.parse_args()

    document = json.loads(args.gltf.read_text())
    binary = args.gltf.with_name(document["buffers"][0]["uri"]).read_bytes()
    matrices = world_matrices(document)
    axes = {"x": 0, "y": 1, "z": 2}
    h_axis = axes[args.horizontal]
    v_axis = axes[args.vertical]
    d_axis = axes[args.depth]
    h_min, h_max = args.horizontal_range
    v_min, v_max = args.vertical_range
    d_min, d_max = args.depth_range
    width, height = args.size
    pixels = bytearray([10, 12, 16]) * (width * height)

    points: list[tuple[float, float, float, int]] = []
    for colour_index, node_index in enumerate(args.nodes):
        node = document["nodes"][node_index]
        mesh = document["meshes"][node["mesh"]]
        for primitive in mesh["primitives"]:
            positions = read_accessor(
                document, binary, primitive["attributes"]["POSITION"]
            )
            indexed_positions = (
                [
                    positions[value[0]]
                    for value in read_accessor(document, binary, primitive["indices"])
                ]
                if "indices" in primitive
                else positions
            )
            for position in indexed_positions:
                point = transform_point(matrices[node_index], position)
                if (
                    h_min <= point[h_axis] <= h_max
                    and v_min <= point[v_axis] <= v_max
                    and d_min <= point[d_axis] <= d_max
                ):
                    points.append((point[h_axis], point[v_axis], point[d_axis], colour_index))

    palette = [(56, 189, 248), (244, 114, 182), (250, 204, 21), (74, 222, 128), (196, 181, 253)]
    for horizontal, vertical, depth, colour_index in sorted(points, key=lambda point: point[2], reverse=True):
        x = round((horizontal - h_min) / (h_max - h_min) * (width - 1))
        y = height - 1 - round((vertical - v_min) / (v_max - v_min) * (height - 1))
        depth_mix = max(0.25, min(1.0, 1.0 - (depth - d_min) / (d_max - d_min)))
        colour = tuple(round(channel * depth_mix) for channel in palette[colour_index % len(palette)])
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                px, py = x + dx, y + dy
                if 0 <= px < width and 0 <= py < height:
                    offset = (py * width + px) * 3
                    pixels[offset : offset + 3] = bytes(colour)

    with args.output.open("wb") as output:
        output.write(f"P6\n{width} {height}\n255\n".encode())
        output.write(pixels)
    print(f"{args.output}: rendered {len(points)} points")


if __name__ == "__main__":
    main()
