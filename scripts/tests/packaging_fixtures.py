"""Shared fixtures: header-only executables and a licence tree, nothing is ever run."""
import shutil
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "packaging"))
import licenses
import package_release as release


def executable(path, target):
    if "apple" in target:
        cpu = 0x0100000C if target.startswith("aarch64") else 0x01000007
        header = struct.pack("<II", 0xFEEDFACF, cpu) + bytes(56)
    elif "windows" in target:
        header = bytearray(64)
        header[:2] = b"MZ"
        struct.pack_into("<I", header, 60, 64)
        header += b"PE\0\0" + struct.pack("<H", 0xAA64 if target.startswith("aarch64") else 0x8664)
    else:
        header = bytearray(64)
        header[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", header, 18, 183 if target.startswith("aarch64") else 62)
    path.write_bytes(header)
    return path


def release_directory(root, target, programs=release.DESKTOP_PROGRAMS):
    directory = root / "release"
    directory.mkdir(exist_ok=True)
    for program in programs:
        executable(directory / release.executable_name(program, target), target)
    return directory


def licence_tree(root):
    """A copy of the files licenses.py reads, with a generated notices file."""
    tree = root / "tree"
    for relative in ("LICENSE", "README.md", "contrib/systemd/relayd.service",
                     "contrib/launchd/org.relay.relayd.plist"):
        (tree / relative).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(release.ROOT / relative, tree / relative)
    shutil.copytree(release.ROOT / "assets/font", tree / "assets/font",
                    ignore=shutil.ignore_patterns("*.ttf", "*.otf", "*.py"))
    (tree / licenses.NOTICES).write_text("notices\n")
    return tree
