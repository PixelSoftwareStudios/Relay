#!/usr/bin/env python3
"""Name the APK built by `just android-apk` for the release and checksum it."""
import argparse
from pathlib import Path
import shutil
import zipfile

import package_release as release

ABI_LIBRARY = "lib/arm64-v8a/"


def inspect(apk):
    with zipfile.ZipFile(apk) as archive:
        names = archive.namelist()
    if "AndroidManifest.xml" not in names:
        raise ValueError("APK has no AndroidManifest.xml")
    if not any(n.startswith(ABI_LIBRARY) and n.endswith(".so") for n in names):
        raise ValueError("APK has no arm64-v8a native library")


def package(version, apk, output):
    if version != release.workspace_version():
        raise ValueError("APK version must match the workspace manifest")
    if not apk.is_file() or apk.is_symlink():
        raise ValueError(f"APK must be a regular file: {apk}")
    try:
        inspect(apk)
    except zipfile.BadZipFile as error:
        raise ValueError(f"Not a valid APK: {apk}") from error
    output.mkdir(parents=True, exist_ok=True)
    target = output / f"relay-{version}-{release.ANDROID_SUFFIX}.apk"
    shutil.copy2(apk, target)
    release.write_checksum(target)
    return target


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--apk", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        print(package(args.version, args.apk, args.output))
    except (OSError, ValueError) as error:
        parser.exit(1, f"Android packaging failed: {error}\n")
