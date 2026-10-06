"""Focused tests for the offline font-profile generator.

Run with: python -m unittest discover -s scripts -p test_font_profiles.py
"""

from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import generate_font_profiles as profiles


def face(weight: int = 400, **changes) -> profiles.Face:
    source = profiles.Face(
        path=Path(f"test-{weight}.ttf"), index=0, family="Test", aliases=("Test",),
        weight=weight, width=5, italic=False, bold=False, variable=False,
        coverage=frozenset(ord(character) for character in profiles.HAN_SAMPLE),
    )
    return replace(source, **changes)


class FontProfileTests(unittest.TestCase):
    def test_face_metadata_prefers_typographic_names_and_rejects_oblique(self):
        def name_record(name_id, language, value):
            return SimpleNamespace(
                nameID=name_id, platformID=3, platEncID=1, langID=language,
                isUnicode=lambda: True, toUnicode=lambda: value,
            )

        class Font(dict):
            def getBestCmap(self):
                return {0x4E00: "one", 0x4E8C: ".notdef"}

            def getGlyphID(self, name):
                return 0 if name == ".notdef" else 1

        font = Font({
            "name": SimpleNamespace(names=[
                name_record(1, 0x409, "Legacy Light"),
                name_record(16, 0x411, "Localized Family"),
                name_record(16, 0x409, "Typographic Family"),
            ]),
            "OS/2": SimpleNamespace(usWeightClass=290, usWidthClass=4, fsSelection=1 << 9),
            "post": SimpleNamespace(italicAngle=0.0),
            "fvar": SimpleNamespace(axes=[SimpleNamespace(axisTag="wght", minValue=100, maxValue=900)]),
        })
        parsed = profiles.read_face(font, Path("test.ttc"), 1)
        self.assertEqual(parsed.family, "Typographic Family")
        self.assertEqual(parsed.aliases, ("Typographic Family", "Localized Family"))
        self.assertEqual((parsed.weight, parsed.width, parsed.index), (290, 4, 1))
        self.assertTrue(parsed.italic)
        self.assertTrue(parsed.variable)
        self.assertEqual(parsed.weight_axis, (100.0, 900.0))
        self.assertFalse(parsed.normal)
        self.assertEqual(parsed.coverage, frozenset({0x4E00}))

    def test_upright_normal_width_is_required(self):
        normal = face()
        self.assertTrue(normal.normal)
        self.assertFalse(replace(normal, italic=True).normal)
        self.assertFalse(replace(normal, width=4).normal)

    def test_yahei_nonstandard_weight_resolves_through_light_request(self):
        regular, light = face(), face(290)
        self.assertEqual(profiles.query_weight([290, 400, 700], 300), 290)
        self.assertEqual(profiles.requested_weight(light, [regular, light, face(700)]), 300)

    def test_actual_light_face_displaces_nonstandard_companion(self):
        unusual, light, regular = face(290), face(300), face()
        self.assertEqual(profiles.query_weight([290, 300, 400], 300), 300)
        # A 200 request can still select the 290 face; it must not claim 300.
        self.assertEqual(profiles.requested_weight(unusual, [unusual, light, regular]), 200)

    def test_weight_selection_respects_fontdb_direction_and_special_cases(self):
        self.assertEqual(profiles.query_weight([300, 500], 400), 500)
        self.assertEqual(profiles.query_weight([400, 600], 500), 400)
        self.assertEqual(profiles.query_weight([400, 700], 600), 700)
        self.assertEqual(profiles.query_weight([300, 600], 500), 300)
        self.assertIsNone(profiles.query_weight([], 300))

    def test_duplicate_weights_are_not_eligible(self):
        regular = face()
        duplicate = replace(regular, path=Path("duplicate.ttf"))
        self.assertIsNone(profiles.requested_weight(regular, [regular, duplicate]))

    def test_modern_hangul_requires_every_syllable(self):
        full = face(coverage=frozenset(range(0xAC00, 0xD7A4)))
        self.assertTrue(profiles.has_modern_hangul(full))
        self.assertFalse(profiles.has_modern_hangul(replace(full, coverage=full.coverage - {0xD7A3})))

    def test_mask_metric_handles_crop_edges_and_em_normalization(self):
        metric = profiles.mask_metric([255] * 8, width=2, height=4, size=8)
        scaled = profiles.mask_metric([255] * 32, width=4, height=8, size=16)
        padded = profiles.mask_metric(
            [0] * 4 + [0, 255, 255, 0] * 4 + [0] * 4,
            width=4, height=6, size=8,
        )
        self.assertEqual(metric, scaled)
        self.assertEqual(metric, padded)
        with self.assertRaises(ValueError):
            profiles.mask_metric([0], 1, 1, 8)

    def test_variable_family_is_retained_without_measurement(self):
        with patch.object(profiles, "measure") as render:
            result, report = profiles.select_profile("Test", [face(variable=True)], "all", {}, (32,), 0.15, 0.15)
        self.assertIsNone(result)
        self.assertIn("variable", report["decision"])
        render.assert_not_called()

    def test_hangul_candidate_with_missing_syllable_is_rejected(self):
        full = frozenset(range(0xAC00, 0xD7A4))
        regular = face(coverage=full)
        light = face(300, coverage=full - {0xD7A3})
        metrics = {32: [profiles.GlyphMetric(0.2, 0.05)] * len(profiles.HANGUL_SAMPLE)}
        with patch.object(profiles, "measure", return_value=metrics):
            result, report = profiles.select_profile("Test", [regular, light], "modern_hangul", metrics, (32,), 0.15, 0.15)
        self.assertIsNone(result)
        rejected = next(candidate for candidate in report["candidates"] if candidate["face_weight"] == 300)
        self.assertIn("incomplete modern Hangul", rejected["rejected"])

    def test_companion_rejects_size_regression_above_tolerance(self):
        regular, light = face(), face(300)
        reference = {size: [profiles.GlyphMetric(0.2, 0.05)] for size in (32, 48)}
        baseline = {size: [profiles.GlyphMetric(0.2, 0.08)] for size in (32, 48)}
        candidate = {
            32: [profiles.GlyphMetric(0.2, 0.05)],
            48: [profiles.GlyphMetric(0.2, 0.083)],
        }
        with patch.object(profiles, "measure", side_effect=lambda selected, *_: baseline if selected == regular else candidate):
            result, report = profiles.select_profile("Test", [regular, light], "all", reference, (32, 48), 0.15, 1.0)
        self.assertIsNone(result)
        self.assertIn("regression exceeds tolerance", report["decision"])

    def test_companion_allows_small_raster_regression_with_strong_mean_improvement(self):
        regular, light = face(), face(300)
        reference = {size: [profiles.GlyphMetric(0.2, 0.05)] for size in (32, 48)}
        baseline = {size: [profiles.GlyphMetric(0.2, 0.08)] for size in (32, 48)}
        candidate = {
            32: [profiles.GlyphMetric(0.2, 0.05)],
            48: [profiles.GlyphMetric(0.2, 0.081)],
        }
        with patch.object(profiles, "measure", side_effect=lambda selected, *_: baseline if selected == regular else candidate):
            result, report = profiles.select_profile("Test", [regular, light], "all", reference, (32, 48), 0.15, 1.0)
        self.assertEqual(result["requested_weight"], 300)
        self.assertEqual(report["decision"], "suggested_companion")

    def test_argparse_rejects_invalid_or_overlapping_outputs(self):
        self.assertEqual(profiles.parse_sizes("64,32,32"), (32, 64))
        with self.assertRaises(SystemExit), patch("sys.stderr"):
            profiles.parse_args(["--output", "same.json", "--report", "same.json"])

    def test_generator_emits_one_profile_when_both_scopes_are_suggested(self):
        regular = face(coverage=frozenset(range(0xAC00, 0xD7A4)) | face().coverage)
        general = {"family": "Test", "requested_weight": 300, "face_weight": 300, "scope": "all"}
        hangul = {**general, "scope": "modern_hangul"}
        selections = [
            (general, {"decision": "suggested_companion", "profile": general}),
            (hangul, {"decision": "suggested_companion", "profile": hangul}),
        ]
        dependencies = {
            "PIL": SimpleNamespace(__version__="test"),
            "fontTools": SimpleNamespace(__version__="test"),
        }
        with (
            patch.dict("sys.modules", dependencies),
            patch.object(profiles, "scan_faces", return_value=([regular], [])),
            patch.object(profiles, "measure", return_value={32: [profiles.GlyphMetric(0.2, 0.05)]}),
            patch.object(profiles, "select_profile", side_effect=selections),
            patch.object(profiles, "write_json") as writer,
            patch("builtins.print"),
        ):
            status = profiles.main(["--family", "Test", "--reference-family", "Test", "--sizes", "32"])
        self.assertEqual(status, 0)
        self.assertEqual(writer.call_args_list[0].args[1]["profiles"], [general])
        report = writer.call_args_list[1].args[1]
        self.assertEqual(report["families"][1]["decision"], "suppressed_companion: all-scope profile already selected")
        self.assertEqual(report["families"][1]["suppressed_profile"], hangul)


if __name__ == "__main__":
    unittest.main()
