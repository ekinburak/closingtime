#!/usr/bin/env python3
"""Render the README wordmarks and illustrative walkthrough with Pillow.

Run from any directory: python3 scripts/readme-assets.py
Requires Pillow. Fonts and their OFL licenses live in docs/assets/fonts/.
"""

from pathlib import Path
import math
from xml.sax.saxutils import escape

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "docs" / "assets"
FONTS = ASSETS / "fonts"
SCALE = 2
WIDTH, HEIGHT = 1120, 600
COLORS = {
    "page": "#121c21", "surface": "#19272d", "soft": "#202f35",
    "ink": "#edf5f5", "muted": "#a4b7bf", "line": "#35474f",
    "accent": "#9bd0c1", "accent_soft": "#233d38", "kept": "#b1c9f2",
    "kept_soft": "#263751", "warning": "#e8bd79", "warning_soft": "#3c3226",
}


def font(size, mono=False, weight=500):
    name = "jetbrains-mono.ttf" if mono else "manrope.ttf"
    result = ImageFont.truetype(str(FONTS / name), round(size * SCALE))
    try:
        result.set_variation_by_axes([weight])
    except (OSError, ValueError):
        pass
    return result


def text(draw, xy, value, size, color="ink", mono=False, weight=500):
    fill = COLORS.get(color, color)
    draw.text(tuple(round(v * SCALE) for v in xy), value,
              font=font(size, mono, weight), fill=fill, anchor="lt")


def box(draw, bounds, fill, outline=None, radius=12, width=1):
    draw.rounded_rectangle(tuple(round(v * SCALE) for v in bounds),
                           radius=radius * SCALE, fill=COLORS.get(fill, fill),
                           outline=COLORS.get(outline, outline), width=width * SCALE)


def line(draw, points, fill, width=2):
    draw.line([(round(x * SCALE), round(y * SCALE)) for x, y in points],
              fill=COLORS.get(fill, fill), width=width * SCALE, joint="curve")


def mark(draw, x, y, size, fill):
    """The existing website identity: brackets, a connector, and an owned node."""
    unit = size / 24
    stroke = max(2, round(1.8 * unit))
    for points in [((8, 4), (4, 4), (4, 20), (8, 20)),
                   ((16, 4), (20, 4), (20, 20), (16, 20)),
                   ((8, 12), (16, 12))]:
        line(draw, [(x + a * unit, y + b * unit) for a, b in points], fill, stroke)
    cx, cy, radius = x + 12 * unit, y + 12 * unit, 2.5 * unit
    draw.ellipse(tuple(round(v * SCALE) for v in
                       (cx - radius, cy - radius, cx + radius, cy + radius)),
                 fill=COLORS.get(fill, fill))


def pill(draw, x, y, label, color="accent", fill="accent_soft", size=12):
    width = font(size, mono=True).getlength(label) / SCALE + 24
    box(draw, (x, y, x + width, y + 29), fill, radius=7)
    text(draw, (x + 12, y + 7), label, size, color, mono=True)
    return width


def ease_out(value):
    return 1 - (1 - max(0, min(1, value))) ** 3


def draw_path(draw, points, progress, color="accent"):
    lengths = [math.dist(a, b) for a, b in zip(points, points[1:])]
    remaining = sum(lengths) * progress
    for a, b, length in zip(points, points[1:], lengths):
        part = min(1, remaining / length)
        if part > 0:
            end = (a[0] + (b[0] - a[0]) * part, a[1] + (b[1] - a[1]) * part)
            line(draw, [a, end], color)
        remaining -= length


def render(t):
    image = Image.new("RGB", (WIDTH * SCALE, HEIGHT * SCALE), COLORS["page"])
    draw = ImageDraw.Draw(image)
    stage = 0 if t < 3.4 else 1 if t < 6.8 else 2
    titles = ["Every run gets its own identity.", "The agent exits. Ownership stays.",
              "Review what would stop."]
    subtitles = ["Record a run and the process evidence you can observe.",
                 "Explain the recorded listeners that are still running.",
                 "Preserve kept work. Leave unknown ownership report-only."]

    mark(draw, 45, 30, 27, "accent")
    text(draw, (83, 34), "closingtime", 23, weight=650)
    text(draw, (817, 40), "ILLUSTRATIVE WALKTHROUGH", 11, "muted", mono=True)
    line(draw, [(48, 82), (1072, 82)], "line", 1)
    text(draw, (48, 111), titles[stage], 40, weight=600)
    text(draw, (49, 167), subtitles[stage], 18, "muted")

    box(draw, (48, 227, 352, 438), "surface", "line")
    text(draw, (72, 250), "RECORDED RUN", 12, "muted", mono=True)
    box(draw, (72, 290, 118, 336), "accent_soft", radius=8)
    mark(draw, 80, 298, 30, "accent")
    text(draw, (134, 291), "run c7b3", 23, weight=650)
    text(draw, (134, 322), "~/checkout", 13, "muted", mono=True)
    pill(draw, 72, 374, "ACTIVE" if stage == 0 else "ENDED",
         "accent" if stage == 0 else "muted", "accent_soft" if stage == 0 else "soft")
    text(draw, (174, 382), "claude", 13, "muted", mono=True)

    # Draw only the two recorded relationships. The unknown listener has no owner edge.
    progress = ease_out((t - 0.5) / 0.8)
    draw_path(draw, [(352, 313), (444, 313), (444, 270), (557, 270)], progress)
    draw_path(draw, [(352, 313), (444, 313), (444, 370), (557, 370)], progress)

    for index, (port, process, evidence) in enumerate([
            (":3000", "node", "run c7b3 / inherited tag"),
            (":5173", "vite", "run c7b3 / observed ancestry"),
            (":5432", "postgres", "No recorded owner")]):
        y = 227 + index * 100
        color = "warning" if index == 2 else "kept" if stage == 2 and index == 1 else "accent"
        box(draw, (557, y, 1072, y + 82), "surface", "line")
        if index == 2:
            # Dashed perimeter differentiates missing ownership from recorded identity.
            bounds = tuple(round(v * SCALE) for v in (579, y + 21, 599, y + 41))
            for start in range(0, 360, 60):
                draw.arc(bounds, start, start + 32, fill=COLORS[color], width=2 * SCALE)
        else:
            draw.ellipse(tuple(round(v * SCALE) for v in (582, y + 24, 596, y + 38)),
                         fill=COLORS[color])
        text(draw, (616, y + 17), port, 21, mono=True, weight=500)
        text(draw, (709, y + 19), process, 20, weight=600)
        text(draw, (616, y + 51), evidence, 11, "muted", mono=True)
        if index == 2:
            label = "REPORT ONLY" if stage == 2 else "UNKNOWN"
            background = "warning_soft"
        elif stage == 2:
            label = "PREVIEW STOP" if index == 0 else "KEPT"
            background = "accent_soft" if index == 0 else "kept_soft"
        else:
            label, background = "RECORDED", "accent_soft"
        tag_width = font(11, mono=True).getlength(label) / SCALE + 24
        pill(draw, 1051 - tag_width, y + 18, label, color, background, size=11)

    # One focal transition at a time; settle into a readable review state.
    if stage == 2:
        box(draw, (48, 461, 352, 509), "accent_soft", radius=8)
        text(draw, (65, 476), "PREVIEW ONLY", 12, "accent", mono=True)
        text(draw, (196, 476), "No signals sent", 11, "muted", mono=True)
    else:
        text(draw, (49, 472), "Local ledger. Explicit evidence.", 13, "muted")

    line(draw, [(48, 535), (1072, 535)], "line", 1)
    for index, label in enumerate(["01 Record", "02 Explain", "03 Review"]):
        x = 48 + index * 142
        if index == stage:
            box(draw, (x, 552, x + 127, 582), "accent_soft", radius=7)
        text(draw, (x + 13, 560), label, 12, "accent" if index == stage else "muted", mono=True)
    text(draw, (805, 561), "NOT A LIVE PROCESS VIEW", 11, "muted", mono=True)
    return image.resize((WIDTH, HEIGHT), Image.Resampling.LANCZOS)


def wordmark(theme):
    image = Image.new("RGBA", (1060 * SCALE, 190 * SCALE))
    draw = ImageDraw.Draw(image)
    ink = "#edf5f5" if theme == "dark" else "#1b2e3e"
    accent = "#9bd0c1" if theme == "dark" else "#18655e"
    mark(draw, 8, 23, 144, accent)
    text(draw, (174, 32), "closingtime", 114, ink, weight=650)
    left, top, right, bottom = image.getbbox()
    padding = 12 * SCALE
    image = image.crop((left - padding, top - padding, right + padding, bottom + padding))
    image.resize((round(image.width / SCALE), round(image.height / SCALE)),
                 Image.Resampling.LANCZOS).save(ASSETS / f"logo-{theme}.png")


def badges():
    directory = ASSETS / "badges"
    directory.mkdir(parents=True, exist_ok=True)
    specifications = [
        ("license", "license", "Apache 2.0", 47, 78, "#18655e"),
        ("rust", "rust", "1.85+", 34, 47, "#685847"),
        ("platforms", "platforms", "Linux / macOS", 63, 99, "#385ba4"),
        ("status", "status", "prototype", 46, 72, "#805522"),
    ]
    for filename, label, value, left, right, color in specifications:
        width = left + right
        svg = (
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="22" '
            f'role="img" aria-label="{escape(label)}: {escape(value)}">'
            f'<title>{escape(label)}: {escape(value)}</title>'
            f'<clipPath id="r"><rect width="{width}" height="22" rx="4"/></clipPath>'
            f'<g clip-path="url(#r)"><rect width="{left}" height="22" fill="#35474f"/>'
            f'<rect x="{left}" width="{right}" height="22" fill="{color}"/></g>'
            '<g fill="#fff" text-anchor="middle" font-family="Verdana,Arial,sans-serif" font-size="10">'
            f'<text x="{left / 2}" y="15">{escape(label)}</text>'
            f'<text x="{left + right / 2}" y="15">{escape(value)}</text></g></svg>\n'
        )
        (directory / f"{filename}.svg").write_text(svg)


def main():
    ASSETS.mkdir(parents=True, exist_ok=True)
    badges()
    for theme in ("light", "dark"):
        wordmark(theme)
    frames = [render(index / 12) for index in range(126)]
    palette_source = Image.new("RGB", (WIDTH, HEIGHT * 3))
    for index, time in enumerate([2, 5, 9]):
        palette_source.paste(render(time), (0, index * HEIGHT))
    palette = palette_source.quantize(colors=128, method=Image.Quantize.MEDIANCUT)
    frames = [frame.quantize(palette=palette, dither=Image.Dither.NONE) for frame in frames]
    durations = [80 if index % 3 else 90 for index in range(len(frames))]
    durations[-1] = 2500
    # Play twice, then leave the final preview state visible.
    frames[0].save(ASSETS / "ownership-walkthrough.gif", save_all=True,
                   append_images=frames[1:], duration=durations, loop=1,
                   optimize=True, disposal=1)
    render(9).save(ASSETS / "ownership-static.png", optimize=True)
    print("Rendered light/dark wordmarks, walkthrough GIF, and static fallback.")


if __name__ == "__main__":
    main()
