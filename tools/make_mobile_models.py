#!/usr/bin/env python3
"""Build the lighter 3D models used by the Android (phone) build.

The desktop game ships film-quality scans: one traffic car alone has more than
a million triangles and the player's car almost 800 thousand. A phone GPU has
to transform every one of them, for the main view and again for each shadow
cascade, so the phone build swaps them for simplified copies.

This script writes those copies to ``android/asset-overrides/`` with the same
relative paths as in ``assets/``. The Android asset step (``GameAssets.java``)
copies ``assets/`` first and then lays the overrides on top, so the game code
loads exactly the same paths on every platform. The textures keep their
original files and paths (``textures/*.png``), so the phone-sized textures that
``GameAssets`` produces are still used.

Requirements: Python 3 with Pillow, and ``gltfpack`` 0.24
(``npm install -g gltfpack``). Run from the repository root::

    python3 tools/make_mobile_models.py

Simplification per model (see ``MODELS``):

* traffic cars and the street light are merged per material and simplified
  aggressively - they are seen at a distance and nothing in the code looks up
  their parts by name;
* the player's car keeps every named node and material (the wheel rig, lamps,
  instruments and damage code find parts by name; mesh names are restored
  after packing) and is simplified within a small error bound, so the
  silhouette stays exact up close.
"""

from __future__ import annotations

import copy
import io
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
OUT = ROOT / "android" / "asset-overrides"

# relative path, gltfpack options, longest edge of embedded images (GLB only)
MODELS = [
    (
        "traffic/mercedes_m_class_2011/scene.gltf",
        ["-si", "0.03", "-sa", "-mm"],
        None,
    ),
    (
        "traffic/kia_picanto_gt_line_2017/scene.gltf",
        ["-si", "0.02", "-se", "0.05", "-mm"],
        None,
    ),
    (
        "traffic/town_bus/scene.gltf",
        ["-si", "0.2", "-se", "0.05", "-mm"],
        None,
    ),
    (
        "environment/roadside/bridge_street_light.glb",
        ["-si", "0.06", "-sa", "-mm"],
        256,
    ),
    (
        "models/bugatti/scene.gltf",
        ["-si", "0.12", "-kn", "-km", "-ke"],
        None,
    ),
]


def run_gltfpack(source: Path, target: Path, options: list[str]) -> str:
    command = ["gltfpack", "-i", str(source), "-o", str(target), "-noq", "-v", *options]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
        sys.exit(f"gltfpack failed for {source}:\n{result.stdout}\n{result.stderr}")
    return "\n".join(
        line for line in result.stdout.splitlines() if line.startswith(("input:", "output:"))
    )


def keep_original_texture_paths(source: Path, packed: Path) -> None:
    """gltfpack copies images next to its output under their base names.

    Point the URIs back at the original ``textures/...`` paths (which the
    Android asset step downscales) and drop the copies.
    """
    original = json.loads(source.read_text())
    by_name = {Path(image["uri"]).name: image["uri"] for image in original.get("images", []) if "uri" in image}
    gltf = json.loads(packed.read_text())
    for image in gltf.get("images", []):
        uri = image.get("uri")
        if uri is None:
            continue
        name = Path(uri).name
        if name not in by_name:
            sys.exit(f"{packed}: image {uri} has no original in {source}")
        image["uri"] = by_name[name]
        copied = packed.parent / uri
        if copied.is_file():
            copied.unlink()
    packed.write_text(json.dumps(gltf, separators=(",", ":")))


def restore_mesh_names(source: Path, packed: Path) -> None:
    """Give every mesh the name its node's mesh had in the original file.

    gltfpack drops mesh names and lets identical meshes share one entry.
    Bevy names each primitive entity "<mesh name>.<material name>", and the
    game finds parts such as the instrument needles by that name, so each
    node gets back a mesh with its original name (a shared mesh is split into
    named copies that reuse the same vertex data).
    """
    original = json.loads(source.read_text())
    names_by_node: dict[str, str] = {}
    for node in original.get("nodes", []):
        if "mesh" in node and node.get("name"):
            mesh_name = original["meshes"][node["mesh"]].get("name")
            if mesh_name:
                names_by_node.setdefault(node["name"], mesh_name)
    gltf = json.loads(packed.read_text())
    meshes = gltf.get("meshes", [])
    named: dict[int, str] = {}
    for node in gltf.get("nodes", []):
        wanted = names_by_node.get(node.get("name", ""))
        if "mesh" not in node or wanted is None:
            continue
        index = node["mesh"]
        if index in named and named[index] != wanted:
            twin = copy.deepcopy(meshes[index])
            twin["name"] = wanted
            meshes.append(twin)
            node["mesh"] = len(meshes) - 1
            named[node["mesh"]] = wanted
        else:
            meshes[index]["name"] = wanted
            named[index] = wanted
    packed.write_text(json.dumps(gltf, separators=(",", ":")))


def shrink_glb_images(path: Path, limit: int) -> None:
    """Downscale the images embedded in a GLB to ``limit`` pixels on the long edge."""
    data = path.read_bytes()
    magic, version, _length = struct.unpack_from("<4sII", data, 0)
    if magic != b"glTF" or version != 2:
        sys.exit(f"{path} is not a glTF 2 binary")
    offset = 12
    chunks = []
    while offset < len(data):
        chunk_length, chunk_type = struct.unpack_from("<I4s", data, offset)
        chunks.append((chunk_type, data[offset + 8 : offset + 8 + chunk_length]))
        offset += 8 + chunk_length
    gltf = json.loads(chunks[0][1].decode("utf-8"))
    binary = chunks[1][1] if len(chunks) > 1 else b""

    image_views = {image["bufferView"]: image for image in gltf.get("images", []) if "bufferView" in image}
    out = bytearray()
    for index, view in enumerate(gltf.get("bufferViews", [])):
        start = view.get("byteOffset", 0)
        payload = binary[start : start + view["byteLength"]]
        image = image_views.get(index)
        if image is not None:
            picture = Image.open(io.BytesIO(payload))
            picture.load()
            if max(picture.size) > limit:
                scale = limit / max(picture.size)
                size = (max(1, round(picture.width * scale)), max(1, round(picture.height * scale)))
                picture = picture.resize(size, Image.LANCZOS)
            encoded = io.BytesIO()
            if image.get("mimeType") == "image/jpeg":
                picture.convert("RGB").save(encoded, "JPEG", quality=88)
            else:
                picture.save(encoded, "PNG", optimize=True)
            payload = encoded.getvalue()
        while len(out) % 4:
            out.append(0)
        view["byteOffset"] = len(out)
        view["byteLength"] = len(payload)
        out.extend(payload)
    while len(out) % 4:
        out.append(0)
    gltf["buffers"][0]["byteLength"] = len(out)

    json_bytes = json.dumps(gltf, separators=(",", ":")).encode("utf-8")
    while len(json_bytes) % 4:
        json_bytes += b" "
    total = 12 + 8 + len(json_bytes) + 8 + len(out)
    result = bytearray(struct.pack("<4sII", b"glTF", 2, total))
    result += struct.pack("<I4s", len(json_bytes), b"JSON") + json_bytes
    result += struct.pack("<I4s", len(out), b"BIN\0") + bytes(out)
    path.write_bytes(bytes(result))


def main() -> None:
    if shutil.which("gltfpack") is None:
        sys.exit("gltfpack not found: npm install -g gltfpack")
    for relative, options, image_limit in MODELS:
        source = ASSETS / relative
        target = OUT / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory() as temporary:
            packed = Path(temporary) / source.name
            report = run_gltfpack(source, packed, options)
            if packed.suffix == ".gltf":
                keep_original_texture_paths(source, packed)
                restore_mesh_names(source, packed)
                bin_name = json.loads(packed.read_text())["buffers"][0]["uri"]
                shutil.copyfile(packed, target)
                # gltfpack names the buffer after the output file.
                shutil.copyfile(Path(temporary) / bin_name, target.parent / bin_name)
            else:
                if image_limit:
                    shrink_glb_images(packed, image_limit)
                shutil.copyfile(packed, target)
        print(f"{relative}\n{report}\n")


if __name__ == "__main__":
    main()
