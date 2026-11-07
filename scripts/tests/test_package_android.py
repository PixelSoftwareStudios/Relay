from pathlib import Path
import tempfile
import unittest
import zipfile

from packaging_fixtures import release_directory  # noqa: F401  (sets sys.path)
import package_android as android
import package_release as release


class AndroidTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="relay-android-test-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.version = release.workspace_version()

    def apk(self, *names):
        path = self.root / "app-debug.apk"
        with zipfile.ZipFile(path, "w") as stream:
            for name in names:
                stream.writestr(name, b"x")
        return path

    def test_apk_is_renamed_and_checksummed(self):
        apk = self.apk("AndroidManifest.xml", "lib/arm64-v8a/librelay.so")
        target = android.package(self.version, apk, self.root / "out")
        self.assertEqual(target.name, f"relay-{self.version}-android-arm64-v8a.apk")
        self.assertTrue(target.with_name(target.name + ".sha256").is_file())

    def test_wrong_contents_are_refused(self):
        for names in (("lib/arm64-v8a/a.so",), ("AndroidManifest.xml", "lib/x86_64/a.so")):
            with self.assertRaises(ValueError):
                android.package(self.version, self.apk(*names), self.root / "out")
        notzip = self.root / "bad.apk"
        notzip.write_bytes(b"nope")
        with self.assertRaises(ValueError):
            android.package(self.version, notzip, self.root / "out")

    def test_version_must_match(self):
        with self.assertRaises(ValueError):
            android.package("9.9.9", self.apk("AndroidManifest.xml"), self.root / "out")


if __name__ == "__main__":
    unittest.main()
