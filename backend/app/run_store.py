"""Run history persistence — one gzip-compressed JSON file per finished run.

A project's runs live in its runs folder (:func:`app.storage.location`):
``runs/<project id>/`` inside :func:`app.paths.projects_dir`, or
``<name>.lightsim-runs/`` beside a .lightsim file, so they survive a
reload, an app restart and opening another project. ``index.json`` in that
folder lists each run's case, start time, status and summary values, so the
history can be listed without decompressing every run. The index is repaired
from the run files if it goes missing or falls out of step with them.

The project's parameter studies (a sweep's definition and results table)
are kept there too, as ``studies/<study id>.json``, and not in the model
file (PLT-34): running a sweep leaves the model file as it is, so git shows
only real model edits and two people's sweeps never collide.

This is a stop-gap until the columnar run store (Arrow/Parquet) lands.
"""
from __future__ import annotations

import contextlib
import gzip
import json
import shutil
import threading
import zlib
from pathlib import Path

from .schemas import StoredRun, Study
from .storage import _write_atomic, location, project_path, safe_id, user_dir

#: Disk budget for one project's stored runs. When a new run takes the project
#: over it, its oldest runs are deleted and the caller is told which.
BUDGET_BYTES = 500 * 1024 * 1024
#: Budget for all projects' runs together. Past it, runs of projects that were
#: never saved (New, imported, deleted) go first, then the oldest of the rest.
TOTAL_BUDGET_BYTES = 2 * 1024 * 1024 * 1024

_INDEX = "index.json"
_SUFFIX = ".json.gz"
# index read-modify-write must not interleave (the API serves from a thread pool)
_lock = threading.Lock()


_safe_id = safe_id


def _runs_dir(project_id: str) -> Path:
    return location(project_id).runs


def _make_runs_dir(project_id: str) -> Path:
    loc = location(project_id)
    return loc.make_dir(loc.runs)


def _valid_entry(e: object, files: dict) -> bool:
    """An index entry this module can use: anything else (a hand edit, another
    version's format) is dropped and rebuilt from its run file."""
    return (isinstance(e, dict) and isinstance(e.get("id"), str) and e["id"] in files
            and isinstance(e.get("startedAt", 0), (int, float))
            and not isinstance(e.get("startedAt", 0), bool)
            and isinstance(e.get("bytes", 0), int) and not isinstance(e.get("bytes", 0), bool))


def _entry(run: StoredRun, size: int) -> dict:
    """Index entry: the run without its channel data or model snapshot, plus
    key results."""
    meta = run.model_dump(exclude={"result", "snapshot"}, exclude_none=True)
    return {**meta, "summary": [s.model_dump() for s in run.result.summary], "bytes": size}


def _load_index(folder: Path) -> list[dict]:
    """Index entries for exactly the run files in ``folder``; call with _lock held.

    Entries whose file is gone are dropped and files missing from the index
    (for example after a crash between the two writes) are read back in.
    """
    try:
        entries = json.loads((folder / _INDEX).read_text(encoding="utf-8"))["runs"]
        if not isinstance(entries, list):
            entries = []
    except (OSError, ValueError, KeyError, TypeError):
        entries = []
    files = {p.name[: -len(_SUFFIX)]: p for p in folder.glob(f"*{_SUFFIX}")}
    kept = [e for e in entries if _valid_entry(e, files)]
    changed = len(kept) != len(entries)
    known = {e["id"] for e in kept}
    for run_id, path in files.items():
        if run_id in known:
            continue
        try:
            run = StoredRun.model_validate_json(gzip.decompress(path.read_bytes()))
        except (OSError, EOFError, zlib.error, ValueError):
            continue  # unreadable: leave the file alone, but do not list it
        kept.append({**_entry(run, path.stat().st_size), "id": run_id})
        changed = True
    if changed:
        _write_index(folder, kept)
    return kept


def _write_index(folder: Path, entries: list[dict]) -> None:
    _write_atomic(folder / _INDEX, json.dumps({"runs": entries}).encode("utf-8"))


def _newest_first(entries: list[dict]) -> list[dict]:
    return sorted(entries, key=lambda e: e.get("startedAt", 0), reverse=True)


def list_runs(project_id: str) -> list[dict]:
    """The project's stored runs, newest first (empty if it has none)."""
    folder = _runs_dir(project_id)
    if not folder.is_dir():
        return []
    with _lock:
        return _newest_first(_load_index(folder))


def save_run(project_id: str, run: StoredRun) -> tuple[list[dict], list[str]]:
    """Store (or replace) a run. Returns the project's runs, newest first, and
    the ids of old runs deleted to keep within :data:`BUDGET_BYTES`."""
    folder = _runs_dir(project_id)
    name = _safe_id(run.id, "run") + _SUFFIX
    data = gzip.compress(run.model_dump_json(exclude_unset=True).encode("utf-8"), compresslevel=6)
    with _lock:
        _make_runs_dir(project_id)
        entries = [e for e in _load_index(folder) if e["id"] != run.id]
        _write_atomic(folder / name, data)
        entries.append(_entry(run, len(data)))

        pruned: list[str] = []
        total = sum(e.get("bytes", 0) for e in entries)
        for e in reversed(_newest_first(entries)):
            if total <= BUDGET_BYTES:
                break
            if e["id"] == run.id:
                continue
            (folder / f"{e['id']}{_SUFFIX}").unlink(missing_ok=True)
            total -= e.get("bytes", 0)
            pruned.append(e["id"])
        entries = [e for e in entries if e["id"] not in pruned]
        _write_index(folder, entries)
        for other, run_id in _trim_total(folder / name):
            if other == folder:
                pruned.append(run_id)
                entries = [e for e in entries if e["id"] != run_id]
    return _newest_first(entries), pruned


def _trim_total(keep: Path) -> list[tuple[Path, str]]:
    """Delete runs, never `keep`, until all projects' runs fit
    :data:`TOTAL_BUDGET_BYTES`; call with _lock held. Returns (folder, run id)
    of each deleted run. Folders' indexes repair themselves on the next read."""
    runs = [(p, p.stat()) for p in (user_dir() / "runs").glob(f"*/*{_SUFFIX}")]
    total = sum(st.st_size for _, st in runs)
    if total <= TOTAL_BUDGET_BYTES:
        return []

    def unsaved(folder: Path) -> bool:
        try:
            return not project_path(folder.name).is_file()
        except ValueError:
            return True

    # never-saved projects first, then oldest first
    runs.sort(key=lambda r: (not unsaved(r[0].parent), r[1].st_mtime))
    deleted = []
    for path, st in runs:
        if total <= TOTAL_BUDGET_BYTES:
            break
        if path == keep:
            continue
        path.unlink(missing_ok=True)
        total -= st.st_size
        deleted.append((path.parent, path.name[: -len(_SUFFIX)]))
    for folder in {f for f, _ in deleted}:
        _load_index(folder)
    return deleted


def run_path(project_id: str, run_id: str) -> Path:
    """The run's gzip file; raises FileNotFoundError if it is not stored."""
    path = _runs_dir(project_id) / f"{_safe_id(run_id, 'run')}{_SUFFIX}"
    if not path.is_file():
        raise FileNotFoundError(run_id)
    return path


def delete_run(project_id: str, run_id: str) -> bool:
    folder = _runs_dir(project_id)
    path = folder / f"{_safe_id(run_id, 'run')}{_SUFFIX}"
    with _lock:
        if not path.is_file():
            return False
        path.unlink()
        _load_index(folder)  # drops the entry
    return True


def clear_runs(project_id: str) -> int:
    """Delete every stored run of the project (its studies stay); returns
    how many there were."""
    folder = _runs_dir(project_id)
    with _lock:
        if not folder.is_dir():
            return 0
        runs = list(folder.glob(f"*{_SUFFIX}"))
        for path in runs:
            path.unlink(missing_ok=True)
        (folder / _INDEX).unlink(missing_ok=True)
        with contextlib.suppress(OSError):
            folder.rmdir()  # only if nothing else (studies, .gitignore) is in it
    return len(runs)


def clear_all(project_id: str) -> int:
    """Delete the project's runs and studies (the project is being deleted);
    returns how many runs there were."""
    folder = _runs_dir(project_id)
    with _lock:
        if not folder.is_dir():
            return 0
        count = len(list(folder.glob(f"*{_SUFFIX}")))
        shutil.rmtree(folder)
    return count


# ---- studies (PLT-34) ------------------------------------------------------

_STUDIES = "studies"


def _study_path(project_id: str, study_id: str) -> Path:
    return _runs_dir(project_id) / _STUDIES / f"{_safe_id(study_id, 'study')}.json"


def list_studies(project_id: str) -> list[dict]:
    """The project's studies, oldest first (by when each was started).
    Files that are not a study are skipped."""
    folder = _runs_dir(project_id) / _STUDIES
    out = []
    for path in folder.glob("*.json") if folder.is_dir() else []:
        try:
            out.append(Study.model_validate_json(path.read_bytes()).model_dump(mode="json"))
        except (OSError, ValueError):
            continue
    return sorted(out, key=lambda s: (s["startedAt"], s["id"]))


def save_study(project_id: str, study: Study) -> None:
    """Store (or replace) a study."""
    path = _study_path(project_id, study.id)
    with _lock:
        _make_runs_dir(project_id)
        path.parent.mkdir(exist_ok=True)
        _write_atomic(path, study.model_dump_json(indent=1).encode("utf-8"))


def delete_study(project_id: str, study_id: str) -> bool:
    path = _study_path(project_id, study_id)
    with _lock:
        if not path.is_file():
            return False
        path.unlink()
    return True


def adopt_studies(project_id: str, studies: list[dict]) -> int:
    """Keep studies an upgrade took out of a version 1 file (PLT-07) with the
    project's runs. A study already stored is left as it is, so reading the
    old file again changes nothing. Returns how many were stored."""
    stored = 0
    for raw in studies:
        try:
            study = Study.model_validate(raw)
            if _study_path(project_id, study.id).exists():
                continue
            save_study(project_id, study)
            stored += 1
        except ValueError:
            continue  # a malformed study: the old file still has it
    return stored
