"""Calibrate lap mode on a logged lap and check it on another (VAL-38).

No team's log is bundled, so the "logged" laps are LightSim's own, from the
FS example with its grip scaled to 0.9 and 2.5 m² of downforce, with noise
on the speed (σ 0.3 km/h) and the lateral acceleration (σ 0.03 g): the
Autocross layout to calibrate on and the same lap driven the other way
round to check, through the API as the app sends them."""
import math
import random

import pytest
from fastapi.testclient import TestClient
from helpers import series

from app import laplog
from app.main import app
from app.solver import lapsim, simulate
from app.solver.calibrate import read_logged_lap
from app.storage import load_example


def _logged(track: dict, grip=0.9, cza=2.5, seed=1) -> str:
    p = load_example("fs-electric")
    for e in p.systems[0].elements:
        if e.componentDefId == "propulsion.wheel":
            for k in ("mu", "mu_lateral", "mu_load_sensitivity_per_kN"):
                e.parameterOverrides[k] *= grip
        if e.componentDefId == "vehicle.body":
            e.parameterOverrides["downforce_cza_m2"] = cza
    case = p.cases[1]
    case.parameterOverrides, case.outputEvery = {"el-track": {**track, "laps": 3}}, 1
    r = simulate(p, case.id)
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


def _reversed_autocross() -> dict:
    pts = lapsim._from_segments(lapsim.layouts()["Autocross"]["segments"], True)
    length = pts[-1][0]
    return {"layout": "Custom", "closed": True,
            "curvature_table": {f"{length - x:g}": -k for x, k in reversed(pts)}}


def test_a_logged_lap_becomes_a_track():
    log = read_logged_lap(_logged({"layout": "Autocross"}))
    track = lapsim.load_track({"layout": "Autocross"})
    assert log.s[-1] == pytest.approx(track.length, rel=0.005)
    assert max(abs(k) for k in log.kappa) == pytest.approx(max(abs(k) for k in track.kappa),
                                                         rel=0.15)
    assert log.energy_kwh > 0
    with pytest.raises(laplog.LapLogError, match="lateral acceleration"):
        read_logged_lap("Time,Speed\n" + "\n".join(f"{i},{50}" for i in range(20)))


def test_calibrated_on_one_lap_it_predicts_another_within_5_percent():
    """The item's metric: calibrated on one logged lap, the blind prediction
    of another is within 5 % on lap time (measured −0.5 %, against −3.3 %
    with the model's own grip and no downforce), and reports the speed
    trace's RMS error and the energy error (0.6 %)."""
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
    assert math.isfinite(check["energy_error_pct"]) and abs(check["energy_error_pct"]) < 5.0
    assert out["calibration_lap"]["status"] in ("success", "warning")


def test_a_log_the_calibration_cannot_read_is_refused():
    client = TestClient(app)
    proj = load_example("fs-electric")
    res = client.post("/api/laplog/calibrate", json={
        "project": proj.model_dump(mode="json"), "calibration": {"text": "a,b\n1,2\n3,4"}})
    assert res.status_code == 400
