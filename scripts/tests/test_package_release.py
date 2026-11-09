"""Naming, archive contents, checksums and the complete-release contract."""
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from packaging_fixtures import executable, licence_tree, release_directory
import licenses
import package_release as release
import package_cli as cli

LINUX = "x86_64-unknown-linux-gnu"
WINDOWS = "aarch64-pc-windows-msvc"


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="relay-release-test-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.version = release.workspace_version()
        self.output = self.root / "output with spaces"
        self.output.mkdir()
        patcher = patch.object(licenses, "ROOT", licence_tree(self.root))
        patcher.start()
        self.addCleanup(patcher.stop)

    def full_set(self):
        for name in release.expected_assets(self.version):
            path = self.output / name
            path.write_bytes(b"fixture " + name.encode())
            if name in release.package_assets(self.version):
                release.write_checksum(path)

    def test_tag_must_match_workspace_version(self):
        release.validate("")
        release.validate(f"v{self.version}")
        for tag in (self.version, "v9.9.9", f"v{self.version}-rc.1"):
            with self.assertRaises(ValueError):
                release.validate(tag)

    def test_validate_writes_github_output(self):
        output = self.root / "github-output"
        with patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}):
            release.validate("")
        self.assertEqual(output.read_text(),
                         f"version={self.version}\ntoolchain={release.toolchain()}\n")
        self.assertRegex(release.toolchain(), r"^\d+\.\d+\.\d+$")

    def test_asset_names_cover_every_platform(self):
        v = self.version
        names = release.expected_assets(v)
        self.assertEqual(len(names), 45)
        self.assertEqual(len(release.package_assets(v)), 23)
        self.assertEqual(len(release.signed_assets(v)), 22)
        self.assertIn(f"relay-{v}-macos-arm64-update.zip.minisig", names)
        self.assertNotIn(f"relay-{v}-android-arm64-v8a.apk.minisig", names)
        self.assertFalse([n for n in names if n.endswith(".sha256") or "SHA256" in n])
        for expected in (
                f"relay-{v}-android-arm64-v8a.apk",
                f"relay-{v}-windows-x64.msi", f"relay-{v}-windows-arm64-portable.zip",
                f"relay-cli-{v}-windows-x64.zip",
                f"relay-{v}-macos-arm64.dmg", f"relay-{v}-macos-arm64-update.zip",
                f"relay-{v}-macos-x64.dmg", f"relay-{v}-macos-x64-update.zip",
                f"relay-cli-{v}-macos-arm64.tar.gz",
                f"relay-{v}-linux-x64.AppImage", f"relay-{v}-linux-arm64.deb",
                f"relay-{v}-linux-x64.rpm", f"relay-{v}-linux-arm64.tar.gz",
                f"relay-cli-{v}-linux-x64.tar.gz"):
            self.assertIn(expected, names)

    def test_binary_headers_are_checked_per_target(self):
        binary = executable(self.root / "relay", "x86_64-apple-darwin")
        release.validate_binary("x86_64-apple-darwin", binary)
        for target in ("aarch64-apple-darwin", LINUX, WINDOWS):
            with self.assertRaises(ValueError):
                release.validate_binary(target, binary)

    def test_linux_archive_layout_and_modes(self):
        directory = release_directory(self.root, LINUX)
        release.package(self.version, LINUX, directory, self.output)
        archive = self.output / f"relay-{self.version}-linux-x64.tar.gz"
        top = archive.name.removesuffix(".tar.gz")
        with tarfile.open(archive) as stream:
            members = {m.name: m for m in stream.getmembers()}
        for program in release.DESKTOP_PROGRAMS:
            self.assertEqual(members[f"{top}/bin/{program}"].mode, 0o755)
        for document in ("LICENSE", "THIRD_PARTY_LICENSES", "OFL-noto-sans.txt",
                         "LICENSE-phosphor.txt", "examples/relayd.service"):
            self.assertEqual(members[f"{top}/share/doc/relay/{document}"].mode, 0o644)
        self.assertTrue((self.output / (archive.name + ".sha256")).is_file())

    def test_windows_portable_zip_has_exe_suffixes(self):
        directory = release_directory(self.root, WINDOWS)
        release.package(self.version, WINDOWS, directory, self.output)
        with zipfile.ZipFile(self.output / f"relay-{self.version}-windows-arm64-portable.zip") as z:
            names = z.namelist()
        top = f"relay-{self.version}-windows-arm64"
        for program in release.DESKTOP_PROGRAMS:
            self.assertIn(f"{top}/bin/{program}.exe", names)
        self.assertFalse(any("examples" in n for n in names))

    def test_cli_archive_is_headless(self):
        directory = release_directory(self.root, LINUX)
        archive = cli.package(self.version, LINUX, directory, self.output)
        self.assertEqual(archive.name, f"relay-cli-{self.version}-linux-x64.tar.gz")
        with tarfile.open(archive) as stream:
            names = stream.getnames()
        top = archive.name.removesuffix(".tar.gz")
        self.assertIn(f"{top}/bin/relay", names)
        self.assertIn(f"{top}/bin/relayd", names)
        self.assertNotIn(f"{top}/bin/relay-desktop", names)
        self.assertFalse(any("OFL" in n or "phosphor" in n for n in names))

    def test_missing_binary_or_notice_is_refused(self):
        directory = release_directory(self.root, LINUX, ("relay", "relayd"))
        with self.assertRaises(ValueError):
            release.package(self.version, LINUX, directory, self.output)
        directory = release_directory(self.root, LINUX)
        (licenses.ROOT / licenses.NOTICES).unlink()
        with self.assertRaisesRegex(ValueError, "cargo about"):
            release.package(self.version, LINUX, directory, self.output)

    def test_finalize_requires_every_asset_and_writes_sums(self):
        self.full_set()
        release.verify_assets(self.version, self.output, finalize=True)
        sums = (self.output / "SHA256SUMS.txt").read_text().splitlines()
        self.assertEqual(len(sums), 45)
        self.assertTrue(any(line.endswith(".deb.minisig") for line in sums))
        self.assertFalse(list(self.output.glob("*.sha256")))
        release.verify_assets(self.version, self.output)

    def test_finalize_rejects_missing_unexpected_and_tampered(self):
        self.full_set()
        victim = self.output / f"relay-{self.version}-linux-x64.deb"
        victim.write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "Checksum"):
            release.verify_assets(self.version, self.output, finalize=True)
        release.write_checksum(victim)
        (self.output / "stray.txt").write_text("x")
        with self.assertRaisesRegex(ValueError, "unexpected"):
            release.verify_assets(self.version, self.output, finalize=True)
        (self.output / "stray.txt").unlink()
        victim.unlink()
        (self.output / (victim.name + ".sha256")).unlink()
        with self.assertRaisesRegex(ValueError, "missing"):
            release.verify_assets(self.version, self.output, finalize=True)

    def test_finalize_requires_every_signature(self):
        self.full_set()
        (self.output / f"relay-{self.version}-linux-x64.deb.minisig").unlink()
        with self.assertRaisesRegex(ValueError, r"missing=.*\.deb\.minisig"):
            release.verify_assets(self.version, self.output, finalize=True)

    def test_notes_list_downloads_and_reject_empty_changes(self):
        text = release.release_notes(self.version, "### Features\n- one\n")
        self.assertIn("- one", text)
        self.assertIn(f"relay-{self.version}-macos-arm64-update.zip", text)
        self.assertIn(f"relay-{self.version}-android-arm64-v8a.apk", text)
        self.assertIn("64-bit ARM", text)
        self.assertIn("Other ABIs", text)
        self.assertIn("not code-signed", text)
        self.assertIn("release key", text)
        self.assertIn(".minisig", text)
        self.assertNotIn("CHANGELOG.md", text)
        with self.assertRaises(ValueError):
            release.release_notes(self.version, "  \n")


if __name__ == "__main__":
    unittest.main()
