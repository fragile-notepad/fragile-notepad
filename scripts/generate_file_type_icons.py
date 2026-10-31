"""Generate desktop document icons from the large and small file-type SVGs."""

from io import BytesIO
import json
from pathlib import Path

from PIL import Image
import resvg_py


ROOT = Path(__file__).resolve().parents[1]
SOURCES = ROOT / "assets/icons/file-types"
OUTPUT = ROOT / "target/file-type-icons"


def render(name, size):
    # The small artwork keeps its badge readable at Explorer's smallest sizes.
    variant = "-small" if size <= 32 else ""
    source = (SOURCES / "svg" / f"{name}{variant}.svg").read_text(encoding="utf-8")
    png = resvg_py.svg_to_bytes(svg_string=source, width=size, height=size)
    with Image.open(BytesIO(png)) as image:
        return image.convert("RGBA")


def generate():
    windows = OUTPUT / "windows"
    macos = OUTPUT / "macos"
    windows.mkdir(parents=True, exist_ok=True)
    macos.mkdir(parents=True, exist_ok=True)
    for entry in json.loads((SOURCES / "catalog.json").read_text(encoding="utf-8")):
        name = entry["id"]
        sizes = (16, 24, 32, 48, 64, 128, 256)
        images = [render(name, size) for size in sizes]
        images[-1].save(windows / f"{name}.ico", append_images=images[:-1],
                        sizes=[(size, size) for size in sizes])
        sizes = (16, 32, 64, 128, 256, 512, 1024)
        images = [render(name, size) for size in sizes]
        images[-1].save(macos / f"{name}.icns", append_images=images[:-1])


if __name__ == "__main__":
    generate()
