"""Saved results keep full precision and the peaks between recorded points
(ENG-16): each channel carries its lowest, highest and time-averaged value
over every output interval, taken at every solver step, and no stored or
summary number is rounded."""
from __future__ import annotations

import pytest

from app.solver import simulate
from app.storage import load_example
from tests.helpers import example_result


def channel(result, el_id: str, port_id: str):
    return next(c for c in result.channels if c.elementId == el_id and c.portId == port_id)


def summary(result, label: str) -> float:
    return next(s.value for s in result.summary if s.label == label)


def test_short_regen_bursts_show_in_the_stored_minimum():
    fine = example_result("bev-car", "case-city")  # a point every 1 s, 100 solver steps each
    p = load_example("bev-car")
    p.cases[0].outputEvery = 10  # a point every 10 s
    coarse = simulate(p, "case-city")
    power = channel(coarse, "el-battery", "sig_power")  # kW, negative while recuperating
    assert power.min is not None and power.max is not None and power.mean is not None
    assert len(power.min) == len(power.timeSeries) == 61
    # the strongest recuperation falls between two of the coarse points, but
    # the envelope keeps it: the same as every solver step of the fine run
    recorded = min(p["value"] for p in power.timeSeries)
    lowest = min(channel(fine, "el-battery", "sig_power").min)
    assert lowest < -10.0
    assert recorded > lowest + 1.0
    assert min(power.min) == pytest.approx(lowest, abs=1e-9)
    assert max(power.max) == pytest.approx(max(channel(fine, "el-battery", "sig_power").max),
                                           abs=1e-9)
    for p, lo, hi, mean in zip(power.timeSeries, power.min, power.max, power.mean):
        assert lo <= p["value"] <= hi
        assert lo - 1e-9 <= mean <= hi + 1e-9


def test_energy_figures_match_the_solver_step_integral():
    r = example_result("bev-car", "case-city")
    power = channel(r, "el-battery", "sig_power")  # kW, its interval means over solver steps
    ts = [p["t"] for p in power.timeSeries]
    kwh = sum(m * (b - a) for m, a, b in zip(power.mean[1:], ts, ts[1:])) / 3600.0
    net = (summary(r, "HV Battery Pack — energy delivered")
           - summary(r, "HV Battery Pack — energy recuperated"))
    assert kwh == pytest.approx(net, rel=1e-6)


def test_no_number_is_rounded():
    r = example_result("bev-car", "case-city")
    values = [s.value for s in r.summary] + [
        p["value"] for c in r.channels for p in c.timeSeries if p["value"] is not None]
    # a value rounded to 5 decimals (or fewer) is a multiple of 1e-5
    unrounded = [v for v in values if abs(v * 1e5 - round(v * 1e5)) > 1e-3]
    assert len(unrounded) > 0.5 * len(values)


def test_one_kilogram_moves_the_figures():
    def run(dm: float):
        p = load_example("bev-car")
        veh = next(e for s in p.systems for e in s.elements if e.componentDefId == "vehicle.body")
        veh.parameterOverrides["mass_kg"] = veh.parameterOverrides["mass_kg"] + dm
        r = simulate(p, "case-city")
        return summary(r, "HV Battery Pack — final SOC"), summary(r, "Consumption")

    (soc_lo, use_lo), (soc_0, use_0), (soc_hi, use_hi) = run(-1.0), run(0.0), run(1.0)
    # heavier: less charge left, more energy per km; each kilogram about the same
    assert soc_lo > soc_0 > soc_hi
    assert use_lo < use_0 < use_hi
    assert (use_hi - use_0) == pytest.approx(use_0 - use_lo, rel=0.5)


def test_no_envelope_when_every_point_is_one_solver_step():
    p = load_example("bev-car")
    case = p.cases[0]
    case.timeStep, case.duration = 0.01, 5.0
    r = simulate(p, case.id)
    assert all(c.min is None and c.max is None and c.mean is None for c in r.channels)
