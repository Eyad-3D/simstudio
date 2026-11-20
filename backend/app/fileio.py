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


def _retried(action):
    """`action()`, tried again for a moment when Windows refuses it: a file
    another process briefly holds open, or is renaming a new version over,
    is refused with a PermissionError there."""
    for attempt in range(5):
        try:
            return action()
        except PermissionError:
            if attempt == 4:
                raise
            time.sleep(0.05 * (attempt + 1))


def replace(src: str, dst: Path) -> None:
    # Windows refuses the rename while another process (a virus scanner, a
    # sync client, a reader) briefly holds the target open; give it a moment.
    _retried(lambda: os.replace(src, dst))


def read_bytes(path: Path) -> bytes:
    """The file's bytes. Every store here writes by renaming a new file over
    the old one (:func:`write_atomic`); on Windows a read in that instant is
    refused, so it is tried again, as the rename is."""
    return _retried(path.read_bytes)


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
