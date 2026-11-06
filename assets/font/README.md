# Fonts

The icon font is described in the last section. The rest of this file is the Noto text fonts.

Noto Sans subsets for the egui UI (`crates/relay-ui/src/fonts.rs`, embedded with `include_bytes!`).
Regular weight only: egui does not synthesise bold or italic, and the UI uses one face per size.

| File | Source | Covers | Size |
|---|---|---|---|
| `NotoSans-Regular.ttf` | Noto Sans v2.015 | Latin, Latin Extended (incl. Vietnamese), Greek, Cyrillic, general punctuation, currency, arrows | 260,928 B |
| `NotoSansJP-Regular.otf` | Noto Sans JP, Noto CJK Sans2.004 | JIS X 0208 symbols/kana/rows 1-8, JIS level 1 kanji (2965), Joyo kanji (2010 list), kana blocks, CJK punctuation, full-width forms | 734,508 B |
| `NotoSansKR-Regular.otf` | Noto Sans KR, Noto CJK Sans2.004 | KS X 1001 Hangul syllables (2350) and symbols, Hangul Jamo and compatibility Jamo, CJK punctuation | 374,808 B |
| `NotoSansSC-Regular.otf` | Noto Sans SC, Noto CJK Sans2.004 | GB2312 (6763 hanzi, symbols, kana), CJK punctuation | 1,559,876 B |
| `NotoSansDevanagari-Regular.ttf` | Noto Sans Devanagari v2.007 | Devanagari block, Vedic extensions subset, danda, rupee sign, Latin digits and punctuation | 170,532 B |

Total 3,100,652 B. Every subset also keeps the characters in `ui-chars.txt` (non-ASCII characters used in UI strings: `…`, `·`, `•`, `é`). When a new string adds a non-ASCII character outside these sets, append it to `ui-chars.txt` and rebuild.

## Licence

All fonts are SIL Open Font License 1.1 (verified in each upstream repository and release archive):

- `OFL-noto-sans.txt`: Noto Sans, Copyright 2022 The Noto Project Authors (https://github.com/notofonts/latin-greek-cyrillic).
- `OFL-noto-devanagari.txt`: Noto Sans Devanagari, Copyright 2022 The Noto Project Authors (https://github.com/notofonts/devanagari).
- `OFL-noto-cjk.txt`: Noto Sans CJK (JP, KR, SC), Copyright 2014-2021 Adobe (http://www.adobe.com/); the same text ships in the JP, KR and SC archives.

The OFL allows bundling, subsetting and redistribution with software if the copyright notice and licence text travel with the fonts and the fonts are not sold alone. The subsets keep the upstream `name` table (copyright, licence and URL entries) unchanged. No Reserved Font Name is declared by the Noto Project; the CJK files declare none that applies to the family names kept here.

Attribution in releases: `cargo about` only covers crates, so release packaging must copy the three `OFL-*.txt` files next to `THIRD_PARTY_LICENSES` in the shipped bundle (see `docs/release.md`). The generated `THIRD_PARTY_LICENSES` is not committed.

## Sources

Official releases from the `notofonts` GitHub organisation (the Google Fonts / Noto upstream):

- https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015 (`NotoSans-v2.015.zip`, published 2024-11-20); used `NotoSans/unhinted/ttf/NotoSans-Regular.ttf`.
- https://github.com/notofonts/devanagari/releases/tag/NotoSansDevanagari-v2.007 (`NotoSansDevanagari-v2.007.zip`); used `NotoSansDevanagari/unhinted/ttf/NotoSansDevanagari-Regular.ttf`.
- https://github.com/notofonts/noto-cjk/releases/tag/Sans2.004 (`16_NotoSansJP.zip`, `17_NotoSansKR.zip`, `18_NotoSansSC.zip`, published 2022-01-27); used `NotoSansJP-Regular.otf`, `NotoSansKR-Regular.otf`, `NotoSansSC-Regular.otf`.
- Joyo list: the `kJoyoKanji` field of `Unihan_OtherMappings.txt` from https://www.unicode.org/Public/16.0.0/ucd/Unihan.zip. JIS, GB2312 and KS X 1001 sets come from Python's `euc_jp`, `gb2312` and `euc_kr` codecs.

## Reproduce

Unpack the archives so that `SRC` contains `lgc/` (Noto Sans zip), `dev/` (Devanagari zip), `jp/`, `kr/`, `sc/` (the three CJK zips), then:

```sh
python3 -m venv venv && venv/bin/pip install fonttools brotli   # fonttools 4.68.0
PATH=$PWD/venv/bin:$PATH python3 -I assets/font/subset.py SRC SRC/../unihan/Unihan_OtherMappings.txt assets/font
```

`subset.py` builds each code point set (ranges, legacy codecs, Joyo, `ui-chars.txt`) and runs
`pyftsubset FONT --unicodes-file=SET --output-file=OUT --layout-features=FEATURES --no-hinting --drop-tables+=DSIG --notdef-outline --name-IDs=* --glyph-names=false`.
Layout features: `kern,ccmp,locl,mark,mkmk` for Noto Sans, all for Devanagari (egui 0.37 shapes with harfrust, so the GSUB/GPOS tables are used), none for CJK. Hinting is dropped because egui rasterises unhinted outlines. The CJK faces stay OpenType/CFF, which egui's font loader (skrifa) reads directly.

## Limits

- Devanagari is shaped by egui's own harfrust layout (conjuncts, matras, reph; tests in `crates/relay-ui/tests/shaping_*.rs`). Not yet confirmed on screen by a person, which is why Hindi keeps its experimental label.
- egui has no per-locale font choice: Han characters shared by Japanese and Chinese resolve to the first face that has them (JP, then KR, then SC), so Chinese text may show Japanese glyph forms for shared characters. Characters outside GB2312, JIS level 1/Joyo and KS X 1001 show as missing glyphs.

## Icons

`Phosphor-Icons.ttf` (regular, 16,752 B) and `Phosphor-Icons-Fill.ttf` (fill, 1,344 B) are subsets of Phosphor Icons (https://phosphoricons.com, MIT, Copyright 2020 Phosphor Icons; text in `LICENSE-phosphor.txt`). Each has its own egui family (`icons`, `icons-fill`, `crates/relay-ui/src/fonts.rs`), so icons never fall back through text fonts. `icon-glyphs.txt` lists the kept glyphs (name, codepoint, weight); the codepoints used by the UI are in `crates/relay-ui/src/icons.rs` and `crates/relay-ui/tests/icons.rs` checks each one exists. When an icon is added, add its line to `icon-glyphs.txt`, rebuild, and add the variant to `Icon`.

Source: `res/Phosphor.ttf` and `res/Phosphor-Fill.ttf` of the egui-phosphor 0.14.0 crate (unmodified upstream fonts), or the same files from `@phosphor-icons/web`.

```sh
PATH=$PWD/venv/bin:$PATH python3 -I assets/font/subset-icons.py PHOSPHOR_RES_DIR assets/font
```
