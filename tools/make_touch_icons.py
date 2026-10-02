#!/usr/bin/env python3
"""Draw the icons of the phone build's on-screen pedals and buttons.

Writes white-on-transparent PNGs to assets/ui/touch/. The game tints them
with each button's accent colour (src/mobile/touch_controls.rs), so they
stay single-colour glyphs in the style of the steering arrows: solid shapes
with rounded corners, cut-outs for detail.

Usage: python3 tools/make_touch_icons.py        (requires Pillow)
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "assets" / "ui" / "touch"
# Shapes are drawn this many times larger, then reduced: smooth edges.
SUPERSAMPLE = 8


class Canvas:
    """A single-channel coverage mask in unit coordinates (0..1 across the width)."""

    def __init__(self, width: int, height: int) -> None:
        self.width, self.height = width, height
        self.scale = width * SUPERSAMPLE
        self.mask = Image.new("L", (width * SUPERSAMPLE, height * SUPERSAMPLE), 0)
        self.draw = ImageDraw.Draw(self.mask)

    def _xy(self, x: float, y: float) -> tuple[float, float]:
        return x * self.scale, y * self.scale

    def rounded(self, x0: float, y0: float, x1: float, y1: float, radius: float, on: bool = True) -> None:
        self.draw.rounded_rectangle(
            (*self._xy(x0, y0), *self._xy(x1, y1)), radius=radius * self.scale, fill=255 if on else 0
        )

    def circle(self, cx: float, cy: float, r: float, on: bool = True) -> None:
        self.draw.ellipse((*self._xy(cx - r, cy - r), *self._xy(cx + r, cy + r)), fill=255 if on else 0)

    def path(self, start: tuple[float, float], curves: list[tuple], on: bool = True) -> None:
        """Closed outline of cubic Bezier segments (c1, c2, end)."""
        points = [start]
        current = start
        for c1, c2, end in curves:
            for i in range(1, 41):
                t = i / 40
                u = 1 - t
                points.append(
                    (
                        u**3 * current[0] + 3 * u * u * t * c1[0] + 3 * u * t * t * c2[0] + t**3 * end[0],
                        u**3 * current[1] + 3 * u * u * t * c1[1] + 3 * u * t * t * c2[1] + t**3 * end[1],
                    )
                )
            current = end
        self.draw.polygon([self._xy(x, y) for x, y in points], fill=255 if on else 0)

    def rounded_polygon(self, corners: list[tuple[float, float]], radius: float, on: bool = True) -> None:
        """Convex polygon with each corner rounded by a quadratic curve."""
        points = []
        count = len(corners)
        for i, (cx, cy) in enumerate(corners):
            px, py = corners[i - 1]
            nx, ny = corners[(i + 1) % count]

            def towards(x: float, y: float) -> tuple[float, float]:
                dx, dy = x - cx, y - cy
                length = (dx * dx + dy * dy) ** 0.5
                cut = min(radius, length / 2)
                return cx + dx / length * cut, cy + dy / length * cut

            a = towards(px, py)
            b = towards(nx, ny)
            for j in range(17):
                t = j / 16
                u = 1 - t
                points.append(
                    (u * u * a[0] + 2 * u * t * cx + t * t * b[0], u * u * a[1] + 2 * u * t * cy + t * t * b[1])
                )
        self.draw.polygon([self._xy(x, y) for x, y in points], fill=255 if on else 0)

    def save(self, name: str) -> None:
        alpha = self.mask.resize((self.width, self.height), Image.LANCZOS)
        image = Image.new("RGBA", (self.width, self.height), (255, 255, 255, 0))
        image.putalpha(alpha)
        # Fully transparent pixels stay white, so filtering never darkens edges.
        image.save(OUT / name, optimize=True)
        print(f"wrote {OUT / name} ({self.width}x{self.height})")


def pedal_grooves(canvas: Canvas, x0: float, x1: float, top: float, bottom: float, count: int) -> None:
    """Horizontal anti-slip grooves cut into a pedal plate."""
    step = (bottom - top) / count
    height = step * 0.42
    for i in range(count):
        y = top + step * i + (step - height) / 2
        canvas.rounded(x0, y, x1, y + height, height / 2, on=False)


def gas() -> None:
    # Accelerator: a tall plate, slightly narrower at the floor hinge.
    c = Canvas(128, 208)
    c.rounded_polygon([(0.13, 0.04), (0.87, 0.04), (0.79, 1.30), (0.21, 1.30)], 0.14)
    top, bottom, count = 0.18, 1.16, 5
    step = (bottom - top) / count
    height = step * 0.40
    for i in range(count):
        y = top + step * i + (step - height) / 2
        # Grooves follow the taper of the plate.
        inset = 0.15 + 0.08 * ((y - 0.04) / 1.26)
        c.rounded(0.13 + inset, y, 0.87 - inset, y + height, height / 2, on=False)
    # Floor hinge below the plate.
    c.rounded(0.26, 1.38, 0.74, 1.56, 0.08)
    c.save("gas.png")


def brake() -> None:
    # Brake: a wide plate hanging from its arm.
    c = Canvas(128, 128)
    c.rounded(0.42, 0.03, 0.58, 0.40, 0.06)
    c.rounded(0.08, 0.30, 0.92, 0.94, 0.14)
    pedal_grooves(c, 0.22, 0.78, 0.40, 0.84, 4)
    c.save("brake.png")


def nitro() -> None:
    # Flame with a hollow inner flame.
    c = Canvas(128, 128)
    c.path(
        (0.50, 0.97),
        [
            ((0.24, 0.97), (0.14, 0.78), (0.19, 0.60)),
            ((0.23, 0.46), (0.31, 0.38), (0.30, 0.22)),
            ((0.41, 0.30), (0.45, 0.39), (0.46, 0.44)),
            ((0.45, 0.27), (0.52, 0.14), (0.60, 0.03)),
            ((0.68, 0.20), (0.87, 0.36), (0.84, 0.63)),
            ((0.82, 0.87), (0.67, 0.97), (0.50, 0.97)),
        ],
    )
    c.path(
        (0.50, 0.87),
        [
            ((0.38, 0.87), (0.33, 0.78), (0.36, 0.69)),
            ((0.39, 0.61), (0.46, 0.57), (0.50, 0.46)),
            ((0.57, 0.58), (0.67, 0.62), (0.66, 0.73)),
            ((0.65, 0.84), (0.59, 0.87), (0.50, 0.87)),
        ],
        on=False,
    )
    c.save("nitro.png")


def camera() -> None:
    c = Canvas(128, 128)
    c.rounded(0.33, 0.16, 0.67, 0.34, 0.06)
    c.rounded(0.08, 0.27, 0.92, 0.84, 0.12)
    c.circle(0.50, 0.555, 0.215, on=False)
    c.circle(0.50, 0.555, 0.135)
    c.circle(0.79, 0.38, 0.04, on=False)
    c.save("camera.png")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    gas()
    brake()
    nitro()
    camera()


if __name__ == "__main__":
    main()
