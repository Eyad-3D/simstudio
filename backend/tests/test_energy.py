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
from app.solver.runtime import V_EPS

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


def _tire_slip_loss_j(result, dt):
    """Energy the tires dissipate by slipping: force × slip speed, where the
    recorded slip is (ω·r − v) / max(|v|, V_EPS)."""
    speed = series(result, "veh", "sig_speed")
    total = 0.0
    for w in ("whl", "whr"):
        force = series(result, w, "sig_force")
        slip = series(result, w, "sig_slip")
        total += sum(force[i]["value"] * slip[i]["value"] * max(speed[i]["value"] / 3.6, V_EPS)
                     for i in range(1, len(force))) * dt
    return total


def test_battery_energy_matches_kinetic_energy():
    """Recorded at the solver step, so the recorded powers are exactly the
    ones each step used. The battery's net output must equal the kinetic
    energy plus the tire-slip loss within 0.5 %: a 5 % leak anywhere along
    battery → bus → motor → gears → wheels → vehicle fails here (before,
    the bound allowed the battery to deliver up to 10 % more)."""
    proj = _lossless_accel_project()
    proj.cases[0].timeStep = 0.01
    result = simulate(proj, "case")
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
    slip = _tire_slip_loss_j(result, 0.01)
    assert 0.0 < slip < 0.05 * ke  # stiff tires: a small, positive loss

    e_battery = sum(p["value"] for p in series(result, "batt", "sig_power")[1:]) * 1000.0 * 0.01
    e_motor = sum(p["value"] for p in series(result, "mot", "sig_elec_power")[1:]) * 1000.0 * 0.01
    assert e_motor == pytest.approx(e_battery, rel=1e-4)  # no loss on the bus or in R0 ≈ 0

    ratio = e_battery / (ke + slip)
    assert 0.995 < ratio < 1.005, (
        f"battery {e_battery:.0f} J vs kinetic {ke:.0f} J + tire slip {slip:.0f} J "
        f"(ratio {ratio:.4f})")

    s = {x.label: x.value for x in result.summary}
    net_kwh = s["Battery — energy delivered"] - s["Battery — energy recuperated"]
    assert net_kwh * 3.6e6 == pytest.approx(e_battery, abs=0.0011 * 3.6e6)  # 3-decimal rounding
    assert s["Electrical energy balance error"] == 0.0


def test_summary_energy_matches_integrated_power_channel():
    """The summarized battery energy must equal the integral of the
    recorded battery power channel — recording and accounting must agree
    to the summary's 3-decimal rounding (0.3 % here; before, 2 %).

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
    assert delivered_wh == pytest.approx(e_wh, abs=0.6), (delivered_wh, e_wh)
