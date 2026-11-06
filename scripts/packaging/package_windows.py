#!/usr/bin/env python3
"""Build x64 and ARM64 MSI installers from one WiX 5 template."""
import argparse
from pathlib import Path
import struct
import subprocess
import tempfile
from xml.sax.saxutils import quoteattr

import licenses
import package_release as release

ARCHITECTURES = {"x86_64-pc-windows-msvc": "x64", "aarch64-pc-windows-msvc": "arm64"}
ICON_SIZES = (16, 32, 48, 64, 128, 256)


def png_size(data):
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("Icon source is not a PNG")
    return struct.unpack(">II", data[16:24])


def make_ico(pngs):
    """Pack PNG images into an ICO (PNG-compressed entries, Windows Vista and later)."""
    images = [(png_size(data), data) for data in pngs]
    out = bytearray(struct.pack("<HHH", 0, 1, len(images)))
    offset = 6 + 16 * len(images)
    for (width, height), data in images:
        if width != height or width > 256:
            raise ValueError("ICO images must be square and at most 256 px")
        out += struct.pack("<BBBBHHII", width % 256, height % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return bytes(out) + b"".join(data for _, data in images)


def documents_fragment(documents):
    """A WiX fragment with one component per licence or example file."""
    lines = ['<?xml version="1.0" encoding="UTF-8"?>',
             '<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">', "\t<Fragment>",
             '\t\t<DirectoryRef Id="DOCFOLDER">']
    for index, (relative, source) in enumerate(sorted(documents.items())):
        name = relative.removeprefix(licenses.DOCS + "/")
        if "/" in name:
            continue
        lines += [f'\t\t\t<Component Id="Doc{index}" Guid="*" Bitness="always64">',
                  f"\t\t\t\t<File Id=\"DocFile{index}\" Source={quoteattr(str(source))} "
                  f"Name={quoteattr(name)} KeyPath=\"yes\" />", "\t\t\t</Component>"]
    lines += ["\t\t</DirectoryRef>", '\t\t<ComponentGroup Id="Documents">']
    lines += [f'\t\t\t<ComponentRef Id="Doc{i}" />' for i, (r, _) in enumerate(sorted(documents.items()))
              if "/" not in r.removeprefix(licenses.DOCS + "/")]
    lines += ["\t\t</ComponentGroup>", "\t</Fragment>", "</Wix>", ""]
    return "\n".join(lines)


def package(version, target, bin_dir, output):
    release.validate_package_input(version, target)
    if target not in ARCHITECTURES:
        raise ValueError(f"Unsupported MSI architecture: {target}")
    binaries = release.find_binaries(target, bin_dir, release.DESKTOP_PROGRAMS)
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    installer = output / f"relay-{version}-{release.TARGETS[target]}.msi"
    with tempfile.TemporaryDirectory(prefix="relay-msi-") as directory:
        temporary = Path(directory)
        icon = temporary / "relay.ico"
        icon.write_bytes(make_ico([(release.ROOT / f"assets/icon/relay-mark-{size}.png").read_bytes()
                                   for size in ICON_SIZES]))
        fragment = temporary / "documents.wxs"
        fragment.write_text(documents_fragment(licenses.documents(True, target)), encoding="utf-8")
        subprocess.run(["wix", "build", "-arch", ARCHITECTURES[target], "-pdbtype", "none",
                        "-d", f"Version={release.numeric_version(version)}",
                        "-d", f"Desktop={binaries['relay-desktop'].resolve()}",
                        "-d", f"Cli={binaries['relay'].resolve()}",
                        "-d", f"Daemon={binaries['relayd'].resolve()}",
                        "-d", f"Icon={icon}", "-o", str(installer),
                        str(release.ROOT / "packaging/windows.wxs"), str(fragment)], check=True)
    release.write_checksum(installer)
    print(installer)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", choices=ARCHITECTURES, required=True)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        package(args.version, args.target, args.bin_dir, args.output)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Windows packaging failed: {error}\n")


if __name__ == "__main__":
    main()
