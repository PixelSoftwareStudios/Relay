import unittest

from packaging_fixtures import release_directory  # noqa: F401  (sets sys.path)
import check_android_signature as sig

DIGEST = "ab" * 32


def output(subject, digest=DIGEST, signers=1):
    return "\n".join(
        f"Signer #{n} certificate DN: {subject}\nSigner #{n} certificate SHA-256 digest: {digest}"
        for n in range(1, signers + 1))


class SignatureTests(unittest.TestCase):
    def test_a_release_key_passes(self):
        self.assertEqual(sig.problems(output("CN=Relay, O=Pixel")), [])

    def test_the_debug_key_is_refused(self):
        self.assertTrue(sig.problems(output("C=US, O=Android, CN=Android Debug")))

    def test_the_fingerprint_must_match_in_any_notation(self):
        colons = ":".join(DIGEST[i:i + 2] for i in range(0, len(DIGEST), 2)).upper()
        self.assertEqual(sig.problems(output("CN=Relay"), colons), [])
        self.assertTrue(sig.problems(output("CN=Relay"), "00" * 32))

    def test_exactly_one_signer(self):
        self.assertTrue(sig.problems(output("CN=Relay", signers=2)))
        self.assertTrue(sig.problems(""))


if __name__ == "__main__":
    unittest.main()
