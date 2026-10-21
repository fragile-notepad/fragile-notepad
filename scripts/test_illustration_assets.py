"""Tests for generated illustration artwork."""

from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class IllustrationAssetsTest(unittest.TestCase):
    def test_blink_changes_only_the_eyes(self):
        from PIL import Image, ImageChops

        art = ROOT / "assets/illustrations/bunny"
        for family, size in (("app", 256), ("about-bunny", 384)):
            def load(name):
                return Image.frombytes("RGBA", (size, size), (art / f"{name}.rgba").read_bytes())
            opened = load(family)
            scale = size / 256
            for name in (f"{family}-half", f"{family}-closed"):
                with self.subTest(frame=name):
                    frame = load(name)
                    self.assertEqual(frame.getchannel("A").tobytes(), opened.getchannel("A").tobytes())
                    diff = ImageChops.difference(opened, frame).convert("RGB")
                    bounds = diff.getbbox()
                    self.assertIsNotNone(bounds)
                    self.assertTrue(98 * scale <= bounds[0] < bounds[2] <= 155 * scale)
                    self.assertTrue(82 * scale <= bounds[1] < bounds[3] <= 112 * scale)
                    for eye in ((98, 82, 120, 112), (135, 82, 155, 112)):
                        self.assertIsNotNone(diff.crop(tuple(int(v * scale) for v in eye)).getbbox())

    def test_app_background_and_title_bar_transparency(self):
        from PIL import Image

        art = ROOT / "assets/illustrations/bunny"
        app = Image.frombytes("RGBA", (256, 256), (art / "app.rgba").read_bytes())
        title = Image.frombytes("RGBA", (64, 64), (art / "title-bar.rgba").read_bytes())
        self.assertEqual(app.getpixel((0, 0))[3], 0)
        self.assertEqual(title.getpixel((0, 0))[3], 0)
        # Beside the rabbit: solid blue in the app tile, clear in the title bar.
        self.assertGreater(app.getpixel((16, 128))[3], 250)
        self.assertEqual(title.getpixel((4, 32))[3], 0)
        self.assertGreater(title.getchannel("A").getextrema()[1], 250)
        for icon in (app, title):
            for edge in (icon.crop((0, 0, icon.width, 1)),
                         icon.crop((0, icon.height - 1, icon.width, icon.height))):
                self.assertEqual(edge.getchannel("A").getextrema()[1], 0)


if __name__ == "__main__":
    unittest.main()
