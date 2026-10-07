"""Which Script code this user has agreed to run (PLT-35).

A project's Script blocks hold Python code that comes with the project. Code
from someone else should not run until the user has seen it and said yes, so
every run checks each Script block's code against the code this user trusts:

* the code of the examples that ship with LightSim, and the Script block's
  default code from the library;
* code the user approved in the app's prompt ("Run scripts");
* code the user typed or pasted into a Script block in the app (the UI
  approves it as it is entered);
* on the first start of a version with this check, the code in the projects
  already in the projects folder, which this user saved before.

Approval is remembered per exact code: a SHA-256 hash of its text, kept in
``.script-trust.json`` in the projects folder. Changing one character asks
again. With the policy file's ``scriptTrust: "always-prompt"`` (PLT-36),
approvals last only until LightSim closes.

The check runs where the solver starts the Script worker (solver/domains.py),
so no run of any kind, REST, live or a study, gets past it. Data Checks only
compile scripts and never run them, so they are not affected.
"""
from __future__ import annotations

import hashlib
import json
import logging
import os
import threading
from pathlib import Path

from .paths import EXAMPLES_DIR, projects_dir

log = logging.getLogger(__name__)

SCRIPT_COMPONENT = "signal.script"
_FILE = ".script-trust.json"
_lock = threading.Lock()
_session: set[str] = set()  # approvals that last until the engine stops
_builtin: set[str] | None = None


class ScriptsNotApproved(Exception):
    """A run would execute Script code the user has not approved."""

    def __init__(self, labels: list[str]):
        self.labels = labels
        names = ", ".join(f"'{label}'" for label in labels)
        super().__init__(
            f"Script {names} {'has' if len(labels) == 1 else 'have'} not been approved to run. "
            "This project's scripts came from outside this computer or changed since you "
            "approved them: review them and choose Run scripts when LightSim asks.")


def policy() -> dict:
    """The settings the machine-wide policy file fixes (set by the desktop
    shell as LIGHTSIM_POLICY); {} without one."""
    try:
        raw = json.loads(os.environ.get("LIGHTSIM_POLICY") or "{}")
    except ValueError:
        return {}
    return raw if isinstance(raw, dict) else {}


def mode() -> str:
    """"prompt" (remember approvals), "always-prompt" (only until LightSim
    closes) or "off" (no check: the test suite and LIGHTSIM_SCRIPT_TRUST=off)."""
    if os.environ.get("LIGHTSIM_SCRIPT_TRUST", "").lower() == "off":
        return "off"
    return "always-prompt" if policy().get("scriptTrust") == "always-prompt" else "prompt"


def code_hash(code: str) -> str:
    """The same code saved on Windows or Linux has the same hash."""
    return hashlib.sha256(code.replace("\r\n", "\n").encode("utf-8")).hexdigest()


def _default_code() -> str:
    from .library import load_library
    for comp in load_library():
        if comp.id == SCRIPT_COMPONENT:
            for p in comp.parameters:
                if p.key == "code":
                    return str(p.default or "")
    return ""


def project_scripts(raw: dict) -> list[dict]:
    """Every Script code a project would run: each Script block's code (its
    own or the library default) and any code a case sets for it. Tolerates
    a malformed project (it is checked again, strictly, when it runs)."""
    default = _default_code()
    out = []
    script_ids: dict[str, str] = {}

    def items(value):
        return [v for v in value if isinstance(v, dict)] if isinstance(value, list) else []

    for system in items(raw.get("systems")):
        for el in items(system.get("elements")):
            if el.get("componentDefId") != SCRIPT_COMPONENT:
                continue
            overrides = el.get("parameterOverrides")
            code = overrides.get("code", default) if isinstance(overrides, dict) else default
            el_id = str(el.get("id", ""))
            script_ids[el_id] = str(el.get("label") or el_id)
            out.append({"elementId": el_id, "label": script_ids[el_id], "code": str(code)})
    for case in items(raw.get("cases")):
        case_overrides = case.get("parameterOverrides")
        if not isinstance(case_overrides, dict):
            continue
        for el_id, overrides in case_overrides.items():
            if el_id in script_ids and isinstance(overrides, dict) and "code" in overrides:
                out.append({"elementId": el_id,
                            "label": f"{script_ids[el_id]} (case '{case.get('name', '')}')",
                            "code": str(overrides["code"])})
    return out


def _builtin_hashes() -> set[str]:
    global _builtin
    if _builtin is None:
        hashes = {code_hash(_default_code())}
        for f in sorted(EXAMPLES_DIR.glob("*.json")):
            try:
                raw = json.loads(f.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                continue
            hashes |= {code_hash(s["code"]) for s in project_scripts(raw)}
        _builtin = hashes
    return _builtin


def _store() -> Path:
    return projects_dir() / _FILE


def _read() -> set[str] | None:
    try:
        raw = json.loads(_store().read_text(encoding="utf-8"))
    except FileNotFoundError:
        return None
    except (OSError, ValueError):
        return set()
    return {h for h in raw.get("approved", []) if isinstance(h, str)} if isinstance(raw, dict) else set()


def _write(hashes: set[str]) -> None:
    path = _store()
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(json.dumps({"approved": sorted(hashes)}, indent=2) + "\n", encoding="utf-8")
    os.replace(tmp, path)


def _saved() -> set[str]:
    """Approvals on disk. The first time, the scripts of the projects this
    user already saved in the projects folder count as approved."""
    hashes = _read()
    if hashes is not None:
        return hashes
    hashes = set()
    folder = projects_dir()
    for f in sorted(folder.glob("*.json")) if folder.is_dir() else []:
        try:
            raw = json.loads(f.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if isinstance(raw, dict):
            hashes |= {code_hash(s["code"]) for s in project_scripts(raw)}
    try:
        _write(hashes)
    except OSError as e:
        log.warning("script trust: could not write %s (%s)", _store(), e)
    return hashes


def trusted_hashes() -> set[str]:
    with _lock:
        saved = _saved() if mode() == "prompt" else set()
        return _builtin_hashes() | saved | _session


def is_approved(code: str) -> bool:
    return mode() == "off" or code_hash(code) in trusted_hashes()


def approve(codes: list[str]) -> list[str]:
    """Trust these codes from now on (until LightSim closes, under
    always-prompt). Returns their hashes."""
    hashes = {code_hash(c) for c in codes}
    with _lock:
        _session.update(hashes)
        if mode() == "prompt":
            _write(_saved() | hashes)
    return sorted(hashes)


def check(scripts: list[tuple[str, str]]) -> None:
    """Raise ScriptsNotApproved for any (label, code) the user has not
    approved. Called before a run starts its Script worker."""
    if mode() == "off":
        return
    trusted = trusted_hashes()
    missing = [label for label, code in scripts if code_hash(code) not in trusted]
    if missing:
        raise ScriptsNotApproved(missing)


def _reset_for_tests() -> None:
    global _builtin
    with _lock:
        _session.clear()
        _builtin = None
