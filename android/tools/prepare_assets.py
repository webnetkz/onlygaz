#!/usr/bin/env python3
"""Copy the game's asset tree for the Android APK, sized for phone GPUs.

The desktop build ships 2K-8K PNG textures. Phones have a fraction of the
memory bandwidth and share their RAM with the GPU, so the APK copy uses
smaller versions of the largest images. File names, formats and folder
structure stay identical, so the glTF scenes and the game code load the same
paths. The original `assets/` folder is never modified.

Usage: prepare_assets.py <source assets dir> <destination dir>
Requires Pillow (`pip install pillow`).
"""

from __future__ import annotations

import os
import shutil
import sys
from pathlib import Path

from PIL import Image

Image.MAX_IMAGE_PIXELS = None

# Longest edge allowed per folder prefix (first match wins). `None` keeps the
# original resolution.
LIMITS: list[tuple[str, int | None]] = [
    # The player's car is always close to the camera: keep its detail.
    ("models/", 2048),
    # Logos and on-screen icons are drawn 1:1 by the interface.
    ("ui/", None),
    ("fonts/", None),
    # Day/night cubemaps are 1x6 strips: 1024-px faces are plenty on a phone.
    ("environment/skyboxes/", 6144),
    # Distant scenery and traffic.
    ("environment/fir_trees/", 1024),
    ("environment/", 1024),
    ("traffic/", 1024),
    ("effects/", 768),
]
DEFAULT_LIMIT = 1024

# Read-me files are documentation only; licence .txt files are kept.
SKIP_SUFFIXES = {".md"}
IMAGE_SUFFIXES = {".png", ".jpg", ".jpeg"}


def limit_for(relative: str) -> int | None:
    for prefix, limit in LIMITS:
        if relative.startswith(prefix):
            return limit
    return DEFAULT_LIMIT


def resize_image(source: Path, destination: Path, limit: int) -> tuple[tuple[int, int], tuple[int, int]] | None:
    with Image.open(source) as image:
        width, height = image.size
        longest = max(width, height)
        if longest <= limit:
            return None
        scale = limit / longest
        size = (max(1, round(width * scale)), max(1, round(height * scale)))
        mode = image.mode
        working = image
        if mode == "P":
            working = image.convert("RGBA" if "transparency" in image.info else "RGB")
        elif mode not in ("RGB", "RGBA", "L", "LA"):
            working = image.convert("RGBA")
        resized = working.resize(size, Image.LANCZOS)
        if source.suffix.lower() in (".jpg", ".jpeg"):
            resized.convert("RGB").save(destination, quality=90, optimize=True)
        else:
            resized.save(destination, optimize=True)
        return (width, height), size


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    source_root = Path(sys.argv[1]).resolve()
    destination_root = Path(sys.argv[2]).resolve()
    if destination_root.exists():
        shutil.rmtree(destination_root)
    destination_root.mkdir(parents=True)

    copied = resized = 0
    saved_bytes = 0
    for directory, _, files in os.walk(source_root):
        for name in sorted(files):
            source = Path(directory) / name
            relative = source.relative_to(source_root).as_posix()
            if source.suffix.lower() in SKIP_SUFFIXES:
                continue
            destination = destination_root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            limit = limit_for(relative)
            result = None
            if source.suffix.lower() in IMAGE_SUFFIXES and limit is not None:
                result = resize_image(source, destination, limit)
            if result is None:
                shutil.copy2(source, destination)
                copied += 1
            else:
                resized += 1
                saved_bytes += source.stat().st_size - destination.stat().st_size
                (w, h), (nw, nh) = result
                print(f"  {relative}: {w}x{h} -> {nw}x{nh}")

    total = sum(f.stat().st_size for f in destination_root.rglob("*") if f.is_file())
    print(
        f"assets: {copied} copied, {resized} downscaled, "
        f"{saved_bytes / 1e6:.0f} MB saved, {total / 1e6:.0f} MB total"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
