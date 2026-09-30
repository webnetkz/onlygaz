#!/usr/bin/env python3
"""Project one photographed 2:1 panorama to a seamless WebGPU cube strip.

Usage: python3 tools/build_skybox.py input.jpg assets/environment/skyboxes/day.png
Requires NumPy and Pillow. The six faces use the WebGPU +X,-X,+Y,-Y,+Z,-Z
convention. Pixel centres, a wrapping longitude sampler, identical exposure and
lossless storage preserve continuity; do not independently edit cube faces.
"""
import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image


def direction(face, u, v):
    u, v = np.broadcast_arrays(u, v)
    one = np.ones_like(u)
    return np.stack((
        (one, -v, -u), (-one, -v, u), (u, one, v),
        (u, -one, -v), (u, -v, one), (-u, -v, -one),
    )[face], axis=-1)


def sample(panorama, rays):
    height, width = panorama.shape[:2]
    x, y, z = np.moveaxis(rays, -1, 0)
    longitude = np.arctan2(x, z)
    latitude = np.arctan2(y, np.sqrt(x*x + z*z))
    px = (longitude / (2*np.pi) + 0.5) * width - 0.5
    py = np.clip((0.5 - latitude / np.pi) * height - 0.5, 0, height - 1)
    ix, iy = np.floor(px).astype(int), np.floor(py).astype(int)
    fx, fy = (px - ix)[..., None], (py - iy)[..., None]
    x0, x1 = ix % width, (ix + 1) % width
    y1 = np.minimum(iy + 1, height - 1)
    top = panorama[iy, x0] * (1-fx) + panorama[iy, x1] * fx
    bottom = panorama[y1, x0] * (1-fx) + panorama[y1, x1] * fx
    return np.uint8(np.clip(np.rint(top * (1-fy) + bottom * fy), 0, 255))


def edge_report(faces):
    """Match all 12 edges geometrically, including rotated top/bottom edges."""
    size = faces[0].shape[0]
    t = (np.arange(size, dtype=np.float32) + 0.5) / size * 2 - 1
    groups = {}
    for face, pixels in enumerate(faces):
        for edge in range(4):
            u, v = [(t, -1), (t, 1), (-1, t), (1, t)][edge]
            rays = direction(face, u, v)
            key = tuple(np.mean(rays, axis=0).round(4))
            data = [pixels[0], pixels[-1], pixels[:, 0], pixels[:, -1]][edge]
            inner = [pixels[1], pixels[-2], pixels[:, 1], pixels[:, -2]][edge]
            groups.setdefault(key, []).append((face, edge, rays, data, inner))
    report = []
    for pair in groups.values():
        assert len(pair) == 2, "cube edge has no matching neighbour"
        a, b = pair
        reverse = np.linalg.norm(a[2][0]-b[2][-1]) < np.linalg.norm(a[2][0]-b[2][0])
        diff = np.abs(a[3].astype(float) - (b[3][::-1] if reverse else b[3]))
        interior = np.concatenate([np.abs(p[3].astype(float)-p[4]) for p in pair])
        report.append(dict(faces=[a[0], b[0]], edges=[a[1], b[1]],
                           mean_rgb_delta=round(float(diff.mean()), 3),
                           p99_rgb_delta=round(float(np.percentile(diff, 99)), 3),
                           interior_p99_delta=round(float(np.percentile(interior, 99)), 3)))
    assert len(report) == 12
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--size', type=int, default=2048)
    args = parser.parse_args()
    panorama = np.asarray(Image.open(args.source).convert('RGB'))
    assert panorama.shape[1] == panorama.shape[0] * 2, "expected a 2:1 panorama"
    size = args.size
    u = (np.arange(size, dtype=np.float32)[None, :] + 0.5) / size * 2 - 1
    faces = []
    for face in range(6):
        pixels = np.empty((size, size, 3), dtype=np.uint8)
        for start in range(0, size, 128):
            v = (np.arange(start, min(start+128, size), dtype=np.float32)[:, None] + 0.5) / size * 2 - 1
            pixels[start:start+128] = sample(panorama, direction(face, u, v))
        faces.append(pixels)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(np.concatenate(faces, axis=0)).save(args.output, compress_level=6)
    report = edge_report(faces)
    print(json.dumps({'output': str(args.output), 'face_size': size, 'edges': report}, indent=2))
    # Adjacent texel centres represent slightly different rays, so nonzero
    # differences are expected near detailed clouds; large discontinuities fail.
    assert max(edge['mean_rgb_delta'] for edge in report) < 3.0, 'visible cube seam'
    # Sharp stars/cloud edges also have natural gradients within each face.
    assert all(edge['p99_rgb_delta'] <= max(12, 2*edge['interior_p99_delta'])
               for edge in report), 'cube edge discontinuity'


if __name__ == '__main__':
    main()
