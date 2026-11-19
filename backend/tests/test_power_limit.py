"""A battery's Output Power Limit and Voltage Class (MOD-39).

The limit holds the power at the battery's terminals (volts × amps, as the
Formula Student energy meter measures it) on the discharge side of the
source-limit handshake; recuperation is not limited. With a limit or a class
set, the run summary checks the terminal power (averaged over a window) and
the pack voltage against them, with pass/fail on the rows."""
import random

import pytest
from helpers import bev_axle, series

from app.library import load_library
from app.solver import simulate
from app.solver.runtime import TerminalCheck, ocv_mean
from app.storage import load_example

BAT = "Battery"
DEFAULT_OCV = [(0, 300.0), (10, 318.0), (20, 330.0), (40, 342.0), (60, 352.0), (80, 362.0),
               (90, 368.0), (100, 376.0)]


def _car(profile: str, duration: float, dt: float, performance: bool = False, v0: float = 0.0,
         **battery):
    proj = bev_axle(profile=profile)
    case = proj.cases[0]
    case.duration, case.timeStep = duration, dt
    if performance:
        case.kind = "performance"
    for e in proj.systems[0].elements:
        if e.id == "batt":
            e.parameterOverrides.update(battery)
        if e.id == "veh" and v0:
            e.parameterOverrides["initial_speed_kmh"] = v0
    return proj


def _rows(result):
    return {s.label: s for s in result.summary}


def _values(result, el_id, port):
    return [p["value"] for p in series(result, el_id, port)]


def test_output_power_limit_holds_terminal_v_times_i():
    """The item's metric: the cap matches V·I at the terminals within 0.1 %
    (measured 2.5e-7 on the recorded channels). Without it the car draws
    120 kW."""
    result = simulate(_car("0:0; 0.01:100; 30:100", 20.0, 0.01, performance=True,
                           output_power_limit_kW=20), "case")
    vi = [v * i / 1000.0 for v, i in zip(_values(result, "batt", "sig_voltage"),
                                         _values(result, "batt", "sig_current"))]
    assert abs(max(vi) / 20.0 - 1.0) <= 1e-3
    assert all(p <= 20.0 * (1 + 1e-3) for p in vi)
    assert max(_values(result, "mot", "sig_elec_power")) <= 20.0 * (1 + 1e-3)
    s = _rows(result)
    assert s["Electrical energy balance error"].value == 0.0
    assert s[f"{BAT} — peak terminal power"].value == pytest.approx(20.0, rel=1e-3)
    held = s[f"{BAT} — time held at the output power limit"].value
    assert held == pytest.approx(s["E-Motor — time limited by supply"].value, abs=0.02)
    assert held > 19.0  # measured 19.76 s: the cap holds the whole run after launch
    assert result.status == "success", [m.text for m in result.messages]
    assert any(m.level == "info" and "held at its Output Power Limit (20 kW" in m.text
               for m in result.messages)


def test_power_limit_leaves_recuperation_alone():
    """FS Rules 2026 EV 2.2.3: regeneration is unrestricted in power, so the
    limit caps the discharge side only. Speeding up from 100 to 120 km/h
    draws 117 kW without the limit; braking to a stop then feeds back
    99.3 kW, with the 20 kW limit too."""
    result = simulate(_car("0:100; 2:120; 5:120; 10:0; 14:0", 14.0, 0.1, v0=100,
                           output_power_limit_kW=20), "case")
    power = _values(result, "batt", "sig_power")
    assert max(power) == pytest.approx(20.0, rel=1e-3)
    assert min(power) < -90.0


def test_check_only_flags_a_run_over_the_limit():
    """With Hold Power to Limit off the car drives as without a limit and the
    averaged power check fails; with it on the same check passes at 80 kW."""
    free = simulate(_car("0:0; 0.01:100; 30:100", 20.0, 0.1, performance=True,
                         output_power_limit_kW=80, power_limit_window_s=0.5,
                         power_limit_enforced=False), "case")
    s = _rows(free)
    avg = s[f"{BAT} — peak terminal power, averaged"]
    assert avg.value == pytest.approx(119.95, abs=0.1)
    assert (avg.limit, avg.passed) == (80.0, False)
    assert s[f"{BAT} — time over the output power limit"].value > 0
    assert free.status == "warning"
    assert any(m.level == "warning" and "Output Power Limit" in m.text and "80 kW" in m.text
               for m in free.messages)
    assert s["Time to 100 km/h"].value == pytest.approx(8.45, abs=0.005)  # as without any limit

    held = simulate(_car("0:0; 0.01:100; 30:100", 20.0, 0.1, performance=True,
                         output_power_limit_kW=80, power_limit_window_s=0.5), "case")
    avg = _rows(held)[f"{BAT} — peak terminal power, averaged"]
    assert avg.value == pytest.approx(80.0, rel=1e-3)
    assert (avg.limit, avg.passed) == (80.0, True)
    assert held.status == "success", [m.text for m in held.messages]


def _bev_performance(**battery):
    proj = load_example("bev-car")
    case = proj.cases[0]
    case.kind, case.duration, case.timeStep = "performance", 20.0, 0.1
    case.parameterOverrides = {**case.parameterOverrides, "el-task": {"profile": "0:100; 20:100"}}
    next(e for e in proj.systems[0].elements
         if e.id == "el-battery").parameterOverrides.update(battery)
    return simulate(proj, case.id)


def test_fs_limit_on_the_bev_example_succeeds():
    """The bev-car example (150 kW motor) held to 80 kW over a 0.5 s window:
    a success, not a warning about the battery's maximum-power point (which
    the cells never reached). Its 0-100 km/h goes from 7.10 s to 12.76 s."""
    label = "HV Battery Pack"
    result = _bev_performance(output_power_limit_kW=80, power_limit_window_s=0.5)
    assert result.status == "success", [m.text for m in result.messages]
    s = _rows(result)
    assert s["Time to 100 km/h"].value == pytest.approx(12.76, abs=0.05)
    avg = s[f"{label} — peak terminal power, averaged"]
    assert avg.value <= 80.0 and avg.passed is True
    assert s[f"{label} — time held at the output power limit"].value == pytest.approx(11.67, abs=0.05)
    assert any(m.level == "info" and "held at its Output Power Limit" in m.text
               for m in result.messages)

    # a 2.5 % margin: the car runs at 78 kW, the check stays against 80 kW
    result = _bev_performance(output_power_limit_kW=80, power_limit_window_s=0.5,
                              power_limit_margin_pct=2.5)
    s = _rows(result)
    assert s[f"{label} — peak terminal power"].value == pytest.approx(78.0, rel=1e-3)
    assert s["Time to 100 km/h"].value == pytest.approx(13.08, abs=0.05)
    avg = s[f"{label} — peak terminal power, averaged"]
    assert (avg.limit, avg.passed) == (80.0, True)
    # the message and the time row name the margin, not the reduced cap as the limit
    assert s[f"{label} — time held at the power cap (limit less margin)"].value > 0
    assert f"{label} — time held at the output power limit" not in s
    assert any(m.level == "info" and "held at its Output Power Limit less its 2.5 % margin (78 of "
               "80 kW at the terminals)" in m.text for m in result.messages)


def _tracked(steps, window):
    chk = TerminalCheck(limit_w=80e3, window_s=window, v_class=0.0, v_full=0.0)
    for t0, t1, p in steps:
        chk.add(t0, t1, p, 0.0, False)
    return chk.avg_peak_w


def test_moving_average_window():
    """The check's moving average of the stepwise terminal power, with no
    power before t = 0, against closed forms and a brute-force integral."""
    dt = 0.01

    def pulse(duration):
        return [(k * dt, (k + 1) * dt, 100e3 if 1.0 <= k * dt < 1.0 + duration - 1e-9 else 0.0)
                for k in range(300)]

    assert _tracked(pulse(0.2), 0.5) == pytest.approx(40e3, rel=1e-9)
    assert _tracked(pulse(1.0), 0.5) == pytest.approx(100e3, rel=1e-9)
    start = [(k * dt, (k + 1) * dt, 100e3) for k in range(10)]
    assert _tracked(start, 0.5) == pytest.approx(20e3, rel=1e-9)

    rng = random.Random(1)
    t, steps = 0.0, []
    for _ in range(2000):
        h = rng.choice((0.01, 0.003, 0.03, 0.007))
        steps.append((t, t + h, rng.uniform(-50e3, 120e3)))
        t += h

    def brute(w):
        return max(sum(p * max(0.0, min(b, t1) - max(a, t1 - w)) for a, b, p in steps) / w
                   for _, t1, _ in steps)

    assert _tracked(steps, 0.5) == pytest.approx(brute(0.5), rel=1e-12)
    assert _tracked(steps, 0.0) == max(p for *_, p in steps)


def _pack(cells: int, r0: float, **battery):
    """A pack of `cells` Li-ion cells in series (4.2 V full), braking from
    100 km/h at 95 % SOC."""
    cell = {0: 3.0, 10: 3.45, 20: 3.55, 40: 3.65, 60: 3.8, 80: 3.95, 90: 4.05, 100: 4.2}
    table = {str(k): round(v * cells, 3) for k, v in cell.items()}
    return _car("0:100; 3:100; 8:0; 12:0", 12.0, 0.1, v0=100, ocv_table=table,
                initial_soc_pct=95, capacity_kWh=7, internal_resistance_ohm=r0, **battery)


def test_voltage_class_run_check():
    """The highest pack voltage is the higher of the open-circuit voltage at
    100 % SOC and the terminal voltage while recuperating. (The status is not
    asserted: the default motor's 250-396 V map warns on these packs.)"""
    result = simulate(_pack(150, 0.05, voltage_class_V=600), "case")
    s = _rows(result)
    row = s[f"{BAT} — maximum pack voltage"]
    assert (row.value, row.limit, row.passed) == (630.0, 600.0, False)
    assert any(m.level == "warning" and "630.0 V open-circuit at 100 % SOC" in m.text
               for m in result.messages)

    result = simulate(_pack(140, 0.3, voltage_class_V=600), "case")
    row = _rows(result)[f"{BAT} — maximum pack voltage"]
    assert row.value == pytest.approx(627.5, abs=0.5) and row.passed is False
    assert any(m.level == "warning" and "while recuperating at t = " in m.text
               for m in result.messages)

    row = _rows(simulate(_pack(140, 0.05, voltage_class_V=600), "case"))[
        f"{BAT} — maximum pack voltage"]
    assert row.value == pytest.approx(588.6, abs=0.5) and row.passed is True
    # a class alone gives no power rows
    assert not [label for label in s if "terminal power" in label]


def test_usable_energy_left():
    """What the battery can still give before its minimum SOC, from the OCV
    table (hand trapezoids for the default table from 90 to 10 %); it fails
    when the battery reached its minimum SOC."""
    idle = simulate(_car("0:0; 20:0", 20.0, 0.1, output_power_limit_kW=80), "case")
    q_ah = 60e3 / ocv_mean(DEFAULT_OCV)
    by_hand = q_ah / 100 * (10 * (318 + 330) / 2 + 20 * (330 + 342) / 2 + 20 * (342 + 352) / 2
                            + 20 * (352 + 362) / 2 + 10 * (362 + 368) / 2) / 1e3
    row = _rows(idle)[f"{BAT} — usable energy left"]
    assert _rows(idle)[f"{BAT} — final SOC"].value == 90.0
    assert row.value == pytest.approx(by_hand, abs=1e-3) and row.passed is True
    assert (ocv_mean(DEFAULT_OCV, lo=10.0, hi=90.0) * 0.8 * q_ah / 1e3
            == pytest.approx(by_hand, rel=1e-12))

    empty = simulate(_car("0:0; 0.01:100; 30:100", 20.0, 0.1, performance=True,
                          output_power_limit_kW=80, capacity_kWh=0.2), "case")
    row = _rows(empty)[f"{BAT} — usable energy left"]
    assert row.value == pytest.approx(0.0, abs=1e-9) and row.passed is False


def test_cancelled_run_marks_passes_not_valid():
    """A check a stopped run passed so far says so."""
    calls = {"n": 0}

    def control():
        calls["n"] += 1
        return [{"type": "cancel"}] if calls["n"] == 11 else []

    result = simulate(_car("0:0; 0.01:100; 30:100", 20.0, 0.1, performance=True,
                           output_power_limit_kW=80, power_limit_window_s=0.5), "case",
                      control=control)
    assert result.status == "cancelled"
    avg = _rows(result)[f"{BAT} — peak terminal power, averaged"]
    assert avg.passed is True and avg.notValid.startswith("run cancelled at t = 1")


def test_presets_name_real_parameters():
    """A preset only sets parameters its component has, with values of their
    type; the Formula Student one names the rules it was read from."""
    for comp in load_library():
        types = {p.key: p.type for p in comp.parameters}
        for preset in comp.presets:
            for key, value in preset.values.items():
                assert key in types, f"{comp.id} preset '{preset.name}': no parameter {key}"
                if types[key] == "boolean":
                    assert isinstance(value, bool), (comp.id, key)
                else:
                    assert types[key] == "number" and not isinstance(value, bool), (comp.id, key)
    battery = next(c for c in load_library() if c.id == "battery.generic")
    fs = next(p for p in battery.presets if p.name == "Formula Student Electric")
    # (500 A: FS Rules 2026 v1.1 (FSG) EV 2.2.2, the pack's current limit since MOD-08)
    assert fs.values == {"output_power_limit_kW": 80, "power_limit_window_s": 0.5,
                         "voltage_class_V": 600, "power_limit_enforced": True,
                         "max_discharge_current_A": 500}
    for rules in ("FS Rules 2026 v1.1 (FSG)", "FSUK 2026", "FSAE Rules 2025", "500 A",
                  "current season"):
        assert rules in fs.note


def test_no_limit_changes_nothing():
    """The defaults (no limit, no class) add no rows and no messages."""
    plain = simulate(_car("0:0; 0.01:100; 30:100", 5.0, 0.1, performance=True), "case")
    assert not [s for s in plain.summary if s.limit is not None or s.passed is not None]
    assert not [s for s in plain.summary if "terminal" in s.label or "pack voltage" in s.label]


@pytest.mark.parametrize("margin", [120.0, -20.0])
def test_a_margin_outside_0_to_100_percent_is_refused_not_a_crash(margin):
    """A case's Power Limit Margin of 120 % once gave a negative power cap
    and a ZeroDivisionError in the source-limit handshake. Data Checks
    refuse it for that case; run without them (the solver called directly),
    the margin is held to 0-100 % and the run ends normally."""
    from app.validation import run_blockers, validate_project

    proj = _car("0:0; 0.01:100; 30:100", 2.0, 0.1, performance=True, output_power_limit_kW=40)
    case = proj.cases[0]
    case.parameterOverrides = {"batt": {"power_limit_margin_pct": margin}}
    blockers = run_blockers(validate_project(proj), case.id)
    assert [c.text for c in blockers] == [
        f"Power Limit Margin of '{BAT}' in case '{case.name}' must be at least 0 and at most "
        f"100 % — got {margin:g}."]
    result = simulate(proj, case.id)
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    peak = _rows(result)[f"{BAT} — peak terminal power"].value
    assert peak == (pytest.approx(0.0, abs=1e-6) if margin > 100 else pytest.approx(40.0, rel=1e-3))
