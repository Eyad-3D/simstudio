"""The app's AI access page (AI-01): GET/PUT /api/ai/access and POST
/api/ai/access/folders read and write the same settings file as `lightsim
ai …`, add a folder only from the desktop shell (the page never names paths,
PLT-33), take trust away but never give it, list both audit logs, and keep
access off when the organisation's policy (PLT-36) says so. The run time cap
set there also caps an AI app's runs."""
from __future__ import annotations

import json
import shutil
from pathlib import Path

import pytest
from conftest import allow_ai
from fastapi.testclient import TestClient

import lightsim as ls
from app import machine_policy
from app.ai.access import AuditLog
from app.ai.access import Policy as ServerPolicy
from app.ai.engine import Engine
from app.ai.tools import CallContext, Tools
from app.main import app
from lightsim.ai_access import AgentSession, Policy, script_fingerprint

EXAMPLES = Path(__file__).parent.parent / "projects"
client = TestClient(app)


@pytest.fixture
def projects(tmp_path, monkeypatch):
    folder = tmp_path / "projects"
    folder.mkdir()
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(folder))
    monkeypatch.delenv("LIGHTSIM_DEV_FILE_PATHS", raising=False)
    monkeypatch.delenv("LIGHTSIM_SHELL_TOKEN", raising=False)
    return folder


def test_the_page_shows_the_settings_off_by_default(projects):
    body = client.get("/api/ai/access").json()
    assert body["enabled"] is False and body["on"] is False and body["managed"] is None
    assert body["folders"] == [] and body["trusted"] == [] and body["audit"] == []
    assert body["examples"] is True and body["maxRunSeconds"] == 300
    assert body["projectsFolder"] == str(projects.resolve())
    assert body["settingsPath"] == str(Policy.load().path)


def test_changes_land_in_the_file_the_command_line_reads(projects, capsys):
    from lightsim.cli import main

    reply = client.put("/api/ai/access", json={"enabled": True, "examples": False, "maxRunSeconds": 60})
    assert reply.status_code == 200
    body = reply.json()
    assert body["enabled"] is True and body["on"] is True and body["examples"] is False
    assert body["maxRunSeconds"] == 60
    policy = Policy.load()
    assert policy.on() and not policy.examples and policy.max_run_s == 60
    capsys.readouterr()
    assert main(["ai", "status", "--json"]) == 0
    status = json.loads(capsys.readouterr().out)
    assert status["enabled"] is True and status["examples"] is False and status["maxRunSeconds"] == 60
    # and the other way round: the command line's change shows on the page
    assert main(["ai", "off"]) == 0
    assert client.get("/api/ai/access").json()["enabled"] is False
    # a field left out is left as it is
    assert client.put("/api/ai/access", json={}).json()["maxRunSeconds"] == 60


@pytest.mark.parametrize("seconds", [0, -5, 86401])
def test_the_run_time_cap_must_be_sensible(projects, seconds):
    assert client.put("/api/ai/access", json={"maxRunSeconds": seconds}).status_code == 422
    assert Policy.load().max_run_s == 300


def test_only_the_desktop_shell_adds_a_folder(projects, tmp_path, monkeypatch):
    team = tmp_path / "team"
    team.mkdir()
    reply = client.post("/api/ai/access/folders", json={"path": str(team)})
    assert reply.status_code == 403 and "lightsim ai allow" in reply.json()["detail"]
    assert Policy.load().folders == []

    monkeypatch.setenv("LIGHTSIM_SHELL_TOKEN", "shell-secret")
    assert client.post("/api/ai/access/folders", json={"path": str(team)},
                       headers={"X-LightSim-Shell": "guess"}).status_code == 403
    reply = client.post("/api/ai/access/folders", json={"path": str(team)},
                        headers={"X-LightSim-Shell": "shell-secret"})
    assert reply.status_code == 200
    assert [f["path"] for f in reply.json()["folders"]] == [str(team.resolve())]
    assert Policy.load().folders == [str(team.resolve())]


def test_a_folder_must_be_an_absolute_path_to_a_folder(projects, tmp_path, monkeypatch):
    monkeypatch.setenv("LIGHTSIM_DEV_FILE_PATHS", "1")  # a development engine: any local request
    for bad in ("team", "", "/tmp/a\0b"):
        assert client.post("/api/ai/access/folders", json={"path": bad}).status_code == 400, bad
    assert client.post("/api/ai/access/folders", json={"path": str(tmp_path / "nope")}).status_code == 404
    afile = tmp_path / "a.json"
    afile.write_text("{}", encoding="utf-8")
    assert client.post("/api/ai/access/folders", json={"path": str(afile)}).status_code == 404
    # twice is once; the projects folder is marked as such
    for _ in range(2):
        body = client.post("/api/ai/access/folders", json={"path": str(projects)}).json()
    assert body["folders"] == [{"path": str(projects.resolve()), "exists": True, "projects": True}]
    # taking it off needs no shell
    monkeypatch.delenv("LIGHTSIM_DEV_FILE_PATHS")
    body = client.put("/api/ai/access", json={"removeFolders": [str(projects.resolve())]}).json()
    assert body["folders"] == [] and Policy.load().folders == []


def test_trust_is_listed_and_can_be_taken_away_but_not_given(projects):
    path = projects / "hybrid-car.json"
    shutil.copy(EXAMPLES / "hybrid-car.json", path)
    project = ls.load(path)
    policy = Policy.load()
    policy.trusted[str(path.resolve())] = script_fingerprint(project)
    policy.save()
    [entry] = client.get("/api/ai/access").json()["trusted"]
    assert entry == {"path": str(path.resolve()), "exists": True, "name": project.name, "current": True}

    script = next(e for e in project.elements if e.componentDefId == "signal.script")
    script.parameterOverrides["code"] = script.parameterOverrides.get("code", "") + "\n# changed\n"
    project.save()
    [entry] = client.get("/api/ai/access").json()["trusted"]
    assert entry["current"] is False  # changed scripts: the trust no longer holds

    # the request has no way to trust a project: unknown fields are ignored
    client.put("/api/ai/access", json={"trusted": {str(path): "x"}, "trust": [str(path)]})
    assert list(Policy.load().trusted) == [str(path.resolve())]
    body = client.put("/api/ai/access", json={"untrust": [str(path.resolve()), "/not/there.json"]}).json()
    assert body["trusted"] == [] and Policy.load().trusted == {}


def test_a_trusted_file_that_is_gone_is_listed_so_it_can_be_untrusted(projects):
    policy = Policy.load()
    policy.trusted["/gone/car.json"] = "abc"
    policy.save()
    assert client.get("/api/ai/access").json()["trusted"] == [
        {"path": "/gone/car.json", "exists": False, "name": None, "current": False}]


@pytest.fixture
def policy_file(tmp_path, monkeypatch):
    path = tmp_path / "policy.json"
    monkeypatch.setattr(machine_policy, "policy_path", lambda platform=None: path)
    monkeypatch.delenv("LIGHTSIM_POLICY", raising=False)
    return lambda settings: path.write_text(json.dumps(settings), encoding="utf-8")


def test_the_organisations_policy_keeps_access_off(projects, policy_file):
    allow_ai(projects)
    policy_file({"ai": "off"})
    body = client.get("/api/ai/access").json()
    assert body["enabled"] is True and body["on"] is False and body["managed"] == machine_policy.AI_OFF
    reply = client.put("/api/ai/access", json={"enabled": True})
    assert reply.status_code == 403 and "organisation" in reply.json()["detail"]
    # turning it off, and taking access away, still work
    body = client.put("/api/ai/access", json={"enabled": False, "examples": False}).json()
    assert body["enabled"] is False and body["examples"] is False
    assert client.put("/api/ai/access", json={"enabled": True}).status_code == 403
    assert Policy.load().enabled is False


def test_the_latest_calls_from_both_audit_logs_newest_first(projects, tmp_path):
    allow_ai(projects)
    shutil.copy(EXAMPLES / "bev-car.json", projects / "bev-car.json")
    AgentSession(client="notebook").open(str(projects / "bev-car.json"))
    log = AuditLog(projects)
    log.record(tool="run_checks", project="example:bev-car", ok=True, ms=12, client="desk app")
    log.record(tool="model_edit", project="bev-car", ok=False, ms=3, client="")
    audit = client.get("/api/ai/access").json()["audit"]
    assert [(a["tool"], a["outcome"], a["via"]) for a in audit] == [
        ("model_edit", "refused or failed", "mcp"),
        ("run_checks", "ok", "mcp"),
        ("open", "ok", "python"),
    ]
    assert audit[1]["client"] == "desk app" and audit[0]["client"] is None
    assert audit[2]["project"] == str(projects / "bev-car.json") and audit[2]["client"] == "notebook"
    assert audit[0]["time"] >= audit[1]["time"] >= audit[2]["time"]


def test_a_damaged_audit_line_is_skipped(projects):
    (projects / ".ai").mkdir()
    (projects / ".ai" / "audit.jsonl").write_text(
        'not json\n[1]\n{"t": 5, "tool": "x", "ok": true}\n{"tool": "no time"}\n{"t": NaN, "tool": "y"}\n',
        encoding="utf-8")
    assert [a["tool"] for a in client.get("/api/ai/access").json()["audit"]] == ["x"]


def test_the_cap_set_on_the_page_caps_an_ai_apps_runs(projects):
    allow_ai(projects)
    engine = Engine(user_folder=projects)
    assert engine.run_cap(300) == 300
    client.put("/api/ai/access", json={"maxRunSeconds": 1})
    assert engine.run_cap(300) == 1 and engine.run_cap(0.5) == 0.5  # the shorter one
    policy = Policy.load()
    policy.max_run_s = 1e-6  # (the page asks for 1 s at least)
    policy.save()
    tools = Tools(engine, ServerPolicy(), AuditLog(projects))
    answer = tools.call("run_case", {"project": "example:bev-car", "case": "case-wltc"}, CallContext())
    assert answer["status"] == "cancelled" and "longer than 1e-06 s" in answer["incomplete"]
