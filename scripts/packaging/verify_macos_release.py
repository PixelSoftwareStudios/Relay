#!/usr/bin/env python3
"""Inspect the DMG and update ZIP without launching the app: both must hold the identical app."""
import argparse
import os
from pathlib import Path, PurePosixPath
import plistlib
import stat
import subprocess
import sys
import tempfile
import zipfile

import package_macos as macos
import package_release as release


def check_zip_entries(entries):
    """entries: (name, external_attr) pairs. Everything must sit inside Relay.app."""
    for name, attributes in entries:
        path = PurePosixPath(name)
        if (not name.startswith(macos.APP + "/") and name != macos.APP) or ".." in path.parts \
                or "\\" in name or stat.S_ISLNK(attributes >> 16):
            raise ValueError(f"Unexpected updater ZIP entry: {name}")


def app_files(bundle):
    paths = list(bundle.rglob("*"))
    if any(path.is_symlink() for path in paths):
        raise ValueError(f"Unexpected symlink inside {macos.APP}")
    return {path.relative_to(bundle).as_posix(): release.digest(path)
            for path in paths if path.is_file()}


def check_bundle(app, version, target):
    for program in release.DESKTOP_PROGRAMS:
        binary = app / "Contents/MacOS" / program
        release.validate_binary(target, binary)
        if binary.stat().st_mode & 0o111 != 0o111:
            raise ValueError(f"Lost executable permissions: {binary}")
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    numeric = release.numeric_version(version)
    if info.get("CFBundleExecutable") != "relay-desktop" or info.get("CFBundleShortVersionString") != numeric:
        raise ValueError("Info.plist does not match the release")
    if not info.get("NSMicrophoneUsageDescription"):
        raise ValueError("Info.plist lost NSMicrophoneUsageDescription")


def verify_layout(mount):
    from ds_store import DSStore

    with DSStore.open(str(mount / ".DS_Store"), "r") as store:
        for item, place in macos.DMG_LAYOUT["icon_locations"].items():
            if store[item]["Iloc"] != place:
                raise ValueError(f"DMG icon position wrong for {item}")


def verify(version, target, image, update):
    if sys.platform != "darwin":
        raise ValueError("DMG verification requires macOS")
    with zipfile.ZipFile(update) as archive:
        check_zip_entries((i.filename, i.external_attr) for i in archive.infolist())
    with tempfile.TemporaryDirectory(prefix="relay-macos-verify-") as directory:
        root = Path(directory)
        subprocess.run(["/usr/bin/ditto", "-x", "-k", str(update.resolve()), str(root / "extracted")],
                       check=True)
        app = root / "extracted" / macos.APP
        subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)], check=True)
        check_bundle(app, version, target)
        mount = root / "mounted"
        mount.mkdir()
        subprocess.run(["/usr/bin/hdiutil", "attach", "-readonly", "-nobrowse", "-mountpoint",
                        str(mount), str(image.resolve())], check=True)
        try:
            installed = mount / macos.APP
            subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(installed)],
                           check=True)
            if app_files(installed) != app_files(app):
                raise ValueError("DMG and update ZIP contain different application files")
            if not (mount / "Applications").is_symlink() or os.readlink(mount / "Applications") != "/Applications":
                raise ValueError("DMG is missing its Applications shortcut")
            verify_layout(mount)
        finally:
            subprocess.run(["/usr/bin/hdiutil", "detach", str(mount)], check=True)
    print(f"Verified {target}: identical ad-hoc signed app in DMG and update ZIP")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", choices=[t for t in release.TARGETS if "apple" in t], required=True)
    parser.add_argument("--dmg", type=Path, required=True)
    parser.add_argument("--update", type=Path, required=True)
    args = parser.parse_args()
    try:
        verify(args.version, args.target, args.dmg, args.update)
    except (OSError, ValueError, subprocess.CalledProcessError, zipfile.BadZipFile) as error:
        parser.exit(1, f"macOS release verification failed: {error}\n")


if __name__ == "__main__":
    main()
