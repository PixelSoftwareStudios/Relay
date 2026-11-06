"""The macOS bundle, the update ZIP and the DMG/ZIP identity checks, without codesign."""
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from packaging_fixtures import licence_tree, release_directory
import licenses
import package_macos as macos
import package_release as release
import verify_macos_release as verify

ARM = "aarch64-apple-darwin"
INTEL = "x86_64-apple-darwin"


class MacosTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="relay-macos-test-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.version = release.workspace_version()
        patcher = patch.object(licenses, "ROOT", licence_tree(self.root))
        patcher.start()
        self.addCleanup(patcher.stop)

    def bundle(self, target=ARM):
        binaries = release.find_binaries(target, release_directory(self.root, target),
                                         release.DESKTOP_PROGRAMS)
        app = self.root / macos.APP
        macos.populate_bundle(binaries, app, self.version)
        return app

    def test_bundle_keeps_the_three_programs_side_by_side(self):
        app = self.bundle()
        for program in release.DESKTOP_PROGRAMS:
            self.assertTrue((app / "Contents/MacOS" / program).stat().st_mode & 0o111)
        self.assertTrue((app / "Contents/Resources/relay.icns").is_file())
        self.assertTrue((app / "Contents/Resources/THIRD_PARTY_LICENSES").is_file())
        self.assertTrue((app / "Contents/Resources/OFL-noto-sans.txt").is_file())

    def test_info_plist_keeps_the_microphone_text_and_version(self):
        info = plistlib.loads((self.bundle() / "Contents/Info.plist").read_bytes())
        self.assertEqual(info["CFBundleExecutable"], "relay-desktop")
        self.assertEqual(info["CFBundleIdentifier"], "dev.relay.relay-desktop")
        self.assertEqual(info["CFBundleShortVersionString"], release.numeric_version(self.version))
        self.assertIn("microphone", info["NSMicrophoneUsageDescription"])

    def test_update_zip_holds_the_raw_app(self):
        app = self.bundle()
        archive = self.root / "update.zip"
        macos.write_update_archive(app, archive)
        with zipfile.ZipFile(archive) as stream:
            names = stream.namelist()
            modes = {i.filename: i.external_attr >> 16 for i in stream.infolist()}
        self.assertEqual(names[0], "Relay.app/")
        self.assertIn("Relay.app/Contents/MacOS/relayd", names)
        self.assertIn("Relay.app/Contents/Info.plist", names)
        self.assertTrue(modes["Relay.app/Contents/MacOS/relay-desktop"] & 0o111)
        self.assertTrue(archive.with_name("update.zip.sha256").is_file())
        verify.check_zip_entries((i, 0) for i in names)

    def test_update_zip_refuses_symlinks(self):
        app = self.bundle()
        (app / "Contents/link").symlink_to("/tmp")
        with self.assertRaises(ValueError):
            macos.write_update_archive(app, self.root / "update.zip")

    def test_update_zip_entry_checks(self):
        for name in ("evil/x", "Relay.app/../x", "Relay.app\\x", "Relay.appx/y"):
            with self.assertRaises(ValueError):
                verify.check_zip_entries([(name, 0)])
        with self.assertRaises(ValueError):
            verify.check_zip_entries([("Relay.app/a", 0o120777 << 16)])

    def test_image_and_zip_names_per_target(self):
        names = release.expected_assets(self.version)
        for suffix in ("macos-arm64", "macos-x64"):
            self.assertIn(f"relay-{self.version}-{suffix}.dmg", names)
            self.assertIn(f"relay-{self.version}-{suffix}-update.zip", names)

    def test_bundle_checks_catch_wrong_architecture_and_modes(self):
        app = self.bundle(ARM)
        verify.check_bundle(app, self.version, ARM)
        with self.assertRaises(ValueError):
            verify.check_bundle(app, self.version, INTEL)
        (app / "Contents/MacOS/relayd").chmod(0o644)
        with self.assertRaises(ValueError):
            verify.check_bundle(app, self.version, ARM)

    def test_identical_apps_compare_equal_and_changes_do_not(self):
        app = self.bundle()
        before = verify.app_files(app)
        self.assertEqual(before, verify.app_files(app))
        (app / "Contents/PkgInfo").write_bytes(b"other")
        self.assertNotEqual(before, verify.app_files(app))

    def test_disk_image_layout_has_applications_shortcut(self):
        settings = macos.disk_image_settings(Path("Relay.app"))
        self.assertEqual(settings["symlinks"], {"Applications": "/Applications"})
        self.assertEqual(set(settings["icon_locations"]), {"Relay.app", "Applications"})


if __name__ == "__main__":
    unittest.main()
