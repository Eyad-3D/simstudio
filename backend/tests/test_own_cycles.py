"""CON-11: drive cycles of the user's own, kept in the project file
(``Project.cycles``): imported from a CSV or Excel file, named by id like the
bundled cycles, against time or against distance."""
from __future__ import annotations

import base64
import json

import pytest
from fastapi.testclient import TestClient
from pydantic import ValidationError

from app import cycles
from app.main import app
from app.schemas import ElementInstance, Project, ProjectCycle
from app.solver import simulate
from app.solver.network import ModelError, build_model
from app.sources import sources_of
from app.storage import load_example
from app.validation import validate_project

client = TestClient(app)

# a short city trace: 0 to 40 km/h and back, with a 2 % hill on the way
TIME_CYCLE = {"id": "own:commute", "name": "My commute", "axis": "time",
              "x": [0, 10, 40, 60, 70], "speed": [0, 30, 40, 20, 0],
              "grade": [0, 0, 2, 2, 0], "source": "commute.csv"}
# a lap logged against distance: speed and grade at each metre mark
LAP = {"id": "own:lap", "name": "Test track lap", "axis": "distance",
       "x": [0, 200, 400, 600, 800], "speed": [20, 60, 40, 70, 30],
       "grade": [0, 1, -1, 0, 0]}
# a road's grade only, against distance
HILL = {"id": "own:hill", "name": "Hill road", "axis": "distance",
        "x": [0, 500, 1000], "grade": [0, 4, 0]}


def _project(*own, task_cycle: str | None = None) -> Project:
    p = load_example("bev-car")
    p.cycles = [ProjectCycle(**c) for c in own]
    if task_cycle is not None:
        _task(p).parameterOverrides["cycle"] = task_cycle
    return p


def _task(p: Project) -> ElementInstance:
    return next(e for s in p.systems for e in s.elements if e.id == "el-task")


def _road(p: Project, cycle_id: str) -> Project:
    p.systems[0].elements.append(ElementInstance(
        id="el-road", componentDefId="signal.road_profile", label="Route",
        position={"x": 0, "y": 0}, parameterOverrides={"cycle": cycle_id}))
    return p


def _errors(p: Project) -> list[str]:
    return [c.text for c in validate_project(p) if c.level == "error"]


# ---- the project file ----------------------------------------------------------

def test_a_project_without_cycles_saves_as_before():
    p = load_example("bev-car")
    assert p.cycles == [] and "cycles" not in json.loads(p.model_dump_json())


def test_a_projects_cycles_go_round_the_file_unchanged():
    p = _project(TIME_CYCLE, LAP, HILL)
    raw = json.loads(p.model_dump_json())
    assert [c["id"] for c in raw["cycles"]] == ["own:commute", "own:lap", "own:hill"]
    assert raw["cycles"][2]["speed"] is None
    again = Project.model_validate(raw)
    assert again.cycles == p.cycles


@pytest.mark.parametrize("bad", ["wltc-3b", "own:", "own:a b", "mine:x", "own:" + "x" * 65])
def test_an_own_cycle_id_starts_with_own(bad):
    """A bundled id (or one a later version may bundle) is never an own
    cycle's: a LightSim that does not know the project's cycles then reports
    an unknown cycle instead of driving a bundled one of the same name."""
    with pytest.raises(ValidationError):
        ProjectCycle(**{**TIME_CYCLE, "id": bad})


def test_unknown_fields_of_a_cycle_are_kept():
    p = Project.model_validate({**json.loads(load_example("bev-car").model_dump_json()),
                                "cycles": [{**TIME_CYCLE, "phases": [["A", 0, 70]]}]})
    assert json.loads(p.model_dump_json())["cycles"][0]["phases"] == [["A", 0, 70]]


# ---- driving them -----------------------------------------------------------------

def test_a_driving_task_drives_its_projects_own_cycle():
    p = _project(TIME_CYCLE, task_cycle="own:commute")
    params = build_model(p).params_of["el-task"]
    assert params["profile"] == "0.0:0.0; 10.0:30.0; 40.0:40.0; 60.0:20.0; 70.0:0.0"
    assert params["mode"] == "time"
    assert not _errors(p)
    case = next(c for c in p.cases if c.id == "case-city")
    case.duration = 70
    r = simulate(p, "case-city")
    assert r.status in ("success", "warning")
    assert any("'My commute', a drive cycle of this project's own" in m.text
               for m in r.messages), [m.text for m in r.messages]
    km = next(s.value for s in r.summary if s.key == "distance_km")
    assert km == pytest.approx(cycles.own_info(TIME_CYCLE)["distance_km"], rel=0.03)


def test_a_case_can_drive_an_own_cycle():
    p = _project(TIME_CYCLE)
    case = next(c for c in p.cases if c.id == "case-wltc")
    case.parameterOverrides["el-task"]["cycle"] = "own:commute"
    params = build_model(p, case_overrides=case.parameterOverrides).params_of["el-task"]
    assert params["profile"] == cycles.Catalogue.of(p).profile_text("own:commute")


def test_a_cycle_against_distance_is_read_against_distance():
    """The task reads a cycle logged against distance against the distance
    the car has driven, whatever its Profile Axis says."""
    p = _project(LAP, task_cycle="own:lap")
    params = build_model(p).params_of["el-task"]
    assert params["mode"] == "distance"
    assert params["profile"].startswith("0.0:20.0; 200.0:60.0")
    assert not _errors(p)
    case = next(c for c in p.cases if c.id == "case-city")
    case.duration, case.endLaps = 120, 1
    r = simulate(p, "case-city")
    assert r.status in ("success", "warning"), [m.text for m in r.messages]
    km = next(s.value for s in r.summary if s.key == "distance_km")
    assert km == pytest.approx(0.8, abs=0.01)  # one lap of 800 m


def test_a_cycle_against_time_still_refuses_a_distance_axis():
    p = _project(TIME_CYCLE, task_cycle="own:commute")
    _task(p).parameterOverrides["mode"] = "distance"
    assert ("Driving Task 'Vehicle Task' drives the drive cycle 'My commute', a speed against "
            "time, but its Profile Axis is Distance." in _errors(p))


def test_a_distance_cycle_in_a_case_gets_the_distance_checks():
    stop = {**LAP, "speed": [20, 60, 0, 70, 30]}
    p = _project(stop)
    case = next(c for c in p.cases if c.id == "case-wltc")
    case.parameterOverrides["el-task"]["cycle"] = "own:lap"
    texts = [c.text for c in validate_project(p)]
    assert any("asks for 0 km/h at 400 m" in t for t in texts), texts
    assert not any("Profile Axis is Distance" in t for t in texts)


def test_a_road_profile_takes_an_own_cycles_grade():
    # against time: placed along the distance the cycle's speed covers
    p = _road(_project(TIME_CYCLE), "own:commute")
    road = build_model(p).params_of["el-road"]
    assert road["mode"] == "distance"
    pts = [tuple(map(float, x.split(":"))) for x in road["profile"].split("; ")]
    assert pts[0] == (0.0, 0.0) and pts[2][1] == 2.0
    assert pts[-1][0] / 1000 == pytest.approx(cycles.own_info(TIME_CYCLE)["distance_km"], abs=1e-3)
    # against distance: as it is
    p = _road(_project(HILL), "own:hill")
    assert build_model(p).params_of["el-road"]["profile"] == "0.0:0.0; 500.0:4.0; 1000.0:0.0"
    assert not _errors(p)


def test_a_grade_only_cycle_cannot_drive_a_task():
    p = _project(HILL, task_cycle="own:hill")
    assert _errors(p) == ["Driving Task 'Vehicle Task' uses the drive cycle 'Hill road', which "
                          "has a grade but no speed; a Road Profile can take its grade."]
    p = _road(_project({**HILL, "grade": None, "speed": [10, 20, 10]}), "own:hill")
    assert any("which has no grade" in t for t in _errors(p))


def test_a_missing_own_cycle_is_one_error_that_fails_the_run():
    p = _project(task_cycle="own:gone")
    errors = [c for c in validate_project(p) if c.level == "error"]
    assert [c.text for c in errors] == [
        "Driving Task 'Vehicle Task' uses the drive cycle 'own:gone', which is not among this "
        "project's own cycles."]
    assert errors[0].fix and errors[0].elementIds == ["el-task"]
    r = simulate(p, "case-city")
    assert r.status == "failed" and "'own:gone'" in r.messages[0].text


@pytest.mark.parametrize("change, why", [
    ({"speed": [0, 30, 40]}, "it has 3 speed values for 5 points"),
    ({"x": [0, 10, 5, 60, 70]}, "its time goes back from 10 s to 5 s"),
    ({"speed": [0, 30, -4, 20, 0]}, "a speed is below 0 km/h (-4)"),
    ({"speed": None, "grade": None}, "it has neither a speed nor a grade"),
    ({"speed": None}, "a cycle against time needs a speed"),
    ({"x": [5, 5, 5, 5, 5]}, "it does not move on from 5 s"),
])
def test_a_broken_own_cycle_says_why_it_cannot_be_driven(change, why):
    p = _project({**TIME_CYCLE, **change}, task_cycle="own:commute")
    with pytest.raises(ModelError) as e:
        build_model(p)
    assert e.value.errors == [f"Driving Task 'Vehicle Task' uses the drive cycle 'My commute', "
                              f"which cannot be driven: {why}."]


def test_an_own_cycle_counts_as_the_users_own_values():
    p = _project(TIME_CYCLE, task_cycle="own:commute")
    sources = sources_of(p, "case-city")
    own = next(s for s in sources.sources if s.id == "own")
    assert "Vehicle Task · Drive Cycle" in own.usedBy


def test_own_info_figures():
    info = cycles.own_info(TIME_CYCLE)
    assert info["duration_s"] == 70 and info["vmax_kmh"] == 40 and info["axis"] == "time"
    # trapezoids: (0+30)/2*10 + (30+40)/2*30 + (40+20)/2*20 + (20+0)/2*10 = 1,900 km/h·s
    assert info["distance_km"] == pytest.approx(1900 / 3600, abs=1e-3)
    lap = cycles.own_info(LAP)
    assert lap["distance_km"] == 0.8 and lap["axis"] == "distance" and lap["speed"]
    # the time its own speeds take: 200 m at a mean of 40, 50, 55 and 50 km/h
    assert lap["duration_s"] == pytest.approx(sum(200 / (v / 3.6) for v in (40, 50, 55, 50)),
                                              abs=1e-3)
    assert cycles.own_info(HILL)["duration_s"] == 0 and not cycles.own_info(HILL)["speed"]


# ---- importing them ----------------------------------------------------------------

def _import(text: str | bytes, name: str = "cycle.csv", **opts) -> dict:
    data = text.encode("utf-8") if isinstance(text, str) else text
    r = client.post("/api/import/cycle", json={
        "filename": name, "data": base64.b64encode(data).decode(), **opts})
    assert r.status_code == 200, r.text
    return r.json()


def test_import_reads_time_speed_and_grade():
    res = _import("t_s,speed_kmh,grade_pct\n0,0,0\n1,5,1\n2,10,2\n")
    assert res["ok"] and res["axis"] == "time"
    assert res["cycle"] == {"axis": "time", "x": [0, 1, 2], "speed": [0, 5, 10],
                            "grade": [0, 1, 2]}
    assert (res["xColumn"], res["speedColumn"], res["gradeColumn"]) == (0, 1, 2)
    assert res["info"]["duration_s"] == 2 and res["points"] == 3


def test_import_finds_a_distance_axis_and_converts_units():
    res = _import("Distance [km];Speed [m/s];Throttle [%]\n0;5;0\n0,5;10;50\n1;5;20\n")
    assert res["ok"] and res["axis"] == "distance" and res["gradeColumn"] == -1
    assert res["cycle"]["x"] == [0, 500, 1000] and res["cycle"]["speed"] == [18, 36, 18]
    assert res["units"]["x"]["used"] == "km" and res["units"]["speed"]["used"] == "m/s"


def test_import_reads_a_grade_against_distance_alone():
    res = _import("Distance [m],Grade [%]\n0,0\n100,2\n200,-1\n")
    assert res["ok"] and res["cycle"]["speed"] is None and res["cycle"]["grade"] == [0, 2, -1]
    assert res["valueName"] == "Grade"
    # and the speed column can be dropped by hand
    res = _import("Distance [m],Speed [km/h],Grade [%]\n0,5,0\n100,50,2\n",
                  speedColumn=-1)
    assert res["ok"] and res["cycle"]["speed"] is None


def test_import_refuses_what_cannot_be_driven():
    res = _import("Time [s],Speed [km/h]\n0,0\n10,abc\n5,3\n")
    assert not res["ok"] and res["cycle"] is None
    assert res["errors"][0] == {"text": "Row 3: 'abc' in cell B3 (Target Speed) is not a number.",
                                "row": 3, "cell": "B3"}
    res = _import("Time [s],Speed [km/h]\n0,0\n10,3\n5,3\n")
    assert "must increase" in res["errors"][0]["text"]
    res = _import("Time [s],Speed [km/h]\n0,0\n10,-3\n")
    assert res["errors"][0]["text"] == "Row 3: the speed -3 km/h is below zero."
    res = _import("Time [s],Grade [%]\n0,0\n10,3\n", speedColumn=-1)
    assert res["errors"][0]["text"] == "A cycle against time needs a speed column; choose it."


def test_import_warns_of_a_stop_against_distance():
    res = _import("Distance [m],Speed [km/h]\n0,5\n100,0\n200,10\n")
    assert res["ok"] and "0 km/h before the end" in res["warnings"][0]["text"]


def test_import_reads_excel(tmp_path):
    openpyxl = pytest.importorskip("openpyxl")
    wb = openpyxl.Workbook()
    ws = wb.active
    ws.append(["Logged lap"])
    ws.append([])
    ws.append(["Distance", "Speed"])
    ws.append(["m", "km/h"])
    for d, v in ((0, 10), (50, 40), (100, 30)):
        ws.append([d, v])
    path = tmp_path / "lap.xlsx"
    wb.save(path)
    res = _import(path.read_bytes(), "lap.xlsx")
    assert res["ok"] and res["axis"] == "distance" and res["cycle"]["speed"] == [10, 40, 30]


def test_an_imported_cycle_drives_once_kept_in_the_project():
    res = _import("Time [s],Speed [km/h]\n0,0\n20,50\n40,0\n")
    p = _project({"id": "own:imported", "name": "Imported", **res["cycle"]},
                 task_cycle="own:imported")
    assert not _errors(p)
    assert build_model(p).params_of["el-task"]["profile"] == "0.0:0.0; 20.0:50.0; 40.0:0.0"
