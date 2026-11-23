"""The Differential's Type (Open, Locked, Limited-slip) through the app, on
the new engine, against hand answers.

A split-grip launch: the two-wheel axle of bev_axle (1,800 kg on two 0.33 m
wheels, so N = 8,829 N on each; no load transfer: CG height 0), its left
wheel on ice (μ 0.3), its right one on dry road (μ 1.0), a motor strong
enough to spin either. The axle pushes with at most

* open: twice the icy wheel's grip, 2 × 0.3 × N = 5,297 N (both wheels get
  the same torque);
* locked: both wheels' grip, 1.3 × N = 11,478 N;
* limited-slip (Preload P = 40 N·m, 40 % under power): the icy wheel's
  torque plus twice the locking torque P + ½·0.4·T, T the axle's torque:
  T = 2·(0.3·N·r + P)/(1 − 0.4) = 3,047 N·m, 9,233 N.

In a full run the icy wheel spins and its force is its grip exactly; the
torques follow each Type's law exactly at every instant. In quick mode
(every wheel at the car's speed) the split follows the wheels' grip, and a
cycle asking for more than the axle can push is flagged."""
import os

import pytest

from app.schemas import ElementInstance
from app.solver import new_lap, simulate
from app.solver.core import simulate_today
from app.storage import load_example
from helpers import bev_axle, conn, el

N = 1800 * 9.81 / 2
R = 0.33
P, L = 40.0, 0.4
TYPES = ["Open", "Locked", "Limited-slip"]


def _launch(typ: str, profile: str, duration: float, mode: str = "full"):
    p = bev_axle(mu_left=0.3, profile=profile)
    els = {e.id: e for e in p.systems[0].elements}
    els["diff"].parameterOverrides.update(type=typ, preload_Nm=P, lock_drive_pct=L * 100)
    els["mot"].parameterOverrides["torque_scale_pct"] = 300
    p.cases[0].duration = duration
    p.cases[0].timeStep = 0.5
    p.cases[0].mode = mode
    return p


def _at(r, el_id: str, port: str) -> dict[float, float]:
    ch = next(c for c in r.channels if c.elementId == el_id and c.portId == port)
    return {round(q["t"], 6): q["value"] for q in ch.timeSeries}


@pytest.mark.parametrize("typ", TYPES)
def test_a_split_grip_launch_in_a_full_run(typ):
    r = simulate(_launch(typ, "0:0; 4:100; 10:100", 3.0), "case")
    assert r.status == "success" and r.engine == "new"
    fl, fr = _at(r, "whl", "sig_force"), _at(r, "whr", "sig_force")
    ta, tb = _at(r, "diff", "sig_torque_a"), _at(r, "diff", "sig_torque_b")
    for t in (1.0, 2.0, 3.0):
        assert fl[t] == pytest.approx(0.3 * N, rel=1e-6)  # the icy wheel spins at its grip
        if typ == "Open":  # (to the solver tolerance: the output points are interpolated)
            assert ta[t] == pytest.approx(tb[t], rel=1e-6)
        elif typ == "Locked":  # both wheels' grip
            assert fl[t] + fr[t] == pytest.approx(1.3 * N, rel=1e-6)
        else:  # the coupling slips: the sides differ by twice its locking torque
            assert tb[t] - ta[t] == pytest.approx(2 * (P + 0.5 * L * (ta[t] + tb[t])), rel=1e-6)
            assert 0.3 * N < fl[t] + fr[t] < 1.3 * N
    loss = _at(r, "diff", "sig_losses")
    if typ == "Limited-slip":  # its clutch's heat: τ_c × the slip
        assert loss[2.0] > 1.0
    # every part's books close, the differential's with its clutch's heat
    for f in r.partEnergy:
        assert f.energyIn - f.energyOut - f.losses - f.stored == pytest.approx(0.0, abs=2e-6)
    assert not any("books" in m.text for m in r.messages if m.level != "info"), r.messages


@pytest.mark.parametrize("typ, follows, cannot", [
    ("Open", 2.5, 4.0),            # hand limit 2.94 m/s² (before road load and inertia)
    ("Limited-slip", 4.0, 5.8),    # 5.13 m/s²
    ("Locked", 5.8, 7.0),          # 6.38 m/s²
])
def test_a_split_grip_launch_in_quick_mode(typ, follows, cannot):
    for a, ok in ((follows, True), (cannot, False)):
        r = simulate(_launch(typ, f"0:0; 2:{a * 2 * 3.6}", 2.0, "quick"), "case")
        assert r.engine == "new" and r.status == ("success" if ok else "warning"), (typ, a)
        ta, tb = _at(r, "diff", "sig_torque_a"), _at(r, "diff", "sig_torque_b")
        t = 1.0
        if typ == "Open":
            assert ta[t] == pytest.approx(tb[t], rel=1e-9)
        elif typ == "Locked":  # the torque splits as the grip: 0.3 : 1.0
            assert ta[t] / (ta[t] + tb[t]) == pytest.approx(0.3 / 1.3, rel=1e-9)
        else:  # the grip's split as far as the locking torque reaches
            assert tb[t] - ta[t] == pytest.approx(2 * (P + 0.5 * L * (ta[t] + tb[t])), rel=1e-9)


def _lap_time(typ: str, layout: str) -> float:
    p = load_example("fs-electric")
    d = next(e for s in p.systems for e in s.elements if e.id == "el-diff")
    d.parameterOverrides.update(type=typ, preload_Nm=20, lock_drive_pct=40, lock_coast_pct=25)
    case = next(c for c in p.cases if c.id == "case-autocross")
    case.parameterOverrides = {"el-track": {"layout": layout, "laps": 2}}
    track = new_lap.track_of(p, case.parameterOverrides)
    return new_lap.solve(new_lap.lap_model(p, case.id), track, 0.0, more=True)["t"][-1]


def test_a_lap_with_tight_corners_orders_the_types():
    """On the FS autocross (hairpins and slaloms), driving out of a corner the
    inner rear wheel is unloaded: open, its grip holds the car back; limited-
    slip, the outer wheel takes the locking torque more; locked, the two
    wheels' grip together. (A locked axle's resistance to turning is not
    modelled, so here it is the fastest.)"""
    t = {typ: _lap_time(typ, "Autocross") for typ in TYPES}
    assert t["Locked"] < t["Limited-slip"] < t["Open"], t
    assert t["Open"] - t["Locked"] > 0.1, t


def test_the_older_engine_refuses_a_limited_slip_one_plainly():
    r = simulate_today(_launch("Limited-slip", "0:0; 4:100", 1.0), "case")
    assert r.status == "failed"
    assert "Limited-slip differential, which only the app's engine models" in r.messages[0].text
    r = simulate_today(_launch("Locked", "0:0; 4:100", 1.0), "case")
    assert r.status != "failed"


def test_an_older_projects_locked_box_opens_as_type_locked():
    def diff(**over):
        return ElementInstance(id="d", componentDefId="mech.differential", label="D",
                               position={"x": 0, "y": 0}, parameterOverrides=over)
    assert diff(locked=True).parameterOverrides == {"type": "Locked"}
    assert diff(locked=False, ratio=3).parameterOverrides == {"ratio": 3}
    assert diff(type="Limited-slip").parameterOverrides == {"type": "Limited-slip"}
    # a transfer case keeps its box
    tc = ElementInstance(id="t", componentDefId="mech.transfer_case", label="T",
                         position={"x": 0, "y": 0}, parameterOverrides={"locked": True})
    assert tc.parameterOverrides == {"locked": True}


def test_quick_mode_names_a_locked_differential_without_a_wheel_on_each_output():
    p = _launch("Locked", "0:0; 4:50", 1.0, "quick")
    root = p.systems[0]
    root.elements.append(el("gear", "mech.final_drive", "Side Gear", ratio=1.0))
    root.connections = [c for c in root.connections if c.targetElementId != "whl"]
    root.connections += [conn(70, "diff", "flange_out_a", "gear", "flange_in"),
                         conn(71, "gear", "flange_out", "whl", "shaft")]
    r = simulate(p, "case")
    assert r.status == "failed"
    assert any("needs one Wheel of its own" in m.text for m in r.messages), r.messages


if os.environ.get("LIGHTSIM_ENGINE") == "today":  # the new engine's behaviour only
    pytestmark = pytest.mark.skip("the Type is the new engine's")
