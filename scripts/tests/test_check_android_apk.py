from pathlib import Path
import tempfile
import unittest
import zipfile

from packaging_fixtures import release_directory  # noqa: F401  (sets sys.path)
import check_android_apk as check


MANIFEST = """\
N: android=http://schemas.android.com/apk/res/android
  E: manifest (line=2)
    E: uses-permission (line=4)
      A: android:name(0x01010003)="android.permission.INTERNET" (Raw: "android.permission.INTERNET")
    E: application (line=9)
      E: service (line=20)
        A: android:name(0x01010003)="dev.relay.app.CallService" (Raw: "dev.relay.app.CallService")
      E: provider (line=30)
        A: android:name(0x01010003)="androidx.startup.InitializationProvider" (Raw: "x")
"""


class CheckTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="relay-apk-check-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)

    def apk(self, **entries):
        path = self.root / "a.apk"
        with zipfile.ZipFile(path, "w") as stream:
            for name, data in entries.items():
                stream.writestr(name.replace("__", "/").replace("_dex", ".dex"), data)
        return path

    def test_names_are_found_case_insensitively(self):
        self.assertEqual(check.forbidden_in_text("MlKitInitProvider"), ["mlkit"])
        self.assertEqual(check.forbidden_in_text("com.google.android.gms.common.api.GoogleApiActivity"), ["com.google.android.gms", "googleapiactivity"])
        self.assertEqual(check.forbidden_in_text("dev.relay.app.CallService"), [])

    def test_clean_apk_passes(self):
        apk = self.apk(classes_dex=b"Ldev/relay/app/Main;Lcom/google/zxing/Result;")
        self.assertEqual(check.check(apk), [])

    def test_dex_strings_are_checked_in_every_dex_file(self):
        apk = self.apk(classes_dex=b"clean", **{"classes2.dex": b"Lcom/google/mlkit/vision/Foo;Lcom/google/android/datatransport/Bar;"})
        problems = check.check(apk)
        self.assertTrue(any("classes2.dex contains mlkit" in p for p in problems))
        self.assertTrue(any("datatransport" in p for p in problems))

    def test_the_photo_picker_action_string_is_not_the_library(self):
        apk = self.apk(classes_dex=b"2com.google.android.gms.provider.action.PICK_IMAGES")
        self.assertEqual(check.check(apk), [])
        apk = self.apk(classes_dex=b"Lcom/google/android/gms/common/Foo;")
        self.assertTrue(check.check(apk))

    def test_entry_names_are_checked(self):
        apk = self.apk(**{"META-INF__com.google.firebase_firebase-components.version": b"1"})
        self.assertTrue(any("firebase" in p for p in check.check(apk)))

    def test_debuggable_is_read_from_badging(self):
        plain = "package: name='dev.relay.app'\napplication-label:'Relay'\n"
        self.assertFalse(check.is_debuggable(plain))
        self.assertTrue(check.is_debuggable(plain + "application-debuggable\n"))

    def test_manifest_components_are_listed(self):
        self.assertEqual(
            check.manifest_components(MANIFEST),
            [
                ("uses-permission", "android.permission.INTERNET"),
                ("service", "dev.relay.app.CallService"),
                ("provider", "androidx.startup.InitializationProvider"),
            ],
        )


if __name__ == "__main__":
    unittest.main()
