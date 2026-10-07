"""Files kept with a project: other tools' models, AI models, measured data (STD-02).

Some models need files that are not JSON: a Functional Mock-up Unit (FMU,
another tool's model packed as a .fmu file), a neural network (.onnx) or a
table of measured data. Such a file is *attached* to the project: it is
copied into the project's resources folder (:func:`app.storage.location`)::

    car.lightsim
    car.lightsim-resources/motor.fmu        (beside a .lightsim file)
    projects/resources/<project id>/motor.fmu  (in the projects folder)

and the project lists it in ``attachments`` with its path relative to the
project (``resources/motor.fmu``, the layout of an SSP archive's
``resources/`` folder, STD-08), its size and its SHA-256 hash (a fingerprint
of its bytes) from when it was attached. A changed or missing file is
caught by :func:`check`. A parameter of type ``file`` holds such a path.

Sent as one file, a project is a zip "bundle" (:func:`export_bundle`):
``project.json`` and a ``resources/`` folder; :func:`import_bundle` unpacks
one. Saving a project as a new file copies its attachments along
(:func:`copy_all`).

API for code that uses attached files (the FMU and ONNX blocks)::

    from app import attachments
    info = attachments.add(project_id, "motor.fmu", data)  # → {"path": "resources/motor.fmu", ...}
    attachments.list_files(project_id)                     # what is in the folder
    attachments.read(project_id, "resources/motor.fmu")    # its bytes
    attachments.path_of(project_id, "resources/motor.fmu") # a Path, for tools that need one
    attachments.remove(project_id, "resources/motor.fmu")

Each raises ValueError for a path or name that is not a plain file name in
the resources folder, and FileNotFoundError for one that is not there.
"""
from __future__ import annotations

import hashlib
import io
import json
import re
import shutil
import zipfile
from pathlib import Path

from .fileio import write_atomic
from .schemas import DataCheck, Project
from .storage import Location, location

PREFIX = "resources/"
#: A file name in the resources folder: no folders, no leading dot or
#: space, nothing Windows refuses in a name, at most 128 characters.
SAFE_NAME = re.compile(r"[A-Za-z0-9_\-+()\[\]](?:[A-Za-z0-9 _.\-+()\[\]]{0,126}[A-Za-z0-9_\-+()\[\]])?")
_WINDOWS_RESERVED = re.compile(r"(con|prn|aux|nul|com\d|lpt\d)(\..*)?", re.IGNORECASE)
#: The largest file that can be attached, and the largest bundle unpacked.
MAX_FILE_BYTES = 1024 * 1024 * 1024
MAX_BUNDLE_BYTES = 2 * 1024 * 1024 * 1024

#: What a file holds, by its extension. "fmu" and "onnx" files (and other
#: programs) carry code that runs when the model runs, so a project with one
#: asks to be trusted before its first run (see frontend trust.ts).
KINDS = {
    ".fmu": "fmu",
    ".onnx": "onnx",
    ".csv": "data", ".tsv": "data", ".txt": "data", ".mat": "data", ".json": "data",
    ".parquet": "data", ".xlsx": "data", ".mf4": "data", ".mdf": "data",
    ".ssp": "model", ".ssd": "model",
    ".py": "program", ".dll": "program", ".so": "program", ".dylib": "program",
    ".exe": "program",
}
EXECUTABLE_KINDS = frozenset({"fmu", "onnx", "program"})


def kind_of(name: str) -> str:
    return KINDS.get(Path(name).suffix.lower(), "file")


def _check_name(name: str) -> str:
    if (not isinstance(name, str) or not SAFE_NAME.fullmatch(name)
            or _WINDOWS_RESERVED.fullmatch(name) or name.endswith(".")):
        raise ValueError(f"Not a usable attachment name: {name!r}")
    return name


def name_of(ref: str) -> str:
    """The file name in a project-relative path ("resources/motor.fmu")."""
    if not isinstance(ref, str) or not ref.startswith(PREFIX):
        raise ValueError(f"An attached file's path starts with {PREFIX!r}: {ref!r}")
    return _check_name(ref[len(PREFIX):])


def clean_name(name: str) -> str:
    """A usable name for an uploaded file: its base name with characters
    the resources folder cannot take replaced by "_"."""
    base = re.split(r"[\\/]", name or "")[-1].strip()
    cleaned = re.sub(r"[^A-Za-z0-9 _.\-+()\[\]]", "_", base).strip(" .")[:128]
    stem, dot, ext = cleaned.rpartition(".")
    if not cleaned or (dot and not stem):
        cleaned = f"file{('.' + ext) if dot else ''}"
    if _WINDOWS_RESERVED.fullmatch(cleaned) or not SAFE_NAME.fullmatch(cleaned):
        cleaned = f"file-{cleaned}".strip(" .")[:128]
    return _check_name(cleaned)


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def _info(path: Path) -> dict:
    return {"path": PREFIX + path.name, "name": path.name, "sha256": sha256_of(path),
            "bytes": path.stat().st_size, "kind": kind_of(path.name)}


def _folder(project_id: str) -> Path:
    return location(project_id).resources


def path_of(project_id: str, ref: str) -> Path:
    """The attached file's path on disk (FileNotFoundError if it is not there)."""
    path = _folder(project_id) / name_of(ref)
    if not path.is_file():
        raise FileNotFoundError(ref)
    return path


def read(project_id: str, ref: str) -> bytes:
    return path_of(project_id, ref).read_bytes()


def list_files(project_id: str) -> list[dict]:
    """The files in the project's resources folder, by name: each one's
    path, name, SHA-256, size and kind."""
    folder = _folder(project_id)
    if not folder.is_dir():
        return []
    return [_info(p) for p in sorted(folder.iterdir())
            if p.is_file() and SAFE_NAME.fullmatch(p.name)]


def add(project_id: str, name: str, data: bytes | Path) -> dict:
    """Attach a file: copy `data` (bytes, or a file to copy) into the
    project's resources folder under `name` (cleaned up), and return its
    path, name, SHA-256, size and kind. The same bytes under a name already
    there return that file; other bytes under a taken name get a new name
    ("motor-2.fmu")."""
    if isinstance(data, Path):
        size = data.stat().st_size
        digest = sha256_of(data)
    else:
        size = len(data)
        digest = hashlib.sha256(data).hexdigest()
    if size > MAX_FILE_BYTES:
        raise ValueError(f"Attached files can be at most {MAX_FILE_BYTES // 2**20} MB.")
    loc = location(project_id)
    folder = loc.resources
    folder.mkdir(parents=True, exist_ok=True)
    wanted = clean_name(name)
    stem, dot, ext = wanted.rpartition(".")
    if not dot:
        stem, ext = wanted, ""
    n = 1
    while True:
        candidate = wanted if n == 1 else f"{stem[:110]}-{n}{dot}{ext}"
        target = folder / candidate
        if not target.exists():
            break
        if target.is_file() and target.stat().st_size == size and sha256_of(target) == digest:
            return _info(target)
        n += 1
    if isinstance(data, Path):
        tmp = target.with_name(f".{target.name}.part")
        shutil.copyfile(data, tmp)
        tmp.replace(target)
    else:
        write_atomic(target, data)
    return _info(target)


def remove(project_id: str, ref: str) -> bool:
    """Delete an attached file from the resources folder (the project's
    list of attachments is the UI's to change, with the model)."""
    path = _folder(project_id) / name_of(ref)
    if not path.is_file():
        return False
    path.unlink()
    return True


def copy_all(src: Location, dst: Location) -> int:
    """Copy every attached file from one project location to another (Save
    As a new file); a file already there is left as it is. Returns how many
    were copied."""
    if not src.resources.is_dir() or src.resources == dst.resources:
        return 0
    copied = 0
    for path in sorted(src.resources.iterdir()):
        if not (path.is_file() and SAFE_NAME.fullmatch(path.name)):
            continue
        target = dst.resources / path.name
        if target.exists():
            continue
        dst.resources.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        copied += 1
    return copied


def check(project: Project) -> list[DataCheck]:
    """Data Checks for the project's attached files: a parameter that names a
    file that is not attached or not in the resources folder (an error, the
    run would fail), a listed file missing from the folder (a warning while
    no part uses it), and one changed since it was attached (a warning)."""
    try:
        folder = _folder(project.id)
    except ValueError:
        return []
    used: dict[str, list] = {}
    for system in project.systems:
        for el in system.elements:
            for key, value in el.parameterOverrides.items():
                if isinstance(value, str) and value.startswith(PREFIX):
                    used.setdefault(value, []).append((el, key))
    checks: list[DataCheck] = []
    listed = set()
    for att in project.attachments:
        listed.add(att.path)
        try:
            path = folder / name_of(att.path)
        except ValueError:
            continue
        users = used.get(att.path, [])
        if not path.is_file():
            checks.append(DataCheck(
                level="error" if users else "warning",
                elementId=users[0][0].id if users else None,
                elementLabel=users[0][0].label if users else None,
                elementIds=[el.id for el, _ in users],
                text=f"The attached file '{att.path}' is missing from the project's "
                     f"resources folder ({folder}).",
                fix="Copy the file back into that folder, or attach it again "
                    "(Project → Attached files)."))
        elif path.stat().st_size != att.bytes or sha256_of(path) != att.sha256:
            checks.append(DataCheck(
                level="warning",
                elementIds=[el.id for el, _ in users],
                text=f"The attached file '{att.path}' has changed since it was attached.",
                fix="If the new file is the one you want, attach it again to record it "
                    "(Project → Attached files)."))
    for ref, users in used.items():
        if ref in listed:
            continue
        for el, key in users:
            checks.append(DataCheck(
                level="error", elementId=el.id, elementLabel=el.label, elementIds=[el.id],
                text=f"{el.label}: '{key}' names '{ref}', which is not attached to the project.",
                fix="Attach the file (Project → Attached files) or pick another."))
    return checks


def rename_refs(project: Project, renamed: dict[str, str]) -> None:
    """Point the project's list of attachments and its file parameters at
    the new paths in `renamed` (old path → new path)."""
    for att in project.attachments:
        att.path = renamed.get(att.path, att.path)
    for system in project.systems:
        for el in system.elements:
            for key, value in list(el.parameterOverrides.items()):
                if isinstance(value, str) and value in renamed:
                    el.parameterOverrides[key] = renamed[value]
    for case in project.cases:
        for values in case.parameterOverrides.values():
            for key, value in list(values.items()):
                if isinstance(value, str) and value in renamed:
                    values[key] = renamed[value]


# ---- bundles: one zip file to send ----------------------------------------

BUNDLE_PROJECT = "project.json"


def export_bundle(project: Project) -> bytes:
    """The project as one zip file: project.json and resources/ with each
    attached file the project lists (those missing on disk are left out)."""
    buf = io.BytesIO()
    folder = _folder(project.id)
    with zipfile.ZipFile(buf, "w", compression=zipfile.ZIP_DEFLATED) as z:
        z.writestr(BUNDLE_PROJECT, project.model_dump_json(indent=2))
        for att in project.attachments:
            try:
                path = folder / name_of(att.path)
            except ValueError:
                continue
            if path.is_file():
                z.write(path, att.path)
    return buf.getvalue()


def read_bundle(data: bytes) -> tuple[dict, dict[str, bytes]]:
    """A bundle's project JSON and its attached files (by name). Anything
    else in the zip is ignored; a bundle that unpacks to more than
    MAX_BUNDLE_BYTES, or has no project.json, raises ValueError."""
    try:
        z = zipfile.ZipFile(io.BytesIO(data))
    except zipfile.BadZipFile as e:
        raise ValueError("Not a LightSim bundle (not a zip file).") from e
    with z:
        infos = z.infolist()
        if sum(i.file_size for i in infos) > MAX_BUNDLE_BYTES:
            raise ValueError("The bundle is too large to unpack.")
        try:
            raw = json.loads(z.read(BUNDLE_PROJECT))
        except KeyError as e:
            raise ValueError(f"Not a LightSim bundle (no {BUNDLE_PROJECT} in it).") from e
        resources: dict[str, bytes] = {}
        for info in infos:
            if info.is_dir() or not info.filename.startswith(PREFIX):
                continue
            try:
                name = name_of(info.filename)
            except ValueError:
                continue
            resources[name] = z.read(info)
    return raw, resources
