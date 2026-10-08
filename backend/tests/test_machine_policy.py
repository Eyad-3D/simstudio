"""The machine-wide policy file as the engine reads it (PLT-36): the same
keys and checks as the desktop app, read by an engine an AI app starts, and
its "ai": "off" honoured by every AI path."""
from __future__ import annotations

import json
import sys

import pytest
from conftest import allow_ai
from fastapi.testclient import TestClient

from app import machine_policy, script_trust
from app.ai.engine import Engine
from app.main import app
from lightsim.ai_access import AccessDenied, AgentSession, Policy, cli_ai
from lightsim.project import LightSimError


@pytest.fixture
def policy_file(tmp_path, monkeypatch):
    """A policy file on this 'computer', and no LIGHTSIM_POLICY."""
    path = tmp_path / "policy.json"
    monkeypatch.setattr(machine_policy, "policy_path", lambda platform=None: path)
    monkeypatch.delenv("LIGHTSIM_POLICY", raising=False)

    def write(settings: dict) -> None:
        path.write_text(json.dumps(settings), encoding="utf-8")
    return write


def test_only_known_keys_with_valid_values_are_kept():
    raw = {"ai": "off", "scriptTrust": "sometimes", "examples": 0, "updates": "notify",
           "projectsRoots": ["P:\\lab", ""], "licenceFile": " ", "colour": "red"}
    assert machine_policy.validate(raw) == {"ai": "off", "updates": "notify"}
    assert machine_policy.validate(["ai", "off"]) == {}


def test_without_the_desktop_app_the_engine_reads_the_file(policy_file, monkeypatch):
    assert machine_policy.settings() == {}
    policy_file({"scriptTrust": "always-prompt", "examples": False})
    assert script_trust.policy() == {"scriptTrust": "always-prompt", "examples": False}
    monkeypatch.delenv("LIGHTSIM_SCRIPT_TRUST")  # the suite's switch (conftest.py)
    assert script_trust.mode() == "always-prompt"


def test_a_file_with_a_byte_order_mark_or_bad_json(policy_file, tmp_path):
    (tmp_path / "policy.json").write_text('\ufeff{"ai": "off"}', encoding="utf-8")
    assert machine_policy.ai_off()
    (tmp_path / "policy.json").write_text('{"ai": "off"', encoding="utf-8")
    assert machine_policy.settings() == {}


def test_a_packaged_engine_ignores_lightsim_policy_from_its_environment(policy_file, monkeypatch):
    # an AI app starts the engine with the user's environment: it must not
    # be able to clear what the organisation set
    policy_file({"ai": "off"})
    monkeypatch.setenv("LIGHTSIM_POLICY", "{}")
    assert not machine_policy.ai_off()  # a source checkout: the desktop app's value
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    assert machine_policy.ai_off()


def test_ai_off_stops_every_ai_path(policy_file, tmp_path, monkeypatch):
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path / "projects"))
    allow_ai(tmp_path)  # the user turned AI access on
    policy_file({"ai": "off"})

    policy = Policy.load()
    assert policy.enabled and policy.managed_off and not policy.on()
    with pytest.raises(AccessDenied, match="organisation"):
        AgentSession().list_projects()
    assert Engine(user_folder=tmp_path).access_refusal() == machine_policy.AI_OFF

    class Args:
        ai_command = "on"
    with pytest.raises(LightSimError, match="organisation"):
        cli_ai(Args(), lambda *a: None)

    client = TestClient(app)
    assert client.get("/api/ai/connect").json()["managed"] == machine_policy.AI_OFF
    reply = client.put("/api/ai/connect/claude")
    assert reply.status_code == 403 and "organisation" in reply.json()["detail"]

    policy_file({"ai": "mcp-only"})  # LightSim has no in-app AI: MCP is all there is
    assert Policy.load().on() and Engine(user_folder=tmp_path).access_refusal() is None
