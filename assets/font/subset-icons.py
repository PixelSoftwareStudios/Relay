#!/usr/bin/env python3
"""Build the Relay icon font subsets from Phosphor. See README.md in this directory.

usage: subset-icons.py PHOSPHOR_RES_DIR OUT_DIR
PHOSPHOR_RES_DIR holds Phosphor.ttf (regular) and Phosphor-Fill.ttf, as shipped in
the egui-phosphor 0.14.0 crate (`res/`) or the @phosphor-icons/web release.
"""
import subprocess
import sys
import tempfile
from pathlib import Path

src, out = (Path(a) for a in sys.argv[1:3])
here = Path(__file__).resolve().parent
sets = {"regular": set(), "fill": set()}
for line in (here / "icon-glyphs.txt").read_text(encoding="utf-8").splitlines():
	if line and not line.startswith("#"):
		_, code, style = line.split()
		sets[style].add(code)
jobs = [("regular", "Phosphor.ttf", "Phosphor-Icons.ttf"), ("fill", "Phosphor-Fill.ttf", "Phosphor-Icons-Fill.ttf")]
out.mkdir(parents=True, exist_ok=True)
for style, font, name in jobs:
	with tempfile.NamedTemporaryFile("w", suffix=".txt", encoding="utf-8") as unicodes:
		unicodes.write(",".join(sorted(sets[style])))
		unicodes.flush()
		subprocess.run(
			["pyftsubset", str(src / font), f"--unicodes-file={unicodes.name}",
				f"--output-file={out / name}", "--layout-features=", "--no-hinting",
				"--drop-tables+=DSIG", "--notdef-outline", "--name-IDs=*", "--glyph-names=false"],
			check=True)
	print(name, (out / name).stat().st_size)
