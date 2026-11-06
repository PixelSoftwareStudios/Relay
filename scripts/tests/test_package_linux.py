from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from packaging_fixtures import licence_tree, release_directory
import licenses
import package_linux as linux
import package_release as release

TARGET = "aarch64-unknown-linux-gnu"


class LinuxTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="relay-linux-test-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.version = release.workspace_version()
        patcher = patch.object(licenses, "ROOT", licence_tree(self.root))
        patcher.start()
        self.addCleanup(patcher.stop)
        self.binaries = release.find_binaries(TARGET, release_directory(self.root, TARGET),
                                              release.DESKTOP_PROGRAMS)

    def test_packager_config_installs_all_three_programs(self):
        config = linux.configuration(self.version, TARGET, self.binaries, self.root, "deb")
        mains = {Path(b["path"]).name: b["main"] for b in config["binaries"]}
        self.assertEqual(mains, {"relay-desktop": True, "relay": False, "relayd": False})
        self.assertEqual(config["formats"], ["deb"])
        self.assertEqual(config["version"], self.version)
        self.assertIn("usr/share/doc/relay/THIRD_PARTY_LICENSES", config["deb"]["files"].values())
        self.assertIn("usr/share/doc/relay/examples/relayd.service", config["deb"]["files"].values())

    def test_appimage_launcher_is_main_and_keeps_the_dotted_id(self):
        config = linux.configuration(self.version, TARGET, self.binaries, self.root, "appimage")
        config = linux.appimage_configuration(config, TARGET, self.root)
        self.assertEqual(Path(config["binaries"][0]["path"]).name, f"{linux.APP_ID}.sh")
        self.assertEqual([b["main"] for b in config["binaries"]], [True, False, False, False])
        desktop = Path(config["deb"]["desktopTemplate"]).read_text()
        self.assertIn(f"Exec={linux.APP_ID}.sh", desktop)
        self.assertIn(f"Icon={linux.APP_ID}", desktop)
        self.assertIn("appimage", config)

    def test_rpm_metadata_has_binaries_icons_licences_and_requirements(self):
        text = linux.rpm_metadata(TARGET, self.binaries)
        for fragment in ('"/usr/bin/relayd"', '"/usr/bin/relay"', '"/usr/bin/relay-desktop"',
                         "/usr/share/applications/relay.desktop",
                         "/usr/share/icons/hicolor/512x512/apps/relay-desktop.png",
                         "/usr/share/doc/relay/LICENSE", 'alsa-lib" = "*"'):
            self.assertIn(fragment, text)

    def test_formats_and_targets_are_validated(self):
        with self.assertRaises(ValueError):
            linux.package(self.version, "x86_64-apple-darwin", self.root, self.root, ["deb"])
        with self.assertRaises(ValueError):
            linux.package(self.version, TARGET, self.root / "release", self.root, ["snap"])

    def test_desktop_entry_matches_the_installed_names(self):
        entry = (release.ROOT / "packaging/relay.desktop").read_text()
        self.assertIn("Exec=relay-desktop", entry)
        self.assertIn("Icon=relay-desktop", entry)


if __name__ == "__main__":
    unittest.main()
