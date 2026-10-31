import importlib.util
import json
from pathlib import Path
import plistlib
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "create_macos_app", Path(__file__).with_name("create-macos-app.py"),
)
create_macos_app = importlib.util.module_from_spec(spec)
spec.loader.exec_module(create_macos_app)


class MacOSDocumentIconsTests(unittest.TestCase):
    def test_bundle_documents_and_notices(self):
        entries = json.loads((create_macos_app.REPO_ROOT / "assets/icons/file-types/catalog.json").read_text())
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            generated = root / "generated icons"
            generated.mkdir()
            for entry in entries:
                if entry["extensions"]:
                    (generated / f"{entry['id']}.icns").write_bytes(entry["id"].encode())
            contents = root / "Fragile Notepad.app/Contents"
            resources = contents / "Resources"
            resources.mkdir(parents=True)
            create_macos_app.write_bundle_metadata(
                contents, "1.2.3", "11.0", None,
                create_macos_app.document_types(generated),
            )
            metadata = plistlib.loads((contents / "Info.plist").read_bytes())
            self.assertEqual(metadata["CFBundleShortVersionString"], "1.2.3")
            documents = metadata["CFBundleDocumentTypes"]
            c_document = next(document for document in documents if document["CFBundleTypeName"] == "C document")
            self.assertEqual(c_document["CFBundleTypeExtensions"], ["c", "h"])
            self.assertEqual(c_document["CFBundleTypeRole"], "Editor")
            self.assertEqual(c_document["LSHandlerRank"], "Alternate")
            for document in documents:
                icon = resources / document["CFBundleTypeIconFile"]
                self.assertTrue(icon.is_file())
                self.assertTrue(document["CFBundleTypeExtensions"])
                self.assertNotIn("*", document["CFBundleTypeExtensions"])
            self.assertEqual((resources / c_document["CFBundleTypeIconFile"]).read_bytes(), b"c")

            package = root / "portable package"
            (package / "libexec").mkdir(parents=True)
            (package / "libexec/fragile-notepad").write_bytes(b"editor")
            (package / "fragile-notepad").write_bytes(b"#!/bin/bash\r\n")
            notices = package / "FILE-TYPE-ICON-NOTICES.txt"
            notices.write_text("Document artwork terms")
            create_macos_app.copy_runtime(package, resources / "runtime")
            self.assertEqual((resources / "runtime" / notices.name).read_text(), notices.read_text())

    def test_missing_icon_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(ValueError, "missing generated document icon"):
                create_macos_app.document_types(Path(temporary))


if __name__ == "__main__":
    unittest.main()
