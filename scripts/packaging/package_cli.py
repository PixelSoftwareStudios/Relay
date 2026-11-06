#!/usr/bin/env python3
"""Package the headless pair (relay, relayd) separately from the desktop installers."""
import argparse
from pathlib import Path

import package_release as release


def package(version, target, bin_dir, output):
    release.validate_package_input(version, target)
    binaries = release.find_binaries(target, bin_dir, release.CLI_PROGRAMS)
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    windows = "windows" in target
    name = f"relay-cli-{version}-{release.TARGETS[target]}"
    archive = output / (name + (".zip" if windows else ".tar.gz"))
    release.write_archive(archive, name, release.payload(binaries, False, target), windows,
                          {release.executable_name(p, target) for p in release.CLI_PROGRAMS})
    return archive


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", choices=release.TARGETS, required=True)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        print(package(args.version, args.target, args.bin_dir, args.output))
    except (OSError, ValueError) as error:
        parser.exit(1, f"CLI packaging failed: {error}\n")
