"""A car that coasts comes to a stop and stays there (ENG-09).

Rolling resistance acts in full while the car rolls, to the stop. A car
whose speed would pass through zero within a solver step stops in it (its
distance and energy books end exactly where the stop is) and then stays
stopped: at rest the rolling resistance holds it until what pushes it beats
it. Before, rolling resistance faded out below 0.3 m/s, so a coasting car's
speed decayed for ever and never reached 0 (benchmarks' veh_coastdown: no
stop within 220 s, against an exact stop at 193.18 s)."""
from __future__ import annotations

import math

import pytest
from helpers import dbc, el, project, series

from app.solver import simulate

M, A, C = 1500.0, 150.0, 0.4  # kg, N, N/(m/s)² (benchmarks/reference/problems/veh_coastdown)
V0 = 30.0  # m/s


def _coast(v0: float = V0, duration: float = 220.0, step: float = 0.01, grade=None):
    elements = [el("veh", "vehicle.body", "Vehicle", mass_kg=M,
                   road_load_mode="Coefficients A/B/C", road_load_a_N=A,
                   road_load_b_N_per_kmh=0.0, road_load_c_N_per_kmh2=C / 3.6 ** 2,
                   initial_speed_kmh=v0 * 3.6)]
    databus = []
    if grade is not None:
        elements.append(el("g", "signal.constant", "Grade", value=grade))
        databus.append(dbc(1, "g", "sig_out", "veh", "sig_grade_in"))
    return project(elements, [], databus, duration=duration, time_step=step)


def test_a_coasting_car_stops_where_the_closed_form_says_and_stays_stopped():
    r = simulate(_coast(), "case")
    v = [(p["t"], p["value"] / 3.6) for p in series(r, "veh", "sig_speed")]
    x = {round(p["t"], 6): p["value"] for p in series(r, "veh", "sig_distance")}
    a = math.sqrt(A / C)
    t_stop = M / math.sqrt(A * C) * math.atan(V0 / a)
    x_stop = M / (2.0 * C) * math.log(1.0 + C * V0 * V0 / A)
    stopped = next(t for t, s in v if s == 0.0)
    assert stopped == pytest.approx(t_stop, abs=0.011)  # (within the 10 ms step it falls in)
    assert all(s == 0.0 for t, s in v if t >= stopped)  # and stays stopped
    assert x[220.0] == pytest.approx(x_stop, rel=1e-4)
    # its kinetic energy went into the air and the tyres, and the books say so
    veh = next(f for f in r.partEnergy if f.label == "Vehicle")
    terms = {k: e * 3.6e6 for k, e in veh.terms.items()}
    assert terms["acceleration"] == pytest.approx(-0.5 * M * V0 * V0, rel=1e-12)
    assert terms["rolling resistance"] == pytest.approx(A * x[220.0], rel=1e-9)
    assert terms["air drag"] + terms["rolling resistance"] == pytest.approx(
        0.5 * M * V0 * V0, rel=1e-12)


def test_rolling_resistance_holds_a_car_on_a_slope_it_cannot_overcome():
    """On a 0.5 % downhill the slope pulls with 74 N, less than the 150 N
    rolling resistance: a car let go at 1 m/s stops within 20 s and stays
    stopped, where before it crept on for ever at 0.53 km/h (the rolling
    resistance faded out below 0.3 m/s until it matched the pull). On a
    2 % downhill (294 N) it sets off from rest and rolls on, faster and
    faster."""
    held = simulate(_coast(v0=1.0, duration=60.0, grade=-0.5), "case")
    v = [p["value"] for p in series(held, "veh", "sig_speed")]
    assert v[-1] == 0.0 and max(v[-1000:]) == 0.0
    rolls = simulate(_coast(v0=0.0, duration=10.0, grade=-2.0), "case")
    v = [p["value"] / 3.6 for p in series(rolls, "veh", "sig_speed")]
    pull = M * 9.81 * math.sin(math.atan(0.02)) - A * math.cos(math.atan(0.02))
    assert v[-1] == pytest.approx(pull / M * 10.0, rel=1e-3)  # (and a little air drag)
