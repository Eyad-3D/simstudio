"""The Traction Control block (MOD-45) on the FS example's 75 m case.

The block sits between the Driver's Traction Command and the E-Motor,
reads both rear wheels' slip and the vehicle speed, ramps the launch and
then holds the slip at its target with a PI loop."""
import pytest
from helpers import dbc, el, series

from app.solver import simulate
from app.solver.domains import traction_control
from app.storage import load_example


def _accel(tc=None, step=0.01):
    p = load_example("fs-electric")
    p.cases[0].timeStep = step
    if tc is not None:
        p.systems[0].elements.append(el("tc", "control.traction", "Traction Control", **tc))
        p.dataBusConnections = [d for d in p.dataBusConnections if d.element2Id != "el-motor"]
        p.dataBusConnections += [
            dbc(80, "el-driver", "sig_traction_cmd", "tc", "sig_demand_in"),
            dbc(81, "el-wheel-rl", "sig_slip", "tc", "sig_slip_in"),
            dbc(82, "el-wheel-rr", "sig_slip", "tc", "sig_slip2_in"),
            dbc(83, "el-vehicle", "sig_speed", "tc", "sig_speed_in"),
            dbc(84, "tc", "sig_out", "el-motor", "sig_demand_in"),
        ]
    r = simulate(p, "case-accel-75m")
    assert r.status == "success", [m.text for m in r.messages if m.level != "info"]
    return r


def _slips(r):
    """(t, larger rear slip, speed km/h, block output) at each point."""
    rl, rr = series(r, "el-wheel-rl", "sig_slip"), series(r, "el-wheel-rr", "sig_slip")
    v = series(r, "el-vehicle", "sig_speed")
    out = series(r, "tc", "sig_out") if any(c.elementId == "tc" for c in r.channels) else v
    return [(a["t"], max(a["value"], b["value"]), s["value"], o["value"])
            for a, b, s, o in zip(rl, rr, v, out)]


def _time(r):
    return next(s.value for s in r.summary if s.label == "Time to 75 m")


def test_slip_holds_its_target_after_the_launch_ramp():
    """The item's metric, at a 2 ms step (case Step 0.002 s): after the ramp
    and while the block holds the motor back, the slip stays within ±0.02
    of the 0.1 target (measured 0.095-0.100 from 0.4 s to 60 km/h), with no spike above
    0.3 once the car moves; without it the slip reaches 7. The 75 m time
    does not improve (3.756 against 3.754 s) because LightSim's tyres keep
    their grip past the peak (MOD-16), so wheelspin costs no time."""
    tc = _slips(_accel({"kp": 1, "ki": 20}, step=0.002))
    # (above about 63 km/h the 80 kW limit, not the tyres, holds the car
    # back and the slip falls below the target on its own)
    held = [s for t, s, v, o in tc if t >= 0.4 and v < 60 and o < 0.999]
    assert len(held) > 300
    assert all(abs(s - 0.1) <= 0.02 for s in held), (min(held), max(held))
    assert max(s for _, s, v, _ in tc if v > 5) < 0.3
    free = _slips(_accel(None, step=0.002))
    assert max(s for _, s, v, _ in free if v > 5) > 1.0


def test_the_defaults_stay_stable_at_the_10_ms_step():
    r = _accel({})
    tc = _slips(r)
    assert max(s for t, s, v, _ in tc if v > 5) < 0.5
    assert _time(r) == pytest.approx(3.744, abs=0.01)


def test_the_launch_ramp_and_pass_through():
    st = {"integral": 1.0, "launch_t": None}
    p = {"launch_ramp_s": 0.5, "launch_torque_pct": 40, "min_speed_kmh": 5}
    assert traction_control(st, p, 0.0, 0.01, 1.0, 5.0, None, 0.0) == pytest.approx(0.4)
    assert traction_control(st, p, 0.25, 0.01, 1.0, 5.0, None, 2.0) == pytest.approx(0.7)
    assert traction_control(st, p, 0.25, 0.01, 0.5, 5.0, None, 2.0) == pytest.approx(0.5)
    # braking and regeneration pass through and end the launch
    assert traction_control(st, p, 0.3, 0.01, -0.6, 0.0, None, 3.0) == -0.6
    assert st["launch_t"] is None
    # moving, slip under the target: the demand passes
    st = {"integral": 1.0, "launch_t": None}
    assert traction_control(st, {}, 1.0, 0.01, 0.9, 0.05, 0.04, 50.0) == pytest.approx(0.9)
    # far over it: the limit drops below the demand
    assert traction_control(st, {}, 1.01, 0.01, 0.9, 0.6, 0.1, 50.0) < 0.9
