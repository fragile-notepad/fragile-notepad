#!/usr/bin/env python3
"""Suggest CJK optical-weight profiles from installed, static font faces.

This is an offline calibration aid, not a perceptual-equivalence guarantee. It
renders the same common Han glyphs in each family and compares ink density and
an approximate stroke width with a regular reference. Modern Hangul uses the
stroke-width estimate, which accounts for its different number of strokes.
Only real normal-width, upright faces are eligible. The application still
validates the selected face and complete-grapheme coverage at runtime.

Install the optional dependencies with:
    python -m pip install -r scripts/requirements-font-profiles.txt
Then run:
    python scripts/generate_font_profiles.py

The output defaults to target/generated-cjk-profiles.json. Inspect its sibling
measurement report and render a preview before adopting suggested profiles.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
import math
import os
from pathlib import Path
from statistics import mean
import sys
import tempfile
from typing import Any, Iterable


HAN_SAMPLE = "日月山水木火土金人大小中文世界永骨直令門國語漢讀書東西南北"
HANGUL_SAMPLE = "가각간갈감갑강거고구기나다라마바사아자차카타파하한글한국서울문장학교봄밤빛"
DEFAULT_FAMILIES = (
    "Microsoft YaHei",
    "Microsoft JhengHei",
    "Yu Gothic",
    "Malgun Gothic",
)
STANDARD_WEIGHTS = tuple(range(100, 901, 100))
CALIBRATION_DEFAULTS = {
    "sizes": (32, 48, 64),
    "min_improvement": 0.15,
    "max_error": 0.15,
    "max_size_regression": 0.02,
}


@dataclass(frozen=True)
class Face:
    path: Path
    index: int
    family: str
    aliases: tuple[str, ...]
    weight: int
    width: int
    italic: bool
    bold: bool
    variable: bool
    coverage: frozenset[int]
    weight_axis: tuple[float, float] | None = None

    @property
    def normal(self) -> bool:
        return self.width == 5 and not self.italic

    def covers(self, text: str) -> bool:
        return all(ord(character) in self.coverage for character in text)

    def description(self) -> dict[str, Any]:
        return {
            "path": str(self.path),
            "index": self.index,
            "family": self.family,
            "face_weight": self.weight,
            "width_class": self.width,
            "italic": self.italic,
            "bold": self.bold,
            "variable": self.variable,
            "weight_axis": self.weight_axis,
        }


@dataclass(frozen=True)
class GlyphMetric:
    ink_density: float
    stroke_width: float


@dataclass(frozen=True)
class MeasuredFace:
    face: Face
    requested_weight: int
    error: float
    error_by_size: dict[str, float]


def default_font_dirs() -> list[Path]:
    if sys.platform == "win32":
        dirs = [Path(os.environ.get("WINDIR", "C:/Windows")) / "Fonts"]
        if local := os.environ.get("LOCALAPPDATA"):
            dirs.append(Path(local) / "Microsoft/Windows/Fonts")
        return dirs
    if sys.platform == "darwin":
        return [Path("/System/Library/Fonts"), Path("/Library/Fonts"), Path.home() / "Library/Fonts"]
    return [
        Path("/usr/share/fonts"),
        Path("/usr/local/share/fonts"),
        Path.home() / ".local/share/fonts",
        Path.home() / ".fonts",
    ]


def family_names(font: Any) -> tuple[str, ...]:
    """Use fontdb's typographic-family preference, including localized aliases."""
    names = font["name"].names
    for name_id in (16, 1):
        records = [record for record in names if record.nameID == name_id and record.isUnicode()]
        # fontdb also accepts an English Macintosh name when US English is absent.
        if not any(record.platformID == 3 and record.langID == 0x409 for record in records):
            records.extend(
                record for record in names
                if record.nameID == name_id and record.platformID == 1
                and record.platEncID == 0 and record.langID == 0
            )
        records.sort(key=lambda record: not (
            (record.platformID == 3 and record.langID == 0x409)
            or (record.platformID == 1 and record.langID == 0)
        ))
        decoded = []
        for record in records:
            try:
                value = record.toUnicode().strip()
            except (UnicodeError, ValueError):
                continue
            if value and value not in decoded:
                decoded.append(value)
        if decoded:
            return tuple(decoded)
    return ()


def read_face(font: Any, path: Path, index: int) -> Face | None:
    aliases = family_names(font)
    if not aliases or "OS/2" not in font:
        return None
    os2 = font["OS/2"]
    selection = int(os2.fsSelection)
    italic_angle = float(font["post"].italicAngle) if "post" in font else 0.0
    cmap = font.getBestCmap() or {}
    coverage = frozenset(codepoint for codepoint, glyph in cmap.items() if font.getGlyphID(glyph) != 0)
    axes = getattr(font["fvar"], "axes", ()) if "fvar" in font else ()
    weight_axis = next(
        ((float(axis.minValue), float(axis.maxValue)) for axis in axes if axis.axisTag == "wght"),
        None,
    )
    return Face(
        path=path,
        index=index,
        family=aliases[0],
        aliases=aliases,
        weight=int(os2.usWeightClass),
        width=int(os2.usWidthClass),
        italic=bool(selection & ((1 << 0) | (1 << 9))) or italic_angle != 0.0,
        bold=bool(selection & (1 << 5)),
        variable="fvar" in font,
        coverage=coverage,
        weight_axis=weight_axis,
    )


def scan_faces(directories: Iterable[Path]) -> tuple[list[Face], list[str]]:
    from fontTools.ttLib import TTCollection, TTFont

    files: set[Path] = set()
    warnings = []
    for directory in directories:
        if not directory.is_dir():
            warnings.append(f"Font directory is unavailable: {directory}")
            continue
        try:
            files.update(
                path.resolve() for path in directory.rglob("*")
                if path.is_file() and path.suffix.lower() in (".ttf", ".otf", ".ttc", ".otc")
            )
        except OSError as error:
            warnings.append(f"Could not fully scan {directory}: {error}")
    faces = []
    for path in sorted(files, key=lambda value: str(value).casefold()):
        try:
            with path.open("rb") as source:
                collection = source.read(4) == b"ttcf"
            if collection:
                fonts = TTCollection(path, lazy=True)
                try:
                    for index, font in enumerate(fonts.fonts):
                        if face := read_face(font, path, index):
                            faces.append(face)
                finally:
                    fonts.close()
            else:
                with TTFont(path, lazy=True) as font:
                    if face := read_face(font, path, 0):
                        faces.append(face)
        except Exception as error:
            # A bad or unsupported installed font must not prevent other profiles.
            warnings.append(f"Skipped unreadable font {path}: {type(error).__name__}: {error}")
    return faces, warnings


def query_weight(weights: Iterable[int], requested: int) -> int | None:
    """Mirror fontdb 0.23's weight selection after normal style/stretch matching."""
    available = sorted(set(weights))
    if not available:
        return None
    if requested in available:
        return requested
    if 400 <= requested < 450 and 500 in available:
        return 500
    if 450 <= requested <= 500 and 400 in available:
        return 400
    if requested <= 500:
        thinner = [weight for weight in available if weight <= requested]
        return max(thinner) if thinner else min(available)
    heavier = [weight for weight in available if weight >= requested]
    return min(heavier) if heavier else max(available)


def requested_weight(face: Face, family_faces: list[Face]) -> int | None:
    # Prefer the nearest standard request, but only when its query is unambiguous.
    choices = []
    for requested in STANDARD_WEIGHTS:
        selected_weight = query_weight((candidate.weight for candidate in family_faces), requested)
        selected = [candidate for candidate in family_faces if candidate.weight == selected_weight]
        if len(selected) == 1 and selected[0] == face:
            choices.append(requested)
    return min(choices, key=lambda weight: (abs(weight - face.weight), weight)) if choices else None


def static_regular_face(family_faces: list[Face]) -> Face | None:
    """Return the one real Regular face eligible for static calibration."""
    if any(face.variable for face in family_faces):
        return None
    regular = [face for face in family_faces if face.weight == 400 and not face.bold]
    if len(regular) != 1 or requested_weight(regular[0], family_faces) != 400:
        return None
    return regular[0]


def has_modern_hangul(face: Face) -> bool:
    return all(codepoint in face.coverage for codepoint in range(0xAC00, 0xD7A4))


def mask_metric(pixels: Iterable[int], width: int, height: int, size: int) -> GlyphMetric:
    """Estimate average stroke thickness from grayscale area / total variation.

    Accounting for the outside edge keeps the estimate independent of mask
    cropping. Dividing by the em size makes results comparable across font sizes.
    This is a raster proxy: corners, hinting, intersections and design affect it.
    """
    values = list(pixels)
    area = sum(values) / 255.0
    if not width or not height or area == 0:
        raise ValueError("A sample glyph rendered no ink")
    variation = 0
    for y in range(height):
        row = y * width
        variation += values[row] + values[row + width - 1]
        variation += sum(abs(values[row + x] - values[row + x - 1]) for x in range(1, width))
    for x in range(width):
        variation += values[x] + values[(height - 1) * width + x]
        variation += sum(abs(values[y * width + x] - values[(y - 1) * width + x]) for y in range(1, height))
    return GlyphMetric(area / size**2, 2.0 * area / (variation / 255.0) / size)


def measure(face: Face, sample: str, sizes: tuple[int, ...]) -> dict[int, list[GlyphMetric]]:
    from PIL import ImageFont

    if not face.covers(sample):
        raise ValueError("Face does not cover the complete calibration sample")
    metrics = {}
    for size in sizes:
        # BASIC avoids environment-dependent optional shaping libraries. These
        # samples are independent precomposed glyphs and need no shaping.
        font = ImageFont.truetype(str(face.path), size=size, index=face.index, layout_engine=ImageFont.Layout.BASIC)
        glyphs = []
        for character in sample:
            mask = font.getmask(character, mode="L")
            glyphs.append(mask_metric(mask, *mask.size, size))
        metrics[size] = glyphs
    return metrics


def metric_summary(metrics: dict[int, list[GlyphMetric]]) -> dict[str, Any]:
    return {
        str(size): {
            "ink_density": round(mean(metric.ink_density for metric in glyphs), 8),
            "stroke_width": round(mean(metric.stroke_width for metric in glyphs), 8),
            "glyphs": len(glyphs),
        }
        for size, glyphs in metrics.items()
    }


def metric_error(
    metrics: dict[int, list[GlyphMetric]],
    reference: dict[int, list[GlyphMetric]],
    scope: str,
) -> tuple[float, dict[str, float]]:
    per_size = {}
    for size, glyphs in metrics.items():
        target = reference[size]
        if scope == "modern_hangul":
            # Comparing counts of inked pixels between distinct scripts would
            # reward simpler glyphs. Average stroke width provides a useful
            # suggestion without assuming identical glyph complexity.
            error = abs(math.log(
                mean(metric.stroke_width for metric in glyphs)
                / mean(metric.stroke_width for metric in target)
            ))
        else:
            error = mean(
                0.75 * abs(math.log(metric.stroke_width / other.stroke_width))
                + 0.25 * abs(math.log(metric.ink_density / other.ink_density))
                for metric, other in zip(glyphs, target, strict=True)
            )
        per_size[str(size)] = error
    return mean(per_size.values()), per_size


def select_profile(
    family: str,
    family_faces: list[Face],
    scope: str,
    reference: dict[int, list[GlyphMetric]],
    sizes: tuple[int, ...],
    min_improvement: float,
    max_error: float,
    max_size_regression: float = CALIBRATION_DEFAULTS["max_size_regression"],
) -> tuple[dict[str, Any] | None, dict[str, Any]]:
    report: dict[str, Any] = {"family": family, "scope": scope, "candidates": []}
    if any(face.variable for face in family_faces):
        report["decision"] = "retained_regular: variable family requires manual calibration"
        return None, report
    regular = static_regular_face(family_faces)
    if regular is None:
        report["decision"] = "retained_regular: missing or ambiguous real Regular face"
        return None, report
    sample = HANGUL_SAMPLE if scope == "modern_hangul" else HAN_SAMPLE
    eligible = [face for face in family_faces if 200 <= face.weight <= 500 and not face.bold]
    measured = []
    for face in eligible:
        entry = face.description()
        report["candidates"].append(entry)
        request = requested_weight(face, family_faces)
        if request is None:
            entry["rejected"] = "standard weight query cannot identify this face unambiguously"
            continue
        if scope == "modern_hangul" and not has_modern_hangul(face):
            entry["rejected"] = "incomplete modern Hangul syllable coverage"
            continue
        if not face.covers(sample):
            entry["rejected"] = "incomplete calibration-sample coverage"
            continue
        try:
            metrics = measure(face, sample, sizes)
        except (OSError, ValueError) as error:
            entry["rejected"] = f"cannot render sample: {error}"
            continue
        error, per_size = metric_error(metrics, reference, scope)
        entry.update({
            "requested_weight": request,
            "measurements": metric_summary(metrics),
            "error": round(error, 8),
            "error_by_size": {size: round(value, 8) for size, value in per_size.items()},
        })
        measured.append(MeasuredFace(face, request, error, per_size))
    baseline = next((candidate for candidate in measured if candidate.face == regular), None)
    if baseline is None:
        report["decision"] = "retained_regular: Regular cannot be calibrated for this scope"
        return None, report
    report["baseline_error"] = round(baseline.error, 8)
    best = min(
        measured,
        key=lambda candidate: (candidate.error, abs(candidate.face.weight - 400), candidate.face.weight),
    )
    improvement = baseline.error - best.error
    if best.face == regular:
        report["decision"] = "retained_regular: Regular is the closest measured face"
    elif best.error > max_error:
        report["decision"] = "retained_regular: companion remains too far from reference"
    elif improvement < 0.02 or improvement < baseline.error * min_improvement:
        report["decision"] = "retained_regular: companion improvement is too small"
    elif any(
        best.error_by_size[size] > baseline.error_by_size[size] + max_size_regression
        for size in baseline.error_by_size
    ):
        report["decision"] = "retained_regular: sampled-size regression exceeds tolerance"
    else:
        profile = {
            "family": family,
            "requested_weight": best.requested_weight,
            "face_weight": best.face.weight,
            "scope": scope,
        }
        report["decision"] = "suggested_companion"
        report["profile"] = profile
        report["relative_improvement"] = round(improvement / baseline.error, 8)
        return profile, report
    return None, report


def parse_sizes(value: str) -> tuple[int, ...]:
    try:
        sizes = tuple(sorted(set(int(part) for part in value.split(","))))
    except ValueError as error:
        raise argparse.ArgumentTypeError("Sizes must be comma-separated integers") from error
    if not sizes or any(size < 8 or size > 256 for size in sizes):
        raise argparse.ArgumentTypeError("Font sizes must be between 8 and 256 pixels")
    return sizes


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--font-dir", type=Path, action="append", help="scan this directory recursively; repeat to replace system defaults")
    parser.add_argument("--family", action="append", help="calibrate this exact family; repeat to replace the four native defaults")
    parser.add_argument("--reference-family", default="Microsoft JhengHei", help="upright Regular optical reference (default: Microsoft JhengHei)")
    parser.add_argument("--sizes", type=parse_sizes, default=CALIBRATION_DEFAULTS["sizes"], help="comma-separated pixel sizes (default: 32,48,64)")
    parser.add_argument("--output", type=Path, default=Path("target/generated-cjk-profiles.json"), help="suggested profile JSON destination")
    parser.add_argument("--report", type=Path, help="measurement JSON destination (default: OUTPUT.report.json)")
    parser.add_argument("--min-improvement", type=float, default=CALIBRATION_DEFAULTS["min_improvement"], help="minimum fractional error improvement (default: 0.15)")
    parser.add_argument("--max-error", type=float, default=CALIBRATION_DEFAULTS["max_error"], help="maximum accepted mean log-ratio error (default: 0.15)")
    parser.add_argument("--max-size-regression", type=float, default=CALIBRATION_DEFAULTS["max_size_regression"], help="maximum per-size log-ratio error regression, allowing raster quantization (default: 0.02)")
    args = parser.parse_args(argv)
    if not 0 <= args.min_improvement <= 1:
        parser.error("--min-improvement must be between 0 and 1")
    if not 0 < args.max_error <= 1:
        parser.error("--max-error must be greater than zero and at most 1")
    if not 0 <= args.max_size_regression <= 1:
        parser.error("--max-size-regression must be between 0 and 1")
    args.report = args.report or args.output.with_suffix(".report.json")
    if args.output.resolve() == args.report.resolve():
        parser.error("--output and --report must be different paths")
    return args


def atomic_write(path: Path, data: bytes) -> None:
    """Replace an output atomically, keeping timestamps when bytes are unchanged."""
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_file() and path.read_bytes() == data:
        return
    descriptor, temporary = tempfile.mkstemp(prefix=path.name + ".", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(data)
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def write_json(path: Path, value: Any) -> None:
    data = json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
    atomic_write(path, data.encode("utf-8"))


def main(
    argv: list[str] | None = None,
    *,
    scanned_faces: tuple[list[Face], list[str]] | None = None,
) -> int:
    args = parse_args(argv)  # --help works without either optional dependency.
    try:
        import PIL  # noqa: F401
        import fontTools  # noqa: F401
    except ImportError as error:
        print(f"Missing optional dependency: {error.name}. Install with python -m pip install -r scripts/requirements-font-profiles.txt", file=sys.stderr)
        return 2
    directories = args.font_dir if args.font_dir else default_font_dirs()
    faces, warnings = scanned_faces if scanned_faces is not None else scan_faces(directories)
    families = list(dict.fromkeys(args.family or DEFAULT_FAMILIES))
    reference_faces = [face for face in faces if args.reference_family in face.aliases and face.normal]
    reference_regular = static_regular_face(reference_faces)
    if reference_regular is None:
        print(f"Reference family {args.reference_family!r} needs one unambiguous static, upright, normal-width Regular face. Use --reference-family and --font-dir to select an installed reference.", file=sys.stderr)
        return 2
    try:
        reference = measure(reference_regular, HAN_SAMPLE, args.sizes)
    except (OSError, ValueError) as error:
        print(f"Cannot measure reference family {args.reference_family!r}: {error}", file=sys.stderr)
        return 2
    report = {
        "version": 1,
        "method": {
            "description": "Raster suggestions; visual preview is required before adoption. Static normal-width upright faces only; no synthesized weights.",
            "han_score": "mean per-glyph absolute log ratio: 75% area/perimeter stroke-width proxy, 25% ink density",
            "hangul_score": "absolute log ratio of mean Hangul stroke-width proxy to reference Han stroke-width proxy",
            "sizes": args.sizes,
            "han_sample": HAN_SAMPLE,
            "hangul_sample": HANGUL_SAMPLE,
            "min_improvement": args.min_improvement,
            "min_absolute_improvement": 0.02,
            "max_error": args.max_error,
            "max_size_regression": args.max_size_regression,
            "versions": {"Pillow": PIL.__version__, "fontTools": fontTools.__version__},
        },
        "font_directories": [str(path.resolve()) for path in directories],
        "reference": {**reference_regular.description(), "measurements": metric_summary(reference)},
        "warnings": warnings,
        "families": [],
    }
    profiles = []
    for family in families:
        matching = [face for face in faces if family in face.aliases and face.normal]
        general_selected = False
        for scope in ("all", "modern_hangul"):
            if scope == "modern_hangul" and not any(has_modern_hangul(face) for face in matching):
                continue
            if not matching:
                report["families"].append({"family": family, "scope": scope, "decision": "retained_regular: family unavailable"})
                continue
            profile, measurements = select_profile(
                family, matching, scope, reference, args.sizes, args.min_improvement,
                args.max_error, args.max_size_regression,
            )
            if profile and scope == "modern_hangul" and general_selected:
                measurements["decision"] = "suppressed_companion: all-scope profile already selected"
                measurements["suppressed_profile"] = measurements.pop("profile")
                profile = None
            report["families"].append(measurements)
            if profile:
                profiles.append(profile)
                general_selected = scope == "all"
    try:
        write_json(args.output, {"version": 1, "profiles": profiles})
        write_json(args.report, report)
    except OSError as error:
        print(f"Cannot write generated profiles: {error}", file=sys.stderr)
        return 2
    print(f"Suggested {len(profiles)} profiles from {len(faces)} installed faces: {args.output}")
    print(f"Measurements and skipped candidates: {args.report}")
    print("Review the suggestions in a rendered preview before adopting them.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
