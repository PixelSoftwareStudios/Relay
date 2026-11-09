#!/usr/bin/env python3
"""Sign release packages with minisign and verify every signature (fails closed).

The secret key comes from an environment variable and only ever exists as a 0600 temp file
that is deleted in a finally block. The password goes to minisign over stdin, never argv.
"""
import argparse
import base64
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

import package_release as release

PUBLIC_KEY = release.ROOT / "crates/relay-update/update-key.pub"
TEST_KEY_FIXTURES = release.ROOT / "crates/relay-update/tests/fixtures"
TEST_KEY_IDS = Path(__file__).with_name("test_update_key_id.txt")
KEY_ENV = "UPDATE_SIGNING_KEY"
PASSWORD_ENV = "UPDATE_SIGNING_PASSWORD"


def trusted_comment(version, name):
    """The exact trusted comment the updater requires (shared contract with relay-update)."""
    return f"relay-update {version} {name}"


def signed_names(version):
    return sorted(release.signed_assets(version))


def public_key_bytes(text):
    """Algorithm, key id and key (42 bytes) of a minisign public key file."""
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    if len(lines) != 2 or not lines[0].startswith("untrusted comment:"):
        raise ValueError("Not a minisign public key file")
    raw = base64.b64decode(lines[1], validate=True)
    if len(raw) != 42 or raw[:2] != b"Ed":
        raise ValueError("Not an Ed25519 minisign public key")
    return raw[2:]


def test_keys(fixtures=None, ids=None):
    """Known test-only keys: fixture .pub files plus hex key ids from the constant file."""
    fixtures = TEST_KEY_FIXTURES if fixtures is None else fixtures
    ids = TEST_KEY_IDS if ids is None else ids
    keys, key_ids = [], set()
    if fixtures.is_dir():
        keys = [public_key_bytes(p.read_text("utf-8")) for p in sorted(fixtures.glob("*.pub"))]
    if ids.is_file():
        for line in ids.read_text("utf-8").splitlines():
            line = line.split("#")[0].strip().upper()
            if line:
                key_ids.add(line)
    return keys, key_ids


def check_release_key(public_key=None, fixtures=None, ids=None):
    """Refuse the compiled-in key if it is a test key, or if no test key is known to compare."""
    public_key = PUBLIC_KEY if public_key is None else public_key
    key = public_key_bytes(public_key.read_text("utf-8"))
    keys, key_ids = test_keys(fixtures, ids)
    if not keys and not key_ids:
        raise ValueError("No test key to compare against: add a fixture .pub or a key id to "
                         "scripts/packaging/test_update_key_id.txt")
    if key in keys or key[:8][::-1].hex().upper() in key_ids or key[:8].hex().upper() in key_ids:
        raise ValueError(f"{public_key} holds the TEST-ONLY update key; replace it with the "
                         "release public key (docs/releasing.md, Update signing)")


def run(command, **options):
    # Output is captured: minisign must never echo anything secret into the job log.
    return subprocess.run(command, capture_output=True, text=True, check=False, **options)


def sign_one(minisign, key_file, password, source, signature, comment):
    done = run([*minisign, "-S", "-s", str(key_file), "-m", str(source), "-x", str(signature),
                "-t", comment], input=password + "\n")
    if done.returncode != 0 or not signature.is_file():
        raise ValueError(f"minisign failed to sign {source.name}")


def verify_one(minisign, public_key, source, signature, comment):
    done = run([*minisign, "-V", "-p", str(public_key), "-m", str(source), "-x", str(signature)])
    if done.returncode != 0:
        raise ValueError(f"Signature does not verify against {public_key.name}: {source.name}")
    lines = signature.read_text("utf-8").splitlines()
    if len(lines) != 4 or lines[2] != f"trusted comment: {comment}":
        raise ValueError(f"Unexpected trusted comment in {signature.name}")


def sign_release(version, assets, output, minisign, public_key=None, environ=None):
    environ = os.environ if environ is None else environ
    public_key = PUBLIC_KEY if public_key is None else public_key
    if version != release.workspace_version():
        raise ValueError("Signing version must match the workspace manifest")
    key, password = environ.get(KEY_ENV, ""), environ.get(PASSWORD_ENV, "")
    if not key.strip() or not password:
        raise ValueError(f"{KEY_ENV} and {PASSWORD_ENV} must be set")
    names = signed_names(version)
    for name in names:
        path = assets / name
        if not path.is_file() or path.is_symlink() or path.stat().st_size == 0:
            raise ValueError(f"Missing, empty or symlinked asset to sign: {name}")
    output.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="relay-sign-"))
    key_file = scratch / "minisign.key"
    try:
        descriptor = os.open(key_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(key if key.endswith("\n") else key + "\n")
        for name in names:
            comment = trusted_comment(version, name)
            signature = output / (name + ".minisig")
            sign_one(minisign, key_file, password, assets / name, signature, comment)
            verify_one(minisign, public_key, assets / name, signature, comment)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    print(f"Signed and verified {len(names)} release assets")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("check-key")
    sign = commands.add_parser("sign")
    sign.add_argument("--version", required=True)
    sign.add_argument("--assets", type=Path, required=True)
    sign.add_argument("--output", type=Path, required=True)
    sign.add_argument("--minisign", default="minisign", help="minisign executable")
    args = parser.parse_args()
    try:
        if args.command == "check-key":
            check_release_key()
            print("Update public key is not a test key")
        else:
            sign_release(args.version, args.assets, args.output, [args.minisign])
    except (OSError, ValueError) as error:
        parser.exit(1, f"Release signing failed: {error}\n")


if __name__ == "__main__":
    main()
