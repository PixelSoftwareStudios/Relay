#!/usr/bin/env python3
"""Validate a release, build desktop archives and verify the complete release set."""
import argparse
import hashlib
import os
from pathlib import Path
import re
import struct
import subprocess
import tarfile
import tomllib
import zipfile

import licenses

ROOT = Path(__file__).resolve().parents[2]
REPOSITORY = "https://github.com/PixelSoftwareStudios/Relay"
TARGETS = {
    "x86_64-pc-windows-msvc": "windows-x64",
    "aarch64-pc-windows-msvc": "windows-arm64",
    "aarch64-apple-darwin": "macos-arm64",
    "x86_64-apple-darwin": "macos-x64",
    "x86_64-unknown-linux-gnu": "linux-x64",
    "aarch64-unknown-linux-gnu": "linux-arm64",
}
ANDROID_SUFFIX = "android-arm64-v8a"
# relay-desktop is the window; relay and relayd are the headless pair (docs/packaging.md).
CLI_PROGRAMS = ("relay", "relayd")
DESKTOP_PROGRAMS = ("relay-desktop",) + CLI_PROGRAMS
VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
                     r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
                     r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?")


def workspace_version():
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    match = VERSION.fullmatch(version)
    if not match or any(int(n) > limit for n, limit in zip(match.groups(), (255, 255, 65535))):
        raise ValueError(f"Version must be semver within MSI limits: {version}")
    return version


def numeric_version(version):
    return version.split("-")[0].split("+")[0]


def executable_name(program, target):
    return program + (".exe" if "windows" in target else "")


def toolchain():
    """The pinned compiler channel, so workflows never repeat it."""
    return tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]


def validate(tag):
    version = workspace_version()
    if tag and tag != f"v{version}":
        raise ValueError(f"Tag {tag!r} must match workspace version v{version}")
    if output := os.environ.get("GITHUB_OUTPUT"):
        with Path(output).open("a", encoding="utf-8", newline="\n") as stream:
            stream.write(f"version={version}\ntoolchain={toolchain()}\n")
    print(version)


def validate_binary(target, binary):
    """Check the object header against the target without running the file."""
    if target not in TARGETS:
        raise ValueError("Unknown native executable target")
    if not binary.is_file() or binary.is_symlink():
        raise ValueError(f"Release binary must be a regular file: {binary}")
    with binary.open("rb") as stream:
        header = stream.read(64)
        if "apple" in target:
            cpu = 0x0100000C if target.startswith("aarch64") else 0x01000007
            valid = header[:8] == struct.pack("<II", 0xFEEDFACF, cpu)
        elif "windows" in target:
            valid = len(header) == 64 and header[:2] == b"MZ"
            if valid:
                stream.seek(struct.unpack_from("<I", header, 60)[0])
                cpu = 0xAA64 if target.startswith("aarch64") else 0x8664
                valid = stream.read(6) == b"PE\0\0" + struct.pack("<H", cpu)
        else:
            cpu = 183 if target.startswith("aarch64") else 62
            valid = (len(header) == 64 and header[:6] == b"\x7fELF\x02\x01"
                     and struct.unpack_from("<H", header, 18)[0] == cpu
                     and header[7] in (0, 3))
        if not valid:
            raise ValueError(f"Executable header does not match {target}: {binary}")


def find_binaries(target, directory, programs):
    """Return {program: path} for a cargo release directory, validating each."""
    found = {}
    for program in programs:
        path = Path(directory) / executable_name(program, target)
        validate_binary(target, path)
        found[program] = path
    return found


def validate_package_input(version, target):
    if version != workspace_version() or target not in TARGETS:
        raise ValueError("Package version or target does not match the release contract")


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def checksum_line(path):
    return f"{digest(path)}  {path.name}\n".encode("ascii")


def write_checksum(path):
    if not path.is_file() or path.is_symlink() or path.stat().st_size == 0:
        raise ValueError(f"Missing, empty or symlinked asset: {path}")
    sidecar = path.with_name(path.name + ".sha256")
    if sidecar.is_symlink():
        raise ValueError(f"Refusing symlink checksum: {sidecar}")
    sidecar.write_bytes(checksum_line(path))


def tar_entry(executables):
    def apply(info):
        # Windows hosts have no executable bits; set the mode explicitly everywhere.
        info.mode = 0o755 if info.isdir() or Path(info.name).name in executables else 0o644
        info.uid = info.gid = 0
        info.uname = info.gname = ""
        info.mtime = 0
        return info
    return apply


def write_archive(archive, top, files, windows, executables):
    """Write {relative path: source} below one top-level directory."""
    if windows:
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as stream:
            for relative, source in sorted(files.items()):
                stream.write(source, f"{top}/{relative}")
    else:
        with tarfile.open(archive, "w:gz") as stream:
            for relative, source in sorted(files.items()):
                stream.add(source, arcname=f"{top}/{relative}", filter=tar_entry(executables),
                           recursive=False)
    write_checksum(archive)


def payload(binaries, desktop, target):
    files = {f"bin/{path.name}": path for path in binaries.values()}
    files.update(licenses.documents(desktop, target))
    return files


def package(version, target, bin_dir, output):
    """Portable desktop archive; macOS delegates to package_macos for the DMG and update zip."""
    validate_package_input(version, target)
    binaries = find_binaries(target, bin_dir, DESKTOP_PROGRAMS)
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    name = f"relay-{version}-{TARGETS[target]}"
    if "apple" in target:
        from package_macos import release_archives
        return release_archives(binaries, output, version, target, name)
    windows = "windows" in target
    archive = output / (name + ("-portable.zip" if windows else ".tar.gz"))
    write_archive(archive, name, payload(binaries, True, target), windows,
                  {executable_name(p, target) for p in DESKTOP_PROGRAMS})
    print(archive)


def expected_assets(version):
    names = {f"relay-{version}-{ANDROID_SUFFIX}.apk"}
    for target, suffix in TARGETS.items():
        name = f"relay-{version}-{suffix}"
        names.add(f"relay-cli-{version}-{suffix}" + (".zip" if "windows" in target else ".tar.gz"))
        if "apple" in target:
            names.update((name + ".dmg", name + "-update.zip"))
        elif "windows" in target:
            names.update(name + ext for ext in ("-portable.zip", ".msi"))
        else:
            names.update(name + ext for ext in (".tar.gz", ".deb", ".rpm", ".AppImage"))
    return names


def verify_assets(version, output, finalize=False):
    if version != workspace_version():
        raise ValueError("Asset version must match the workspace manifest")
    assets = expected_assets(version)
    manifest = output / "SHA256SUMS.txt"
    if manifest.is_symlink():
        raise ValueError("Refusing symlink SHA256SUMS.txt")
    required = assets | ({name + ".sha256" for name in assets} if finalize else {manifest.name})
    actual = {p.name for p in output.iterdir()}
    if finalize:
        actual.discard(manifest.name)
    if actual != required:
        raise ValueError(f"Release mismatch: missing={sorted(required - actual)}, "
                         f"unexpected={sorted(actual - required)}")
    lines = []
    for name in sorted(assets):
        path = output / name
        if not path.is_file() or path.is_symlink() or path.stat().st_size == 0:
            raise ValueError(f"Invalid release asset: {name}")
        line = checksum_line(path)
        if finalize:
            sidecar = output / (name + ".sha256")
            if sidecar.is_symlink() or sidecar.read_bytes() != line:
                raise ValueError(f"Checksum verification failed: {name}")
        lines.append(line)
    expected = b"".join(lines)
    if finalize:
        manifest.write_bytes(expected)
        for name in assets:
            (output / (name + ".sha256")).unlink()
    elif manifest.read_bytes() != expected:
        raise ValueError("SHA256SUMS.txt does not match the complete release")
    print(f"Verified all {len(assets)} release assets and SHA-256 checksums")


def release_notes(version, changes):
    """Changelog section from git-cliff, then downloads and verification instructions."""
    if version != workspace_version():
        raise ValueError("Release notes version must match the workspace manifest")
    changes = changes.strip()
    if not changes:
        raise ValueError("The generated changelog section is empty; use conventional commits")
    rows = ["| Platform | Desktop | Headless (`relay`, `relayd`) |", "|---|---|---|"]
    assets = expected_assets(version)
    for suffix in TARGETS.values():
        desktop = ", ".join(f"`{n}`" for n in sorted(assets) if n.startswith(f"relay-{version}-{suffix}"))
        cli = ", ".join(f"`{n}`" for n in sorted(assets) if n.startswith(f"relay-cli-{version}-{suffix}"))
        rows.append(f"| {suffix} | {desktop} | {cli} |")
    apk = f"relay-{version}-{ANDROID_SUFFIX}.apk"
    return "\n".join([
        changes, "", "## Downloads", "", *rows, "",
        f"Android: `{apk}` for 64-bit ARM phones (arm64-v8a, nearly every phone sold since about "
        "2016), installable by sideloading. It is signed with the Relay release key (its SHA-256 "
        "fingerprint is in docs/releasing.md), so later releases install over it. Other ABIs "
        "(32-bit ARM, x86) are not shipped.", "",
        "These builds are not code-signed or notarized: macOS apps are ad-hoc signed (on first "
        "launch macOS blocks them; allow Relay under System Settings > Privacy & Security > Open "
        "Anyway, or right-click, Open before macOS 15) and Windows installers are unsigned "
        "(SmartScreen warns).", "",
        "Verify downloads with `sha256sum --check --ignore-missing SHA256SUMS.txt` "
        "(`shasum -a 256 -c` on macOS).", "",
        f"[Commit history]({REPOSITORY}/commits/v{version})", ""])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("validate").add_argument("--tag", default="")
    notes = commands.add_parser("notes")
    notes.add_argument("--version", required=True)
    notes.add_argument("--changes", type=Path, required=True)
    notes.add_argument("--output", type=Path, required=True)
    archive = commands.add_parser("package")
    archive.add_argument("--version", required=True)
    archive.add_argument("--target", choices=TARGETS, required=True)
    archive.add_argument("--bin-dir", type=Path, required=True)
    archive.add_argument("--output", type=Path, required=True)
    for command in ("finalize", "verify"):
        verify = commands.add_parser(command)
        verify.add_argument("--version", required=True)
        verify.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "validate":
            validate(args.tag)
        elif args.command == "notes":
            args.output.write_text(release_notes(args.version, args.changes.read_text("utf-8")),
                                   encoding="utf-8")
        elif args.command in ("verify", "finalize"):
            verify_assets(args.version, args.output, args.command == "finalize")
        else:
            package(args.version, args.target, args.bin_dir, args.output)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Release packaging failed: {error}\n")


if __name__ == "__main__":
    main()
