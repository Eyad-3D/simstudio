"""Small file helpers shared by the stores: safe ids and all-or-nothing writes."""
from __future__ import annotations

import contextlib
import os
import re
import secrets
import time
from pathlib import Path

#: A project or run id: it names a file or folder, so it is a plain name of
#: at most 128 characters that does not start or end with a dot (that rules
#: out "." and "..", and names Windows would shorten).
SAFE_ID = re.compile(r"[A-Za-z0-9_-](?:[A-Za-z0-9._-]{0,126}[A-Za-z0-9_-])?")


def safe_id(value: str, what: str) -> str:
    if not isinstance(value, str) or not SAFE_ID.fullmatch(value):
        raise ValueError(f"Invalid {what} id: {value!r}")
    return value


def replace(src: str, dst: Path) -> None:
    # Windows refuses the rename while another process (a virus scanner, a
    # sync client) briefly holds the target open; give it a moment.
    for attempt in range(5):
        try:
            os.replace(src, dst)
            return
        except PermissionError:
            if attempt == 4:
                raise
            time.sleep(0.05 * (attempt + 1))


def write_atomic(path: Path, data: bytes) -> None:
    """Write `path` so that it holds either its old or its new bytes, never a
    mix: write a temp file beside it, flush it to disk, then rename it over."""
    tmp = path.with_name(f".{path.name}.{secrets.token_hex(4)}.tmp")
    try:
        with open(tmp, "xb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        replace(str(tmp), path)
    except BaseException:
        with contextlib.suppress(OSError):
            tmp.unlink()
        raise
