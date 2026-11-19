"""The energy books close to round-off (MOD-10): every part books the power
the solver's step actually moved through it, so in − out over all parts is
zero but for floating-point rounding, and the stored kinetic energy is what
the speeds hold.

Before, the books read each torque at the speed a step started with while
the step integrated the speed to its end: a motor spinning up an inertia at
the 10 ms step gave the inertia T²·dt²/2J more than its books said, and
1.3 % of the energy of the benchmarks' DC-motor spin-up went unaccounted for.
The same happened at every wheel (its tyre force times the speed at the
step's end), at a brake that stopped a shaft within a step and at a car that
stopped within one."""
from __future__ import annotations

import math

import pytest
from helpers import conn, dbc, el, example_result, project

from app.solver import simulate

RPM = 60.0 / (2.0 * math.pi)
KWH = 3.6e6


def _residual_pct(result) -> float:
    return next(s.value for s in result.summary if s.label == "Energy balance residual")


def _books(result) -> dict:
    return {f.label: f for f in result.partEnergy}


def _dc_motor(step: float = 0.01, duration: float = 1.5):
    """benchmarks/reference/problems/motor_dc_spinup_l0.toml as the
    benchmarks express it: a 48 V source, an E-Motor whose full-load curve is
    a DC motor's torque-speed line and whose loss map is R i² + b ω², at full
    command on its own rotor inertia."""
    V, R, k, J, b = 48.0, 0.1, 0.1, 0.02, 1e-4
    w0 = k * V / (k * k + R * b)
    line = {"0": k * V / R, f"{w0 * RPM:.9g}": 0.0}
    loss = {}
    for i in range(401):
        w = w0 * 1.2 * i / 400
        kw = ((V - k * w) ** 2 / R + b * w * w) / 1000.0
        loss[f"{w * RPM:.9g}"] = {"0": kw, "100": kw}
    elements = [el("src", "electric.voltage_source", "Supply", voltage_V=V),
                el("bus", "electric.node", "Bus"),
                el("mot", "motor.emotor", "E-Motor", inertia_kgm2=J,
                   full_load_torque={"10": line, "100": line}, power_loss=loss,
                   drag_torque={"0": 0, f"{w0 * 1.2 * RPM:.9g}": 0},
                   max_speed_rpm=round(w0 * 1.2 * RPM, 3)),
                el("load", "propulsion.propeller", "Load", torque_ref_Nm=0, inertia_kgm2=0),
                el("dem", "signal.constant", "Demand", value=1.0)]
    connections = [conn(1, "src", "pos", "bus", "t1"), conn(2, "bus", "t2", "mot", "pos"),
                   conn(3, "mot", "shaft", "load", "shaft")]
    return project(elements, connections, [dbc(1, "dem", "sig_out", "mot", "sig_demand_in")],
                   duration=duration, time_step=step), J


def test_a_motor_spinning_up_an_inertia_closes_its_books():
    proj, J = _dc_motor()
    r = simulate(proj, "case")
    books = _books(r)
    supplied = books["Supply"].energyOut * KWH
    speed = next(c for c in r.channels if c.elementId == "mot" and c.portId == "sig_speed")
    held = 0.5 * J * (speed.timeSeries[-1]["value"] / RPM) ** 2
    rotating = books["Rotating parts"].stored * KWH
    assert rotating == pytest.approx(held, rel=1e-12)
    # the motor gave the shaft what the inertia holds, and lost the rest
    motor = books["E-Motor"]
    assert motor.energyOut * KWH == pytest.approx(held, rel=1e-12)
    assert (motor.losses * KWH + held) == pytest.approx(supplied, rel=1e-12)
    assert abs(_residual_pct(r)) < 1e-9  # % (it was 1.27 %)


@pytest.mark.parametrize("project_id,case_id", [
    ("bev-car", "case-city"), ("hybrid-car", "case-mixed"), ("fs-electric", "case-accel-75m")])
def test_the_examples_close_their_books(project_id, case_id):
    """Every part's books together: the battery, motors, engine, clutch,
    gears, brakes, tyres, rotating parts and the Vehicle (0.011 %, 0.019 %
    and 0.39 % before)."""
    r = example_result(project_id, case_id)
    assert abs(_residual_pct(r)) < 1e-8
