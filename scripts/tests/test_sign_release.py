"""Signing flow against a fake minisign: key handling, trusted comments, fail-closed checks."""
import base64
import os
from pathlib import Path
import sys
import tempfile
import unittest

import packaging_fixtures  # noqa: F401  (sets sys.path)
import package_release as release
import sign_release as sign

FAKE = [sys.executable, str(Path(__file__).with_name("fake_minisign.py"))]
KEY_TEXT = "SECRET-KEY-CONTENT"


def public_key(identifier, key):
    raw = b"Ed" + identifier + key
    return "untrusted comment: minisign public key\n" + base64.b64encode(raw).decode() + "\n"


class SignTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="relay-sign-test-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.version = release.workspace_version()
        self.assets = self.root / "assets"
        self.output = self.root / "sigs"
        self.assets.mkdir()
        for name in release.package_assets(self.version):
            (self.assets / name).write_bytes(b"fixture " + name.encode())
        self.environ = {sign.KEY_ENV: KEY_TEXT, sign.PASSWORD_ENV: "hunter2"}
        self.pub = self.root / "update-key.pub"
        self.pub.write_text(public_key(b"A" * 8, b"K" * 32))

    def sign(self, **changes):
        sign.sign_release(self.version, self.assets, self.output, FAKE, self.pub,
                          {**self.environ, **changes})

    @unittest.skipIf(sys.platform == "win32", "signing runs on Linux; the fake signer checks the POSIX 0600 mode of the key file")
    def test_signs_every_package_except_the_apk(self):
        self.sign()
        names = {p.name for p in self.output.iterdir()}
        self.assertEqual(names, {n + ".minisig" for n in release.signed_assets(self.version)})
        self.assertEqual(len(names), 22)
        name = f"relay-{self.version}-macos-arm64-update.zip"
        self.assertIn(f"trusted comment: relay-update {self.version} {name}\n",
                      (self.output / (name + ".minisig")).read_text())

    def test_trusted_comment_format(self):
        self.assertEqual(sign.trusted_comment("0.1.1", "a.zip"), "relay-update 0.1.1 a.zip")

    @unittest.skipIf(sys.platform == "win32", "signing runs on Linux; the fake signer checks the POSIX 0600 mode of the key file")
    def test_secret_files_are_removed_and_never_logged(self):
        before = set(Path(tempfile.gettempdir()).glob("relay-sign-*"))
        self.sign()
        self.assertEqual(set(Path(tempfile.gettempdir()).glob("relay-sign-*")), before)
        self.assertNotIn(KEY_TEXT, "".join(p.read_text() for p in self.output.iterdir()))

    def test_failure_still_removes_the_key_file(self):
        before = set(Path(tempfile.gettempdir()).glob("relay-sign-*"))
        with self.assertRaises(ValueError):
            self.sign(**{sign.PASSWORD_ENV: "wrong"})
        self.assertEqual(set(Path(tempfile.gettempdir()).glob("relay-sign-*")), before)

    def test_missing_secrets_or_assets_fail_closed(self):
        for variable in (sign.KEY_ENV, sign.PASSWORD_ENV):
            with self.assertRaises(ValueError):
                self.sign(**{variable: ""})
        (self.assets / f"relay-{self.version}-linux-x64.deb").unlink()
        with self.assertRaisesRegex(ValueError, "Missing"):
            self.sign()

    def test_bad_trusted_comment_or_signature_is_rejected(self):
        source = self.assets / f"relay-{self.version}-linux-x64.deb"
        signature = self.root / "x.minisig"
        signature.write_text("untrusted comment: x\nSIG\ntrusted comment: other\nG\n")
        with self.assertRaises(ValueError):
            sign.verify_one(FAKE, self.pub, source, signature, "relay-update")

    def test_missing_minisign_binary_is_an_error(self):
        with self.assertRaises(OSError):
            sign.sign_release(self.version, self.assets, self.output, ["/nonexistent/minisign"],
                              self.pub, self.environ)


class KeyCheckTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="relay-key-test-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.fixtures = self.root / "fixtures"
        self.fixtures.mkdir()
        self.ids = self.root / "ids.txt"
        self.pub = self.root / "update-key.pub"

    def check(self):
        sign.check_release_key(self.pub, self.fixtures, self.ids)

    def test_test_key_in_fixtures_is_refused(self):
        text = public_key(b"T" * 8, b"X" * 32)
        (self.fixtures / "test.pub").write_text(text)
        self.pub.write_text(text)
        with self.assertRaisesRegex(ValueError, "TEST-ONLY"):
            self.check()

    def test_test_key_id_in_constant_file_is_refused(self):
        self.pub.write_text(public_key(bytes.fromhex("0102030405060708"), b"X" * 32))
        self.ids.write_text("# comment\n0807060504030201\n")
        with self.assertRaisesRegex(ValueError, "TEST-ONLY"):
            self.check()

    def test_other_key_passes(self):
        (self.fixtures / "test.pub").write_text(public_key(b"T" * 8, b"X" * 32))
        self.pub.write_text(public_key(b"R" * 8, b"Y" * 32))
        self.check()

    def test_nothing_to_compare_fails_closed(self):
        self.pub.write_text(public_key(b"R" * 8, b"Y" * 32))
        with self.assertRaisesRegex(ValueError, "No test key"):
            self.check()

    def test_malformed_key_is_refused(self):
        self.pub.write_text("garbage\n")
        with self.assertRaises(ValueError):
            self.check()


if __name__ == "__main__":
    unittest.main()
