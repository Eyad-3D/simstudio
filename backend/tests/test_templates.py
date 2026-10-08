"""CON-18: vehicle templates with fixed slots and a short form; any model
can become one."""
from __future__ import annotations

import pytest
from fastapi.testclient import TestClient

from app import templates
from app.main import app
from app.solver import simulate
from app.storage import load_example

client = TestClient(app)


@pytest.fixture
def user_dir(tmp_path, monkeypatch):
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path / "projects"))
    return tmp_path / "projects"


def test_built_in_templates_have_slots_and_a_form_that_match_their_example(user_dir):
    listed = {t["id"]: t for t in client.get("/api/templates").json()}
    assert {"bev-one-motor", "p2-hybrid", "fs-electric"} <= set(listed)
    for tid in ("bev-one-motor", "p2-hybrid", "fs-electric"):
        t = templates.get(tid)
        assert t.builtin and t.slots and t.form and "project" not in listed[tid]
        model = templates.model_of(t)
        templates.check(t, model)
        els = {e.id: e for s in model.systems for e in s.elements}
        for f in t.form:  # the form starts from the example's own values
            assert els[f.elementId].parameterOverrides[f.key] == f.default, (tid, f.key)


def test_a_new_project_takes_the_forms_values_and_records_its_template(user_dir):
    r = client.post("/api/templates/bev-one-motor/new",
                    json={"values": {"0": 1600, "el-battery.capacity_kWh": 50}, "name": "My EV"})
    assert r.status_code == 200
    project = r.json()
    assert project["name"] == "My EV" and project["template"] == {
        "id": "bev-one-motor", "name": "Electric car, one motor", "version": 1}
    els = {e["id"]: e for s in project["systems"] for e in s["elements"]}
    assert els["el-vehicle"]["parameterOverrides"]["mass_kg"] == 1600
    assert els["el-battery"]["parameterOverrides"]["capacity_kWh"] == 50
    assert els["el-vehicle"]["parameterOverrides"]["cd"] == 0.27  # not asked: as in the template
    # a value outside the form's limits is refused
    bad = client.post("/api/templates/bev-one-motor/new", json={"values": {"0": 50}})
    assert bad.status_code == 400 and "Test mass" in bad.json()["detail"]


def test_any_model_becomes_a_template_and_makes_new_projects(user_dir):
    project = load_example("hybrid-car")
    body = {"project": project.model_dump(mode="json"), "name": "Lecture 3 hybrid",
            "description": "Change the final drive and compare fuel use.",
            "form": [{"elementId": "el-fd", "key": "ratio", "label": "Final drive", "default": 3.38,
                      "minimum": 2, "maximum": 5}],
            "slots": {"Engine": "el-engine", "Driveline": "el-fd"}}
    saved = client.post("/api/templates", json=body).json()
    assert saved["id"] == "user-lecture-3-hybrid" and saved["version"] == 1 and not saved["builtin"]
    assert (user_dir / "templates" / "user-lecture-3-hybrid.json").is_file()
    assert client.post("/api/templates", json=body).json()["version"] == 2  # saved again
    new = client.post("/api/templates/user-lecture-3-hybrid/new", json={"values": {"0": 4.1}}).json()
    fd = next(e for s in new["systems"] for e in s["elements"] if e["id"] == "el-fd")
    assert fd["parameterOverrides"]["ratio"] == 4.1 and new["template"]["version"] == 2
    # a slot or a form field that names nothing is refused
    body["slots"] = {"Engine": "el-nothing"}
    assert client.post("/api/templates", json=body).status_code == 400
    assert client.delete("/api/templates/user-lecture-3-hybrid").status_code == 200
    assert client.delete("/api/templates/bev-one-motor").status_code == 404  # built-ins stay


def test_a_project_from_a_template_runs(user_dir):
    from app.schemas import Project
    project = Project.model_validate(templates.instantiate("bev-one-motor", {"0": 1700}).model_dump())
    case = next(c for c in project.cases if c.id == "case-city")
    case.duration = 60
    assert simulate(project, "case-city").status == "success"
