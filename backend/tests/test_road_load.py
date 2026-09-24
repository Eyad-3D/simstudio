"""Road load (MOD-11): the air density the Ambient block sets, exact slopes,
and road load entered as a coast-down's A/B/C coefficients, with the axle's
drag counted once."""
import math

import pytest
from helpers import bev_axle, coast_project, series

from app.solver import simulate
from app.solver.runtime import solve_linear

# EPA 2022 Test Car List, Hyundai Ioniq Hybrid Blue (the P2 hybrid example):
# 15.431 lbf, 0.32897 lbf/mph, 0.014602 lbf/mph² in N, N/(km/h), N/(km/h)²
LBF, MPH = 4.4482216152605, 1.609344
IONIQ_ABC = (15.431 * LBF, 0.32897 * LBF / MPH, 0.014602 * LBF / MPH ** 2)


def _abc(a, b, c, **more):
    return {"road_load_mode": "Coefficients A/B/C", "road_load_a_N": a,
            "road_load_b_N_per_kmh": b, "road_load_c_N_per_kmh2": c, **more}


def _speed(project, control=None) -> list[float]:
    return [p["value"] for p in series(simulate(project, "case", control=control),
                                       "veh", "sig_speed")]


def test_ambient_sets_the_air_density():
    """Air drag follows rho = p / (R·T): at -7 °C it is 296.15 / 266.15 =
    1.113 times that at 23 °C. The cold air is set as a case value, the way
    a study or a sweep sets it, and as a live edit."""
    def coast(ambient, case_values=None, control=None):
        proj = coast_project({"cd": 0.3}, {"rolling_resistance": 0.0}, ambient=ambient,
                             v0=110.0, duration=1.0, dt=0.01)
        proj.cases[0].parameterOverrides = case_values or {}
        return _speed(proj, control)

    cold = coast({"temperature_C": 23}, {"amb": {"temperature_C": -7}})
    warm = coast({"temperature_C": 23})
    ratio = (cold[0] - cold[-1]) / (warm[0] - warm[-1])
    assert ratio == pytest.approx(296.15 / 266.15, rel=0.005)
    edits = iter([[{"type": "set_param", "elementId": "amb", "key": "temperature_C", "value": -7}]])
    assert coast({"temperature_C": 23}, control=lambda: next(edits, [])) == pytest.approx(cold, abs=1e-9)
    # an Ambient at the library's values is the air a model without one gets
    assert coast({}) == pytest.approx(coast(None), abs=1e-9)


@pytest.mark.parametrize("rolling", [0.0, 0.01])
@pytest.mark.parametrize("grade", [10.0, 25.0])
def test_slope_force_and_rolling_resistance_are_exact(grade, rolling):
    """Uphill the weight pulls back with m·g·sin(atan(grade)) and presses on
    the road with m·g·cos (before: grade / 100 and no cos, 0.5 % too much at
    10 % and 3 % at 25 %)."""
    speed = _speed(coast_project(
        {"cd": 0.0, "mass_kg": 1500}, {"rolling_resistance": rolling}, grade=grade,
        v0=100.0, duration=8.0, dt=0.01))
    decel = (speed[100] - speed[600]) / 3.6 / 5.0  # mean over 1-6 s
    theta = math.atan(grade / 100.0)
    assert decel == pytest.approx(9.81 * (math.sin(theta) + rolling * math.cos(theta)), rel=0.001)


def test_coast_down_reproduces_road_load_coefficients():
    """A coast-down of a car entered with A/B/C gives A/B/C back: the force
    -m_eff·dv/dt fitted on 1, v, v² between 115 and 15 km/h."""
    mass, radius, inertia = 1474.0, 0.31, 0.001
    speed = _speed(coast_project({"mass_kg": mass, **_abc(*IONIQ_ABC)}, {"radius_m": radius},
                                 v0=120.0, duration=300.0, dt=1.0, inertia=inertia))
    m_eff = mass + 8 * inertia / radius ** 2
    rows, force = [], []
    for v0, v1, v2 in zip(speed, speed[1:], speed[2:]):  # central differences, 1 s apart
        if 15.0 < v1 < 115.0:
            rows.append((1.0, v1, v1 * v1))
            force.append(-m_eff * (v2 - v0) / 3.6 / 2.0)
    normal = [[sum(r[i] * r[j] for r in rows) for j in range(3)] for i in range(3)]
    fitted = solve_linear(normal, [sum(r[i] * f for r, f in zip(rows, force)) for i in range(3)])
    for got, entered in zip(fitted, IONIQ_ABC):
        assert got == pytest.approx(entered, rel=0.005)


@pytest.mark.parametrize("included", [True, False])
def test_axle_gears_are_lossless_when_coefficients_include_driveline_losses(included):
    """At a steady 50 km/h the motor gives the entered road load through the
    final drive (9.7): with no gear loss when the coefficients include the
    driveline's losses, through the final drive's 97 % and the differential's
    98 % when they do not."""
    a, b, c = 200.0, 1.0, 0.03
    proj = bev_axle(profile="0:0; 5:50; 60:50")
    proj.cases[0].duration = 60
    next(e for e in proj.systems[0].elements if e.id == "veh").parameterOverrides.update(
        _abc(a, b, c, abc_include_driveline_losses=included))
    torque = series(simulate(proj, "case"), "mot", "sig_torque")[-1]["value"]
    at_wheels = (a + b * 50 + c * 50 ** 2) * 0.33 / 9.7
    assert torque == pytest.approx(at_wheels if included else at_wheels / (0.97 * 0.98), abs=0.05)
