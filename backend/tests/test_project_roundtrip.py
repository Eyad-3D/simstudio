"""Saving through the engine keeps everything the UI sends (PLT-01).

The engine re-serialises each project through its schema; anything the schema
did not know (node sizes, pin offsets, newer UI fields) used to vanish on save.
"""
import copy
import json

import pytest
from fastapi.testclient import TestClient

from app import storage
from app.main import app
from app.paths import EXAMPLES_DIR
from app.schemas import Project
from app.validation import validate_project

client = TestClient(app)


@pytest.fixture(autouse=True)
def _projects_dir(tmp_path, monkeypatch):
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path))


def _example(name: str) -> dict:
    return json.loads((EXAMPLES_DIR / f"{name}.json").read_text(encoding="utf-8"))


def _with_ui_fields(raw: dict) -> dict:
    """The example as the UI holds it after resizing nodes, moving pins and
    fields a newer UI version might add at every level."""
    p = copy.deepcopy(raw)
    p["id"] = "roundtrip"
    p["uiLayout"] = {"zoom": 1.25, "panels": ["results", "checks"]}
    root = p["systems"][0]
    root["collapsed"] = True
    el = root["elements"][0]
    el["size"] = {"width": 212.5, "height": 96}
    el["portOffsets"] = {"shaft": 0.25, "sig_speed": 0.8}
    el["portSides"] = {"shaft": "top"}
    el["color"] = "#ff8800"
    root["connections"][0]["route"] = [{"x": 10, "y": 20}, {"x": 30, "y": 20}]
    p["dataBusConnections"][0]["note"] = "speed feedback"
    p["cases"][0]["note"] = "hot day, check the aux load"
    p["cases"][0]["tags"] = ["baseline"]
    return p


def _roundtrip(body: dict) -> dict:
    r = client.put(f"/api/projects/{body['id']}", json=body)
    assert r.status_code == 200, r.text
    got = client.get(f"/api/projects/{body['id']}")
    assert got.status_code == 200, got.text
    data = got.json()
    data.pop("revision", None)  # save bookkeeping (PLT-04), not project content
    return data


@pytest.mark.parametrize("name", ["bev-car", "hybrid-car"])
def test_put_then_get_returns_what_the_ui_sent(name):
    # the UI holds projects in the engine's full shape (every field present)
    full = Project.model_validate(_example(name)).model_dump(mode="json")
    original = _with_ui_fields(full)
    assert _roundtrip(original) == original


@pytest.mark.parametrize("name", ["bev-car", "hybrid-car"])
def test_nothing_in_a_file_is_dropped(name):
    # a sparse file (as written by hand or an older version): every key it has
    # must come back with the same value; the engine may only add defaults
    original = _with_ui_fields(_example(name))
    got = _roundtrip(original)

    def contains(big, small, path="$"):
        if isinstance(small, dict):
            assert isinstance(big, dict), path
            for k, v in small.items():
                assert k in big, f"{path}.{k} was dropped"
                contains(big[k], v, f"{path}.{k}")
        elif isinstance(small, list):
            assert isinstance(big, list) and len(big) == len(small), path
            for i, (b, s) in enumerate(zip(big, small)):
                contains(b, s, f"{path}[{i}]")
        else:
            assert big == small, f"{path}: {small!r} came back as {big!r}"

    contains(got, original)


def test_layout_fields_are_typed():
    body = _with_ui_fields(_example("bev-car"))
    body["systems"][0]["elements"][0]["size"] = {"width": "wide", "height": 1}
    assert client.put(f"/api/projects/{body['id']}", json=body).status_code == 422
    body = _with_ui_fields(_example("bev-car"))
    body["systems"][0]["elements"][0]["portOffsets"] = {"shaft": "left"}
    assert client.put(f"/api/projects/{body['id']}", json=body).status_code == 422


def test_saved_file_keeps_the_fields(tmp_path):
    _roundtrip(_with_ui_fields(_example("bev-car")))
    on_disk = json.loads((tmp_path / "roundtrip.json").read_text(encoding="utf-8"))
    el = on_disk["systems"][0]["elements"][0]
    assert el["size"] == {"width": 212.5, "height": 96}
    assert el["portOffsets"] == {"shaft": 0.25, "sig_speed": 0.8}
    assert on_disk["uiLayout"] == {"zoom": 1.25, "panels": ["results", "checks"]}
    assert on_disk["cases"][0]["note"] == "hot day, check the aux load"
    # unknown fields ride along without changing the model the solver sees
    project = storage.load_project("roundtrip")
    assert project.model_extra == {"uiLayout": {"zoom": 1.25, "panels": ["results", "checks"]}}
    assert validate_project(project) == validate_project(
        Project.model_validate(_example("bev-car")))


def _study(study_id: str, values: list[float]) -> dict:
    """A finished one-factor study as the UI saves it (STU-03)."""
    return {
        "id": study_id,
        "startedAt": 1_758_000_000_000,
        "caseId": "case-city",
        "caseName": "City Cycle",
        "factors": [{"elementId": "el-vehicle", "paramKey": "mass_kg", "elementLabel": "Vehicle",
                     "paramLabel": "Vehicle Mass", "unit": "kg", "values": values}],
        "kpis": [{"label": "HV Battery Pack — final SOC", "unit": "%"}],
        "points": [
            {"values": [values[0]], "runId": "run-1", "status": "success",
             "kpis": {"HV Battery Pack — final SOC": 88.5}, "notValid": {}},
            {"values": [values[1]], "runId": "run-2", "status": "warning", "incomplete": "stopped at t = 12 s",
             "kpis": {"HV Battery Pack — final SOC": 97.25},
             "notValid": {"HV Battery Pack — final SOC": "cycle not followed"}},
            *({"values": [v], "status": "not run", "kpis": {}, "notValid": {}} for v in values[2:]),
        ],
    }


def test_studies_are_saved_with_the_project(tmp_path):
    """STU-03: every study and its results table come back after a save."""
    body = Project.model_validate(_example("bev-car")).model_dump(mode="json")
    body["id"] = "studies"
    assert body["studies"] == [], "a project without studies has an empty list"
    body["studies"] = [_study("sweep-1", [1500.0, 1800.0]), _study("sweep-2", [1200.0, 1400.0, 1600.0])]
    full = Project.model_validate(body).model_dump(mode="json")
    assert _roundtrip(full) == full
    assert full["studies"][1]["points"][2]["kpis"] == {}
    on_disk = json.loads((tmp_path / "studies.json").read_text(encoding="utf-8"))
    assert [len(s["points"]) for s in on_disk["studies"]] == [2, 3]
    assert on_disk["studies"][1]["points"][2]["status"] == "not run"


def test_study_tables_are_typed():
    body = _example("bev-car")
    body["id"] = "studies"
    body["studies"] = [_study("sweep-1", [1500.0, 1800.0])]
    body["studies"][0]["points"][0]["kpis"]["HV Battery Pack — final SOC"] = "high"
    assert client.put("/api/projects/studies", json=body).status_code == 422
    body["studies"] = [_study("sweep-1", [1500.0, 1800.0])]
    body["studies"][0]["points"][0]["status"] = "done"
    assert client.put("/api/projects/studies", json=body).status_code == 422
