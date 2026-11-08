#!/usr/bin/env python3
"""Build DEB, RPM and AppImage packages on each native Linux architecture."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

import licenses
import package_release as release

APP_ID = "dev.relay.Relay"
ARCHITECTURES = ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu")
FORMATS = ("deb", "rpm", "appimage")
ICON_SIZES = (512, 256, 128, 64, 48, 32)
RPM_REQUIRES = ["alsa-lib", "libxkbcommon", "libwayland-client", "libX11",
                "mesa-libEGL", "mesa-libGL", "xdg-desktop-portal"]


def native_version(version):
    """deb and rpm sort `1.0~rc.1` before `1.0`; a `-` would start the revision and sort after."""
    return version.replace("-", "~", 1)


def package_files(target):
    """{source: destination below the package root} for licences and examples."""
    return {str(source): "usr/" + relative
            for relative, source in licenses.documents(True, target).items()}


def configuration(version, target, binaries, output, format_name):
    config = json.loads((release.ROOT / "packaging/cargo-packager.json").read_text())
    ordered = [binaries["relay-desktop"], binaries["relay"], binaries["relayd"]]
    config.update(version=native_version(version), targetTriple=target, outDir=str(output.resolve()),
                  binaries=[{"path": str(path.resolve()), "main": path.name == "relay-desktop"}
                            for path in ordered],
                  formats=[format_name],
                  icons=[str(release.ROOT / f"assets/icon/relay-mark-{size}.png") for size in ICON_SIZES])
    config["deb"].update(desktopTemplate=str(release.ROOT / "packaging/relay.desktop"),
                         files=package_files(target))
    return config


def appimage_configuration(config, target, temporary):
    """Keep the dotted desktop ID intact through cargo-packager's file stem."""
    launcher = temporary / f"{APP_ID}.sh"
    shutil.copy2(release.ROOT / "packaging/appimage-launcher.sh", launcher)
    launcher.chmod(0o755)
    others = [b for b in config["binaries"]]
    for binary in others:
        binary["main"] = False
    config["binaries"] = [{"path": str(launcher), "main": True}, *others]
    desktop = temporary / f"{APP_ID}.desktop"
    desktop.write_text((release.ROOT / "packaging/relay.desktop").read_text()
                       .replace("Exec=relay-desktop", f"Exec={launcher.name}")
                       .replace("Icon=relay-desktop", f"Icon={APP_ID}"), encoding="utf-8")
    config["deb"]["desktopTemplate"] = str(desktop)
    config.setdefault("appimage", {})["files"] = package_files(target)
    return config


def native_package(version, target, binaries, archive, temporary, format_name):
    output = temporary / "output"
    output.mkdir()
    config = configuration(version, target, binaries, output, format_name)
    env = dict(os.environ)
    if format_name == "appimage":
        config = appimage_configuration(config, target, temporary)
        env["APPIMAGE_EXTRACT_AND_RUN"] = "1"
    subprocess.run(["cargo", "packager", "--config", json.dumps(config)],
                   cwd=release.ROOT, env=env, check=True)
    extension = "AppImage" if format_name == "appimage" else "deb"
    artifacts = list(output.glob(f"*.{extension}"))
    if len(artifacts) != 1:
        raise ValueError(f"Expected exactly one {extension}, found {len(artifacts)}")
    shutil.copy2(artifacts[0], archive)
    if format_name == "appimage":
        archive.chmod(0o755)


def rpm_assets(target, binaries):
    assets = [{"source": str(binaries[name].resolve()), "dest": f"/usr/bin/{name}", "mode": "755"}
              for name in release.DESKTOP_PROGRAMS]
    assets += [{"source": str(release.ROOT / "packaging/relay.desktop"),
                "dest": "/usr/share/applications/relay-desktop.desktop", "mode": "644"}]
    assets += [{"source": str(release.ROOT / f"assets/icon/relay-mark-{size}.png"),
                "dest": f"/usr/share/icons/hicolor/{size}x{size}/apps/relay-desktop.png",
                "mode": "644"} for size in ICON_SIZES]
    assets += [{"source": source, "dest": "/" + dest, "mode": "644"}
               for source, dest in package_files(target).items()]
    return assets


def rpm_metadata(version, target, binaries):
    """TOML for cargo-generate-rpm's --set-metadata: every file and runtime requirement."""
    entries = ["{ " + ", ".join(f"{key} = {json.dumps(value)}" for key, value in entry.items()) + " }"
               for entry in rpm_assets(target, binaries)]
    requires = ", ".join(f"{json.dumps(name)} = \"*\"" for name in RPM_REQUIRES)
    return (f"version = {json.dumps(native_version(version))}\nrelease = \"1\"\n"
            f"assets = [{', '.join(entries)}]\nrequires = {{ {requires} }}\n")


def rpm(version, target, binaries, archive):
    subprocess.run(["cargo", "generate-rpm", "-p", "apps/relay-desktop", "--target", target,
                    "--output", str(archive), "--set-metadata", rpm_metadata(version, target, binaries)],
                   cwd=release.ROOT, check=True)


def package(version, target, bin_dir, output, formats):
    release.validate_package_input(version, target)
    if target not in ARCHITECTURES:
        raise ValueError(f"Unsupported Linux target: {target}")
    binaries = release.find_binaries(target, bin_dir, release.DESKTOP_PROGRAMS)
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for format_name in formats:
        if format_name not in FORMATS:
            raise ValueError(f"Unsupported Linux format: {format_name}")
        extension = "AppImage" if format_name == "appimage" else format_name
        archive = output / f"relay-{version}-{release.TARGETS[target]}.{extension}"
        if format_name == "rpm":
            rpm(version, target, binaries, archive)
        else:
            with tempfile.TemporaryDirectory(prefix="relay-linux-") as directory:
                native_package(version, target, binaries, archive, Path(directory), format_name)
        release.write_checksum(archive)
        print(archive)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", choices=ARCHITECTURES, required=True)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--formats", nargs="+", choices=FORMATS, default=list(FORMATS))
    args = parser.parse_args()
    try:
        package(args.version, args.target, args.bin_dir, args.output, args.formats)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Linux packaging failed: {error}\n")


if __name__ == "__main__":
    main()
