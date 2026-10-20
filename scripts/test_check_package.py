import io
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import check_package


class PackageTests(unittest.TestCase):
    def test_zip_and_tar_extract_into_directory_with_spaces(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for extension in ("zip", "tar.gz"):
                archive = root / f"package.{extension}"
                destination = root / f"extracted package {extension}"
                destination.mkdir()
                if extension == "zip":
                    with zipfile.ZipFile(archive, "w") as package:
                        package.writestr("fragile-notepad.exe", b"executable")
                else:
                    with tarfile.open(archive, "w:gz") as package:
                        member = tarfile.TarInfo("fragile-notepad.exe")
                        member.size = 10
                        package.addfile(member, io.BytesIO(b"executable"))
                check_package.extract_archive(archive, destination)
                self.assertEqual((destination / "fragile-notepad.exe").read_bytes(), b"executable")

    def test_unsafe_archive_entries_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "extracted"
            destination.mkdir()
            for extension in ("zip", "tar.gz"):
                archive = root / f"unsafe.{extension}"
                if extension == "zip":
                    with zipfile.ZipFile(archive, "w") as package:
                        package.writestr("../outside", b"x")
                else:
                    with tarfile.open(archive, "w:gz") as package:
                        member = tarfile.TarInfo("../outside")
                        member.size = 1
                        package.addfile(member, io.BytesIO(b"x"))
                with self.assertRaises((ValueError, tarfile.TarError)):
                    check_package.extract_archive(archive, destination)
            self.assertFalse((root / "outside").exists())

    @patch("check_package.run_checked")
    def test_success_requires_cli_output_and_a_rendered_frame(self, run):
        run.side_effect = ["fragile-notepad 0.1.0", "Usage:", check_package.FRAME_MARKER + "42"]
        environment = {"APPDATA": "isolated"}
        check_package.check_launcher(["editor"], environment, 3)
        self.assertEqual(run.call_count, 3)
        invocation = run.call_args_list[-1].args
        self.assertEqual(invocation[0], ["editor", "--no-session"])
        self.assertEqual(invocation[1]["FRAGILE_NOTEPAD_EXIT_AFTER_FRAME"], "1")
        self.assertEqual(invocation[1]["FRAGILE_NOTEPAD_RENDER_BACKEND"], "software")
        self.assertEqual(environment, {"APPDATA": "isolated"})

    @patch("check_package.run_checked")
    def test_successful_exit_without_frame_is_rejected(self, run):
        run.side_effect = ["fragile-notepad 0.1.0", "Usage:", ""]
        with self.assertRaisesRegex(RuntimeError, "without rendering"):
            check_package.check_launcher(["editor"], {}, 3)

    @patch("check_package.subprocess.run")
    def test_crashes_and_timeouts_fail_validation(self, run):
        run.return_value = subprocess.CompletedProcess(["editor"], 1, "", "crashed")
        with self.assertRaisesRegex(RuntimeError, "crashed"):
            check_package.run_checked(["editor"], {}, 3)
        run.side_effect = subprocess.TimeoutExpired(["editor"], 3)
        with self.assertRaises(subprocess.TimeoutExpired):
            check_package.run_checked(["editor"], {}, 3)

    def test_macos_requires_both_portable_and_native_launchers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "fragile-notepad").touch()
            with self.assertRaisesRegex(ValueError, "Missing packaged launcher"):
                check_package.package_launchers(root, "darwin")

    def test_macos_rejects_missing_libraries_and_external_manifest_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in ("fragile-notepad", "Fragile Notepad.app/Contents/MacOS/Fragile Notepad"):
                launcher = root / name
                launcher.parent.mkdir(parents=True, exist_ok=True)
                launcher.touch()
            with self.assertRaisesRegex(ValueError, "Missing packaged file"):
                check_package.package_launchers(root, "darwin")
            for directory in (root, root / "Fragile Notepad.app/Contents/Resources/runtime"):
                for name in ("lib/libvulkan.1.dylib", "lib/libvulkan.dylib",
                             "lib/libMoltenVK.dylib", "libexec/fragile-notepad"):
                    library = directory / name
                    library.parent.mkdir(parents=True, exist_ok=True)
                    library.touch()
                manifest = directory / "share/vulkan/icd.d/MoltenVK_icd.json"
                manifest.parent.mkdir(parents=True)
                manifest.write_text('{"ICD":{"library_path":"../../../lib/libMoltenVK.dylib"}}')
            self.assertEqual(len(check_package.package_launchers(root, "darwin")), 2)
            manifest.write_text('{"ICD":{"library_path":"../../../../../../../../outside"}}')
            with self.assertRaisesRegex(ValueError, "Invalid Vulkan library path"):
                check_package.package_launchers(root, "darwin")



if __name__ == "__main__":
    unittest.main()
