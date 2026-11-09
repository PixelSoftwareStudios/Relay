"""A stand-in for minisign: -S writes a signature file, -V checks it. No real cryptography."""
import hashlib
from pathlib import Path
import sys

args = sys.argv[1:]
mode = args[0]
options = dict(zip(args[1::2], args[2::2]))
message = Path(options["-m"]).read_bytes()
tag = hashlib.sha256(message).hexdigest()
if mode == "-S":
    key = Path(options["-s"])
    if oct(key.stat().st_mode & 0o777) != "0o600" or sys.stdin.readline().strip() != "hunter2":
        sys.exit(1)
    Path(options["-x"]).write_text(
        f"untrusted comment: fake\nSIG{tag}\ntrusted comment: {options['-t']}\nGLOBAL\n")
else:
    signature = Path(options["-x"]).read_text().splitlines()
    sys.exit(0 if signature[1] == f"SIG{tag}" else 1)
