#!/usr/bin/env python3
"""Report scene-space bounds and likely wheel-sized mesh groups in a glTF."""
import json
import sys
from pathlib import Path

from split_combined_wheels import read_accessor, transform_point, world_matrices

path = Path(sys.argv[1])
name_filter = sys.argv[2].lower() if len(sys.argv) > 2 else None
doc = json.loads(path.read_text())
binary = path.with_name(doc["buffers"][0]["uri"]).read_bytes()
matrices = world_matrices(doc)
if name_filter:
    for index, node in enumerate(doc["nodes"]):
        if name_filter in node.get("name", "").lower():
            origin = transform_point(matrices[index], (0.0, 0.0, 0.0))
            print("named", index, node.get("name", ""), [round(v, 3) for v in origin])
overall_min = [float("inf")] * 3
overall_max = [float("-inf")] * 3
groups = []
for node_index, node in enumerate(doc["nodes"]):
    if "mesh" not in node:
        continue
    points = []
    for primitive in doc["meshes"][node["mesh"]]["primitives"]:
        accessor = primitive.get("attributes", {}).get("POSITION")
        if accessor is None:
            continue
        points.extend(transform_point(matrices[node_index], p) for p in read_accessor(doc, binary, accessor))
    if not points:
        continue
    low = [min(p[a] for p in points) for a in range(3)]
    high = [max(p[a] for p in points) for a in range(3)]
    for axis in range(3):
        overall_min[axis] = min(overall_min[axis], low[axis])
        overall_max[axis] = max(overall_max[axis], high[axis])
    size = [high[a] - low[a] for a in range(3)]
    centre = [(high[a] + low[a]) * 0.5 for a in range(3)]
    groups.append((max(size), node_index, node.get("name", ""), centre, size))

print("bounds", [round(v, 4) for v in overall_min], [round(v, 4) for v in overall_max])
print("largest mesh nodes")
selected = [g for g in groups if name_filter in g[2].lower()] if name_filter else sorted(groups, reverse=True)[:25]
for _, index, name, centre, size in selected:
    print(index, name, "center", [round(v, 3) for v in centre], "size", [round(v, 3) for v in size])
