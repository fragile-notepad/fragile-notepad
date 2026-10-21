"""Tests for the SVG-to-Rust icon compiler."""

from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from compile_vector_icons import FAMILIES, compile_icons, compile_svg, parse_path

ROOT = Path(__file__).resolve().parents[1]


def icon(content: str, attributes: str = "") -> str:
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 22 22" '
        + attributes + '>' + content + '</svg>'
    )


class VectorIconCompilerTest(unittest.TestCase):
    def test_relative_paths_and_close(self):
        commands = parse_path("m2 3 4 0 h2 v3 l-2 1 z m1 2 l3 4")
        self.assertEqual(commands, [
            "builder.move_to(Point::new(2.0, 3.0));",
            "builder.line_to(Point::new(6.0, 3.0));",
            "builder.line_to(Point::new(8.0, 3.0));",
            "builder.line_to(Point::new(8.0, 6.0));",
            "builder.line_to(Point::new(6.0, 7.0));",
            "builder.close();",
            "builder.move_to(Point::new(3.0, 5.0));",
            "builder.line_to(Point::new(6.0, 9.0));",
        ])

    def test_cubic_and_quadratic_curves(self):
        commands = parse_path("M1 2 c1 2 3 4 5 6 s2 3 4 5 q1 2 3 4 t3 2")
        self.assertEqual(commands, [
            "builder.move_to(Point::new(1.0, 2.0));",
            "builder.bezier_curve_to(Point::new(2.0, 4.0), Point::new(4.0, 6.0), Point::new(6.0, 8.0));",
            "builder.bezier_curve_to(Point::new(8.0, 10.0), Point::new(8.0, 11.0), Point::new(10.0, 13.0));",
            "builder.quadratic_curve_to(Point::new(11.0, 15.0), Point::new(13.0, 17.0));",
            "builder.quadratic_curve_to(Point::new(15.0, 19.0), Point::new(16.0, 19.0));",
        ])

    def test_smooth_controls_reset_after_other_commands(self):
        commands = parse_path("M0 0 C1 1 2 2 3 3 L5 5 S6 6 7 7 Q8 8 9 9 H10 T12 12")
        self.assertIn(
            "builder.bezier_curve_to(Point::new(5.0, 5.0), Point::new(6.0, 6.0), Point::new(7.0, 7.0));",
            commands,
        )
        self.assertEqual(
            commands[-1],
            "builder.quadratic_curve_to(Point::new(10.0, 9.0), Point::new(12.0, 12.0));",
        )

    def test_arc_rotation_flags_and_endpoints(self):
        commands = parse_path("M2 3 a4 5 30 0110 12 A6 7 0 1 0 1 2")
        self.assertEqual(commands[1:], [
            "svg_arc(builder, Point::new(2.0, 3.0), Vector::new(4.0, 5.0), 30.0, false, true, Point::new(12.0, 15.0));",
            "svg_arc(builder, Point::new(12.0, 15.0), Vector::new(6.0, 7.0), 0.0, true, false, Point::new(1.0, 2.0));",
        ])

    def test_fractional_and_exponent_coordinates(self):
        self.assertEqual(parse_path("M.5-.25 L1e1,+2E-1"), [
            "builder.move_to(Point::new(0.5, -0.25));",
            "builder.line_to(Point::new(10.0, 0.2));",
        ])

    def test_evenodd_holes(self):
        generated = compile_svg(icon(
            '<path d="M2 2 H20 V20 H2 Z M6 6 H16 V16 H6 Z" '
            'fill="#123456" fill-rule="evenodd"/>'
        ), "HOLE")
        self.assertEqual(generated.count("builder.move_to"), 2)
        self.assertEqual(generated.count("builder.close"), 2)
        self.assertIn("fill: Paint::Solid(Color::from_rgb8(18, 52, 86))", generated)
        self.assertIn("fill_rule: fill::Rule::EvenOdd", generated)

    def test_inherited_paints_and_stroke_properties(self):
        generated = compile_svg(icon(
            '<g fill="none" stroke="currentColor" stroke-width="1.75" '
            'stroke-linecap="round" stroke-linejoin="bevel">'
            '<path d="M1 2 L3 4"/>'
            '<path d="M5 6 L7 8" stroke="#ff8040" stroke-linecap="square"/>'
            '</g>'
        ), "INHERITED")
        self.assertEqual(generated.count("fill: Paint::None"), 2)
        self.assertIn("stroke: Paint::CurrentColor", generated)
        self.assertIn("stroke: Paint::Solid(Color::from_rgb8(255, 128, 64))", generated)
        self.assertEqual(generated.count("stroke_width: 1.75"), 2)
        self.assertIn("line_cap: LineCap::Round", generated)
        self.assertIn("line_cap: LineCap::Square", generated)
        self.assertEqual(generated.count("line_join: LineJoin::Bevel"), 2)

    def test_viewbox_and_default_paint(self):
        generated = compile_svg(
            '<svg viewBox="-3, 2, 44, 22"><path d="M0 0 L1 1"/></svg>', "DEFAULT"
        )
        self.assertIn("view_box: [-3.0, 2.0, 44.0, 22.0]", generated)
        self.assertIn("fill: Paint::Solid(Color::from_rgb8(0, 0, 0))", generated)
        self.assertIn("stroke: Paint::None", generated)
        self.assertIn("fill_rule: fill::Rule::NonZero", generated)
        self.assertIn("line_cap: LineCap::Butt", generated)
        self.assertIn("line_join: LineJoin::Miter", generated)

    def test_unsupported_svg_content_is_rejected(self):
        unsupported = [
            '<circle cx="11" cy="11" r="5"/>',
            '<rect width="10" height="10"/>',
            '<image href="image.png"/>',
            '<text>Icon</text>',
            '<defs><linearGradient id="paint"/></defs>',
            '<g transform="scale(2)"><path d="M0 0 L1 1"/></g>',
            '<path d="M0 0 L1 1" fill="url(#paint)"/>',
            '<path d="M0 0 L1 1" style="stroke: currentColor"/>',
            '<path d="M0 0 L1 1" opacity="0.5"/>',
            '<path d="M0 0 L1 1" stroke-dasharray="1 2"/>',
            '<path d="M0 0 L1 1" fill-rule="bad"/>',
            '<path d="M0 0 L1 1" stroke-width="2px"/>',
            '<path d="M0 0 L1 1" stroke-width="-1"/>',
            '<path d="M0 0 L1 1" stroke-linecap="bad"/>',
            '<path d="M0 0 L1 1" stroke-linejoin="bad"/>',
            '<path d="M0 0 L1 1"/>unrendered text',
            '<svg viewBox="0 0 22 22"><path d="M0 0 L1 1"/></svg>',
        ]
        for content in unsupported:
            with self.subTest(content=content), self.assertRaises(ValueError):
                compile_svg(icon(content), "UNSUPPORTED")

    def test_malformed_paths_are_rejected(self):
        for path in (
            "L1 1", "M", "M0 0 L1", "M0 0 Z1 1", "M0 0 R1 1",
            "M0 0 A1 1 0 2 0 3 4", "M0 0 A-1 1 0 0 1 3 4",
            "M0 0 @1 1", "M1e999 0", "M0 0 L1e39 1",
        ):
            with self.subTest(path=path), self.assertRaises(ValueError):
                parse_path(path)

    def test_invalid_roots_and_viewboxes_are_rejected(self):
        for source in (
            '<g><path d="M0 0 L1 1"/></g>',
            '<svg><path d="M0 0 L1 1"/></svg>',
            '<svg viewBox="0 0 0 22"><path d="M0 0 L1 1"/></svg>',
            '<svg viewBox="0 0 22 -1"><path d="M0 0 L1 1"/></svg>',
            '<svg xmlns="urn:other" viewBox="0 0 22 22"><path d="M0 0 L1 1"/></svg>',
            '<svg viewBox="0 0 22 22"/>',
        ):
            with self.subTest(source=source), self.assertRaises(ValueError):
                compile_svg(source, "INVALID")

    def test_application_icons_compile(self):
        icons_root = ROOT / "assets/icons"
        generated = compile_icons(icons_root)
        sources = [path for family in FAMILIES for path in (icons_root / family / "svg").glob("*.svg")]
        self.assertEqual(generated.count("LazyLock<VectorIcon>"), len(sources))
        self.assertIn("static COLORED_DOCUMENT_SAVE:", generated)
        self.assertIn("static HEROICONS_X_MARK:", generated)
        self.assertIn("static BOOTSTRAP_PIN_ANGLE_FILL:", generated)
        self.assertIn("builder.quadratic_curve_to", generated)
        self.assertIn("svg_arc(builder", generated)
        self.assertNotIn("FILE_TYPES_", generated)
        self.assertNotIn(".rgba", generated)
        self.assertEqual(generated, compile_icons(icons_root))

    def test_source_errors_identify_the_icon(self):
        with TemporaryDirectory() as directory:
            icons_root = Path(directory)
            source_dir = icons_root / "colored/svg"
            source_dir.mkdir(parents=True)
            (source_dir / "broken.svg").write_text(
                icon('<path d="M0 0 L1 1" transform="scale(2)"/>'), encoding="utf-8"
            )
            with self.assertRaisesRegex(ValueError, r"broken\.svg: Unsupported path attributes"):
                compile_icons(icons_root)


if __name__ == "__main__":
    unittest.main()
