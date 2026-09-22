"""Cross-domain energy-conservation audit.

With every configurable loss turned off (aero, rolling resistance, motor
loss/drag maps, gear efficiencies, battery resistance), the energy the battery
delivers during a pure acceleration must equal the vehicle's kinetic energy —
translational plus rotational — up to the tire-slip dissipation and the
first-order integrator's residual. A leak or double-count anywhere along
battery → bus → motor → gears → wheels → vehicle shows up here.
"""
import pytest
from helpers import bev_axle, series

from app.solver import simulate

# catalog defaults for the rotating inertias in the bev_axle chain
J_MOTOR = 0.045       # motor.emotor inertia_kgm2 (motor axis)
J_FD_IN = 0.01        # mech.final_drive inertia_in_kgm2 (motor axis)
J_FD_OUT = 0.02       # mech.final_drive inertia_out_kgm2 (diff-input axis)
J_DIFF = 0.015        # mech.differential inertia_kgm2 (diff-input axis)
J_WHEEL = 1.2         # propulsion.wheel inertia_kgm2 (wheel axis)
MASS = 1500.0
RPM_TO_RAD = 2.0 * 3.141592653589793 / 60.0


def _lossless_accel_project():
    proj = bev_axle(profile="0:0; 5:100; 40:100")
    proj.cases[0].duration = 40
    proj.cases[0].timeStep = 0.1
    overrides = {
        "veh": {"cd": 0.0, "mass_kg": MASS},
        "mot": {"power_loss": {"0": {"0": 0}}, "drag_torque": {"0": 0}},
        "fd": {"efficiency_pct": 100},
        "diff": {"efficiency_pct": 100},
        "whl": {"rolling_resistance": 0.0, "slip_stiffness": 30, "mu": 1.2},
        "whr": {"rolling_resistance": 0.0, "slip_stiffness": 30, "mu": 1.2},
        "batt": {"internal_resistance_ohm": 1e-6, "capacity_kWh": 60},
    }
    for el in proj.systems[0].elements:
        if el.id in overrides:
            el.parameterOverrides.update(overrides[el.id])
    return proj


def test_battery_energy_matches_kinetic_energy():
    result = simulate(_lossless_accel_project(), "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]

    v = series(result, "veh", "sig_speed")[-1]["value"] / 3.6  # m/s
    w_wheel_l = series(result, "whl", "sig_speed")[-1]["value"] * RPM_TO_RAD
    w_wheel_r = series(result, "whr", "sig_speed")[-1]["value"] * RPM_TO_RAD
    w_motor = series(result, "mot", "sig_speed")[-1]["value"] * RPM_TO_RAD
    w_diff_in = series(result, "fd", "sig_speed_out")[-1]["value"] * RPM_TO_RAD

    ke = (
        0.5 * MASS * v * v
        + 0.5 * (J_MOTOR + J_FD_IN) * w_motor * w_motor
        + 0.5 * (J_FD_OUT + J_DIFF) * w_diff_in * w_diff_in
        + 0.5 * J_WHEEL * (w_wheel_l * w_wheel_l + w_wheel_r * w_wheel_r)
    )

    delivered_kwh = next(
        s.value for s in result.summary if s.label.endswith("energy delivered"))
    recuperated_kwh = next(
        s.value for s in result.summary if s.label.endswith("energy recuperated"))
    e_battery = (delivered_kwh - recuperated_kwh) * 3.6e6  # J

    # battery output must cover the kinetic energy exactly, plus only the
    # (small, positive) tire-slip dissipation and integrator residual
    ratio = e_battery / ke
    assert 0.995 < ratio < 1.10, f"battery {e_battery:.0f} J vs kinetic {ke:.0f} J (ratio {ratio:.4f})"


def test_summary_energy_matches_integrated_power_channel():
    """The summarized battery energy must equal the trapezoidal integral of the
    recorded battery power channel — recording and accounting must agree.

    Recorded at substep resolution (timeStep = MAX_SUBSTEP): coarser recording
    samples the launch transient too sparsely for the integral to close."""
    proj = _lossless_accel_project()
    proj.cases[0].timeStep = 0.01
    result = simulate(proj, "case")
    pts = series(result, "batt", "sig_power")
    e_wh = 0.0
    for (a, b) in zip(pts, pts[1:]):
        pa = max(0.0, a["value"]) * 1000.0
        pb = max(0.0, b["value"]) * 1000.0
        e_wh += 0.5 * (pa + pb) * (b["t"] - a["t"]) / 3600.0
    delivered_wh = next(
        s.value for s in result.summary if s.label.endswith("energy delivered")) * 1000.0
    assert delivered_wh == pytest.approx(e_wh, rel=0.02), (delivered_wh, e_wh)
