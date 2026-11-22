#!/usr/bin/env python3
"""Render a login entry for this checkout; never write system files."""
from pathlib import Path
import sys

root = Path(sys.argv[1]).resolve()
if any(ord(c) < 32 for c in str(root)) or any(c in str(root) for c in "=%"):
    raise SystemExit("The checkout path cannot contain control characters, '=' or '%'.")

def desktop_string(value):
    return value.replace("\\", "\\\\")

def executable(value):
    # Desktop string unescaping precedes Exec argument unquoting.
    quoted = "".join("\\" + c if c in '\\"`$' else c for c in value)
    return '"' + desktop_string(quoted.replace("%", "%%")) + '"'

template = (root / "packaging/emberwm.desktop").read_text()
print(template.replace("@SESSION_EXEC@", executable(str(root / "scripts/start-session.sh")))
      .replace("@BINARY_PATH@", desktop_string(str(root / "target/release/emberwm"))), end="")
