"""Compile the application's small SVG icons into Rust vector paths.

Only the flat-paint icon contract is accepted. Artwork needing SVG filters,
transforms, gradients, embedded images, or text must use another asset pipeline.
This compiler uses only the Python standard library and never rasterizes paths.
"""

from __future__ import annotations

import argparse
import math
from pathlib import Path
import re
from xml.etree import ElementTree

ROOT = Path(__file__).resolve().parents[1]
FAMILIES = ("colored", "heroicons", "bootstrap")
NUMBER = re.compile(r"[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?")
PAINT_ATTRIBUTES = {
    "fill", "stroke", "fill-rule", "stroke-width", "stroke-linecap", "stroke-linejoin"
}
DEFAULTS = {
    "fill": "#000000",
    "stroke": "none",
    "fill-rule": "nonzero",
    "stroke-width": "1",
    "stroke-linecap": "butt",
    "stroke-linejoin": "miter",
}
SVG_NAMESPACE = "http://www.w3.org/2000/svg"


def rust_number(value: float) -> str:
    if not math.isfinite(value) or abs(value) > 3.4028234663852886e38:
        raise ValueError(f"Number is outside the finite f32 range: {value}")
    value = 0.0 if value == 0.0 else value
    return repr(float(value))


def point(x: float, y: float) -> str:
    return f"Point::new({rust_number(x)}, {rust_number(y)})"


class PathReader:
    def __init__(self, data: str):
        self.data = data
        self.position = 0

    def peek(self) -> str:
        while self.position < len(self.data) and self.data[self.position] in " \t\r\n,":
            self.position += 1
        return self.data[self.position:self.position + 1]

    def number(self) -> float:
        self.peek()
        match = NUMBER.match(self.data, self.position)
        if match is None:
            raise ValueError(f"Expected path number at {self.data[self.position:]!r}")
        self.position = match.end()
        result = float(match.group())
        rust_number(result)
        return result

    def flag(self) -> bool:
        value = self.peek()
        if value not in ("0", "1"):
            raise ValueError(f"Expected arc flag 0 or 1 at {self.data[self.position:]!r}")
        self.position += 1
        return value == "1"


def parse_path(data: str) -> list[str]:
    """Normalize SVG path commands while preserving their curves and subpaths."""
    reader = PathReader(data)
    result = []
    current = (0.0, 0.0)
    start = current
    previous = ""
    command = ""
    cubic_control = current
    quadratic_control = current

    def destination(relative: bool) -> tuple[float, float]:
        x, y = reader.number(), reader.number()
        return (x + current[0], y + current[1]) if relative else (x, y)

    while reader.peek():
        token = reader.peek()
        if token.isalpha():
            command = token
            reader.position += 1
            if command not in "MmLlHhVvCcSsQqTtAaZz":
                raise ValueError(f"Unsupported path command: {command}")
            if not result and command not in "Mm":
                raise ValueError("An SVG path must begin with moveto")
            if command in "Zz":
                result.append("builder.close();")
                current = start
                previous = "Z"
                command = ""
                continue
        elif not command:
            raise ValueError(f"Expected path command at {data[reader.position:]!r}")

        relative = command.islower()
        operation = command.upper()
        match operation:
            case "M":
                current = destination(relative)
                start = current
                result.append(f"builder.move_to({point(*current)});")
                command = "l" if relative else "L"
            case "L":
                current = destination(relative)
                result.append(f"builder.line_to({point(*current)});")
            case "H":
                x = reader.number()
                current = (x + current[0] if relative else x, current[1])
                result.append(f"builder.line_to({point(*current)});")
            case "V":
                y = reader.number()
                current = (current[0], y + current[1] if relative else y)
                result.append(f"builder.line_to({point(*current)});")
            case "C":
                control_a = destination(relative)
                control_b = destination(relative)
                end = destination(relative)
                result.append(
                    f"builder.bezier_curve_to({point(*control_a)}, {point(*control_b)}, {point(*end)});"
                )
                cubic_control, current = control_b, end
            case "S":
                control_a = (
                    (2 * current[0] - cubic_control[0], 2 * current[1] - cubic_control[1])
                    if previous in ("C", "S") else current
                )
                control_b = destination(relative)
                end = destination(relative)
                result.append(
                    f"builder.bezier_curve_to({point(*control_a)}, {point(*control_b)}, {point(*end)});"
                )
                cubic_control, current = control_b, end
            case "Q":
                control = destination(relative)
                end = destination(relative)
                result.append(f"builder.quadratic_curve_to({point(*control)}, {point(*end)});")
                quadratic_control, current = control, end
            case "T":
                control = (
                    (2 * current[0] - quadratic_control[0], 2 * current[1] - quadratic_control[1])
                    if previous in ("Q", "T") else current
                )
                end = destination(relative)
                result.append(f"builder.quadratic_curve_to({point(*control)}, {point(*end)});")
                quadratic_control, current = control, end
            case "A":
                radius_x, radius_y = reader.number(), reader.number()
                if radius_x < 0 or radius_y < 0:
                    raise ValueError("Arc radii must be nonnegative")
                rotation = reader.number()
                large_arc, sweep = reader.flag(), reader.flag()
                end = destination(relative)
                result.append(
                    f"svg_arc(builder, {point(*current)}, "
                    f"Vector::new({rust_number(radius_x)}, {rust_number(radius_y)}), "
                    f"{rust_number(rotation)}, {str(large_arc).lower()}, {str(sweep).lower()}, "
                    f"{point(*end)});"
                )
                current = end
            case _:
                raise ValueError(f"Unsupported path command: {command}")
        previous = operation
    return result


def paint(value: str) -> str:
    value = value.strip()
    if value == "none":
        return "Paint::None"
    if value == "currentColor":
        return "Paint::CurrentColor"
    if not re.fullmatch(r"#[0-9a-fA-F]{6}", value):
        raise ValueError(f"Unsupported paint {value!r}; expected none, currentColor, or #RRGGBB")
    red, green, blue = (int(value[index:index + 2], 16) for index in (1, 3, 5))
    return f"Paint::Solid(Color::from_rgb8({red}, {green}, {blue}))"


def numeric_attribute(value: str) -> float:
    if NUMBER.fullmatch(value.strip()) is None:
        raise ValueError(f"Expected a unitless number: {value!r}")
    number = float(value)
    rust_number(number)
    return number


def validate_style(style: dict[str, str]) -> None:
    paint(style["fill"])
    paint(style["stroke"])
    if style["fill-rule"] not in ("nonzero", "evenodd"):
        raise ValueError(f"Unsupported fill-rule: {style['fill-rule']}")
    if numeric_attribute(style["stroke-width"]) < 0:
        raise ValueError("Stroke width must be nonnegative")
    if style["stroke-linecap"] not in ("butt", "square", "round"):
        raise ValueError(f"Unsupported stroke-linecap: {style['stroke-linecap']}")
    if style["stroke-linejoin"] not in ("miter", "bevel", "round"):
        raise ValueError(f"Unsupported stroke-linejoin: {style['stroke-linejoin']}")


def tag_name(element: ElementTree.Element) -> str:
    tag = element.tag
    if tag.startswith("{"):
        namespace, tag = tag[1:].split("}", 1)
        if namespace != SVG_NAMESPACE:
            raise ValueError(f"Unsupported element namespace: {namespace}")
    return tag


def compile_svg(source: str, name: str) -> str:
    root = ElementTree.fromstring(source)
    if tag_name(root) != "svg":
        raise ValueError("An icon must have an SVG root")
    view_box = root.attrib.get("viewBox", "").replace(",", " ").split()
    if len(view_box) != 4:
        raise ValueError("An icon must have a four-number viewBox")
    bounds = [numeric_attribute(value) for value in view_box]
    if bounds[2] <= 0 or bounds[3] <= 0:
        raise ValueError("The viewBox width and height must be positive")
    paths = []

    def visit(element: ElementTree.Element, inherited: dict[str, str], is_root: bool = False):
        tag = tag_name(element)
        if tag not in (("svg",) if is_root else ("g", "path")):
            raise ValueError(f"Unsupported SVG element: {tag}")
        allowed = PAINT_ATTRIBUTES | {"id"}
        if is_root:
            allowed |= {"viewBox"}
        if tag == "path":
            allowed |= {"d"}
        unsupported = element.attrib.keys() - allowed
        if unsupported:
            raise ValueError(f"Unsupported {tag} attributes: {', '.join(sorted(unsupported))}")
        if element.text and element.text.strip():
            raise ValueError(f"Unexpected text inside SVG {tag}")
        style = inherited | {
            key: value.strip() for key, value in element.attrib.items() if key in PAINT_ATTRIBUTES
        }
        validate_style(style)
        if tag == "path":
            if len(element):
                raise ValueError("An SVG path must not have children")
            if "d" not in element.attrib:
                raise ValueError("An SVG path must have path data")
            commands = parse_path(element.attrib["d"])
            path = ["        PaintedPath {", "            path: Path::new(|builder| {"]
            path.extend(f"                {command}" for command in commands)
            path.extend([
                "            }),",
                f"            fill: {paint(style['fill'])},",
                f"            stroke: {paint(style['stroke'])},",
                f"            fill_rule: fill::Rule::{'EvenOdd' if style['fill-rule'] == 'evenodd' else 'NonZero'},",
                f"            stroke_width: {rust_number(numeric_attribute(style['stroke-width']))},",
                f"            line_cap: LineCap::{style['stroke-linecap'].title()},",
                f"            line_join: LineJoin::{style['stroke-linejoin'].title()},",
                "        },",
            ])
            paths.extend(path)
        for child in element:
            visit(child, style)
            if child.tail and child.tail.strip():
                raise ValueError("Unexpected text between SVG elements")

    visit(root, DEFAULTS, True)
    if not paths:
        raise ValueError("An icon must contain at least one path")
    view_box_rust = ", ".join(rust_number(value) for value in bounds)
    return "\n".join([
        f"pub(in crate::ui::icons) static {name}: LazyLock<VectorIcon> = LazyLock::new(|| VectorIcon {{",
        f"    view_box: [{view_box_rust}],",
        "    paths: vec![",
        *paths,
        "    ],",
        "});",
    ])


def compile_icons(icons_root: Path) -> str:
    output = ["// Generated by scripts/compile_vector_icons.py. Do not edit."]
    for family in FAMILIES:
        sources = sorted((icons_root / family / "svg").glob("*.svg"))
        if not sources:
            raise ValueError(f"No SVG icons found for {family}")
        for source in sources:
            name = f"{family}_{source.stem}".replace("-", "_").upper()
            if re.fullmatch(r"[A-Z_][A-Z0-9_]*", name) is None:
                raise ValueError(f"Unsupported icon filename: {source.name}")
            try:
                output.append(compile_svg(source.read_text(encoding="utf-8"), name))
            except (ValueError, ElementTree.ParseError) as error:
                raise ValueError(f"{source}: {error}") from error
    return "\n\n".join(output) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--icons-dir", type=Path, default=ROOT / "assets/icons")
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    generated = compile_icons(args.icons_dir)
    args.out_dir.mkdir(parents=True, exist_ok=True)
    output = args.out_dir / "vector_icons.rs"
    if not output.exists() or output.read_text(encoding="utf-8") != generated:
        output.write_text(generated, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
