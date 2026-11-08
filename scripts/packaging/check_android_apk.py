#!/usr/bin/env python3
"""Fail if the Android APK contains Google services, ML Kit or telemetry code.

The app holds INTERNET only for LAN sockets and must not bundle anything that
phones home. This looks at what is really in the built APK: the file names, the
merged manifest (read with aapt2) and the strings of every dex file.
"""
import argparse
import os
from pathlib import Path
import re
import subprocess
import zipfile

# Names that must not appear in the manifest, a dex file or an entry name.
FORBIDDEN = (
    "mlkit",
    "datatransport",
    "firebase",
    "firelog",
    "com/google/android/gms",
    "com.google.android.gms",
    "play-services",
    "play_services",
    "googleapiactivity",
    "com/google/android/play",
    "com.google.android.play",
    "crashlytics",
    "com.google.android.datatransport",
)


def forbidden_in_text(text):
    """The forbidden names found in `text`, case-insensitively."""
    lowered = text.lower()
    return sorted({name for name in FORBIDDEN if name in lowered})


# androidx.activity names the Play services photo picker in two intent-action strings
# (`com.google.android.gms.provider.action.PICK_IMAGES`); that is a constant, not the library.
# In a dex file a class always appears as a slash descriptor, so the dotted forms are only
# looked for in the manifest and entry names.
DEX_SKIPPED = ("com.google.android.gms", "com.google.android.play", "com.google.android.datatransport")


def forbidden_in_bytes(data):
    lowered = data.lower()
    return sorted({name for name in FORBIDDEN if name not in DEX_SKIPPED and name.encode() in lowered})


def find_aapt2():
    explicit = os.environ.get("AAPT2")
    if explicit:
        return Path(explicit)
    for home in (os.environ.get("ANDROID_HOME"), os.environ.get("ANDROID_SDK_ROOT"), str(Path.home() / "Library/Android/sdk")):
        if not home:
            continue
        found = sorted(Path(home).glob("build-tools/*/aapt2"))
        if found:
            return found[-1]
    return None


def manifest_text(apk, aapt2):
    result = subprocess.run(
        [str(aapt2), "dump", "xmltree", "--file", "AndroidManifest.xml", str(apk)],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout


def badging_text(apk, aapt2):
    result = subprocess.run(
        [str(aapt2), "dump", "badging", str(apk)],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout


def is_debuggable(badging):
    """aapt2 prints `application-debuggable` only for a debuggable app."""
    return any(line.strip() == "application-debuggable" for line in badging.splitlines())


def manifest_components(text):
    """(kind, name) for every service, receiver, provider, activity and permission."""
    found = []
    kind = None
    for line in text.splitlines():
        element = re.match(r"\s*E: (activity|activity-alias|service|receiver|provider|uses-permission|permission) ", line)
        if element:
            kind = element.group(1)
            continue
        name = re.search(r'A: (?:android|http://schemas.android.com/apk/res/android):name\(0x01010003\)="([^"]*)"', line)
        if name and kind:
            found.append((kind, name.group(1)))
            kind = None
    return found


def check(apk, aapt2=None, allow_debuggable=False):
    """The list of problems found; empty when the APK is clean."""
    problems = []
    with zipfile.ZipFile(apk) as archive:
        for name in archive.namelist():
            for hit in forbidden_in_text(name):
                problems.append(f"entry {name} contains {hit}")
            if re.fullmatch(r"classes\d*\.dex", name):
                for hit in forbidden_in_bytes(archive.read(name)):
                    problems.append(f"{name} contains {hit}")
    if aapt2 is not None:
        for hit in forbidden_in_text(manifest_text(apk, aapt2)):
            problems.append(f"merged manifest contains {hit}")
        if not allow_debuggable and is_debuggable(badging_text(apk, aapt2)):
            problems.append("APK is debuggable (build without -Prelay.debuggable=true, or pass --allow-debuggable)")
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("apk", type=Path)
    parser.add_argument("--allow-debuggable", action="store_true", help="accept a debuggable APK (development builds)")
    parser.add_argument("--list", action="store_true", help="print the manifest components and permissions")
    args = parser.parse_args()
    aapt2 = find_aapt2()
    if aapt2 is None or not aapt2.is_file():
        parser.exit(1, "aapt2 not found (set ANDROID_HOME or AAPT2): cannot read the merged manifest\n")
    if args.list:
        for kind, name in manifest_components(manifest_text(args.apk, aapt2)):
            print(f"{kind}\t{name}")
    problems = check(args.apk, aapt2, args.allow_debuggable)
    if problems:
        parser.exit(1, "APK contains forbidden components:\n  " + "\n  ".join(problems) + "\n")
    print("APK check passed: no Google services, ML Kit or telemetry, not debuggable")


if __name__ == "__main__":
    main()
