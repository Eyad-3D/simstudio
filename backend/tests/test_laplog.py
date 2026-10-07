"""Import a lap from a logger or lap simulator as a drive cycle (STD-35).

The files here are made up in the layouts the presets expect (no team's
file is bundled); the lap simulator's trace is LightSim's own lap of the
test FS car, so its distance is known."""
import math

import pytest
from fastapi.testclient import TestClient
from helpers import dbc, el, fs_car, series

from app import laplog
from app.main import app
from app.solver import simulate
from app.storage import load_example


def _lap_trace():
    """(distance m, time s, speed m/s) every metre of the test car's
    Autocross lap, from lap mode."""
    r = simulate(fs_car("Autocross", 1), "case")
    d = [p["value"] for p in series(r, "trk", "sig_lap_distance")]
    v = [p["value"] / 3.6 for p in series(r, "veh", "sig_speed")]
    t = [p["t"] for p in series(r, "veh", "sig_speed")]
    return d, t, v


def test_a_lap_simulator_trace_against_distance_keeps_its_distance():
    """The item's metric: a lap simulator's speed trace reproduces its own
    lap distance within 0.5 % (TUM layout: s m, t s, v m/s; time not used
    here, OpenLAP layout: distance and speed only)."""
    d, t, v = _lap_trace()
    tum = "s,t,v\n" + "\n".join(f"{a:.3f},{b:.4f},{c:.4f}" for a, b, c in zip(d, t, v))
    out = laplog.read_lap(tum, "TUM laptime-simulation", columns={"time": None})
    assert out.picked["speed"] == "v" and out.picked["distance"] == "s"
    assert abs(out.distance_m - d[-1]) / d[-1] < 0.005
    assert abs(out.points[-1][0] - t[-1]) / t[-1] < 0.01  # its time, from ∫ ds / v
    openlap = "Distance [m],Speed [km/h]\n" + "\n".join(
        f"{a:.3f},{c * 3.6:.3f}" for a, c in zip(d, v))
    out = laplog.read_lap(openlap, "OpenLAP")
    assert abs(out.distance_m - d[-1]) / d[-1] < 0.005
    assert any("against distance" in w for w in out.warnings)


def _motec(laps=((0, 40.0), (1, 30.0), (2, 31.0), (3, 50.0))):
    """A MoTeC-like export: metadata, a header, a units row and a lap
    column; each lap a speed hump over its duration."""
    lines = ['"Format","MoTeC CSV File"', '"Venue","Test"', '', '"Time","Lap Number",'
             '"Ground Speed","Lap Distance"', '"s","","km/h","m"']
    t = 0.0
    for k, dur in laps:
        n = int(dur / 0.05)
        dist = 0.0
        for i in range(n):
            v = 30 + 60 * math.sin(math.pi * i / n)
            lines.append(f"{t:.3f},{k},{v:.3f},{dist:.2f}")
            dist += v / 3.6 * 0.05
            t += 0.05
    return "\n".join(lines)


def test_a_logged_session_gives_its_fastest_full_lap():
    out = laplog.read_lap(_motec(), "MoTeC i2 CSV")
    assert out.picked == {"time": "Time", "distance": "Lap Distance", "speed": "Ground Speed",
                          "lap": "Lap Number"}
    assert [li["lap"] for li in out.laps] == [0, 1, 2, 3]
    assert out.lap == 1  # the out lap (0) and in lap (3) are left out
    assert out.points[-1][0] == pytest.approx(29.95, abs=0.2)
    assert out.points[0] == (0.0, pytest.approx(30.0, abs=0.01))
    # a lap picked by hand
    assert laplog.read_lap(_motec(), "MoTeC i2 CSV", lap=2).lap == 2
    with pytest.raises(laplog.LapLogError, match="no lap 7"):
        laplog.read_lap(_motec(), "MoTeC i2 CSV", lap=7)


def test_semicolons_decimal_commas_and_spikes():
    rows = ["Zeit;Geschw"] + [f"{i * 0.1:.1f}".replace(".", ",") + ";"
                               + f"{50 + (40 if i == 30 else 0)},0".replace(".", ",")
                               for i in range(100)]
    out = laplog.read_lap("\n".join(rows), "Generic",
                          columns={"time": "Zeit", "speed": "Geschw"})
    assert out.points[-1][0] == pytest.approx(9.8, abs=0.01)
    assert any("2.5 g" in w for w in out.warnings)


def test_what_cannot_be_read_says_why():
    with pytest.raises(laplog.LapLogError, match="speed column"):
        laplog.read_lap("time,rpm\n0,1000\n1,2000\n2,3000", "Generic")
    with pytest.raises(laplog.LapLogError, match="header"):
        laplog.read_lap("1,2\n3,4\n5,6", "Generic")
    with pytest.raises(laplog.LapLogError, match="backwards"):
        laplog.read_lap("time,speed\n0,10\n2,10\n1,10", "Generic")


def test_a_lap_repeated_to_an_endurance_with_a_driver_change():
    out = laplog.read_lap(_motec(), "MoTeC i2 CSV", repeat_to_km=22, driver_change_s=180)
    lap_m = laplog.read_lap(_motec(), "MoTeC i2 CSV").distance_m
    assert out.repeated == round(22000 / lap_m)
    assert abs(out.distance_m - 22000) < lap_m
    stops = [(a, b) for (a, va), (b, vb) in zip(out.points, out.points[1:]) if va == vb == 0]
    assert len(stops) == 1 and stops[0][1] - stops[0][0] == pytest.approx(180)


def test_an_imported_lap_runs_as_a_cycle_through_the_app():
    """From the file to a run: the API reads the lap, the profile drives
    the FS example as a cycle case, which follows it."""
    client = TestClient(app)
    assert any(p["name"] == "AiM Race Studio CSV" for p in client.get("/api/laplog/presets").json())
    res = client.post("/api/laplog/read", json={"text": _motec(), "preset": "MoTeC i2 CSV"})
    assert res.status_code == 200, res.text
    body = res.json()
    assert body["lap"] == 1 and body["profile"].startswith("0:30")
    bad = client.post("/api/laplog/read", json={"text": "a,b\n1,2\n3,4", "preset": "Generic"})
    assert bad.status_code == 400
    proj = load_example("fs-electric")
    case = proj.cases[0]
    case.kind, case.endDistance, case.startLine = "cycle", None, 0.0
    case.duration, case.timeStep = body["duration_s"], 0.1
    # the FS example has no Driving Task: the app adds one wired to the Driver
    task = el("task", "signal.driving_task", "Imported lap")
    proj.systems[0].elements.append(task)
    drv = next(e for e in proj.systems[0].elements if e.componentDefId == "driver.driver")
    proj.dataBusConnections.append(dbc(99, "task", "sig_demand", drv.id, "sig_target_in"))
    case.parameterOverrides = {task.id: {"profile": body["profile"], "cycle": ""}}
    r = simulate(proj, case.id)
    assert r.status == "success", [m.text for m in r.messages if m.level != "info"]
    rows = {s.label: s.value for s in r.summary}
    assert rows["Accumulator — energy delivered"] > 0
