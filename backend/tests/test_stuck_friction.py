"""A stuck friction on a driveline a drive cycle drives: a Clutch, a
limited-slip differential's clutch. Its break-away condition reads the
cycle's time and an E-Motor's rate (J·dω/dt) through the assignments it is
computed by; the run's look-ahead follows it along each step (it takes a
state's derivative as that state's rate along the step, from the dense
output). Before, it refused the derivative, left the condition to root
finding and warned, and the run ended as warning (STUCK-FRICTION-LOOKAHEAD).

The BEV example's City Cycle with a Clutch between its final drive and its
differential, and with a limited-slip differential: a success, no warning,
its figures the unchanged car's within the run's tolerance, every part's
books closed. A split-grip cycle whose burst passes the axle's limit breaks
the limited-slip clutch away and lets it stick again, located to the
solver's tolerance."""
import lightsim_engine
import pytest
from helpers import bev_axle, conn, el

from app.solver import simulate
from app.storage import load_example


def _clutch(p):
    root = next(s for s in p.systems
                if any(e.componentDefId == "mech.differential" for e in s.elements))
    fd = next(e for e in root.elements if e.componentDefId == "mech.final_drive")
    df = next(e for e in root.elements if e.componentDefId == "mech.differential")
    c = next(c for c in root.connections if {c.sourceElementId, c.targetElementId} == {fd.id, df.id})
    root.connections.remove(c)
    root.elements.append(el("cl", "mech.clutch", "Clutch", max_torque_Nm=5000))
    root.connections += [conn(901, fd.id, "flange_out", "cl", "flange_a"),
                         conn(902, "cl", "flange_b", df.id, "flange_in")]
    return p


def _limited_slip(p):
    d = next(e for s in p.systems for e in s.elements if e.componentDefId == "mech.differential")
    d.parameterOverrides.update(type="Limited-slip", preload_Nm=40, lock_drive_pct=40,
                                lock_coast_pct=25)
    return p


@pytest.mark.parametrize("change", [_clutch, _limited_slip], ids=["clutch", "limited-slip"])
def test_a_stuck_friction_on_a_drive_cycle_runs_clean(change):
    base = simulate(load_example("bev-car"), "case-city")
    r = simulate(change(load_example("bev-car")), "case-city")
    assert r.engine == "new" and r.status == "success"
    assert not [m.text for m in r.messages if m.level != "info"]
    # the clutch holds all the way: the unchanged car's figures, within the
    # run's tolerance (rtol 1e-6; 5e-6 measured)
    ref = {s.label: s.value for s in base.summary if isinstance(s.value, (int, float))}
    for s in r.summary:
        if s.label in ref and isinstance(s.value, (int, float)):
            assert s.value == pytest.approx(ref[s.label], rel=2e-5, abs=1e-9), s.label
    for f in r.partEnergy:
        assert f.energyIn - f.energyOut - f.losses - f.stored == pytest.approx(0.0, abs=2e-6)


def _burst():
    """bev_axle, its left wheel on μ 0.2, a limited-slip differential (40
    N·m, 40 %): a launch at 2.8 m/s², then half a second at 4 m/s², past
    the axle's limit (3.49 m/s²: the clutch breaks away exactly there)."""
    p = bev_axle(mu_left=0.2, profile="0:0; 3:30; 3.5:37.2; 4:39; 10:39")
    els = {e.id: e for e in p.systems[0].elements}
    els["diff"].parameterOverrides.update(type="Limited-slip", preload_Nm=40, lock_drive_pct=40,
                                          lock_coast_pct=25)
    els["mot"].parameterOverrides["torque_scale_pct"] = 300
    p.cases[0].duration = 10
    p.cases[0].timeStep = 0.5
    return p


def test_a_split_grip_burst_breaks_the_coupling_away_where_located():
    p = _burst()
    r = simulate(p, "case")
    assert r.status == "success" and not [m.text for m in r.messages if m.level == "warning"]
    model = lightsim_engine.build(p.model_dump_json(), "case")
    found = {}
    for rtol in (1e-6, 1e-9):
        run = model.simulate(rtol=rtol, atol=rtol * 1e-2)
        assert not run.report["warnings"], run.report["warnings"]
        at = [e["t"] for e in run.events if "'Differential': it breaks away" in e["label"]]
        assert len(at) == 1 and 3.0 < at[0] < 3.1, at  # in the burst (3.0468 s)
        # it sticks again once the burst is over
        assert any(at[0] < e["t"] < 4.0 for e in run.events
                   if "'Differential': it sticks" in e["label"])
        found[rtol] = at[0]
    # located to the solver's tolerance (2.1e-6 s apart at the two tolerances)
    assert found[1e-6] == pytest.approx(found[1e-9], abs=1e-5)
