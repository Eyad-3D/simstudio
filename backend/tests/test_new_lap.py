"""The lap profile on the new engine (new_lap, lsim-lap: stage 2 of the lap
port) against today's lap solver (lapsim), the oracle, on the FS example.

The method is today's (cornering speeds, forward and backward passes); the car
is the engine's, so the profile differs where its part models do, on purpose:
its brakes split the force by their Max Torque (the FS car's 600/250 N·m, 70.6 %
front, with recuperation up to its 40 % Recuperation Weight on the rear axle
first), so the rear tyres reach their grip first and braking takes longer than
today's ideal balance; and its open differential
gives each rear wheel half the drive, so the inner wheel, unloaded in a corner,
reaches its grip before the pair's pooled grip would (today's). Measured
at the example's setup: the autocross lap from rest 60.37 s against 59.14 s
(+2.1 %, 0.80 s of it on braking stretches, the rest the open differential),
the skidpad 10.55 s against 10.38 s (+1.7 %, none of it braking), cornering
speeds 0.3 to 5.9 % lower, increasingly with speed."""
import pytest

from app.solver import lapsim, new_lap
from app.solver.domains import RunContext
from app.solver.network import build_model
from app.solver.runtime import Runtime
from app.storage import load_example


def _both(layout: str):
    p = load_example("fs-electric")
    case = next(c for c in p.cases if c.id == "case-autocross")
    case.parameterOverrides = ov = {"el-track": {"layout": layout, "laps": 2}}
    gear_of: dict = {}
    model = build_model(p, gear_of, ov)
    lap = lapsim.LapRun(RunContext(p, model, Runtime(model, None), gear_of, ov))
    today = lap.solve(0.0)
    new = new_lap.solve(new_lap.lap_model(p, case.id), lap.track, 0.0, more=True)
    return lap, today, new


def test_the_skidpad_is_within_2_percent_of_todays():
    """No braking: the open differential's inner wheel alone (+1.7 %)."""
    _, today, new = _both("Skidpad")
    assert new["t"][-1] == pytest.approx(today.t[-1], rel=0.02)
    assert new["t"][-1] > today.t[-1]  # (the open differential: never faster)
    braking = sum(new["t"][i + 1] - new["t"][i] for i, c in enumerate(new["code"]) if c == 6)
    assert braking < 0.01 * new["t"][-1]  # (into the next lap's first corner only)


def test_the_autocross_brakes_as_the_cars_brakes_are_set():
    """Within 6 % of today's, slower, most of it braking (0.80 s of 1.23 s;
    the rest, about 0.4 s, the open differential's inner wheel), held by the
    rear axle: the brake split and recuperation on the rear."""
    lap, today, new = _both("Autocross")
    assert new["t"][-1] == pytest.approx(today.t[-1], rel=0.06) and new["t"][-1] > today.t[-1]
    braking = sum(new["t"][i + 1] - new["t"][i] - (today.t[i + 1] - today.t[i])
                  for i, c in enumerate(new["code"]) if c == 6)
    assert braking > 0.5 * (new["t"][-1] - today.t[-1])
    rear = sum(new["t"][i + 1] - new["t"][i] for i, (c, w) in enumerate(zip(new["code"], new["cause"]))
               if c == 6 and w == 2)
    assert rear > 0.8 * sum(new["t"][i + 1] - new["t"][i] for i, c in enumerate(new["code"]) if c == 6)
    # every cornering speed below the top speed at or below today's, none
    # more than 7 % below (above it, neither limits the car)
    for kap, s, a in zip(lap.track.kappa, lap.track.sin_t, new["apex"]):
        old = lap.apex(kap, s)
        if abs(kap) > 1e-9 and old < new["v_cap"]:
            assert 0.93 * old <= a <= old * (1 + 1e-6), (kap, a, old)


def test_a_lap_gives_its_limit_band_motor_current_and_hand_checks():
    """As a full run: what held the car back (from the Limit channel, every
    second of the run in one state), the motor's DC current among its duty,
    and the automatic hand calculations."""
    from app.solver import simulate

    r = simulate(load_example("fs-electric"), "case-autocross")
    lane = r.limits.lanes[0]
    assert set(lane.seconds) <= set(r.limits.states) and {"grip", "braking"} <= set(lane.seconds)
    assert sum(lane.seconds.values()) == pytest.approx(r.limits.tEnd, rel=1e-6)
    motor = next(d for d in r.duty if d.elementId == "el-motor")
    assert next(x for x in motor.rows if x.quantity == "DC current").max > 0
    assert sum(ref.automatic for ref in r.references) == 2
