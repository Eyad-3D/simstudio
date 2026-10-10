"""Scripts run only once this user has approved their code (PLT-35)."""
import copy
import json
import sys

import pytest
from fastapi.testclient import TestClient

from app import machine_policy, script_trust
from app.main import app

client = TestClient(app)


@pytest.fixture
def trust(tmp_path, monkeypatch):
    """The check switched on, with an empty projects folder of its own."""
    monkeypatch.delenv("LIGHTSIM_SCRIPT_TRUST", raising=False)
    monkeypatch.delenv("LIGHTSIM_POLICY", raising=False)
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path))
    script_trust._reset_for_tests()
    yield tmp_path
    script_trust._reset_for_tests()


def hybrid():
    return client.get("/api/examples/hybrid-car").json()


def foreign(project):
    """The hybrid example as someone else changed it: one script edited."""
    p = copy.deepcopy(project)
    for system in p["systems"]:
        for el in system["elements"]:
            if el["componentDefId"] == "signal.script":
                el["parameterOverrides"]["code"] += "\n# changed by someone else\n"
                return p, el["label"]
    raise AssertionError("the hybrid example has no Script block")


def short_case(project):
    case = {**project["cases"][0], "duration": 5, "realtimeFactor": 0}
    project["cases"] = [case]
    return case["id"]


def test_examples_and_the_default_code_are_trusted(trust):
    report = client.post("/api/scripts/check", json={"project": hybrid()}).json()
    assert report["mode"] == "prompt"
    assert report["scripts"] and report["unapproved"] == 0
    assert all(s["approved"] for s in report["scripts"])

    # a new Script block with the library's default code
    default = script_trust._default_code()
    assert script_trust.is_approved(default)


def test_code_from_elsewhere_never_runs_before_approval(trust):
    project, label = foreign(hybrid())
    case_id = short_case(project)

    report = client.post("/api/scripts/check", json={"project": project}).json()
    assert report["unapproved"] == 1
    bad = [s for s in report["scripts"] if not s["approved"]]
    assert bad[0]["label"] == label and "changed by someone else" in bad[0]["code"]

    # every way of running refuses it: REST ...
    result = client.post("/api/simulate", json={"project": project, "caseId": case_id}).json()
    assert result["status"] == "failed"
    assert any("not been approved" in m["text"] and label in m["text"] for m in result["messages"])
    assert result["channels"] == []
    # ... and the live run
    with client.websocket_connect("/api/simulate/run") as ws:
        ws.send_json({"type": "start", "project": project, "caseId": case_id})
        while (msg := ws.receive_json())["type"] != "done":
            pass
    assert msg["result"]["status"] == "failed"

    # the user reviews it and chooses Run scripts
    code = bad[0]["code"]
    assert client.post("/api/scripts/approve", json={"codes": [code]}).json()["approved"] == [
        script_trust.code_hash(code)]
    result = client.post("/api/simulate", json={"project": project, "caseId": case_id}).json()
    assert result["status"] != "failed", result["messages"]

    # remembered on disk, for that exact code only
    saved = json.loads((trust / ".script-trust.json").read_text(encoding="utf-8"))
    assert script_trust.code_hash(code) in saved["approved"]
    script_trust._reset_for_tests()
    assert script_trust.is_approved(code)
    assert not script_trust.is_approved(code + " ")
    # the same code saved with Windows line ends is the same code
    assert script_trust.is_approved(code.replace("\n", "\r\n"))


def test_a_case_cannot_slip_in_other_code(trust):
    project = hybrid()
    case_id = short_case(project)
    script = next(el for s in project["systems"] for el in s["elements"]
                  if el["componentDefId"] == "signal.script")
    project["cases"][0]["parameterOverrides"] = {script["id"]: {"code": "def step(t, dt, i, s, p):\n    return {}\n# x"}}
    assert client.post("/api/scripts/check", json={"project": project}).json()["unapproved"] == 1
    result = client.post("/api/simulate", json={"project": project, "caseId": case_id}).json()
    assert result["status"] == "failed"


def test_projects_already_saved_here_count_as_approved(trust):
    project, _ = foreign(hybrid())
    (trust / "mine.json").write_text(json.dumps(project))
    assert client.post("/api/scripts/check", json={"project": project}).json()["unapproved"] == 0
    # decided once: a project saved later is not approved by being saved
    other, _ = foreign(project)
    (trust / "later.json").write_text(json.dumps(other))
    script_trust._reset_for_tests()
    assert client.post("/api/scripts/check", json={"project": other}).json()["unapproved"] == 1


def test_deleting_the_approvals_forgets_them_and_does_not_rescan(trust):
    """The help says: delete .script-trust.json to be asked again about every
    script. That must not count the projects in the folder as approved
    again, such as one from someone else opened without running its
    scripts and then saved."""
    project, _ = foreign(hybrid())
    script_trust.trusted_hashes()  # the first start: nothing saved yet
    assert (trust / ".script-trust-migrated").exists()
    (trust / "from-someone.json").write_text(json.dumps(project))  # opened, then saved
    assert client.post("/api/scripts/check", json={"project": project}).json()["unapproved"] == 1
    (trust / ".script-trust.json").unlink()
    script_trust._reset_for_tests()  # a new launch
    assert client.post("/api/scripts/check", json={"project": project}).json()["unapproved"] == 1
    # an approval made afterwards is remembered as before
    code = next(s["code"] for s in client.post("/api/scripts/check", json={"project": project})
                .json()["scripts"] if not s["approved"])
    client.post("/api/scripts/approve", json={"codes": [code]})
    script_trust._reset_for_tests()
    assert client.post("/api/scripts/check", json={"project": project}).json()["unapproved"] == 0


def test_approvals_are_the_users_own_not_the_projects_folders(tmp_path, monkeypatch):
    """Approvals live in the user's own LightSim folder. A projects folder
    elsewhere (a policy's projectsRoots on a shared lab drive) is never
    scanned, and an approvals file anyone can write there means nothing."""
    monkeypatch.delenv("LIGHTSIM_SCRIPT_TRUST", raising=False)
    monkeypatch.delenv("LIGHTSIM_POLICY", raising=False)
    own, shared = tmp_path / "userData", tmp_path / "lab-drive"
    shared.mkdir()
    monkeypatch.setenv("LIGHTSIM_DATA_DIR", str(own))
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(shared))
    script_trust._reset_for_tests()
    try:
        project, _ = foreign(hybrid())
        (shared / "theirs.json").write_text(json.dumps(project))
        code = next(s["code"] for s in script_trust.project_scripts(project)
                    if "someone else" in s["code"])
        (shared / ".script-trust.json").write_text(
            json.dumps({"approved": [script_trust.code_hash(code)]}))
        assert not script_trust.is_approved(code)
        assert (own / ".script-trust-migrated").exists()
        script_trust.approve([code])
        assert script_trust.code_hash(code) in json.loads(
            (own / ".script-trust.json").read_text(encoding="utf-8"))["approved"]
    finally:
        script_trust._reset_for_tests()


def test_a_packaged_engine_ignores_the_test_switch(trust, monkeypatch):
    """LIGHTSIM_SCRIPT_TRUST=off is for the test suite: in the packaged app a
    user's own environment variable must not turn the check (or the
    organisation's always-prompt policy) off."""
    monkeypatch.setenv("LIGHTSIM_SCRIPT_TRUST", "off")
    assert script_trust.mode() == "off"
    monkeypatch.setenv("LIGHTSIM_DATA_DIR", str(trust))  # as the desktop shell sets it
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    assert script_trust.mode() == "prompt"
    # a packaged engine reads the policy file itself (app/machine_policy.py)
    policy = trust / "policy.json"
    policy.write_text(json.dumps({"scriptTrust": "always-prompt"}), encoding="utf-8")
    monkeypatch.setattr(machine_policy, "policy_path", lambda platform=None: policy)
    assert script_trust.mode() == "always-prompt"
    project, _ = foreign(hybrid())
    assert client.post("/api/scripts/check", json={"project": project}).json()["unapproved"] == 1


def test_always_prompt_forgets_approvals_when_lightsim_closes(trust, monkeypatch):
    monkeypatch.setenv("LIGHTSIM_POLICY", json.dumps({"scriptTrust": "always-prompt"}))
    project, _ = foreign(hybrid())
    code = next(s["code"] for s in client.post("/api/scripts/check", json={"project": project}).json()["scripts"]
                if not s["approved"])
    client.post("/api/scripts/approve", json={"codes": [code]})
    assert script_trust.is_approved(code)
    assert not (trust / ".script-trust.json").exists()
    script_trust._reset_for_tests()  # a new launch
    assert not script_trust.is_approved(code)
    assert client.get("/api/policy").json() == {
        "settings": {"scriptTrust": "always-prompt"}, "scriptTrust": "always-prompt"}


def test_the_policy_can_leave_out_the_examples(trust, monkeypatch):
    assert client.get("/api/examples").json()
    monkeypatch.setenv("LIGHTSIM_POLICY", json.dumps({"examples": False}))
    assert client.get("/api/examples").json() == []


def test_bad_requests(trust):
    assert client.post("/api/scripts/check", json={}).status_code == 422
    assert client.post("/api/scripts/approve", json={"codes": [1]}).status_code == 422
    # a malformed project is scanned, not crashed on
    assert client.post("/api/scripts/check", json={"project": {"systems": [1, {"elements": "x"}]}}).json() == {
        "mode": "prompt", "scripts": [], "unapproved": 0}


def test_the_users_own_folder(monkeypatch, tmp_path):
    from app import paths

    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path / "projects"))
    monkeypatch.delenv("LIGHTSIM_DATA_DIR", raising=False)
    assert paths.data_dir() == tmp_path / "projects"  # development and tests
    monkeypatch.setenv("LIGHTSIM_DATA_DIR", str(tmp_path / "userData"))
    assert paths.data_dir() == tmp_path / "userData"  # set by the desktop shell
    if sys.platform.startswith("linux"):
        # a packaged engine an AI app starts: Electron's userData folder
        monkeypatch.delenv("LIGHTSIM_DATA_DIR")
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path / "config"))
        monkeypatch.setattr(sys, "frozen", True, raising=False)
        assert paths.data_dir() == tmp_path / "config" / "LightSim"
