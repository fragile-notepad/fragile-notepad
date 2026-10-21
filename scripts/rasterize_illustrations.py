"""Render the original Info illustration to reproducible, straight RGBA pixels.

Uses the illustration SVG path parser and Pillow. SVG features here are
intentionally limited to paths, flat fills/strokes, and a single path clip.
"""
from pathlib import Path
from xml.etree import ElementTree as ET

from PIL import Image, ImageChops

from illustration_raster_helpers import draw_filled_path, draw_round_line, parse_path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets/illustrations/macaw-quill.svg"
SCALE = 8
SIZE = (400, 440)


def color(value):
    value = value.lstrip("#")
    return tuple(int(value[i:i + 2], 16) for i in (0, 2, 4)) + (255,)


def render():
    from PIL import ImageDraw

    svg = ET.parse(SOURCE).getroot()
    viewbox = (0, 0, 100, 110)
    canvas = Image.new("RGBA", (100 * SCALE, 110 * SCALE))
    clips = {}
    for clip in svg.findall(".//{*}clipPath"):
        mask = Image.new("RGBA", canvas.size)
        for path in clip.findall("{*}path"):
            draw_filled_path(mask, parse_path(path.attrib["d"]), viewbox, SCALE, (255,) * 4)
        clips[clip.attrib["id"]] = mask.getchannel("A")

    def draw(element, inherited_clip=None):
        tag = element.tag.rsplit("}", 1)[-1]
        if tag == "defs":
            return
        clip = element.attrib.get("clip-path", inherited_clip)
        if tag == "path":
            layer = Image.new("RGBA", canvas.size)
            paths = parse_path(element.attrib["d"])
            fill = element.attrib.get("fill", "none")
            if fill != "none":
                draw_filled_path(layer, paths, viewbox, SCALE, color(fill))
            if "stroke" in element.attrib:
                width = round(float(element.attrib.get("stroke-width", 1)) * SCALE)
                for points in paths:
                    draw_round_line(ImageDraw.Draw(layer), [(x * SCALE, y * SCALE) for x, y in points], width, color(element.attrib["stroke"]))
            if clip:
                layer.putalpha(ImageChops.multiply(layer.getchannel("A"), clips[clip[5:-1]]))
            canvas.alpha_composite(layer)
        for child in element:
            draw(child, clip)

    draw(svg)
    # Pillow resamples RGBA in premultiplied space, then returns straight RGBA.
    result = canvas.resize(SIZE, Image.Resampling.LANCZOS)
    SOURCE.with_suffix(".rgba").write_bytes(result.tobytes())
    preview = ROOT / "target/animation-review/macaw-art-preview.png"
    preview.parent.mkdir(parents=True, exist_ok=True)
    result.save(preview)


if __name__ == "__main__":
    render()
