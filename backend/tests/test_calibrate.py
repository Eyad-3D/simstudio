"""Calibrate lap mode on a logged lap and check it on another (VAL-38).

No team's log is bundled, so the "logged" laps are LightSim's own, from the
FS example with its grip scaled to 0.9 and 2.5 m² of downforce, with noise
on the speed (σ 0.3 km/h) and the lateral acceleration (σ 0.03 g): the
Autocross layout to calibrate on and the same lap driven the other way
round to check, through the API as the app sends them. The laps the fit is
checked on are the new engine's own lap profile, the car the fit tunes (its
brake split and open differential); the log reader's test reads one from
today's lap mode, which also logs the pack power."""
import math
import os
import random

import pytest
from fastapi.testclient import TestClient
from helpers import series

from app import laplog
from app.main import app
from app.solver import lapsim, new_lap, simulate
from app.solver.calibrate import read_logged_lap
from app.storage import load_example


def _car(grip: float, cza: float):
    p = load_example("fs-electric")
    for e in p.systems[0].elements:
        if e.componentDefId == "propulsion.wheel":
            for k in ("mu", "mu_lateral", "mu_load_sensitivity_per_kN"):
                e.parameterOverrides[k] *= grip
        if e.componentDefId == "vehicle.body":
            e.parameterOverrides["downforce_cza_m2"] = cza
    return p


def _logged(track: dict, grip=0.9, cza=2.5, seed=1) -> str:
    """Three laps of the new engine's lap profile (new_lap), logged: time,
    lap, lap distance, speed and lateral acceleration v²·κ, with noise."""
    p = _car(grip, cza)
    ov = {"el-track": {**track, "laps": 3}}
    case = p.cases[1]
    case.parameterOverrides = ov
    lm, tr = new_lap.lap_model(p, case.id), new_lap.track_of(p, ov)
    rnd = random.Random(seed)
    lines = ["Time,Lap Number,Lap Distance,Ground Speed,G Lat", '"s","","m","km/h","g"']
    t0, v0 = 0.0, 0.0
    for k in range(3):
        prof = new_lap.solve(lm, tr, v0, more=k < 2)
        for i, (t, v) in enumerate(zip(prof["t"], prof["v"])):
            if i == len(prof["t"]) - 1 and k < 2:
                break  # (the next lap's first point)
            ay = v * v * tr.kappa[i] / 9.81
            lines.append(f"{t0 + t:.3f},{k + 1},{i * tr.ds:.2f},"
                         f"{v * 3.6 + rnd.gauss(0, 0.3):.3f},{ay + rnd.gauss(0, 0.03):.4f}")
        t0, v0 = t0 + prof["t"][-1], prof["v"][-1]
    return "\n".join(lines)


def _todays_logged(track: dict, grip=0.9, cza=2.5, seed=1) -> str:
    """Three laps of today's lap mode, logged with the pack power."""
    p = _car(grip, cza)
    case = p.cases[1]
    case.parameterOverrides, case.outputEvery = {"el-track": {**track, "laps": 3}}, 1
    r = _todays_lap(p, case.id)
    rnd = random.Random(seed)
    lines = ["Time,Lap Number,Lap Distance,Ground Speed,G Lat,Pack Power",
             '"s","","m","km/h","g","kW"']
    for v, d, ay, pw, k in zip(series(r, "el-vehicle", "sig_speed"),
                               series(r, "el-track", "sig_lap_distance"),
                               series(r, "el-track", "sig_lat_accel"),
                               series(r, "el-battery", "sig_power"),
                               series(r, "el-track", "sig_lap")):
        lines.append(f"{v['t']:.3f},{int(k['value'])},{d['value']:.2f},"
                     f"{v['value'] + rnd.gauss(0, 0.3):.3f},{ay['value'] + rnd.gauss(0, 0.03):.4f},"
                     f"{pw['value']:.3f}")
    return "\n".join(lines)


def _todays_lap(p, case_id):
    """A lap case on today's engine's lap mode: the "logged" laps are input
    data, as a team's logger gives them (the new engine runs no lap case
    yet, NEW-LAP); what the tests check runs on the new engine."""
    os.environ["LIGHTSIM_ENGINE"] = "today"
    try:
        return simulate(p, case_id)
    finally:
        os.environ.pop("LIGHTSIM_ENGINE", None)


def _reversed_autocross() -> dict:
    pts = lapsim._from_segments(lapsim.layouts()["Autocross"]["segments"], True)
    length = pts[-1][0]
    return {"layout": "Custom", "closed": True,
            "curvature_table": {f"{length - x:g}": -k for x, k in reversed(pts)}}


def test_a_logged_lap_becomes_a_track():
    log = read_logged_lap(_todays_logged({"layout": "Autocross"}))
    track = lapsim.load_track({"layout": "Autocross"})
    assert log.s[-1] == pytest.approx(track.length, rel=0.005)
    assert max(abs(k) for k in log.kappa) == pytest.approx(max(abs(k) for k in track.kappa),
                                                         rel=0.15)
    assert log.energy_kwh > 0
    with pytest.raises(laplog.LapLogError, match="lateral acceleration"):
        read_logged_lap("Time,Speed\n" + "\n".join(f"{i},{50}" for i in range(20)))


def test_calibrated_on_one_lap_it_predicts_another_within_5_percent():
    """The item's metric: calibrated on one logged lap, the blind prediction
    of another is within 5 % on lap time, and reports the speed trace's RMS
    error. The laps were logged from today's lap mode; the fit and the
    prediction are the new engine's lap profile (its brake split and open
    differential: the fit takes up the difference in grip). The energy
    error waits for lap cases on the new engine (NEW-LAP): the check says
    so (it was 0.6 % on today's)."""
    client = TestClient(app)
    proj = load_example("fs-electric")
    body = {"project": proj.model_dump(mode="json"),
            "calibration": {"text": _logged({"layout": "Autocross"})},
            "check": {"text": _logged(_reversed_autocross(), seed=2)}}
    res = client.post("/api/laplog/calibrate", json=body)
    assert res.status_code == 200, res.text
    out = res.json()
    fit, check = out["fit"], out["check_lap"]
    # grip and downforce trade off on one lap: the fit need not find 0.9 and
    # 2.5 m², but it finds less grip than the model's own
    assert 0.8 < fit["mu_scale"] < 1.0
    assert fit["rms_kmh"] < 4.0
    assert abs(check["lap_time_error_pct"]) < 5.0
    assert check["speed_rms_kmh"] < 4.0
    assert "energy_error_pct" not in check  # (the log has no pack power; lap cases' energy: NEW-LAP)
    assert out["calibration_lap"]["status"] in ("success", "warning")


def test_a_log_the_calibration_cannot_read_is_refused():
    client = TestClient(app)
    proj = load_example("fs-electric")
    res = client.post("/api/laplog/calibrate", json={
        "project": proj.model_dump(mode="json"), "calibration": {"text": "a,b\n1,2\n3,4"}})
    assert res.status_code == 400


def _circle_log(distance=lambda i: float(i), blank_first=0) -> str:
    """200 rows at 36 km/h on a circle (0.5 g), 0.1 s apart."""
    lines = ["Time,Lap Distance,Ground Speed,G Lat"]
    for i in range(200):
        d = "" if i < blank_first else f"{distance(i):g}"
        lines.append(f"{i * 0.1:.1f},{d},36,0.5")
    return "\n".join(lines)


def test_blank_distance_cells_are_left_out():
    """A logger often leaves the distance blank for its first samples: those
    rows are left out (they raised a TypeError, an HTTP 500)."""
    log = read_logged_lap(_circle_log(blank_first=3))
    assert log.s[-1] == pytest.approx(196.0)
    assert log.time_s == pytest.approx(19.6)


def test_a_lap_that_does_not_move_is_refused():
    with pytest.raises(laplog.LapLogError, match="distance does not change"):
        read_logged_lap(_circle_log(distance=lambda i: 0.0))
    client = TestClient(app)
    proj = load_example("fs-electric")
    res = client.post("/api/laplog/calibrate", json={
        "project": proj.model_dump(mode="json"),
        "calibration": {"text": _circle_log(distance=lambda i: 0.0)}})
    assert res.status_code == 400
    assert "distance does not change" in res.json()["detail"]
