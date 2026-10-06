"""Selection invariants for the shared font-profile catalog (stdlib only)."""

import unittest

from font_families import LANGUAGES, TARGET_OSES, coherent_collections, preferred_families


class RegionalFontCatalogTests(unittest.TestCase):
    def test_every_target_has_named_regional_candidates_and_all_coherent_designs(self):
        self.assertEqual(LANGUAGES, ("sc", "tc", "jp", "kr"))
        for target_os in TARGET_OSES:
            candidates = preferred_families(target_os)
            self.assertEqual(len(candidates), len(LANGUAGES))
            for index, families in enumerate(candidates):
                with self.subTest(target_os=target_os, language=LANGUAGES[index]):
                    self.assertTrue(families)
                    self.assertEqual(len(families), len(set(families)))
                    self.assertTrue(all(name.strip() == name and name for name in families))
                    for collection in coherent_collections():
                        self.assertTrue(set(collection[index]).issubset(families))

    def test_native_body_faces_precede_ui_and_last_resort_families(self):
        windows = preferred_families("windows")
        for families, body, ui in [
            (windows[0], "Microsoft YaHei", "Microsoft YaHei UI"),
            (windows[1], "Microsoft JhengHei", "Microsoft JhengHei UI"),
            (windows[2], "Yu Gothic", "Yu Gothic UI"),
        ]:
            self.assertEqual(families[0], body)
            self.assertEqual(families[-1], ui)
        macos = preferred_families("macos")
        self.assertEqual(tuple(families[0] for families in macos),
                         ("PingFang SC", "PingFang TC", "Hiragino Sans", "Apple SD Gothic Neo"))

    def test_coverage_only_families_do_not_cross_regional_forms(self):
        linux = preferred_families("linux")
        for region in (1, 2, 3):
            self.assertFalse(any(name.startswith("WenQuanYi") for name in linux[region]))
        self.assertIn("IPAexGothic", linux[2])
        self.assertIn("IPAGothic", linux[2])
        self.assertIn("NanumGothic", linux[3])
        self.assertIn("UnDotum", linux[3])
        self.assertNotIn("AR PL UMing CN", linux[1])
        self.assertNotIn("AR PL UMing TW", linux[0])

    def test_collection_priority_and_aliases_remain_region_specific(self):
        collections = coherent_collections()
        self.assertEqual(len(collections), 4)
        for collection in collections:
            self.assertEqual(len(collection), len(LANGUAGES))
            self.assertTrue(all(aliases for aliases in collection))
        self.assertEqual(collections[0][0], ("Noto Sans Mono CJK SC",))
        self.assertEqual(collections[-1][2], ("Source Han Sans JP", "Source Han Sans"))

    def test_unknown_target_does_not_silently_choose_a_host_catalog(self):
        with self.assertRaises(ValueError):
            preferred_families("android")


if __name__ == "__main__":
    unittest.main()
