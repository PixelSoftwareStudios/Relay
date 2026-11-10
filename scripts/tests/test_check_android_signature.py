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

    def test_the_newer_apksigner_wording_is_understood(self):
        newer = ("Verifies\nSigner (minSdkVersion=28, maxSdkVersion=2147483647) certificate DN: CN=Relay\n"
                 f"Signer (minSdkVersion=28, maxSdkVersion=2147483647) certificate SHA-256 digest: {DIGEST.upper()}\n")
        self.assertEqual(sig.problems(newer, DIGEST), [])
        debug = newer.replace("CN=Relay", "C=US, O=Android, CN=Android Debug")
        self.assertTrue(sig.problems(debug))

    def test_the_real_apksigner_output_from_the_release_build(self):
        real = (
            "Verifies\nVerified using v1 scheme (JAR signing): false\n"
            "Verified using v2 scheme (APK Signature Scheme v2): true\nNumber of signers: 1\n"
            "V2 Signer: certificate DN: CN=PixelSoftwareStudios, OU=PixelSoftwareStudios, O=PixelSoftwareStudios, L=San Francisco, ST=California, C=US\n"
            "V2 Signer: certificate SHA-256 digest: 0b6375ad94a1dc965d2a0f993c065161c89e8eb2de724c81c895d25e4aec80bb\n"
            "V2 Signer: certificate SHA-1 digest: 0f57f452b17696315ed86ab0fb8bd0e0b9c7fce1\n")
        pinned = "0B:63:75:AD:94:A1:DC:96:5D:2A:0F:99:3C:06:51:61:C8:9E:8E:B2:DE:72:4C:81:C8:95:D2:5E:4A:EC:80:BB"
        self.assertEqual(sig.problems(real, pinned), [])
        self.assertTrue(sig.problems(real, "00" * 32))

    def test_one_certificate_verified_under_several_schemes_is_one_signer(self):
        twice = output("CN=Relay").replace("Signer #1", "V2 Signer:") + output("CN=Relay").replace("Signer #1", "V3 Signer:")
        self.assertEqual(sig.problems(twice), [])

    def test_exactly_one_signer(self):
        two_keys = output("CN=Relay") + "\n" + output("CN=Other", digest="cd" * 32).replace("Signer #1", "Signer #2")
        self.assertTrue(sig.problems(two_keys))
        self.assertTrue(sig.problems(""))


if __name__ == "__main__":
    unittest.main()
