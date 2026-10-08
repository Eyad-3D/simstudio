"""Where FMU files live on this computer, and which ones may run.

An FMU block names its file by the SHA-256 of the file's bytes
(``fmu_sha256``, its identity); ``fmu_path`` and ``fmu_name`` are only shown
to the user. ``locate`` is the one place that turns a block's parameters
into a file: the project attachment store (STD-02, the "files" lane)
replaces it so that the FMU travels inside the project. Until then an
imported FMU is copied into the user's LightSim folder
(``<projects>/fmus/<sha256>.fmu``) and a project opened on another computer
has to import it again.

Only that folder is ever read. A path written in a project is never opened,
or even looked at: the project may come from someone else, and on Windows
and macOS just checking whether a network path such as
``\\\\host\\share\\x.fmu`` or ``/net/host/x.fmu`` exists makes the computer
connect to that host (on Windows, sending the user's sign-in). A file
outside the folder gets in only when the user picks it in the app, which
uploads its bytes (``store_bytes``).

Trust. An FMU is native code from a third party, so it runs only once the
user has allowed it on this computer. The allowed list is a JSON file in the
user's own LightSim folder (``app.paths.data_dir``), keyed by SHA-256: the app asks when a file is imported,
and an FMU that arrives in someone else's project is refused by Data Checks
until the user allows it. Editing the file inside the
FMU changes its hash, so a changed FMU has to be allowed again.

Unpacking. A run loads an FMU from an unpacked copy in ``fmus/unpacked``
(the user's own folder, not the shared temp folder), made once per file (by
hash) and reused. It is unpacked from a private copy of the kept file whose
bytes are hashed as they are copied, so what runs is exactly the file that
was allowed. Unpacking checks every name (no absolute paths, drive letters
or ``..``) and the total size, so a hostile archive cannot write outside its
folder or fill the disk, and a damaged archive fails with a plain message.
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
import zlib
from pathlib import Path, PurePosixPath, PureWindowsPath

from ..paths import data_dir
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
    # with the user's other approvals, never in a projects folder that a
    # policy may put on a shared drive (app.paths.data_dir)
    return data_dir() / "fmu-allowed.json"


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
    """The kept copy of the FMU an FMU block's parameters name, found by its
    fingerprint (``fmu_sha256``) in the user's own FMU folder. THE seam for
    the project attachment store (STD-02): replace this to read the FMU from
    inside the project instead.

    ``fmu_path`` is never opened: see the module's docstring."""
    name = str(params.get("fmu_name") or "the FMU")
    raw = str(params.get("fmu_path") or "").strip()
    sha = str(params.get("fmu_sha256") or "").strip().lower()
    if not raw and not sha:
        raise FmuFileError("No FMU file chosen yet.")
    path = stored_path(sha) if SHA256.fullmatch(sha) else None
    if path is None or not path.is_file():
        raise FmuFileError(f"{name} was not found on this computer. Import the FMU again on "
                           f"this computer (FMU files are not yet saved inside the project).")
    if sha256_of_cached(path) != sha:
        raise FmuFileError(
            f"LightSim's copy of {name} is damaged (its contents changed after it was "
            f"imported). Import the FMU again.")
    return path


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
        target = _trust_file()
        target.parent.mkdir(parents=True, exist_ok=True)
        tmp = target.with_name(f".{target.name}.part")
        tmp.write_text(json.dumps(allowed, indent=1, sort_keys=True), "utf-8")
        os.replace(tmp, target)


# ---- unpacking -----------------------------------------------------------------

def _unpack_root() -> Path:
    # in the user's own folder, not the shared temp folder, where another
    # account on the computer could plant files for LightSim to load
    return fmu_dir() / "unpacked"


def unpacked(sha: str) -> Path:
    """An unpacked copy of the kept FMU with this fingerprint (made once,
    then reused). It is unpacked from a private copy whose bytes are hashed
    as they are copied: an FMU whose kept file no longer matches its
    fingerprint is refused, so what runs is always the file that was
    allowed (no check-then-read gap)."""
    src = stored_path(sha)
    target = _unpack_root() / sha
    done = target / ".lightsim-unpacked"
    if done.is_file():
        return target
    with _lock:
        if done.is_file():
            return target
        root = _ensure(_unpack_root())
        tmp = Path(tempfile.mkdtemp(prefix=f".{sha[:12]}-", dir=root))
        fd, copy_name = tempfile.mkstemp(prefix=f".{sha[:12]}-", suffix=".fmu", dir=root)
        copy = Path(copy_name)
        try:
            with os.fdopen(fd, "wb") as out:
                if _copy_hashing(src, out) != sha:
                    raise FmuFileError(
                        "LightSim's copy of the FMU does not match the FMU that was allowed "
                        "(it changed after it was imported). Import the FMU again.")
            _safe_extract(copy, tmp)
            (tmp / ".lightsim-unpacked").write_text(sha, "utf-8")
            if target.exists():
                shutil.rmtree(target, ignore_errors=True)
            os.replace(tmp, target)
        except BaseException:
            shutil.rmtree(tmp, ignore_errors=True)
            raise
        finally:
            copy.unlink(missing_ok=True)
    return target


def _copy_hashing(src: Path, out) -> str:
    """Copy ``src`` into the open file ``out``; the SHA-256 of what was copied."""
    h = hashlib.sha256()
    try:
        with open(src, "rb") as f:
            for chunk in iter(lambda: f.read(1 << 20), b""):
                h.update(chunk)
                out.write(chunk)
    except FileNotFoundError:
        raise FmuFileError("The FMU was not found on this computer. Import the FMU "
                           "again.") from None
    except OSError as e:
        raise FmuFileError(f"The FMU could not be copied to unpack it ({e.strerror or e}).") from None
    return h.hexdigest()


def _ensure(p: Path) -> Path:
    p.mkdir(parents=True, exist_ok=True)
    return p


def _unsafe(name: str) -> bool:
    """Whether a name in the archive could land outside the unpack folder, on
    any operating system: absolute, with ``..``, or with a drive letter or
    other ``:`` anywhere (Windows reads ``binaries/D:/x.dll`` as ``D:x.dll``,
    a file on drive D:)."""
    parts = PurePosixPath(name).parts
    if name.startswith("/") or not parts:
        return True
    for part in parts:
        win = PureWindowsPath(part)
        if part in ("..", ".") or ":" in part or win.drive or win.root:
            return True
    return False


def _safe_extract(path: Path, target: Path) -> None:
    """Unpack the FMU at ``path`` into ``target``. Raises FmuFileError, in
    plain words, for an unsafe name, a too-large or damaged archive, or a
    file that cannot be written."""
    try:
        zf = zipfile.ZipFile(path)
    except (OSError, zipfile.BadZipFile) as e:
        raise FmuFileError(f"The FMU file cannot be opened as a zip file ({e}).") from None
    with zf:
        total = 0
        for info in zf.infolist():
            name = info.filename.replace("\\", "/")
            if _unsafe(name.rstrip("/")):
                raise FmuFileError(f"The FMU contains a file with an unsafe name ({info.filename}).")
            total += info.file_size
            if total > MAX_UNPACKED_BYTES:
                raise FmuFileError(
                    f"The FMU unpacks to more than {MAX_UNPACKED_BYTES >> 30} GB.")
        root = os.path.realpath(target)
        try:
            for info in zf.infolist():
                name = info.filename.replace("\\", "/")
                dest = target.joinpath(*PurePosixPath(name).parts)
                if not os.path.realpath(dest).startswith(root + os.sep):
                    raise FmuFileError(
                        f"The FMU contains a file with an unsafe name ({info.filename}).")
                if name.endswith("/"):
                    dest.mkdir(parents=True, exist_ok=True)
                    continue
                dest.parent.mkdir(parents=True, exist_ok=True)
                with zf.open(info) as src, open(dest, "wb") as out:
                    shutil.copyfileobj(src, out, 1 << 20)
        except FmuFileError:
            raise
        except (zipfile.BadZipFile, zlib.error, NotImplementedError, EOFError) as e:
            raise FmuFileError(f"The FMU file is damaged and cannot be unpacked ({e}). Ask "
                               f"for a new copy and import it again.") from None
        except OSError as e:
            raise FmuFileError(f"The FMU could not be unpacked into {target.parent} "
                               f"({e.strerror or e}).") from None
