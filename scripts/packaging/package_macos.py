#!/usr/bin/env python3
"""Build the ad-hoc signed Relay.app, its DMG and the update ZIP from native executables."""
import argparse
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import zipfile

import licenses
import package_release as release

ROOT = release.ROOT
INFO = ROOT / "apps/relay-desktop/resources/Info.plist"
ICON = ROOT / "assets/icon/relay.icns"
APP = "Relay.app"
MINIMUM_SYSTEM = "13.0"
IDENTIFIER_PREFIX = "dev.relay"
BACKGROUND = ROOT / "assets/packaging/macos/dmg-background.png"
DMG_LAYOUT = {"window_rect": ((200, 200), (640, 356)), "icon_size": 128,
              "icon_locations": {APP: (160, 150), "Applications": (480, 150)}}


def metadata(version):
    """The embedded Info.plist (bundle id, microphone text) plus the bundle keys."""
    info = plistlib.loads(INFO.read_bytes())
    numeric = release.numeric_version(version)
    info.update({
        "CFBundleExecutable": "relay-desktop",
        "CFBundleDisplayName": "Relay",
        "CFBundleIconFile": ICON.stem,
        "CFBundlePackageType": "APPL",
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleShortVersionString": numeric,
        "CFBundleVersion": numeric,
        "LSMinimumSystemVersion": MINIMUM_SYSTEM,
        "NSHighResolutionCapable": True,
        "LSApplicationCategoryType": "public.app-category.social-networking",
    })
    return info


def populate_bundle(binaries, destination, version):
    """relay-desktop is the bundle executable; relayd and relay sit beside it (docs/packaging.md)."""
    contents = Path(destination) / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    (contents / "Resources").mkdir()
    for path in binaries.values():
        shutil.copy2(path, contents / "MacOS" / path.name)
        (contents / "MacOS" / path.name).chmod(0o755)
    shutil.copy2(ICON, contents / "Resources" / ICON.name)
    for name, source in licenses.sources(True).items():
        shutil.copy2(source, contents / "Resources" / name)
    with (contents / "Info.plist").open("wb") as stream:
        plistlib.dump(metadata(version), stream, sort_keys=False)
    (contents / "PkgInfo").write_bytes(b"APPL????")


def sign_bundle(bundle):
    """Ad-hoc only. Fixed identifiers keep each program's designated requirement stable."""
    macos = bundle / "Contents/MacOS"
    for program in release.DESKTOP_PROGRAMS:
        subprocess.run(["/usr/bin/codesign", "--force", "--sign", "-", "--timestamp=none",
                        "--identifier", f"{IDENTIFIER_PREFIX}.{program}", str(macos / program)],
                       check=True)
    subprocess.run(["/usr/bin/codesign", "--force", "--sign", "-", "--timestamp=none", str(bundle)],
                   check=True)
    subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(bundle)], check=True)


def write_update_archive(app, archive):
    """The raw .app for self-updating: forward-slash paths, Unix modes, no symlinks."""
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as stream:
        stream.write(app, app.name)
        for entry in sorted(app.rglob("*")):
            if entry.is_symlink():
                raise ValueError(f"Unexpected symlink in updater bundle: {entry}")
            stream.write(entry, entry.relative_to(app.parent).as_posix())
    release.write_checksum(archive)


def retina_background(directory):
    """One multi-resolution TIFF from the 1x and @2x arrows, so the Finder window is sharp on Retina."""
    result = directory / "dmg-background.tiff"
    retina = BACKGROUND.with_name(BACKGROUND.stem + "@2x.png")
    subprocess.run(["/usr/bin/tiffutil", "-cathidpicheck", str(BACKGROUND), str(retina), "-out", str(result)],
                   check=True, capture_output=True)
    return result


def disk_image_settings(app, background):
    return {"format": "UDZO", "files": [str(app)], "symlinks": {"Applications": "/Applications"},
            "icon": str(ICON), "background": str(background), "default_view": "icon-view",
            "text_size": 14, **DMG_LAYOUT}


def build_disk_image(app, archive):
    try:
        import dmgbuild
    except ImportError as error:
        raise ValueError("macOS packaging requires: python -m pip install dmgbuild==1.6.7") from error
    with tempfile.TemporaryDirectory() as scratch:
        background = retina_background(Path(scratch))
        dmgbuild.build_dmg(str(archive), "Relay", settings=disk_image_settings(app, background))
    # Do not hide .app extensions: changing FinderInfo invalidates strict signatures.
    subprocess.run(["/usr/bin/hdiutil", "verify", str(archive)], check=True)


def release_archives(binaries, output, version, target, name):
    """The DMG and the update ZIP carry the exact same signed app."""
    if sys.platform != "darwin":
        raise ValueError("DMG packaging requires macOS")
    with tempfile.TemporaryDirectory(prefix="relay-dmg-") as directory:
        app = Path(directory) / APP
        populate_bundle(binaries, app, version)
        sign_bundle(app)
        archive = output / (name + ".dmg")
        build_disk_image(app, archive)
        release.write_checksum(archive)
        write_update_archive(app, output / (name + "-update.zip"))
    print(archive)
    print(output / (name + "-update.zip"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", choices=[t for t in release.TARGETS if "apple" in t], required=True)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        release.package(args.version, args.target, args.bin_dir, args.output)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"macOS packaging failed: {error}\n")


if __name__ == "__main__":
    main()
