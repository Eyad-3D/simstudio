"""The app's side of the AI connection: Copy for AI (AI-30) and Connect an
AI assistant (AI-29)."""
from __future__ import annotations

import json

import pytest
from fastapi.testclient import TestClient

from app import storage
from app.ai.overview import TARGET_BYTES
from app.main import app
from app.solver import simulate

client = TestClient(app)


@pytest.mark.parametrize("example", ["bev-car", "hybrid-car", "fs-electric"])
def test_copy_for_ai_summarises_each_example_under_8_kb(example):
    project = storage.load_example(example)
    case = project.cases[0]
    result = simulate(project, case.id) if example == "bev-car" else None
    run = None if result is None else {
        "caseName": case.name, "status": result.status,
        "summary": [s.model_dump() for s in result.summary],
        "messages": [m.model_dump() for m in result.messages]}
    reply = client.post("/api/ai/overview", json={"project": project.model_dump(mode="json"), "run": run})
    assert reply.status_code == 200
    body = reply.json()
    assert body["bytes"] == len(body["text"].encode("utf-8")) < TARGET_BYTES
    text = body["text"]
    for part in ("## Parts and wiring", "## Values changed from the library defaults", "## Cases",
                 "## Last run", "## Data Checks", "not validated", "KNOWN-LIMITS.md"):
        assert part in text
    assert project.name in text
    if run:
        assert "final SOC: 88.76 %" in text and "Consumption: 11.12 kWh/100km" in text
    else:
        assert "No run yet." in text


def test_copy_for_ai_hides_values_when_asked():
    project = storage.load_example("bev-car")
    body = {"project": project.model_dump(mode="json"), "hideValues": True,
            "run": {"caseName": "City Cycle", "status": "success",
                    "summary": [{"label": "Consumption", "value": 11.12, "unit": "kWh/100km"}]}}
    text = client.post("/api/ai/overview", json=body).json()["text"]
    assert "1927" not in text and "1,927" not in text and "11.12" not in text
    assert "Vehicle Mass [hidden] kg" in text and "Consumption: [hidden] kWh/100km" in text
    assert "Description:" in text and "62 kWh" not in text


def test_copy_for_ai_is_the_mcp_overview(tmp_path, monkeypatch):
    """One code path: the button and the MCP tool give the same text."""
    from app.ai.access import AuditLog, Policy
    from app.ai.engine import Engine
    from app.ai.tools import CallContext, Tools

    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path))
    project = storage.load_example("fs-electric")
    via_app = client.post("/api/ai/overview", json={"project": project.model_dump(mode="json")}).json()
    tools = Tools(Engine(user_folder=tmp_path), Policy(), AuditLog(tmp_path))
    via_mcp = tools.call("lightsim_overview", {"project": "example:fs-electric"}, CallContext())
    assert via_mcp["markdown"].replace(" · project `example:fs-electric`", "") == via_app["text"]


@pytest.fixture
def home(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("USERPROFILE", str(tmp_path))
    monkeypatch.setenv("APPDATA", str(tmp_path / "AppData" / "Roaming"))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path / ".config"))
    monkeypatch.setenv("CODEX_HOME", str(tmp_path / ".codex"))
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path / "projects"))
    return tmp_path


def test_connect_lists_the_ai_apps_and_installs_with_one_click(home):
    state = client.get("/api/ai/connect").json()
    assert {c["id"] for c in state["clients"]} >= {"claude", "vscode", "codex", "gemini", "copilot"}
    assert not any(c["installed"] for c in state["clients"])
    assert state["command"][-1] == "mcp" and state["lastUsed"] is None

    state = client.put("/api/ai/connect/claude").json()
    claude = next(c for c in state["clients"] if c["id"] == "claude")
    assert claude["installed"]
    entry = json.loads(open(claude["configPath"], encoding="utf-8").read())["mcpServers"]["lightsim"]
    assert entry["args"][-2:] == ["--projects-dir", str(home / "projects")]

    state = client.delete("/api/ai/connect/claude").json()
    assert not next(c for c in state["clients"] if c["id"] == "claude")["installed"]
    assert client.put("/api/ai/connect/nope").status_code == 404


def test_connect_refuses_a_settings_file_it_cannot_read(home):
    state = client.get("/api/ai/connect").json()
    path = next(c["configPath"] for c in state["clients"] if c["id"] == "gemini")
    import pathlib

    pathlib.Path(path).parent.mkdir(parents=True)
    pathlib.Path(path).write_text("{ not json", encoding="utf-8")
    reply = client.put("/api/ai/connect/gemini")
    assert reply.status_code == 409 and "not valid JSON" in reply.json()["detail"]
    assert pathlib.Path(path).read_text(encoding="utf-8") == "{ not json"
