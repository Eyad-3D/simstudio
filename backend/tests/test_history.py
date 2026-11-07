"""The project's history (step 6, item 4 and item 1's undo and change log):
every save, by the app or by an AI agent through the MCP tool or the command
line, is one line beside the backups, with who, why and what changed; the
window lays an agent's saves over its unsaved copy (rebase); one call undoes
an agent's change or its whole session; and the app and an agent's process
cannot overwrite each other's saves."""
from __future__ import annotations

import json
import multiprocessing
import os
import shutil
import subprocess
import sys
from pathlib import Path

import jsonschema
import pytest
from conftest import allow_ai
from fastapi.testclient import TestClient

from app import history, storage
from app.ai.access import AuditLog, Policy
from app.ai.engine import Engine
from app.ai.mcp_server import McpServer
from app.main import app

BACKEND = Path(__file__).resolve().parent.parent
META = {"io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "claude-code", "version": "1"}}


@pytest.fixture
def folder(tmp_path, monkeypatch):
    projects = tmp_path / "projects"
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(projects))
    allow_ai(projects)
    return projects


def _bev(pid: str = "car") -> storage.Project:
    project = storage.load_example("bev-car")
    project.id = pid
    return project


def _mass(doc: dict) -> float:
    return next(e for s in doc["systems"] for e in s["elements"]
                if e["label"] == "Vehicle")["parameterOverrides"]["mass_kg"]


def _vehicle(doc: dict) -> dict:
    return next(e for s in doc["systems"] for e in s["elements"] if e["label"] == "Vehicle")


def _tool(server: McpServer, name: str, args: dict) -> dict:
    reply = server.handle({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                           "params": {"name": name, "arguments": args, "_meta": META}})
    assert "error" not in reply, reply
    assert not reply["result"].get("isError"), reply["result"]
    return reply["result"]["structuredContent"]


def _edit(server: McpServer, ops: list, reason: str | None = "test", project: str = "car") -> dict:
    args = {"project": project, "operations": ops, "dry_run": False}
    if reason:
        args["reason"] = reason
    return _tool(server, "model_edit", args)


def _set(element: str, param: str, value) -> dict:
    return {"op": "set", "element": element, "param": param, "value": value}


# -- diff and apply ------------------------------------------------------------


def test_a_diff_names_list_items_by_id_and_reverts_exactly():
    old = {"name": "a", "systems": [{"id": "s", "elements": [
        {"id": "e1", "p": {"m": 1}}, {"id": "e2", "p": {}}]}], "tags": [1, 2]}
    new = json.loads(json.dumps(old))
    new["systems"][0]["elements"][0]["p"]["m"] = 2
    del new["systems"][0]["elements"][1]
    new["systems"][0]["elements"].append({"id": "e3", "p": {}})
    new["tags"] = [1, 2, 3]
    changes = history.diff(old, new)
    paths = [history.describe(c) for c in changes]
    assert "systems/s/elements/e1/p/m" in paths
    assert "systems/s/elements/e2" in paths and "systems/s/elements/e3" in paths
    assert "tags" in paths, "a list without ids is one value"
    doc = json.loads(json.dumps(old))
    assert history.apply(doc, changes) == [] and doc == new
    assert history.apply(doc, changes, revert=True) == [] and doc == old


def test_a_change_applies_over_other_changes_and_reverts_only_itself():
    base = {"items": [{"id": "a", "v": 1}, {"id": "b", "v": 1}]}
    agent = {"items": [{"id": "a", "v": 2}, {"id": "b", "v": 1}]}
    change = history.diff(base, agent)
    ours = {"items": [{"id": "a", "v": 1}, {"id": "b", "v": 5}, {"id": "c", "v": 0}]}
    assert history.apply(ours, change) == []
    assert ours == {"items": [{"id": "a", "v": 2}, {"id": "b", "v": 5}, {"id": "c", "v": 0}]}
    assert history.apply(ours, change, revert=True) == []
    assert ours["items"][0]["v"] == 1 and ours["items"][1]["v"] == 5, "b's later change stays"


def test_a_value_changed_again_is_not_reverted_and_is_reported():
    change = history.diff({"v": 1}, {"v": 2})
    doc = {"v": 3}
    clashes = history.apply(doc, change, revert=True)
    assert doc == {"v": 3} and [history.describe(c) for c in clashes] == ["v"]


def test_laying_a_save_over_a_different_unsaved_value_takes_the_save_and_says_so():
    change = history.diff({"v": 1}, {"v": 2})
    doc = {"v": 7}
    assert [history.describe(c) for c in history.apply(doc, change)] == ["v"]
    assert doc == {"v": 2}


# -- every save is a line ---------------------------------------------------------


def test_every_save_adds_a_line_with_its_diff_beside_the_backups(folder):
    project = _bev()
    first = storage.save_project(project)
    _vehicle_el = next(e for s in project.systems for e in s.elements if e.label == "Vehicle")
    _vehicle_el.parameterOverrides["mass_kg"] = 2000
    second = storage.save_project(project, first)
    lines = history.entries(storage.location("car").backups)
    assert (folder / ".backups" / "car" / "history.jsonl").is_file()
    assert [(e["from"], e["to"], e["by"], e["tool"]) for e in lines] == [
        (None, first, "app", "save"), (first, second, "app", "save")]
    assert lines[0]["diff"] is None, "a new file has no diff (it is not undone)"
    assert any(c["path"][-1] == "mass_kg" and c["new"] == 2000 for c in lines[1]["diff"])
    assert storage.list_backups("car")[0]["revision"] == first, "the backups are as before"
    # the line follows the format the spec publishes
    schema = json.loads((BACKEND.parent / "docs" / "spec" / "schemas" / "history.schema.json")
                        .read_text(encoding="utf-8"))
    for line in lines:
        jsonschema.Draft202012Validator(schema).validate(line)


def test_a_save_that_changes_nothing_adds_no_line(folder):
    project = _bev()
    rev = storage.save_project(project)
    storage.save_project(project, rev)
    assert len(history.entries(storage.location("car").backups)) == 1


# -- an agent's edit: what, why, who and which session -------------------------


def test_an_mcp_edit_is_one_line_with_its_reason_client_and_session(folder):
    storage.save_project(_bev())
    server = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    done = _edit(server, [_set("Vehicle", "mass_kg", 2100)], reason="heavier battery pack")
    assert "one click" in done["note"]
    line = history.entries(storage.location("car").backups)[-1]
    assert line["by"] == "agent" and line["tool"] == "model_edit"
    assert line["client"] == "claude-code" and line["session"] == server.session
    assert line["reason"] == "heavier battery pack"
    assert line["changes"] == done["changes"] and line["to"] == done["revision"]
    _edit(server, [_set("Vehicle", "mass_kg", 2200)])
    lines = history.entries(storage.location("car").backups)
    assert lines[-1]["session"] == lines[-2]["session"], "one connection is one session"
    other = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    assert other.session != server.session


def test_an_agent_edit_is_applied_again_over_a_save_made_while_it_worked(folder, monkeypatch):
    project = _bev()
    storage.save_project(project)
    server = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    load = Engine.load
    raced = []

    def load_then_window_saves(self, ref):
        loaded = load(self, ref)
        if not raced:  # the window saves its own change after the agent read the file
            raced.append(1)
            window = storage.load_project("car")
            window.name = "Renamed in the window"
            storage.save_project(window, storage.current_revision("car"))
        return loaded

    monkeypatch.setattr(Engine, "load", load_then_window_saves)
    _edit(server, [_set("Vehicle", "mass_kg", 2100)])
    doc = json.loads(storage.project_path("car").read_bytes())
    assert doc["name"] == "Renamed in the window", "the window's save is kept"
    assert _mass(doc) == 2100, "and so is the agent's edit"


def test_a_command_line_edit_is_one_line_in_the_history_the_app_reads(folder, tmp_path):
    storage.save_project(_bev())
    car = storage.project_path("car")
    env = {**os.environ, "LIGHTSIM_SESSION": "s-42", "AI_AGENT": "claude-code_test"}
    env.pop("LIGHTSIM_PROJECTS_DIR")  # a terminal does not know the app's folder
    out = subprocess.run(
        [sys.executable, "-m", "lightsim", "edit", str(car), "--set", "Vehicle.mass_kg=2050",
         "--reason", "match the datasheet", "--json"],
        cwd=BACKEND, env=env, capture_output=True, text=True, timeout=120)
    assert out.returncode == 0, out.stderr
    line = history.entries(storage.location("car").backups)[-1]
    assert (line["by"], line["tool"], line["reason"], line["session"], line["client"]) == (
        "agent", "lightsim edit", "match the datasheet", "s-42", "claude-code_test")
    assert json.loads(out.stdout)["history"] == str(folder / ".backups" / "car" / "history.jsonl")
    assert not (folder / "car.json-backups").exists(), "the history the app reads, not a second one"


def test_a_command_line_edit_of_a_file_keeps_its_history_beside_it(tmp_path):
    car = tmp_path / "team" / "car.json"
    car.parent.mkdir()
    car.write_text(_bev().model_dump_json(indent=2), encoding="utf-8")
    out = subprocess.run(
        [sys.executable, "-m", "lightsim", "edit", str(car), "--set", "Vehicle.mass_kg=2050",
         "--session", "mine"],
        cwd=BACKEND, capture_output=True, text=True, timeout=120)
    assert out.returncode == 0, out.stderr
    lines = history.entries(car.parent / "car.json-backups")
    assert len(lines) == 1 and lines[0]["session"] == "mine" and lines[0]["diff"]


def test_command_line_edits_from_separate_shells_are_one_session(folder):
    """An agent's shell tool starts a new shell for each call and names no
    session: its edits of a file still undo in one click."""
    storage.save_project(_bev())
    car = storage.project_path("car")
    env = {k: v for k, v in os.environ.items()
           if k not in ("LIGHTSIM_SESSION", "CLAUDE_CODE_SESSION_ID", "LIGHTSIM_PROJECTS_DIR")}
    for value in (2050, 2060):
        edit = [sys.executable, "-m", "lightsim", "edit", str(car),
                "--set", f"Vehicle.mass_kg={value}", "--reason", "r"]
        shell = f"import subprocess, sys; sys.exit(subprocess.call({edit!r}))"  # a new shell per call
        out = subprocess.run([sys.executable, "-c", shell],
                             cwd=BACKEND, env=env, capture_output=True, text=True, timeout=120)
        assert out.returncode == 0 and "Saved" in out.stdout, out.stderr
    lines = [e for e in history.entries(storage.location("car").backups) if e["by"] == "agent"]
    assert len(lines) == 2 and lines[0]["session"] == lines[1]["session"]
    assert len(storage.undo("car", session=lines[-1]["session"])["undid"]) == 2
    assert _mass(json.loads(car.read_bytes())) == 1927


def test_a_command_line_edit_keeps_an_app_save_made_while_it_worked(folder, monkeypatch):
    from app.ai import edits
    from lightsim import cli

    storage.save_project(_bev())
    car = storage.project_path("car")
    real = edits.apply_operations
    raced = []

    def apply_then_the_app_saves(model, ops):
        out = real(model, ops)
        if not raced:
            raced.append(1)
            window = storage.load_project("car")
            window.name = "Renamed in the window"
            storage.save_project(window, storage.current_revision("car"))
        return out

    monkeypatch.setattr(edits, "apply_operations", apply_then_the_app_saves)
    assert cli.main(["edit", str(car), "--set", "Vehicle.mass_kg=2050", "--reason", "r"]) == 0
    doc = json.loads(car.read_bytes())
    assert doc["name"] == "Renamed in the window" and _mass(doc) == 2050


# -- the window: rebase, history, undo ---------------------------------------------


def test_the_window_lays_an_agent_edit_over_its_unsaved_change(folder):
    rev = storage.save_project(_bev())
    window = storage.load_project("car").model_dump(mode="json")
    window["name"] = "Unsaved name in the window"
    server = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    done = _edit(server, [_set("Vehicle", "mass_kg", 2100)], reason="heavier")
    with TestClient(app) as client:
        r = client.post("/api/projects/car/rebase", json={"revision": rev, "project": window})
        assert r.status_code == 200, r.text
        merged = r.json()
        assert merged["revision"] == done["revision"]
        assert merged["project"]["name"] == "Unsaved name in the window"
        assert _mass(merged["project"]) == 2100
        assert [e["reason"] for e in merged["entries"]] == ["heavier"] and not merged["conflicts"]
        # saved with the new revision: neither change is lost, and no 409
        saved = client.put("/api/projects/car", json={**merged["project"],
                                                      "revision": merged["revision"]})
        assert saved.status_code == 200, saved.text
    doc = json.loads(storage.project_path("car").read_bytes())
    assert doc["name"] == "Unsaved name in the window" and _mass(doc) == 2100


def test_a_value_both_changed_takes_the_agent_save_and_is_listed(folder):
    rev = storage.save_project(_bev())
    window = storage.load_project("car").model_dump(mode="json")
    _vehicle(window)["parameterOverrides"]["mass_kg"] = 1500
    server = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    _edit(server, [_set("Vehicle", "mass_kg", 2100)])
    with TestClient(app) as client:
        merged = client.post("/api/projects/car/rebase",
                             json={"revision": rev, "project": window}).json()
    assert _mass(merged["project"]) == 2100
    assert any(c.endswith("mass_kg") for c in merged["conflicts"])


def test_a_change_from_outside_lightsim_is_not_rebased(folder):
    rev = storage.save_project(_bev())
    path = storage.project_path("car")
    pulled = json.loads(path.read_bytes())
    pulled["name"] = "Changed by a git pull"
    path.write_text(json.dumps(pulled, indent=2), encoding="utf-8")
    with TestClient(app) as client:
        r = client.post("/api/projects/car/rebase",
                        json={"revision": rev, "project": storage.load_project("car")
                              .model_dump(mode="json")})
    assert r.status_code == 409


def test_one_call_undoes_the_agent_last_change_and_leaves_the_window_change(folder):
    storage.save_project(_bev())
    server = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    _edit(server, [_set("Vehicle", "mass_kg", 2100)], reason="heavier")
    window = storage.load_project("car")
    window.name = "Saved by the window after the agent"
    storage.save_project(window, storage.current_revision("car"))
    with TestClient(app) as client:
        listed = client.get("/api/projects/car/history").json()
        agent = next(e for e in listed if e["by"] == "agent")
        assert agent["reason"] == "heavier" and agent["undoable"] and "diff" not in agent
        r = client.post("/api/projects/car/undo", json={"entry": agent["id"]})
        assert r.status_code == 200, r.text
        assert r.json()["undid"] == [agent["id"]] and not r.json()["conflicts"]
        doc = json.loads(storage.project_path("car").read_bytes())
        assert _mass(doc) == 1927 and doc["name"] == "Saved by the window after the agent"
        again = client.get("/api/projects/car/history").json()
        assert again[0]["tool"] == "undo" and again[0]["undoes"] == [agent["id"]]
        assert next(e for e in again if e["id"] == agent["id"])["undone"] is True
        assert client.post("/api/projects/car/undo", json={"entry": agent["id"]}).status_code == 404


def test_one_call_undoes_a_whole_agent_session(folder):
    storage.save_project(_bev())
    server = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    _edit(server, [_set("Vehicle", "mass_kg", 2100)])
    _edit(server, [{"op": "add", "type": "mech.brake", "label": "Extra brake"}])
    _edit(server, [_set("Vehicle", "mass_kg", 2300)])
    other = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    _edit(other, [{"op": "set_case", "case": storage.load_project("car").cases[0].id,
                   "name": "Kept case name"}])
    with TestClient(app) as client:
        r = client.post("/api/projects/car/undo", json={"session": server.session})
    assert r.status_code == 200 and len(r.json()["undid"]) == 3
    doc = json.loads(storage.project_path("car").read_bytes())
    assert _mass(doc) == 1927
    assert not any(e["label"] == "Extra brake" for s in doc["systems"] for e in s["elements"])
    assert doc["cases"][0]["name"] == "Kept case name", "another session's change stays"


def test_undo_leaves_a_value_changed_again_since(folder):
    storage.save_project(_bev())
    server = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    _edit(server, [_set("Vehicle", "mass_kg", 2100), _set("Vehicle", "cd", 0.3)])
    window = storage.load_project("car")
    next(e for s in window.systems for e in s.elements
         if e.label == "Vehicle").parameterOverrides["mass_kg"] = 1800
    storage.save_project(window, storage.current_revision("car"))
    entry = next(e for e in storage.project_history("car") if e["by"] == "agent")
    done = storage.undo("car", entry=entry["id"])
    doc = json.loads(storage.project_path("car").read_bytes())
    assert _mass(doc) == 1800, "the window's later value stays"
    assert _vehicle(doc)["parameterOverrides"].get("cd") != 0.3, "the rest is undone"
    assert any(c.endswith("mass_kg") for c in done["conflicts"])


def test_undoing_a_remove_restores_the_exact_file(folder):
    """A removed part, its wires and its bus links come back where they were."""
    storage.save_project(_bev())
    path = storage.project_path("car")
    before = json.loads(path.read_bytes())
    server = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    _edit(server, [{"op": "remove", "element": "el-brake-rl"}], reason="drop it")
    entry = next(e for e in history.entries(storage.location("car").backups) if e["by"] == "agent")
    removed = [c for c in entry["diff"] if "new" not in c]
    assert len({str(c["path"][:-1]) for c in removed}) >= 3, "a part, its wires and its bus links"
    storage.undo("car", entry=entry["id"])
    assert json.loads(path.read_bytes()) == before


def test_undoing_an_undo_puts_the_change_back_in_force(folder):
    storage.save_project(_bev())
    server = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    _edit(server, [_set("Vehicle", "mass_kg", 2100)], reason="heavier")
    agent = next(e for e in storage.project_history("car") if e["by"] == "agent")
    storage.undo("car", entry=agent["id"])
    storage.undo("car", entry=storage.project_history("car")[0]["id"])  # undo the undo
    assert _mass(json.loads(storage.project_path("car").read_bytes())) == 2100
    listed = {e["id"]: e for e in storage.project_history("car")}
    assert listed[agent["id"]]["undone"] is False, "the agent's change is in force again"
    storage.undo("car", session=agent["session"])  # and can be undone again
    assert _mass(json.loads(storage.project_path("car").read_bytes())) == 1927


def test_the_window_never_takes_an_agent_save_for_an_outside_change(folder):
    """A save replaces the file before it adds its history line: a rebase
    reading between the two must not refuse (the window would offer a reload)."""
    import threading

    project = _bev()
    project.description = ""
    storage.save_project(project)
    stop = threading.Event()

    def agent():
        i = 0
        while not stop.is_set():
            loaded = storage.load_project_file("car")
            loaded.project.description += f"{i};"
            try:
                storage.save_project(loaded.project, loaded.revision, meta={"by": "agent"})
            except storage.ConflictError:
                pass
            i += 1

    t = threading.Thread(target=agent)
    t.start()
    refused = []
    try:
        for _ in range(300):
            loaded = storage.load_project_file("car")
            try:
                storage.rebase("car", loaded.revision, loaded.project.model_dump(mode="json"))
            except storage.ConflictError as e:
                refused.append(str(e))
    finally:
        stop.set()
        t.join()
    assert not refused, refused[:1]


# -- the app and an agent's process at the same time --------------------------------


def _hammer(projects: str, key: str, n: int) -> None:
    """A process that reads, edits and saves the project n times, the way
    model_edit does (applied again on a conflict)."""
    os.environ["LIGHTSIM_PROJECTS_DIR"] = projects
    from app import storage as st

    for i in range(n):
        while True:
            loaded = st.load_project_file("car")
            car = loaded.project
            car.description = (car.description or "") + f"{key}{i};"
            try:
                st.save_project(car, loaded.revision)
                break
            except st.ConflictError:
                continue


def test_two_processes_saving_at_once_lose_nothing(folder):
    project = _bev()
    project.description = ""
    storage.save_project(project)
    ctx = multiprocessing.get_context("spawn")
    procs = [ctx.Process(target=_hammer, args=(str(folder), k, 15)) for k in ("a", "b", "c")]
    for p in procs:
        p.start()
    for p in procs:
        p.join(120)
        assert p.exitcode == 0
    text = storage.load_project("car").description
    for k in "abc":
        assert all(f"{k}{i};" in text for i in range(15)), (k, text)
    lines = history.entries(storage.location("car").backups)
    assert len(lines) == 46 and history.chain(lines, lines[1]["from"], lines[-1]["to"])


def test_undoing_a_remove_keeps_the_order_of_a_cases_overrides(folder):
    """A case overriding the removed part: the file after the undo is the
    same bytes, not only the same JSON (the dict keys keep their places)."""
    project = _bev()
    case = project.cases[0]
    case.parameterOverrides = {"el-brake-rl": {"a": 1}, "el-brake-fr": {"a": 2}, "el-x": {"a": 3}}
    storage.save_project(project)
    path = storage.project_path("car")
    before = path.read_bytes()
    server = McpServer(Engine(user_folder=folder), Policy(), AuditLog(folder))
    _edit(server, [{"op": "remove", "element": "el-brake-rl"}], reason="drop it")
    entry = next(e for e in history.entries(storage.location("car").backups) if e["by"] == "agent")
    storage.undo("car", entry=entry["id"])
    assert path.read_bytes() == before


def test_a_rebase_of_a_git_pull_in_a_read_only_folder_still_says_conflict(folder):
    """No backups folder, and none can be made: the window must get the 409
    (the reload offer), not a PermissionError."""
    import os

    if os.name == "nt" or os.geteuid() == 0:
        pytest.skip("needs a folder the user cannot write to")
    project = _bev()
    rev = storage.save_project(project)
    backups = storage.location("car").backups
    shutil.rmtree(backups)
    path = storage.project_path("car")
    path.write_text(path.read_text(encoding="utf-8").replace("\"name\"", "\"name\"", 1) + "\n",
                    encoding="utf-8")
    folders = {path.parent, backups.parent}
    for f in folders:
        f.chmod(0o500)
    try:
        with pytest.raises(storage.ConflictError):
            storage.rebase("car", rev, project.model_dump(mode="json"))
    finally:
        for f in folders:
            f.chmod(0o700)
