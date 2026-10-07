"""Where FMU files live on this computer, and which ones may run.

An FMU block references its file by **path** (``fmu_path``) and by the
SHA-256 of the file's bytes (``fmu_sha256``, its identity). ``locate`` is the
one place that turns a block's parameters into a file: the project
attachment store (STD-02, the "files" lane) replaces it so that the FMU
travels inside the project. Until then an imported FMU is copied into the
user's LightSim folder (``<projects>/fmus/<sha256>.fmu``) and a project
opened on another computer has to import it again.

Trust. An FMU is native code from a third party, so it runs only once the
user has allowed it on this computer. The allowed list is a JSON file next to
the stored FMUs, keyed by SHA-256: importing a file counts as allowing it
(the user picked it), and an FMU that arrives in someone else's project is
refused by Data Checks until the user allows it. Editing the file inside the
FMU changes its hash, so a changed FMU has to be allowed again.

Unpacking. A run loads an FMU from an unpacked copy in ``fmus/unpacked``
(the user's own folder, not the shared temp folder), made once per file (by
hash) and reused. Unpacking checks every name (no absolute paths or ``..``)
and the total size, so a hostile archive cannot write outside its folder or
fill the disk.
"""
from __future__ import annotations

import hashlib
import io
import json
import os
import re
import shutil
import tempfile
import threading
import zipfile
from pathlib import Path, PurePosixPath

from ..storage import user_dir
from . import FmuFileError  # noqa: F401 — re-exported

SHA256 = re.compile(r"[0-9a-f]{64}")

#: The biggest FMU file LightSim imports, and the most its contents may
#: unpack to: supplier FMUs with tables run to tens of megabytes.
MAX_FMU_BYTES = 512 * 1024 * 1024
MAX_UNPACKED_BYTES = 2 * 1024 * 1024 * 1024

_lock = threading.Lock()


def fmu_dir() -> Path:
    """The folder imported FMUs are kept in (beside the user's projects)."""
    return user_dir() / "fmus"


def _trust_file() -> Path:
    return fmu_dir() / "allowed.json"


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def store_bytes(data: bytes) -> tuple[str, Path]:
    """Keep an imported FMU's bytes; returns (sha256, where it is kept)."""
    if len(data) > MAX_FMU_BYTES:
        raise FmuFileError(f"The FMU is larger than {MAX_FMU_BYTES >> 20} MB.")
    if not zipfile.is_zipfile(io.BytesIO(data)):
        raise FmuFileError("This is not an FMU: an FMU is a zip file, and this file is not one.")
    sha = hashlib.sha256(data).hexdigest()
    folder = fmu_dir()
    folder.mkdir(parents=True, exist_ok=True)
    target = folder / f"{sha}.fmu"
    if not target.is_file():
        tmp = folder / f".{sha}.part"
        tmp.write_bytes(data)
        os.replace(tmp, target)
    return sha, target


def stored_path(sha: str) -> Path:
    if not SHA256.fullmatch(sha or ""):
        raise FmuFileError("Unknown FMU.")
    return fmu_dir() / f"{sha}.fmu"


def locate(params: dict) -> Path:
    """The FMU file an FMU block's parameters point at, checked against its
    hash. THE seam for the project attachment store (STD-02): replace this
    to read the FMU from inside the project instead of from a path."""
    name = str(params.get("fmu_name") or "the FMU")
    raw = str(params.get("fmu_path") or "").strip()
    sha = str(params.get("fmu_sha256") or "").strip().lower()
    if not raw and not sha:
        raise FmuFileError("No FMU file chosen yet.")
    candidates = [Path(raw).expanduser()] if raw else []
    if SHA256.fullmatch(sha):
        candidates.append(stored_path(sha))  # imported on this computer
    for path in candidates:
        if path.is_file():
            if sha and sha256_of_cached(path) != sha:
                raise FmuFileError(
                    f"The file at {path} is not the {name} this block was set up with "
                    f"(its contents changed). Import the FMU again.")
            return path
    where = raw or "this computer"
    raise FmuFileError(f"{name} was not found at {where}. Import the FMU again on this "
                       f"computer (FMU files are not yet saved inside the project).")


_hash_cache: dict[tuple[str, int, int], str] = {}


def sha256_of_cached(path: Path) -> str:
    st = path.stat()
    key = (str(path.resolve()), st.st_size, st.st_mtime_ns)
    sha = _hash_cache.get(key)
    if sha is None:
        sha = _hash_cache[key] = sha256_of(path)
    return sha


# ---- trust ---------------------------------------------------------------------

def _read_allowed() -> dict:
    try:
        data = json.loads(_trust_file().read_text("utf-8"))
        return data if isinstance(data, dict) else {}
    except (OSError, ValueError):
        return {}


def is_allowed(sha: str) -> bool:
    return bool(SHA256.fullmatch(sha or "")) and sha in _read_allowed()


def allow(sha: str, name: str) -> None:
    """Remember that the user allowed this FMU to run on this computer."""
    if not SHA256.fullmatch(sha or ""):
        raise FmuFileError("Unknown FMU.")
    with _lock:
        allowed = _read_allowed()
        allowed[sha] = {"name": str(name)[:200]}
        folder = fmu_dir()
        folder.mkdir(parents=True, exist_ok=True)
        tmp = folder / ".allowed.json.part"
        tmp.write_text(json.dumps(allowed, indent=1, sort_keys=True), "utf-8")
        os.replace(tmp, _trust_file())


# ---- unpacking -----------------------------------------------------------------

def _unpack_root() -> Path:
    # in the user's own folder, not the shared temp folder, where another
    # account on the computer could plant files for LightSim to load
    return fmu_dir() / "unpacked"


def unpacked(path: Path, sha: str | None = None) -> Path:
    """An unpacked copy of the FMU (made once per file, then reused)."""
    sha = sha or sha256_of_cached(path)
    target = _unpack_root() / sha
    done = target / ".lightsim-unpacked"
    if done.is_file():
        return target
    with _lock:
        if done.is_file():
            return target
        tmp = Path(tempfile.mkdtemp(prefix=f".{sha[:12]}-", dir=_ensure(_unpack_root())))
        try:
            _safe_extract(path, tmp)
            (tmp / ".lightsim-unpacked").write_text(sha, "utf-8")
            if target.exists():
                shutil.rmtree(target, ignore_errors=True)
            os.replace(tmp, target)
        except BaseException:
            shutil.rmtree(tmp, ignore_errors=True)
            raise
    return target


def _ensure(p: Path) -> Path:
    p.mkdir(parents=True, exist_ok=True)
    return p


def _safe_extract(path: Path, target: Path) -> None:
    try:
        zf = zipfile.ZipFile(path)
    except (OSError, zipfile.BadZipFile) as e:
        raise FmuFileError(f"The FMU file cannot be opened as a zip file ({e}).") from None
    with zf:
        total = 0
        for info in zf.infolist():
            name = info.filename.replace("\\", "/")
            parts = PurePosixPath(name).parts
            if name.startswith("/") or ".." in parts or (parts and ":" in parts[0]):
                raise FmuFileError(f"The FMU contains a file with an unsafe name ({info.filename}).")
            total += info.file_size
            if total > MAX_UNPACKED_BYTES:
                raise FmuFileError(
                    f"The FMU unpacks to more than {MAX_UNPACKED_BYTES >> 30} GB.")
        for info in zf.infolist():
            name = info.filename.replace("\\", "/")
            dest = target.joinpath(*PurePosixPath(name).parts)
            if name.endswith("/"):
                dest.mkdir(parents=True, exist_ok=True)
                continue
            dest.parent.mkdir(parents=True, exist_ok=True)
            with zf.open(info) as src, open(dest, "wb") as out:
                shutil.copyfileobj(src, out, 1 << 20)
