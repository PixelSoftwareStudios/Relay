"""Licence and example files that ship with every package."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DOCS = "share/doc/relay"
NOTICES = "THIRD_PARTY_LICENSES"


def sources(desktop):
    """Return {file name: source path}. The fonts only ship with the window."""
    files = {"LICENSE": ROOT / "LICENSE", "README.md": ROOT / "README.md",
             NOTICES: ROOT / NOTICES}
    if desktop:
        for font in sorted((ROOT / "assets/font").glob("OFL-*.txt")):
            files[font.name] = font
        files["LICENSE-phosphor.txt"] = ROOT / "assets/font/LICENSE-phosphor.txt"
    for name, path in files.items():
        if not path.is_file() or path.is_symlink() or path.stat().st_size == 0:
            hint = (" (generate it: cargo about generate about.hbs > THIRD_PARTY_LICENSES)"
                    if name == NOTICES else "")
            raise ValueError(f"Missing licence file {name}{hint}")
    return files


def examples(target):
    """Service units are documentation, not installed: their paths need adjusting."""
    if "linux" in target:
        return {"relayd.service": ROOT / "contrib/systemd/relayd.service"}
    if "apple" in target:
        return {"org.relay.relayd.plist": ROOT / "contrib/launchd/org.relay.relayd.plist"}
    return {}


def documents(desktop, target):
    """Archive-relative documentation paths for a portable archive."""
    files = {f"{DOCS}/{name}": path for name, path in sources(desktop).items()}
    files.update({f"{DOCS}/examples/{name}": path for name, path in examples(target).items()})
    return files
