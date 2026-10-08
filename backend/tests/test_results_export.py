"""Results for MATLAB and spreadsheets (STD-09): a run exported as .mat
reads back through SciPy with the same values and units, the CSV opens in
Excel as written, and the run card says what made the run."""
from __future__ import annotations

import csv
import gzip
import io
import json
import math

import pytest
import scipy.io as sio
from fastapi.testclient import TestClient

from app import run_store
from app.dataio import cli, matfile, results
from app.main import app
from app.schemas import LiveEdit, RunSnapshot, StoredRun
from app.solver import simulate
from app.storage import load_example

client = TestClient(app)


def _run(example: str, case_id: str, seconds: float = 60.0) -> StoredRun:
    project = load_example(example)
    case = next(c for c in project.cases if c.id == case_id)
    case = case.model_copy(update={"duration": seconds})
    project = project.model_copy(update={"cases": [case]})
    result = simulate(project, case.id)
    return StoredRun(
        id="run-1", caseId=case.id, caseName=case.name, startedAt=1_790_000_000_000,
        status=result.status, result=result, name="Mass 2,300 kg", note="for the report",
        snapshot=RunSnapshot(project=project, case=case, appVersion="0.3.0",
                             modelHash="ab" * 32,
                             liveEdits=[LiveEdit(t=12.0, elementId="el-driver", key="kp",
                                                 value=0.5)]))


@pytest.fixture(scope="module", params=[("bev-car", "case-city"), ("hybrid-car", "case-mixed")])
def example_run(request) -> StoredRun:
    return _run(*request.param)


def _load(data: bytes) -> dict:
    return sio.loadmat(io.BytesIO(data), simplify_cells=True)


def test_both_examples_round_trip_through_scipy_with_values_and_units(example_run):
    rt = results.table(example_run)
    m = _load(results.to_mat(rt))
    assert rt.columns, "the run has channels"
    for col in rt.columns:
        s = m[col.mat_struct]
        got = s[col.mat_field]
        want = [math.nan if v is None else v for v in col.values]
        assert len(got) == len(want)
        assert all((math.isnan(a) and math.isnan(b)) or a == b for a, b in zip(got, want)), col.label
        assert list(s["t"]) == col.t
        assert m["meta"]["units"][col.mat_struct][col.mat_field] == col.unit
        assert m["meta"]["labels"][col.mat_struct][col.mat_field] == col.label


def test_the_meta_struct_carries_the_run_card(example_run):
    m = _load(results.to_mat(results.table(example_run)))["meta"]
    assert m["format"] == "lightsim-run-card"
    assert m["project"] == example_run.snapshot.project.name
    assert m["case_name"] == example_run.caseName
    assert m["run_name"] == "Mass 2,300 kg"
    assert m["run_note"] == "for the report"
    assert m["app_version"] == "0.3.0"
    assert m["model_hash"] == "ab" * 32
    assert m["status"] == example_run.status
    assert m["started"] == "2026-09-21T14:13:20Z"
    summary = m["summary"] if isinstance(m["summary"], list) else [m["summary"]]
    assert [s["label"] for s in summary] == [s.label for s in example_run.result.summary]
    edit = m["live_edits"]
    assert edit["element"] == "Driver" and edit["key"] == "kp" and edit["value"] == 0.5
    assert json.loads(m["run_card_json"])["run"]["id"] == "run-1"
    # the parameters the example sets away from the library's defaults
    changed = m["changed_parameters"]
    assert changed and all("element" in c and "unit" in c for c in changed)


def test_unicode_units_survive_in_scipy():
    m = _load(matfile.mat_bytes({"u": {"torque": "N·m", "temp": "°C", "r": "Ω"}}))
    assert m["u"] == {"torque": "N·m", "temp": "°C", "r": "Ω"}


def test_matlab_names_are_valid_and_unique():
    taken: set[str] = {"meta"}
    assert matfile.identifier("HV Battery Pack", taken) == "HV_Battery_Pack"
    assert matfile.identifier("HV Battery-Pack", taken) == "HV_Battery_Pack_2"
    assert matfile.identifier("Meta", taken) == "Meta_2"
    assert matfile.identifier("2nd motor", taken) == "x_2nd_motor"
    assert len(matfile.identifier("a" * 100)) == 63
    with pytest.raises(ValueError):
        matfile.mat_bytes({"not valid": 1})


def test_matrices_cells_and_struct_arrays_read_back():
    m = _load(matfile.mat_bytes({
        "M": matfile.Matrix([[1, 2, 3], [4, None, 6]]),
        "C": matfile.Cell([["a", 1.5], ["b", "c"]]),
        "S": matfile.StructArray([{"x": 1}, {"x": "two"}]),
        "v": [1, 2, None], "e": [], "s": "", "n": 3}))
    assert m["M"].shape == (2, 3) and m["M"][0, 2] == 3 and math.isnan(m["M"][1, 1])
    assert m["C"][1][1] == "c" and m["C"][0][1] == 1.5
    assert m["S"] == [{"x": 1.0}, {"x": "two"}]
    assert list(m["v"][:2]) == [1, 2] and math.isnan(m["v"][2])
    assert m["n"] == 3


def test_csv_is_utf8_with_bom_quoted_and_keeps_its_columns():
    run = _run("bev-car", "case-city", seconds=5)
    # a label with a comma and a quote must not shift the columns
    for ch in run.result.channels:
        if ch.elementId == "el-motor":
            ch.label = ch.label.replace("E-Motor", 'Motor, rear "A"')
    data = results.to_csv(results.table(run))
    assert data.startswith(b"\xef\xbb\xbf")
    rows = list(csv.reader(io.StringIO(data.decode("utf-8-sig"))))
    header = rows[0]
    assert header[0] == "t [s]"
    assert 'Motor, rear "A" · Shaft Torque [N·m]' in header
    assert {len(r) for r in rows} == {len(header)}
    col = header.index('Motor, rear "A" · Shaft Torque [N·m]')
    torque = next(c for c in run.result.channels if c.label.endswith("Shaft Torque")
                  and c.elementId == "el-motor")
    assert [float(r[col]) for r in rows[1:]] == [p["value"] for p in torque.timeSeries]


def test_the_run_card_json_names_every_column():
    run = _run("bev-car", "case-city", seconds=5)
    rt = results.table(run)
    card = json.loads(results.run_card_json(rt))
    assert card["format"] == "lightsim-run-card" and card["formatVersion"] == 1
    assert len(card["channels"]) == len(run.result.channels)
    assert card["channels"][0]["csvColumn"].endswith("]")
    assert card["case"]["name"] == "City Cycle"


def test_a_run_without_a_snapshot_still_exports():
    run = _run("bev-car", "case-city", seconds=3)
    bare = run.model_copy(update={"snapshot": None})
    m = _load(results.to_mat(results.table(bare)))
    assert len(m["meta"]["project"]) == 0  # an empty text
    assert "Vehicle" in m


@pytest.mark.parametrize("started", [4501005553985130082304, -10**20, 10**15])
def test_a_start_time_out_of_range_still_exports(started):
    run = _run("bev-car", "case-city", seconds=2).model_copy(update={"startedAt": started})
    for fmt in ("mat", "csv", "json"):
        r = client.post(f"/api/export/run?format={fmt}", content=run.model_dump_json(),
                        headers={"Content-Type": "application/json"})
        assert r.status_code == 200, fmt
    card = json.loads(results.run_card_json(results.table(run)))
    assert card["run"]["startedAt"] in (None, "33658-09-27T01:46:40Z")


def test_stored_runs_export_over_the_api(tmp_path, monkeypatch):
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path))
    run = _run("bev-car", "case-city", seconds=5)
    run_store.save_run("bev-car", run)
    r = client.get("/api/projects/bev-car/runs/run-1/export?format=mat")
    assert r.status_code == 200
    assert r.headers["content-type"] == "application/x-matlab-data"
    assert "Battery Electric Car - City Cycle - Mass 2,300 kg.mat" in \
        r.headers["content-disposition"].replace("%20", " ").replace("%2C", ",")
    assert "HV_Battery_Pack" in _load(r.content)
    r = client.get("/api/projects/bev-car/runs/run-1/export?format=csv")
    assert r.status_code == 200 and r.content.startswith(b"\xef\xbb\xbf")
    r = client.get("/api/projects/bev-car/runs/run-1/export?format=json")
    assert r.json()["run"]["name"] == "Mass 2,300 kg"
    assert client.get("/api/projects/bev-car/runs/nope/export").status_code == 404
    r = client.post("/api/export/run?format=mat", content=run.model_dump_json(),
                    headers={"Content-Type": "application/json"})
    assert r.status_code == 200 and "meta" in _load(r.content)


def test_the_command_line_runs_a_case_and_writes_mat_and_csv(tmp_path, capsys):
    project = load_example("fs-electric")
    path = tmp_path / "fs.json"
    path.write_text(project.model_dump_json(), encoding="utf-8")
    out = tmp_path / "accel.mat"
    code = cli.main(["run", str(path), "--case", "Acceleration 75 m", "--out", str(out), "--json"])
    assert code == 0
    printed = json.loads(capsys.readouterr().out)
    assert printed["status"] == "success" and printed["files"] == [str(out)]
    assert "Time to 75 m" in printed["summary"]
    m = _load(out.read_bytes())
    assert m["meta"]["model_hash"] == cli.model_hash(project)
    assert m["meta"]["case_name"] == "Acceleration 75 m"

    csv_out = tmp_path / "accel.csv"
    assert cli.main(["run", str(path), "--case", "accel", "--out", str(csv_out)]) == 0
    assert csv_out.read_bytes().startswith(b"\xef\xbb\xbf")
    assert json.loads((tmp_path / "accel.runcard.json").read_text("utf-8"))["case"]["kind"] \
        == "acceleration"

    stored = tmp_path / "run.json.gz"
    run = _run("bev-car", "case-city", seconds=2)
    stored.write_bytes(gzip.compress(run.model_dump_json().encode()))
    assert cli.main(["export", str(stored), "--format", "csv", "--out",
                     str(tmp_path / "r.csv")]) == 0
    assert (tmp_path / "r.csv").exists()


def test_the_command_line_exit_codes(tmp_path, capsys):
    assert cli.main(["run", str(tmp_path / "missing.json")]) == cli.USAGE
    project = load_example("bev-car")
    path = tmp_path / "bev.json"
    path.write_text(project.model_dump_json(), encoding="utf-8")
    assert cli.main(["run", str(path), "--case", "no such case"]) == cli.USAGE
    assert "City Cycle" in capsys.readouterr().err
    with pytest.raises(SystemExit) as e:
        cli.main(["run"])
    assert e.value.code == cli.USAGE
    # a model that cannot run: Data Checks errors give 1
    broken = project.model_dump(mode="json")
    broken["systems"][0]["elements"] = [e for e in broken["systems"][0]["elements"]
                                        if e["componentDefId"] != "battery.generic"]
    path.write_text(json.dumps(broken), encoding="utf-8")
    assert cli.main(["run", str(path), "--out", str(tmp_path / "x.mat")]) == cli.CHECKS_FAILED
