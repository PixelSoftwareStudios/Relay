#!/usr/bin/env python3
"""Fail unless the APK is signed by a release key: valid signature, not the Android debug key.

Release APKs are signed from the android-signing environment in CI. A build that silently fell
back to the debug key would install, but nobody could ever update it, so this stops the release.
With --fingerprint the signer's SHA-256 must also equal the published one (docs/releasing.md).
"""
import argparse
import os
from pathlib import Path
import re
import subprocess


def find_apksigner():
    explicit = os.environ.get("APKSIGNER")
    if explicit:
        return Path(explicit)
    for home in (os.environ.get("ANDROID_HOME"), os.environ.get("ANDROID_SDK_ROOT"), str(Path.home() / "Library/Android/sdk")):
        if home:
            found = sorted(Path(home).glob("build-tools/*/apksigner"))
            if found:
                return found[-1]
    raise FileNotFoundError("apksigner not found (set ANDROID_HOME or APKSIGNER)")


def certificates(output):
    """The distinct (subject, sha256) signing certificates in `apksigner verify --print-certs` output.

    apksigner words the lines differently by version and signature scheme: "Signer #1 certificate DN: ...",
    "Signer (minSdkVersion=28, maxSdkVersion=...) certificate DN: ...", or "V2 Signer: certificate DN: ...".
    An APK verified under several schemes lists the same certificate once per scheme, so equal pairs count once.
    """
    who = r"(?:V[0-9.]+ )?Signer(?: #\d+| \([^)]*\))?:?"
    subjects = re.findall(rf"^{who} certificate DN: (.*)$", output, re.M)
    digests = re.findall(rf"^{who} certificate SHA-256 digest: ([0-9a-fA-F]+)\s*$", output, re.M)
    return sorted(set(zip(subjects, [d.lower() for d in digests])))


def problems(output, fingerprint=None):
    signers = certificates(output)
    if len(signers) != 1:
        # The output is public (certificate names and digests), so show it to make this debuggable.
        return [f"expected exactly one signer, found {len(signers)}; apksigner said:\n{output.strip()[:2000]}"]
    subject, digest = signers[0]
    found = []
    if "android debug" in subject.lower():
        found.append("APK is signed with the Android debug key")
    if fingerprint and digest != fingerprint.replace(":", "").lower():
        found.append(f"signer SHA-256 {digest} does not match the expected fingerprint")
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("apk", type=Path)
    parser.add_argument("--fingerprint", help="expected signer SHA-256 (hex, colons allowed)")
    args = parser.parse_args()
    result = subprocess.run([str(find_apksigner()), "verify", "--print-certs", "--verbose", str(args.apk)],
                            capture_output=True, text=True)
    if result.returncode != 0:
        parser.exit(1, f"apksigner rejected the APK:\n{result.stdout}{result.stderr}")
    found = problems(result.stdout, args.fingerprint)
    if found:
        parser.exit(1, "\n".join(found) + "\n")
    print("APK signature check passed: release-signed")


if __name__ == "__main__":
    main()
