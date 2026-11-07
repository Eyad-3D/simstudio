"""A project's history: one line per save, kept beside its backups (step 6).

Every save of a project file, by the app or by an AI agent (the MCP tool
``model_edit``, the ``lightsim edit`` command), adds one line to
``history.jsonl`` in the project's backups folder (``.backups/<id>/`` or
``<name>.lightsim-backups/``, which git ignores). A line says who saved
(``by``: app or agent, the agent's client and session), with which tool and
why (the ``reason`` an agent gives), what changed in words, the revision the
file had before and after, and the change itself as a ``diff``.

The diff names list items by their ``id`` (parts, wires, cases), not by
their place, so it still applies after other changes: the open window lays
an agent's changes over its own unsaved ones (:func:`rebase`), and undoing
one change reverts only what that change did (:func:`apply` with
``revert``), leaving later changes to other values alone. A change whose
value was changed again since is not reverted: it is reported instead.

The format is in docs/spec/project.md (Project history) and
docs/spec/schemas/history.schema.json.
"""
from __future__ import annotations

import contextlib
import copy
import json
import os
import time
import uuid
from pathlib import Path
from typing import Any, Literal, Optional, Union

from pydantic import BaseModel, Field

#: The file, in the project's backups folder.
HISTORY_NAME = "history.jsonl"
#: Past this size the history is started afresh, the old one kept as .1.
HISTORY_MAX_BYTES = 4 * 1024 * 1024

#: A step of a path: a key of an object, or the item of a list with that id.
PathStep = Union[str, dict]


class Change(BaseModel):
    """One value that changed. ``old`` is left out when the value was added,
    ``new`` when it was removed; ``at`` is the place a list item took when it
    was added, or that a list item or an object's key had when it was removed
    (an undo puts it back there)."""

    path: list[PathStep] = Field(description="keys of objects, and {\"id\": ...} for an item of "
                                             "a list of objects with ids")
    old: Any = None
    new: Any = None
    at: Optional[int] = None


class HistoryEntry(BaseModel):
    """One save of the project file."""

    id: str
    t: int = Field(description="when, in milliseconds since 1 January 1970, UTC")
    by: Literal["app", "agent"]
    tool: str = Field(description="save (the app), model_edit (MCP), lightsim edit (command "
                                  "line), undo, restore")
    client: Optional[str] = Field(None, description="the AI app, as it names itself")
    session: Optional[str] = Field(None, description="the agent's session: one MCP connection, "
                                                     "or the command line's LIGHTSIM_SESSION")
    reason: Optional[str] = Field(None, description="why, in the agent's words")
    changes: list[str] = Field(default_factory=list, description="what changed, in words")
    from_: Optional[str] = Field(None, alias="from",
                                 description="the file's revision before (None: a new file)")
    to: str = Field(description="the file's revision after")
    diff: Optional[list[Change]] = Field(None, description="None: not known (a new file, or one "
                                                           "that could not be read)")
    undoes: list[str] = Field(default_factory=list, description="ids of the entries it undid")

    model_config = {"populate_by_name": True}


# -- diff and apply --------------------------------------------------------

_MISSING = object()


def _by_id(value: Any) -> bool:
    """A list of objects, each with its own text id."""
    if not isinstance(value, list):
        return False
    ids = [v.get("id") if isinstance(v, dict) else None for v in value]
    return all(isinstance(i, str) for i in ids) and len(set(ids)) == len(ids)


def _change(path: list, old: Any = _MISSING, new: Any = _MISSING, at: Optional[int] = None) -> dict:
    out: dict = {"path": path}
    if old is not _MISSING:
        out["old"] = old
    if new is not _MISSING:
        out["new"] = new
    if at is not None:
        out["at"] = at
    return out


def diff(old: Any, new: Any, path: Optional[list] = None) -> list[dict]:
    """The changes that turn ``old`` into ``new`` (parsed JSON)."""
    path = path or []
    if old == new:
        return []
    if isinstance(old, dict) and isinstance(new, dict):
        place = {k: n for n, k in enumerate(old)}
        # removed keys last first, each with its place among the keys, so an
        # undo (which reverts in reverse order) puts them back in the old order
        out = [_change([*path, k], old=old[k], at=place[k]) for k in reversed(list(old)) if k not in new]
        for k in [*old, *(k for k in new if k not in old)]:
            if k not in new:
                continue
            if k not in old:
                out.append(_change([*path, k], new=new[k]))
            else:
                out += diff(old[k], new[k], [*path, k])
        return out
    if _by_id(old) and _by_id(new):
        before = {v["id"]: v for v in old}
        after = {v["id"]: v for v in new}
        # removed items last first, each with the place it had, so an undo
        # (which reverts in reverse order) puts them back where they were
        out = [_change([*path, {"id": v["id"]}], old=v, at=n)
               for n, v in reversed(list(enumerate(old))) if v["id"] not in after]
        for i, v in before.items():
            if i in after:
                out += diff(v, after[i], [*path, {"id": i}])
        out += [_change([*path, {"id": v["id"]}], new=v, at=n)
                for n, v in enumerate(new) if v["id"] not in before]
        # ponytail: a pure reorder of a list by id is not recorded (the
        # canvas does not depend on it); record it if order starts to matter
        return out
    return [_change(path, old=old, new=new)]


def _step(node: Any, step: PathStep) -> Any:
    if isinstance(step, dict):
        if not isinstance(node, list):
            return _MISSING
        return next((v for v in node if isinstance(v, dict) and v.get("id") == step.get("id")),
                    _MISSING)
    return node.get(step, _MISSING) if isinstance(node, dict) else _MISSING


def _get(doc: Any, path: list) -> Any:
    for step in path:
        doc = _step(doc, step)
        if doc is _MISSING:
            break
    return doc


def _put(doc: Any, path: list, value: Any, at: Optional[int]) -> bool:
    """Set (or with _MISSING remove) the value at ``path``; False when the
    place it goes is gone."""
    parent = _get(doc, path[:-1])
    last = path[-1]
    if isinstance(last, dict):
        if not isinstance(parent, list):
            return False
        index = next((n for n, v in enumerate(parent)
                      if isinstance(v, dict) and v.get("id") == last.get("id")), None)
        if value is _MISSING:
            if index is not None:
                del parent[index]
        elif index is not None:
            parent[index] = value
        else:
            parent.insert(min(at if at is not None else len(parent), len(parent)), value)
        return True
    if not isinstance(parent, dict):
        return False
    if value is _MISSING:
        parent.pop(last, None)
    elif at is not None and last not in parent:  # a removed key goes back to its place
        items = list(parent.items())
        items.insert(min(at, len(items)), (last, value))
        parent.clear()
        parent.update(items)
    else:
        parent[last] = value
    return True


def apply(doc: Any, changes: list[dict], *, revert: bool = False) -> list[dict]:
    """Apply ``changes`` to ``doc`` in place, or undo them with ``revert``.
    Returns the changes that met a different value than they expected.

    Forward (laying saved changes over an unsaved copy), the change is made
    anyway: the newer save wins, and the caller says so. Reverting, a value
    that was changed again since is left as it is."""
    conflicts = []
    for c in (reversed(changes) if revert else changes):
        expect = c.get("new" if revert else "old", _MISSING)
        want = c.get("old" if revert else "new", _MISSING)
        now = _get(doc, c["path"])
        if now == want:
            continue
        clash = now != expect
        if clash and revert:
            conflicts.append(c)
            continue
        value = want if want is _MISSING else copy.deepcopy(want)
        placed = _put(doc, c["path"], value, c.get("at"))
        if clash or not placed:
            conflicts.append(c)
    return conflicts


def describe(c: dict) -> str:
    """A change as a short path: 'systems/sys-root/elements/el-car/parameterOverrides/mass_kg'."""
    return "/".join(s.get("id", "?") if isinstance(s, dict) else str(s) for s in c["path"])


# -- the file ---------------------------------------------------------------

def path_in(backups: Path) -> Path:
    return Path(backups) / HISTORY_NAME


def record(backups: Path, before: Optional[bytes], after: bytes, revision_of,
           meta: Optional[dict] = None) -> dict:
    """Add the line for one save (call with the project's save lock held)."""
    meta = dict(meta or {})
    entry: dict = {
        "id": uuid.uuid4().hex[:16],
        "t": time.time_ns() // 1_000_000,
        "by": meta.pop("by", "app"),
        "tool": meta.pop("tool", "save"),
        "client": meta.pop("client", None) or None,
        "session": meta.pop("session", None) or None,
        "reason": str(meta.get("reason") or "").strip()[:500] or None,
        "changes": [str(c) for c in (meta.pop("changes", None) or [])][:200],
        "from": revision_of(before) if before is not None else None,
        "to": revision_of(after),
        "diff": None,
        "undoes": list(meta.pop("undoes", None) or []),
    }
    if before is not None:
        with contextlib.suppress(ValueError):
            entry["diff"] = diff(json.loads(before), json.loads(after))
    path = path_in(backups)
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists() and path.stat().st_size > HISTORY_MAX_BYTES:
        os.replace(path, path.with_suffix(".jsonl.1"))
    with path.open("a", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False, separators=(",", ":")) + "\n")
    return entry


def entries(backups: Path) -> list[dict]:
    """Every line of the history, oldest first (unreadable lines left out)."""
    try:
        text = path_in(backups).read_text(encoding="utf-8")
    except OSError:
        return []
    out = []
    for line in text.splitlines():
        with contextlib.suppress(ValueError):
            entry = json.loads(line)
            if isinstance(entry, dict) and isinstance(entry.get("to"), str):
                out.append(entry)
    return out


def chain(lines: list[dict], start: str, end: str) -> Optional[list[dict]]:
    """The saves that took the file from revision ``start`` to ``end``, in
    order, each with its diff; None when the history does not tell (the
    file was changed by something else, such as a git pull)."""
    out: list[dict] = []
    want = end
    for entry in reversed(lines):
        if want == start:
            break
        if entry.get("to") != want:
            continue
        if not isinstance(entry.get("diff"), list):
            return None
        out.append(entry)
        want = entry.get("from")
    return list(reversed(out)) if want == start else None


def undone(lines: list[dict]) -> set[str]:
    """The ids of the entries undone and not brought back. Newest first, an
    undo counts only while it is not undone itself: undoing an undo puts
    the changes it undid back in force."""
    gone: set[str] = set()
    for e in reversed(lines):
        if e["id"] not in gone:
            gone.update(e.get("undoes") or [])
    return gone


def summary(entry: dict) -> dict:
    """An entry without its diff, for the window's list."""
    return {k: v for k, v in entry.items() if k != "diff"} | {
        "size": len(entry.get("diff") or []), "undoable": isinstance(entry.get("diff"), list)}
