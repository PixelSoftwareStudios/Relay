#!/usr/bin/env python3
"""Build the Relay UI font subsets. See README.md in this directory.

usage: subset.py SRC_DIR UNIHAN_OTHERMAPPINGS OUT_DIR
SRC_DIR holds the unpacked upstream releases (layout in README.md).
"""
import codecs
import subprocess
import sys
import tempfile
from pathlib import Path

src, unihan, out = (Path(a) for a in sys.argv[1:4])
here = Path(__file__).resolve().parent
ui_chars = (here / "ui-chars.txt").read_text(encoding="utf-8").replace("\n", "")


def ranges(*spans):
	return {c for lo, hi in spans for c in range(lo, hi + 1)}


def legacy(codec, rows):
	"""Code points of the double-byte rows (first byte values) of a legacy set."""
	found = set()
	for lead in rows:
		for trail in range(0xA1, 0xFF):
			try:
				found.add(ord(bytes([lead, trail]).decode(codec)))
			except UnicodeDecodeError:
				pass
	return found


def joyo():
	chars = set()
	for line in unihan.read_text(encoding="utf-8").splitlines():
		parts = line.split("\t")
		if len(parts) == 3 and parts[1] == "kJoyoKanji":
			chars.add(int(parts[0][2:], 16))
	return chars


latin = ranges(
	(0x20, 0x7E), (0xA0, 0x2FF), (0x300, 0x36F), (0x370, 0x3FF), (0x400, 0x52F),
	(0x1E00, 0x1EFF), (0x1F00, 0x1FFF), (0x2000, 0x206F), (0x20A0, 0x20CF),
	(0x2100, 0x214F), (0x2190, 0x21FF), (0x2C60, 0x2C7F), (0xA720, 0xA7FF),
)
devanagari = ranges((0x20, 0x7F), (0xA0, 0xFF), (0x900, 0x97F), (0x200C, 0x200D),
	(0x2010, 0x2015), (0x2018, 0x201D), (0x2022, 0x2026), (0x20B9, 0x20B9), (0x25CC, 0x25CC),
	(0xA830, 0xA839), (0xA8E0, 0xA8FF))
cjk_punct = ranges((0x3000, 0x303F), (0xFF00, 0xFFEF))
extra = {ord(c) for c in ui_chars}

# JP: JIS X 0208 rows 1-8 (symbols, kana, Greek, Cyrillic, box drawing) and
# level 1 kanji (rows 16-47), plus the 2010 Joyo list, kana blocks, punctuation.
jp = (legacy("euc_jp", range(0xA1, 0xA9)) | legacy("euc_jp", range(0xB0, 0xD0))
	| joyo() | ranges((0x3040, 0x30FF), (0x31F0, 0x31FF)) | cjk_punct)
# KR: KS X 1001 symbols (rows 1-12) and the 2350 Hangul syllables (rows 16-40),
# plus Hangul Jamo and compatibility Jamo. No Hanja (they fall back to JP/SC).
kr = (legacy("euc_kr", range(0xA1, 0xAD)) | legacy("euc_kr", range(0xB0, 0xC9))
	| ranges((0x1100, 0x11FF), (0x3130, 0x318F)) | cjk_punct)
# SC: all of GB2312 (symbols, kana, 6763 hanzi).
sc = legacy("gb2312", range(0xA1, 0xF8)) | cjk_punct

jobs = [
	("NotoSans-Regular.ttf", src / "lgc/NotoSans/unhinted/ttf/NotoSans-Regular.ttf",
		latin | extra, "kern,ccmp,locl,mark,mkmk"),
	("NotoSansJP-Regular.otf", src / "jp/NotoSansJP-Regular.otf", jp | extra, ""),
	("NotoSansKR-Regular.otf", src / "kr/NotoSansKR-Regular.otf", kr | extra, ""),
	("NotoSansSC-Regular.otf", src / "sc/NotoSansSC-Regular.otf", sc | extra, ""),
	("NotoSansDevanagari-Regular.ttf",
		src / "dev/NotoSansDevanagari/unhinted/ttf/NotoSansDevanagari-Regular.ttf",
		devanagari, "*"),
]
out.mkdir(parents=True, exist_ok=True)
for name, font, chars, features in jobs:
	with tempfile.NamedTemporaryFile("w", suffix=".txt", encoding="utf-8") as unicodes:
		unicodes.write(",".join(f"U+{c:04X}" for c in sorted(chars)))
		unicodes.flush()
		subprocess.run(
			["pyftsubset", str(font), f"--unicodes-file={unicodes.name}",
				f"--output-file={out / name}", f"--layout-features={features}",
				"--no-hinting", "--drop-tables+=DSIG",
				"--notdef-outline", "--name-IDs=*", "--glyph-names=false"],
			check=True)
	print(name, (out / name).stat().st_size)
