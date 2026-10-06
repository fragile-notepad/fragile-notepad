"""Build preparation checks; no optional libraries or live network are needed."""

import argparse
from contextlib import contextmanager
from dataclasses import replace
import hashlib
import io
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import generate_font_profiles as generator
import prepare_font_profiles as preparation


def checksum(data):
    return hashlib.sha256(data).hexdigest()


class Response(io.BytesIO):
    headers = {}


def face(family, weight=400, variable=False):
    return generator.Face(
        path=Path(family + ".ttf"),
        index=0,
        family=family,
        aliases=(family,),
        weight=weight,
        width=5,
        italic=False,
        bold=False,
        variable=variable,
        coverage=frozenset(ord(character) for character in generator.HAN_SAMPLE + "あいうえおカタカナ")
        | frozenset(range(0xAC00, 0xD7A4)),
    )


NATIVE = [face(name) for name in ("Microsoft YaHei", "Microsoft JhengHei", "Yu Gothic", "Malgun Gothic")]
NOTO = [face("Noto Sans" + style + " CJK " + region) for style in ("", " Mono") for region in ("SC", "TC", "JP", "KR")]


class FontPreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.font_dir = self.root / "fonts"
        self.font_dir.mkdir()
        self.args = argparse.Namespace(
            out_dir=self.root / "out", cache_dir=self.root / "cache", target_os="windows",
            font_dir=[self.font_dir], offline=False,
        )

    def fake_generator(self, arguments, **kwargs):
        output = Path(arguments[arguments.index("--output") + 1])
        report = Path(arguments[arguments.index("--report") + 1])
        preparation.atomic_write(output, preparation.json_bytes({"version": 1, "profiles": []}))
        preparation.atomic_write(report, preparation.json_bytes({"faces": len(kwargs["scanned_faces"][0])}))
        return 0

    @contextmanager
    def prepared_fonts(self, installed):
        def scan(directories):
            return (NOTO if directories[0].name == "downloads" else installed), []

        def download(request, **kwargs):
            return Response(b"license" if request.full_url == preparation.LICENSE_URL else b"font")

        with (
            patch.object(preparation, "dependency_versions", return_value={"Pillow": "test", "fontTools": "test"}),
            patch.object(preparation, "host_os", return_value="windows"),
            patch.object(generator, "scan_faces", side_effect=scan) as scanner,
            patch.object(generator, "main", side_effect=self.fake_generator) as generate,
            patch.object(preparation, "FONT_SHA256", checksum(b"font")),
            patch.object(preparation, "LICENSE_SHA256", checksum(b"license")),
            patch.object(preparation.urllib.request, "urlopen", side_effect=download) as network,
            patch("builtins.print"),
        ):
            yield SimpleNamespace(scan=scanner, generate=generate, network=network)

    def test_verified_download_reuses_cache_without_network(self):
        destination = self.root / "font.ttc"
        destination.write_bytes(b"font")
        with patch.object(preparation.urllib.request, "urlopen") as network:
            result = preparation.verified_download("https://example.test/font", destination, checksum(b"font"), 100, True)
        self.assertEqual(result, destination)
        network.assert_not_called()

    def test_corrupt_cache_fails_offline_without_replacing_file(self):
        destination = self.root / "font.ttc"
        destination.write_bytes(b"corrupt")
        with patch.object(preparation.urllib.request, "urlopen") as network:
            with self.assertRaisesRegex(preparation.PreparationError, "verified cached file"):
                preparation.verified_download("https://example.test/font", destination, checksum(b"font"), 100, True)
        network.assert_not_called()
        self.assertEqual(destination.read_bytes(), b"corrupt")

    def test_checksum_failure_and_oversized_stream_are_atomic(self):
        destination = self.root / "font.ttc"
        destination.write_bytes(b"old")
        for received, limit, message in ((b"wrong", 100, "Checksum"), (b"longer", 3, "limit")):
            with patch.object(preparation.urllib.request, "urlopen", return_value=Response(received)):
                with self.assertRaisesRegex(preparation.PreparationError, message):
                    preparation.verified_download("https://example.test/font", destination, checksum(b"new"), limit, False)
            self.assertEqual(destination.read_bytes(), b"old")
            self.assertEqual(list(self.root.glob("*.tmp")), [])

    def test_successful_download_verifies_and_replaces_corrupt_cache(self):
        destination = self.root / "font.ttc"
        destination.write_bytes(b"old")
        with patch.object(preparation.urllib.request, "urlopen", return_value=Response(b"new")):
            preparation.verified_download("https://example.test/font", destination, checksum(b"new"), 100, False)
        self.assertEqual(destination.read_bytes(), b"new")

    def test_connection_failure_closes_and_removes_temporary_download(self):
        destination = self.root / "font.ttc"
        destination.write_bytes(b"old")
        with patch.object(preparation.urllib.request, "urlopen", side_effect=OSError("connection lost")):
            with self.assertRaisesRegex(preparation.PreparationError, "Cannot download verified font resource"):
                preparation.verified_download("https://example.test/font", destination, checksum(b"new"), 100, False)
        self.assertEqual(destination.read_bytes(), b"old")
        self.assertEqual(list(self.root.glob("*.tmp")), [])

    def test_native_fonts_need_no_network_or_embedded_assets(self):
        with self.prepared_fonts(NATIVE) as mocks:
            result = preparation.prepare(self.args)
        mocks.network.assert_not_called()
        self.assertEqual(result["selected_families"], [item.family for item in NATIVE])
        self.assertEqual(result["reference_family"], "Microsoft JhengHei")
        self.assertEqual((self.args.out_dir / "font_assets.rs").read_text(), "&[]\n")
        self.assertEqual((self.args.out_dir / "FONT-NOTICES.txt").read_bytes(), b"")

    def test_regional_body_coverage_requires_hanja_kana_and_all_hangul(self):
        japanese = face("Japanese")
        self.assertTrue(preparation.usable_family([japanese], "Japanese", 2))
        han_only = replace(japanese, coverage=frozenset(ord(character) for character in generator.HAN_SAMPLE))
        self.assertFalse(preparation.usable_family([han_only], "Japanese", 2))
        korean = face("Korean")
        self.assertTrue(preparation.usable_family([korean], "Korean", 3))
        hangul_only = replace(korean, coverage=frozenset(range(0xAC00, 0xD7A4)))
        self.assertFalse(preparation.usable_family([hangul_only], "Korean", 3))
        incomplete = replace(korean, coverage=korean.coverage - {0xD7A3})
        self.assertFalse(preparation.usable_family([incomplete], "Korean", 3))

    def test_selected_variable_body_accepts_real_regular_axis_instance(self):
        variable = replace(face("Variable", weight=350, variable=True), weight_axis=(100.0, 900.0))
        self.assertTrue(preparation.usable_family([variable], "Variable", 0))
        self.assertFalse(preparation.usable_family([replace(variable, weight_axis=(100.0, 350.0))], "Variable", 0))
        self.assertFalse(preparation.usable_family([replace(variable, variable=False)], "Variable", 0))

    def test_unselected_variable_body_does_not_override_static_query_result(self):
        variable = replace(face("Variable", weight=350, variable=True), weight_axis=(100.0, 900.0))
        medium = face("Variable", weight=500)
        self.assertFalse(preparation.usable_family([variable, medium], "Variable", 0))
        # A real Regular face takes priority even if the variable companion lacks it.
        regular = face("Variable")
        self.assertTrue(preparation.usable_family([variable, medium, regular], "Variable", 0))

    def test_selected_variable_body_still_requires_regional_glyph_coverage(self):
        variable = replace(face("Variable", weight=350, variable=True), weight_axis=(100.0, 900.0))
        self.assertFalse(preparation.usable_family([replace(variable, coverage=frozenset())], "Variable", 0))

    def test_missing_regions_embed_verified_collection_and_package_license(self):
        with self.prepared_fonts([]) as mocks:
            result = preparation.prepare(self.args)
        self.assertEqual(mocks.network.call_count, 2)
        self.assertEqual(result["selected_families"], ["Noto Sans Mono CJK " + region for region in ("SC", "TC", "JP", "KR")])
        self.assertEqual(len(result["embedded_assets"]), 1)
        self.assertIn("include_bytes!", (self.args.out_dir / "font_assets.rs").read_text())
        self.assertEqual((self.args.out_dir / "licenses/NotoSansCJK/OFL.txt").read_text(), "license")
        self.assertIn("Open Font License", (self.args.out_dir / "FONT-NOTICES.txt").read_text())

    def test_reference_only_download_does_not_embed_font(self):
        installed = [replace(item, variable=True) if item.family == "Microsoft JhengHei" else item for item in NATIVE]
        with self.prepared_fonts(installed) as mocks:
            result = preparation.prepare(self.args)
        self.assertEqual(mocks.network.call_count, 2)
        self.assertEqual(result["reference_family"], "Noto Sans CJK TC")
        self.assertEqual(result["embedded_assets"], [])
        self.assertEqual((self.args.out_dir / "FONT-NOTICES.txt").read_bytes(), b"")

    def test_partial_installed_noto_does_not_duplicate_downloaded_reference(self):
        installed = [face("Noto Sans CJK TC")]
        with self.prepared_fonts(installed) as mocks:
            preparation.prepare(self.args)
        scanned = mocks.generate.call_args.kwargs["scanned_faces"][0]
        self.assertEqual(sum("Noto Sans CJK TC" in item.aliases for item in scanned), 1)

    def test_cross_os_build_embeds_fallback_even_when_host_has_coherent_fonts(self):
        self.args.target_os = "linux"
        with self.prepared_fonts(NOTO):
            result = preparation.prepare(self.args)
        self.assertEqual(len(result["embedded_assets"]), 1)

    def test_cache_hit_skips_scanning_generation_and_network(self):
        with self.prepared_fonts([]) as mocks:
            preparation.prepare(self.args)
            self.args.offline = True
            before = (self.args.out_dir / "font_profiles.rs").stat().st_mtime_ns
            preparation.prepare(self.args)
            after = (self.args.out_dir / "font_profiles.rs").stat().st_mtime_ns
        self.assertEqual(mocks.scan.call_count, 2)  # Installed inventory + first downloaded collection only.
        self.assertEqual(mocks.generate.call_count, 1)
        self.assertEqual(mocks.network.call_count, 2)
        self.assertEqual(before, after)

    def test_corrupt_measurement_cache_is_regenerated(self):
        with self.prepared_fonts(NATIVE) as mocks:
            result = preparation.prepare(self.args)
            report = next(file for file in result["verified_files"] if file["kind"] == "report")
            Path(report["path"]).write_bytes(b"corrupt")
            preparation.prepare(self.args)
        self.assertEqual(mocks.generate.call_count, 2)

    def test_inventory_target_and_dependencies_invalidate_cache(self):
        dependencies = {"Pillow": "1", "fontTools": "1"}
        identity, _ = preparation.cache_identity(self.args, [], dependencies)
        self.args.target_os = "linux"
        changed_target, _ = preparation.cache_identity(self.args, [], dependencies)
        changed_dependency, _ = preparation.cache_identity(self.args, [], {**dependencies, "Pillow": "2"})
        changed_inventory, _ = preparation.cache_identity(self.args, [{"path": "font", "mtime_ns": 1, "size": 1}], dependencies)
        self.assertEqual(len({identity, changed_target, changed_dependency, changed_inventory}), 4)

    def test_rerun_paths_only_track_existing_inputs(self):
        missing = self.root / "missing"
        self.args.font_dir.append(missing)
        with self.prepared_fonts(NATIVE):
            preparation.prepare(self.args)
        paths = (self.args.out_dir / "rerun-paths.txt").read_text().splitlines()
        self.assertIn(str(self.font_dir.resolve()), paths)
        self.assertNotIn(str(missing.resolve()), paths)
        self.assertFalse(any(path.endswith(".json") for path in paths))

    def test_offline_missing_dependencies_fail_without_installation(self):
        with patch.object(preparation, "dependency_versions", side_effect=ImportError), patch.object(preparation.subprocess, "run") as command:
            with self.assertRaisesRegex(preparation.PreparationError, "populated cache venv"):
                preparation.dependency_python(self.args.cache_dir, True)
        command.assert_not_called()

    def test_offline_existing_isolated_dependencies_are_reusable(self):
        requirement = preparation.digest_file(preparation.SCRIPT_DIR / "requirements-font-profiles.txt")[:12]
        version = f"python-{preparation.sys.version_info.major}.{preparation.sys.version_info.minor}-{requirement}"
        executable = self.args.cache_dir / version / ("Scripts/python.exe" if preparation.sys.platform == "win32" else "bin/python")
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b"placeholder")
        with patch.object(preparation, "dependency_versions", side_effect=ImportError), patch.object(preparation.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as command:
            selected = preparation.dependency_python(self.args.cache_dir, True)
        self.assertEqual(selected, executable)
        self.assertEqual(command.call_count, 1)

    def test_rust_strings_escape_delimiters_and_profiles_reject_duplicates(self):
        self.assertEqual(preparation.rust_string('Font"#Name'), 'r##"Font"#Name"##')
        record = {"family": "Font", "requested_weight": 300, "face_weight": 290, "scope": "all"}
        self.assertIn("Scope::All", preparation.rust_profiles([record]))
        with self.assertRaises(preparation.PreparationError):
            preparation.rust_profiles([record, record])


if __name__ == "__main__":
    unittest.main()
