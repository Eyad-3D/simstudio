"""Old project files are upgraded step by step; newer ones open read-only (PLT-07).

The fixture files in tests/fixtures/migrations/ are project files as older
versions wrote them (one per format version, named v<n>-*.json). Each must
load through the migrations to the current format with nothing lost.
"""
import copy
import json
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import migrations, storage
from app.main import app
from app.migrations import CURRENT_VERSION, NewerFileError, migrate
from app.schemas import Study
from app.version import VERSION

client = TestClient(app)
FIXTURES = Path(__file__).parent / "fixtures" / "migrations"
V1 = FIXTURES / "v1-with-studies.json"


@pytest.fixture(autouse=True)
def projects(tmp_path, monkeypatch):
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path))
    return tmp_path


def _v1() -> dict:
    return json.loads(V1.read_text(encoding="utf-8"))


def test_every_older_version_has_a_step_and_a_fixture():
    assert sorted(migrations.STEPS) == list(range(1, CURRENT_VERSION))
    for version in range(1, CURRENT_VERSION):
        assert list(FIXTURES.glob(f"v{version}-*.json")), f"no fixture file for format {version}"


def test_a_v1_file_migrates_with_no_data_loss():
    raw = _v1()
    before = copy.deepcopy(raw)
    out = migrate(raw)
    assert raw == before, "the input is not changed"
    assert out.from_version == 1 and out.upgraded
    # everything but the studies is as it was; the studies come out whole
    expected = {k: v for k, v in before.items() if k != "studies"}
    expected["schemaVersion"] = CURRENT_VERSION
    assert out.data == expected
    assert out.studies == before["studies"]
    assert out.data["uiLayout"] == {"zoom": 0.8}, "unknown fields ride along"


def test_a_file_without_a_version_is_version_1():
    raw = _v1()
    del raw["schemaVersion"]
    assert migrate(raw).from_version == 1


@pytest.mark.parametrize("bad", [0, -1, "2", 1.5, True, None])
def test_a_nonsense_version_is_refused(bad):
    raw = _v1()
    raw["schemaVersion"] = bad
    with pytest.raises(ValueError):
        migrate(raw)


def test_the_current_version_is_left_alone():
    raw = {**_v1(), "schemaVersion": CURRENT_VERSION}
    out = migrate(raw)
    assert out.data == raw and not out.upgraded and out.studies == []


def test_a_newer_file_is_refused_with_the_version_that_wrote_it():
    raw = {**_v1(), "schemaVersion": CURRENT_VERSION + 1, "savedWith": "0.9.0"}
    with pytest.raises(NewerFileError) as e:
        migrate(raw)
    assert "LightSim 0.9.0" in str(e.value)
    assert "install LightSim 0.9.0 or newer to edit it" in str(e.value)


def test_opening_a_v1_project_upgrades_it_and_keeps_its_studies(projects):
    path = projects / "old-bev.json"
    path.write_bytes(V1.read_bytes())
    res = client.get("/api/projects/old-bev")
    assert res.status_code == 200, res.text
    body = res.json()
    assert body["upgradedFrom"] == 1 and body["schemaVersion"] == CURRENT_VERSION
    assert "studies" not in body
    assert path.read_bytes() == V1.read_bytes(), "opening never writes the model file"
    studies = client.get("/api/projects/old-bev/studies").json()
    assert studies == [Study.model_validate(s).model_dump(mode="json") for s in _v1()["studies"]]
    assert studies[0]["futureField"] == "kept"
    # opening it again stores nothing twice
    client.get("/api/projects/old-bev")
    assert client.get("/api/projects/old-bev/studies").json() == studies


def test_the_first_save_writes_the_new_format_and_keeps_the_old_file(projects):
    path = projects / "old-bev.json"
    path.write_bytes(V1.read_bytes())
    body = client.get("/api/projects/old-bev").json()
    rev = body.pop("revision")
    res = client.put("/api/projects/old-bev", json=body, headers={"If-Match": f'"{rev}"'})
    assert res.status_code == 200, res.text
    saved = json.loads(path.read_text(encoding="utf-8"))
    assert saved["schemaVersion"] == CURRENT_VERSION and saved["savedWith"] == VERSION
    assert "studies" not in saved and "upgradedFrom" not in saved and "revision" not in saved
    assert saved["uiLayout"] == {"zoom": 0.8}
    kept = projects / ".backups" / "old-bev" / "pre-migration-v1.json"
    assert kept.read_bytes() == V1.read_bytes()
    assert len(client.get("/api/projects/old-bev/studies").json()) == 2
    # it is now current: no "upgraded" note, and later saves keep no new copy
    again = client.get("/api/projects/old-bev").json()
    assert "upgradedFrom" not in again
    rev = again.pop("revision")
    again["name"] = "Renamed"
    assert client.put("/api/projects/old-bev", json=again,
                      headers={"If-Match": f'"{rev}"'}).status_code == 200
    assert sorted(p.name for p in kept.parent.glob("pre-migration-*")) == ["pre-migration-v1.json"]


def test_an_older_ui_sending_studies_has_them_kept_with_the_runs(projects):
    body = _v1()
    res = client.put("/api/projects/old-bev", json=body)
    assert res.status_code == 200, res.text
    assert "studies" not in json.loads((projects / "old-bev.json").read_text(encoding="utf-8"))
    assert [s["id"] for s in client.get("/api/projects/old-bev/studies").json()] == ["sweep-1", "sweep-2"]


def test_a_newer_file_opens_read_only_and_is_never_saved_over(projects):
    newer = {**_v1(), "schemaVersion": CURRENT_VERSION + 1, "savedWith": "0.9.0",
             "newThing": {"a": 1}}
    del newer["studies"]
    path = projects / "old-bev.json"
    path.write_text(json.dumps(newer), encoding="utf-8")
    before = path.read_bytes()
    res = client.get("/api/projects/old-bev")
    assert res.status_code == 200, res.text
    body = res.json()
    assert "saved by LightSim 0.9.0" in body["readOnly"]
    assert body["newThing"] == {"a": 1}
    rev = body.pop("revision")
    body.pop("readOnly")
    for headers in ({"If-Match": f'"{rev}"'}, {}):
        res = client.put("/api/projects/old-bev", json=body, headers=headers)
        assert res.status_code == 409, res.text
        assert "LightSim 0.9.0" in res.json()["detail"]
    assert path.read_bytes() == before


def test_a_newer_file_this_build_cannot_read_says_why(projects):
    newer = {"id": "old-bev", "name": "x", "schemaVersion": CURRENT_VERSION + 3,
             "savedWith": "1.4.0", "systems": "now an object"}
    (projects / "old-bev.json").write_text(json.dumps(newer), encoding="utf-8")
    res = client.get("/api/projects/old-bev")
    assert res.status_code == 409
    assert "LightSim 1.4.0" in res.json()["detail"]


def test_an_imported_file_is_upgraded_by_the_engine():
    res = client.post("/api/projects/upgrade", json=_v1())
    assert res.status_code == 200, res.text
    out = res.json()
    assert out["upgradedFrom"] == 1 and out["readOnly"] is None
    assert out["project"]["schemaVersion"] == CURRENT_VERSION and "studies" not in out["project"]
    assert [s["id"] for s in out["studies"]] == ["sweep-1", "sweep-2"]
    assert client.post("/api/projects/upgrade", json={"name": "not a project"}).status_code == 400


def test_backups_and_examples_load_in_the_current_format(projects):
    path = projects / "old-bev.json"
    path.write_bytes(V1.read_bytes())
    body = client.get("/api/projects/old-bev").json()
    body.pop("revision")
    body.pop("upgradedFrom")
    assert client.put("/api/projects/old-bev", json=body).status_code == 200
    backup = client.get("/api/projects/old-bev/backups").json()[0]
    old = client.get(f"/api/projects/old-bev/backups/{backup['id']}").json()
    assert old["schemaVersion"] == CURRENT_VERSION and "studies" not in old
    example = client.get("/api/examples/bev-car").json()
    assert example["schemaVersion"] == CURRENT_VERSION
    assert storage.load_example("bev-car").schemaVersion == CURRENT_VERSION
