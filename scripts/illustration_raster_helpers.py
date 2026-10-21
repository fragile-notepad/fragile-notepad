"""Path parsing and Pillow drawing for the quill illustration renderer."""

from __future__ import annotations

import math
import re

from PIL import Image, ImageChops, ImageDraw

TOKEN_RE = re.compile(
    r"[AaCcHhLlMmQqSsVvZz]|[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?"
)
COMMAND_RE = re.compile(r"[A-Za-z]")
SUPPORTED_COMMANDS = set("AaCcHhLlMmQqSsVvZz")


def is_command(token: str) -> bool:
    return len(token) == 1 and token.isalpha()


def cubic(
    p0: tuple[float, float],
    p1: tuple[float, float],
    p2: tuple[float, float],
    p3: tuple[float, float],
    steps: int = 18,
) -> list[tuple[float, float]]:
    points = []
    for index in range(1, steps + 1):
        t = index / steps
        mt = 1.0 - t
        mt2 = mt * mt
        mt3 = mt2 * mt
        t2 = t * t
        t3 = t2 * t

        x = mt3 * p0[0] + 3.0 * mt2 * t * p1[0] + 3.0 * mt * t2 * p2[0] + t3 * p3[0]
        y = mt3 * p0[1] + 3.0 * mt2 * t * p1[1] + 3.0 * mt * t2 * p2[1] + t3 * p3[1]
        points.append((x, y))
    return points


def quadratic(
    p0: tuple[float, float],
    p1: tuple[float, float],
    p2: tuple[float, float],
    steps: int = 18,
) -> list[tuple[float, float]]:
    points = []
    for index in range(1, steps + 1):
        t = index / steps
        mt = 1.0 - t
        mt2 = mt * mt
        t2 = t * t

        x = mt2 * p0[0] + 2.0 * mt * t * p1[0] + t2 * p2[0]
        y = mt2 * p0[1] + 2.0 * mt * t * p1[1] + t2 * p2[1]
        points.append((x, y))
    return points


def elliptical_arc(
    p0: tuple[float, float],
    radius_x: float,
    radius_y: float,
    x_axis_rotation: float,
    large_arc: bool,
    sweep: bool,
    p1: tuple[float, float],
    steps: int = 48,
) -> list[tuple[float, float]]:
    if radius_x == 0.0 or radius_y == 0.0 or p0 == p1:
        return [p1]

    radius_x, radius_y = abs(radius_x), abs(radius_y)
    phi = math.radians(x_axis_rotation % 360.0)
    cos_phi, sin_phi = math.cos(phi), math.sin(phi)

    dx = (p0[0] - p1[0]) / 2.0
    dy = (p0[1] - p1[1]) / 2.0
    x1_prime = cos_phi * dx + sin_phi * dy
    y1_prime = -sin_phi * dx + cos_phi * dy

    radius_scale = (x1_prime**2) / (radius_x**2) + (y1_prime**2) / (radius_y**2)
    if radius_scale > 1.0:
        scale = math.sqrt(radius_scale)
        radius_x *= scale
        radius_y *= scale

    rx2, ry2 = radius_x**2, radius_y**2
    x1p2, y1p2 = x1_prime**2, y1_prime**2
    denominator = rx2 * y1p2 + ry2 * x1p2
    if denominator == 0.0:
        return [p1]

    sign = -1.0 if large_arc == sweep else 1.0
    coefficient = sign * math.sqrt(max(0.0, (rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2) / denominator))
    cx_prime = coefficient * radius_x * y1_prime / radius_y
    cy_prime = -coefficient * radius_y * x1_prime / radius_x

    center_x = cos_phi * cx_prime - sin_phi * cy_prime + (p0[0] + p1[0]) / 2.0
    center_y = sin_phi * cx_prime + cos_phi * cy_prime + (p0[1] + p1[1]) / 2.0

    def angle(u: tuple[float, float], v: tuple[float, float]) -> float:
        return math.atan2(u[0] * v[1] - u[1] * v[0], u[0] * v[0] + u[1] * v[1])

    start_vector = ((x1_prime - cx_prime) / radius_x, (y1_prime - cy_prime) / radius_y)
    end_vector = ((-x1_prime - cx_prime) / radius_x, (-y1_prime - cy_prime) / radius_y)
    start_angle = angle((1.0, 0.0), start_vector)
    delta_angle = angle(start_vector, end_vector)

    if not sweep and delta_angle > 0:
        delta_angle -= 2.0 * math.pi
    elif sweep and delta_angle < 0:
        delta_angle += 2.0 * math.pi

    segment_count = max(4, math.ceil(abs(delta_angle) / (math.pi / 16.0)))
    segment_count = max(segment_count, steps if abs(delta_angle) > math.pi else steps // 2)
    points = []

    for index in range(1, segment_count + 1):
        theta = start_angle + delta_angle * index / segment_count
        cos_t, sin_t = math.cos(theta), math.sin(theta)
        x = center_x + radius_x * cos_t * cos_phi - radius_y * sin_t * sin_phi
        y = center_y + radius_x * cos_t * sin_phi + radius_y * sin_t * cos_phi
        points.append((x, y))

    return points


def parse_path(d: str) -> list[list[tuple[float, float]]]:
    unsupported = set(COMMAND_RE.findall(d)) - SUPPORTED_COMMANDS
    if unsupported:
        commands = ", ".join(sorted(unsupported))
        raise ValueError(f"Unsupported SVG path command(s): {commands}")

    tokens = TOKEN_RE.findall(d.replace(",", " "))
    index = 0
    command = ""
    current = (0.0, 0.0)
    start = (0.0, 0.0)
    last_cubic_control: tuple[float, float] | None = None
    subpaths: list[list[tuple[float, float]]] = []
    subpath: list[tuple[float, float]] = []

    def number() -> float:
        nonlocal index
        if index >= len(tokens) or is_command(tokens[index]):
            raise ValueError(f"Expected number in path data: {d}")
        value = float(tokens[index])
        index += 1
        return value

    def has_numbers() -> bool:
        return index < len(tokens) and not is_command(tokens[index])

    def point(relative: bool) -> tuple[float, float]:
        x, y = number(), number()
        return (current[0] + x, current[1] + y) if relative else (x, y)

    def finish_subpath() -> None:
        nonlocal subpath
        if subpath:
            subpaths.append(subpath)
            subpath = []

    while index < len(tokens):
        if is_command(tokens[index]):
            command = tokens[index]
            index += 1
        elif not command:
            raise ValueError(f"Path starts without a command: {d}")

        relative = command.islower()
        op = command.upper()

        match op:
            case "M":
                last_cubic_control = None
                first = True
                while has_numbers():
                    destination = point(relative)
                    if first:
                        finish_subpath()
                        subpath = [destination]
                        start = destination
                        first = False
                    else:
                        subpath.append(destination)
                    current = destination
                command = "l" if relative else "L"
            case "L":
                last_cubic_control = None
                while has_numbers():
                    current = point(relative)
                    subpath.append(current)
            case "H":
                last_cubic_control = None
                while has_numbers():
                    x = number()
                    if relative:
                        x += current[0]
                    current = (x, current[1])
                    subpath.append(current)
            case "V":
                last_cubic_control = None
                while has_numbers():
                    y = number()
                    if relative:
                        y += current[1]
                    current = (current[0], y)
                    subpath.append(current)
            case "C":
                while has_numbers():
                    p1, p2, p3 = point(relative), point(relative), point(relative)
                    subpath.extend(cubic(current, p1, p2, p3))
                    current = p3
                    last_cubic_control = p2
            case "S":
                while has_numbers():
                    p1 = (
                        (2.0 * current[0] - last_cubic_control[0],
                         2.0 * current[1] - last_cubic_control[1])
                        if last_cubic_control is not None
                        else current
                    )
                    p2, p3 = point(relative), point(relative)
                    subpath.extend(cubic(current, p1, p2, p3))
                    current = p3
                    last_cubic_control = p2
            case "Q":
                last_cubic_control = None
                while has_numbers():
                    p1, p2 = point(relative), point(relative)
                    subpath.extend(quadratic(current, p1, p2))
                    current = p2
            case "A":
                last_cubic_control = None
                while has_numbers():
                    radius_x = number()
                    radius_y = number()
                    x_axis_rotation = number()
                    large_arc = number() != 0.0
                    sweep = number() != 0.0
                    p1 = point(relative)
                    subpath.extend(
                        elliptical_arc(
                            current, radius_x, radius_y, x_axis_rotation, large_arc, sweep, p1
                        )
                    )
                    current = p1
            case "Z":
                last_cubic_control = None
                if subpath and subpath[-1] != start:
                    subpath.append(start)
                finish_subpath()
                current = start
            case _:
                raise ValueError(f"Unsupported SVG path command: {command}")

    finish_subpath()
    return subpaths


def scale_point(
    point: tuple[float, float],
    view_box: tuple[float, float, float, float],
    scale: float,
) -> tuple[float, float]:
    min_x, min_y, _, _ = view_box
    return ((point[0] - min_x) * scale, (point[1] - min_y) * scale)


def draw_round_line(
    draw: ImageDraw.ImageDraw,
    points: list[tuple[float, float]],
    width: int,
    color: tuple[int, int, int, int],
) -> None:
    if not points:
        return

    radius = width / 2.0
    if len(points) == 1:
        x, y = points[0]
        draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill=color)
        return

    # Draw the main line. joint="curve" natively rounds intermediate corners
    draw.line(points, fill=color, width=width, joint="curve")

    # Explicitly round only the path endpoints (start and finish caps)
    for x, y in (points[0], points[-1]):
        draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill=color)


def draw_filled_path(
    image: Image.Image,
    subpaths: list[list[tuple[float, float]]],
    view_box: tuple[float, float, float, float],
    scale: float,
    color: tuple[int, int, int, int],
) -> None:
    mask = Image.new("1", image.size, 0)

    for subpath in subpaths:
        scaled = [scale_point(point, view_box, scale) for point in subpath]
        if len(scaled) < 3:
            continue

        subpath_mask = Image.new("1", image.size, 0)
        ImageDraw.Draw(subpath_mask).polygon(scaled, fill=1)
        mask = ImageChops.logical_xor(mask, subpath_mask)

    fill = Image.new("RGBA", image.size, color)
    image.paste(fill, (0, 0), mask.convert("L"))
