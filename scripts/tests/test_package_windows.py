import struct
import unittest
from pathlib import Path

from packaging_fixtures import release_directory  # noqa: F401  (sets sys.path)
import package_windows as windows
import package_release as release


class WindowsTests(unittest.TestCase):
    def test_ico_directory_matches_the_images(self):
        pngs = [(release.ROOT / f"assets/icon/relay-mark-{s}.png").read_bytes() for s in (16, 256)]
        ico = windows.make_ico(pngs)
        reserved, kind, count = struct.unpack_from("<HHH", ico)
        self.assertEqual((reserved, kind, count), (0, 1, 2))
        width, _, _, _, _, bits, size, offset = struct.unpack_from("<BBBBHHII", ico, 6 + 16)
        self.assertEqual((width, bits, size), (0, 32, len(pngs[1])))
        self.assertEqual(ico[offset:offset + size], pngs[1])

    def test_ico_rejects_non_png_and_non_square(self):
        with self.assertRaises(ValueError):
            windows.make_ico([b"not a png at all, long enough......"])
        wide = b"\x89PNG\r\n\x1a\n" + bytes(8) + struct.pack(">II", 32, 16)
        with self.assertRaises(ValueError):
            windows.make_ico([wide])

    def test_fragment_lists_each_flat_document_once(self):
        docs = {"share/doc/relay/LICENSE": Path("C:/src/LICENSE"),
                "share/doc/relay/OFL-a.txt": Path("C:/src/OFL-a.txt"),
                "share/doc/relay/examples/x": Path("C:/src/x")}
        text = windows.documents_fragment(docs)
        self.assertEqual(text.count("<Component "), 2)
        self.assertEqual(text.count("<ComponentRef "), 2)
        self.assertNotIn("examples", text)

    def test_template_installs_all_three_programs(self):
        wxs = (release.ROOT / "packaging/windows.wxs").read_text()
        for name in ("relay-desktop.exe", "relay.exe", "relayd.exe"):
            self.assertIn(f'Name="{name}"', wxs)
        self.assertIn('ComponentGroupRef Id="Documents"', wxs)

    def test_only_windows_targets_are_accepted(self):
        with self.assertRaises(ValueError):
            windows.package(release.workspace_version(), "x86_64-unknown-linux-gnu",
                            Path("."), Path("."))


if __name__ == "__main__":
    unittest.main()
