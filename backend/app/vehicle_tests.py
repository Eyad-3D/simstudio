"""One-click vehicle tests (CON-06): the figures vehicle engineers are asked
for, each from runs of the model as it is, with no hand-made cycle.

- 0-100 km/h and 80-120 km/h: full throttle from 0 (or from 80 km/h) until
  the target, a *Performance* case (VAL-39); the time is where the speed
  crosses the target.
- Top speed: full throttle towards 300 km/h for 120 s; the speed held
  over the last 10 s, when it changes by less than 0.2 km/h there, and
  what holds it back: a motor's maximum speed, an engine's rev limit, or
  the power (the road load takes all the drive gives). A speed still
  rising or falling at the end is "not settled", with the highest speed
  reached given on its own (a hybrid's battery boost fades, for one).
- Constant-speed consumption at 50, 90 and 120 km/h: 600 s at the speed,
  starting at it; with the battery's usable energy, the range at that speed
  (none when a fuel cell or a voltage source also supplies the car).
- Gradeability: the steepest constant grade the car climbs at 30 km/h,
  found by halving the interval between a grade it holds and one it does
  not (it holds when it stays within 1 km/h of the target over the last
  20 s of a 40 s run).
- Virtual coast-down: from 130 km/h with no pedal (the driver's gains at
  0), the deceleration between 125 and 15 km/h is fitted to
  F = A + B·v + C·v² (v in km/h) with F = m·a, m the Vehicle's mass plus
  the wheels' rotating inertia (Σ J / r²); the motor's and gears' drag are
  in it, as they are on a real coast-down.

A run that fails gives no figure: its row has no value and the run's
error as its note.

Writing tests as plain sentences is STU-39; distance-based tests are
STU-37's Acceleration case.
"""
from __future__ import annotations

from .label import uncounted_sources
from .library import library_by_id
from .schemas import DataBusConnection, ElementInstance, Project, SimCase
from .solver.network import resolve_params

TESTS = ("accel_0_100", "accel_80_120", "top_speed", "constant_speed", "gradeability", "coast_down")
STEADY_SPEEDS = (50, 90, 120)
GRADE_SPEED = 30.0
COAST_FROM, COAST_FIT = 130.0, (125.0, 15.0)
TOP_RUN_S, TOP_HOLD_S, TOP_SETTLED_KMH = 120.0, 10.0, 0.2  # top speed: run, window, change


class TestSetupError(ValueError):
    """The model cannot run these tests (no vehicle, no Driving Task, ...)."""


def _elements(project: Project) -> list[ElementInstance]:
    return [e for s in project.systems for e in s.elements]


def _one(project: Project, def_id: str, what: str) -> ElementInstance:
    found = [e for e in _elements(project) if e.componentDefId == def_id]
    if len(found) != 1:
        raise TestSetupError(f"The vehicle tests need exactly one {what}; this model has {len(found)}.")
    return found[0]


def _params(el: ElementInstance) -> dict:
    return resolve_params(el, library_by_id()[el.componentDefId])


def _run(project: Project, case: SimCase, extra: list | None = None):
    """Run a case added to a copy of the project (with extra elements and
    bus links, for the grade)."""
    from .solver import simulate
    trial = project.model_copy(deep=True)
    trial.cases = [case]
    for el, link in extra or []:
        trial.systems[0].elements.append(el)
        trial.dataBusConnections.append(link)
    return simulate(trial, case.id)


def _case(cid: str, name: str, kind: str, duration: float, task: ElementInstance,
          profile: str, overrides: dict | None = None) -> SimCase:
    ov = {task.id: {"profile": profile, "cycle": "", "repeat": False, "scale_pct": 100}}
    for el_id, values in (overrides or {}).items():
        ov.setdefault(el_id, {}).update(values)
    return SimCase(id=cid, name=name, kind=kind, duration=duration, timeStep=0.1,
                   realtimeFactor=0.0, parameterOverrides=ov)


def _series(result, el_id: str, port: str) -> list[tuple[float, float]]:
    ch = next((c for c in result.channels if c.elementId == el_id and c.portId == port), None)
    if ch is None:
        return []
    return [(p["t"], p["value"]) for p in ch.timeSeries if p["value"] is not None]


def _crossing(trace: list[tuple[float, float]], level: float) -> float | None:
    for (t0, v0), (t1, v1) in zip(trace, trace[1:]):
        if v0 < level <= v1:
            return t0 + (t1 - t0) * (level - v0) / (v1 - v0)
    return None


def _failure(result) -> str:
    """The run's errors when it failed, else ''."""
    if result.status != "failed":
        return ""
    return "; ".join(m.text for m in result.messages if m.level == "error")[:300] or "the run failed"


def _row(what: str, value: float | None, unit: str, how: str, note: str = "") -> dict:
    return {"what": what, "value": None if value is None else round(value, 3), "unit": unit,
            "how": how, "note": note}


def run_tests(project: Project, which: list[str] | None = None) -> dict:
    which = list(which or TESTS)
    unknown = [w for w in which if w not in TESTS]
    if unknown:
        raise TestSetupError(f"Unknown vehicle test: {', '.join(unknown)}.")
    veh = _one(project, "vehicle.body", "Vehicle")
    task = _one(project, "signal.driving_task", "Driving Task")
    _one(project, "driver.driver", "Driver")
    rows: list[dict] = []
    for test in which:
        rows += globals()[f"_{test}"](project, veh, task)
    return {"rows": rows, "note": "Simulated on the model as it is; not a certified test."}


def _accel(project, veh, task, v0: float, v1: float, label: str) -> list[dict]:
    case = _case("test-accel", label, "performance", 60.0, task, f"0:{v1:g}",
                 {veh.id: {"initial_speed_kmh": v0}} if v0 else None)
    r = _run(project, case)
    trace = _series(r, veh.id, "sig_speed")
    t_hit = _crossing(trace, v1)
    rows = {s.label: s for s in r.summary}
    note = "" if t_hit is not None else f"the car did not reach {v1:g} km/h in 60 s"
    if r.status == "failed":
        note = _failure(r)
    why = rows.get(f"Time to {v1:g} km/h")
    if why is not None and why.notValid:
        note = why.notValid
    return [_row(label, t_hit, "s", f"full throttle from {v0:g} km/h, time where the speed crosses "
                 f"{v1:g} km/h", note)]


def _accel_0_100(project, veh, task):
    return _accel(project, veh, task, 0.0, 100.0, "0-100 km/h")


def _accel_80_120(project, veh, task):
    return _accel(project, veh, task, 80.0, 120.0, "80-120 km/h")


def _slope(points: list[tuple[float, float]]) -> float:
    """Least-squares slope of (t, v) points, per second."""
    n = len(points)
    tm = sum(t for t, _ in points) / n
    vm = sum(v for _, v in points) / n
    den = sum((t - tm) ** 2 for t, _ in points)
    return sum((t - tm) * (v - vm) for t, v in points) / den if den > 0 else 0.0


def _top_speed(project, veh, task) -> list[dict]:
    r = _run(project, _case("test-top", "Top speed", "performance", TOP_RUN_S, task, "0:300"))
    how = (f"full throttle towards 300 km/h for {TOP_RUN_S:g} s, the mean speed over the last "
           f"{TOP_HOLD_S:g} s")
    trace = _series(r, veh.id, "sig_speed")
    if r.status == "failed" or not trace:
        return [_row("Top speed", None, "km/h", how, _failure(r) or "the run gave no speed")]
    t_end = trace[-1][0]
    held = [(t, v) for t, v in trace if t >= t_end - TOP_HOLD_S]
    v_held = sum(v for _, v in held) / len(held)
    change = _slope(held) * TOP_HOLD_S if len(held) > 1 else 0.0  # km/h over the window
    t_peak, v_peak = max(trace, key=lambda p: p[1])
    if abs(change) >= TOP_SETTLED_KMH:
        trend = "rising" if change > 0 else "falling"
        return [
            _row("Top speed", None, "km/h", how,
                 f"not settled in {TOP_RUN_S:g} s: still {trend} at the end ({change:+.1f} km/h "
                 f"over the last {TOP_HOLD_S:g} s, {trace[-1][1]:.1f} km/h at {t_end:g} s)"),
            _row("Highest speed reached", v_peak, "km/h",
                 f"full throttle towards 300 km/h for {TOP_RUN_S:g} s, the highest speed",
                 f"at {t_peak:g} s; not a speed the car holds"),
        ]
    limit = "the power: the road load takes all the drive gives"
    for el in _elements(project):
        p = _params(el)
        if el.componentDefId == "motor.emotor":
            speeds = [v for t, v in _series(r, el.id, "sig_speed") if t >= t_end - TOP_HOLD_S]
            n_max = float(p.get("max_speed_rpm") or 0) or max(
                float(k) for sheet in p["full_load_torque"].values() for k in sheet)
            if speeds and max(abs(s) for s in speeds) >= 0.97 * n_max:
                limit = f"{el.label}'s maximum speed ({n_max:,.0f} 1/min)"
        elif el.componentDefId == "engine.combustion":
            speeds = [v for t, v in _series(r, el.id, "sig_speed") if t >= t_end - TOP_HOLD_S]
            n_max = max(float(k) for k in p["full_load_torque"])
            if speeds and max(abs(s) for s in speeds) >= 0.97 * n_max:
                limit = f"{el.label}'s rev limit ({n_max:,.0f} 1/min)"
    return [_row("Top speed", v_held, "km/h", how, f"limited by {limit}")]


def _constant_speed(project, veh, task) -> list[dict]:
    rows = []
    engines = any(e.componentDefId == "engine.combustion" for e in _elements(project))
    # a fuel cell's or a voltage source's energy is not in the Consumption,
    # so no range follows from it
    others = uncounted_sources(project)
    usable_kwh = 0.0
    for el in _elements(project):
        if el.componentDefId == "battery.generic":
            p = _params(el)
            usable_kwh += float(p["capacity_kWh"]) * (1 - float(p["min_soc_pct"]) / 100)
    for v in STEADY_SPEEDS:
        case = _case(f"test-steady-{v}", f"Constant {v} km/h", "cycle", 600.0, task,
                     f"0:{v}; 600:{v}", {veh.id: {"initial_speed_kmh": v}})
        r = _run(project, case)
        if r.status == "failed":
            label, unit = ("Fuel consumption", "l/100km") if engines else ("Consumption", "kWh/100km")
            rows.append(_row(f"{label} at {v} km/h", None, unit,
                             f"600 s at a constant {v} km/h, from that speed", _failure(r)))
            continue
        s = {x.label: x for x in r.summary}
        for label, unit in (("Consumption", "kWh/100km"), ("Fuel consumption", "l/100km")):
            if label in s and not (engines and label == "Consumption"):  # a hybrid's is its charge
                note = s[label].notValid or ""
                if label == "Consumption" and others:
                    note = "; ".join(x for x in (note, f"at the battery only, without the energy "
                                                       f"of {', '.join(others)}") if x)
                rows.append(_row(f"{label} at {v} km/h", s[label].value, unit,
                                 f"600 s at a constant {v} km/h, from that speed", note))
                if label == "Consumption" and usable_kwh > 0 and s[label].value > 0 and not others:
                    rows.append(_row(f"Range at {v} km/h", usable_kwh / s[label].value * 100, "km",
                                     "the batteries' Usable Capacity above their Minimum SOC ÷ "
                                     "the consumption", note))
    return rows


def _grade_link(project: Project, veh: ElementInstance, grade_pct: float):
    """A Road Profile at a constant grade, linked to the Vehicle; None when
    something else already drives the Vehicle's grade."""
    for link in project.dataBusConnections:
        for a, b in ((1, 2), (2, 1)):
            if getattr(link, f"element{a}Id") == veh.id and getattr(link, f"port{a}Id") == "sig_grade_in":
                return getattr(link, f"element{b}Id")
    el = ElementInstance(id="test-grade", componentDefId="signal.road_profile", label="Test grade",
                         position={"x": 0, "y": 0},
                         parameterOverrides={"profile": f"0:{grade_pct:g}; 1:{grade_pct:g}",
                                             "mode": "time", "cycle": ""})
    link = DataBusConnection(id="test-grade-link", element1Id=el.id, port1Id="sig_grade",
                             element2Id=veh.id, port2Id="sig_grade_in")
    return el, link


def _holds(project, veh, task, grade: float) -> tuple[bool, str]:
    """Whether the car holds the speed on the grade, and the run's errors
    when it failed (a car that rolls back on a steep grade fails its run
    for not driving the cycle: that is a grade it does not hold)."""
    extra = _grade_link(project, veh, grade)
    overrides = {veh.id: {"initial_speed_kmh": GRADE_SPEED}}
    if isinstance(extra, str):  # the model's own Road Profile: give it the grade
        overrides[extra] = {"profile": f"0:{grade:g}; 1:{grade:g}", "mode": "time", "cycle": ""}
        extra = None
    case = _case("test-grade", f"Grade {grade:g} %", "cycle", 40.0, task,
                 f"0:{GRADE_SPEED:g}; 40:{GRADE_SPEED:g}", overrides)
    r = _run(project, case, [extra] if extra else None)
    if r.status == "failed":
        return False, _failure(r)
    late = [v for t, v in _series(r, veh.id, "sig_speed") if t >= 20.0]
    return bool(late) and min(late) >= GRADE_SPEED - 1.0, ""


def _gradeability(project, veh, task) -> list[dict]:
    lo, hi = 0.0, 60.0
    flat, failed = _holds(project, veh, task, lo)
    if not flat:
        return [_row(f"Steepest grade at {GRADE_SPEED:g} km/h", None, "%", "",
                     f"the run on the flat failed: {failed}" if failed
                     else "the car does not hold the speed on the flat")]
    if _holds(project, veh, task, hi)[0]:
        return [_row(f"Steepest grade at {GRADE_SPEED:g} km/h", hi, "%",
                     "the car held the speed on the steepest grade tried", "60 % or more")]
    while hi - lo > 0.5:
        mid = (lo + hi) / 2
        lo, hi = (mid, hi) if _holds(project, veh, task, mid)[0] else (lo, mid)
    return [_row(f"Steepest grade at {GRADE_SPEED:g} km/h", lo, "%",
                 f"the steepest constant grade the car climbs within 1 km/h of {GRADE_SPEED:g} km/h "
                 "(found to 0.5 %)")]


def _coast_down(project, veh, task) -> list[dict]:
    drivers = [e for e in _elements(project) if e.componentDefId == "driver.driver"]
    overrides = {veh.id: {"initial_speed_kmh": COAST_FROM}}
    for d in drivers:
        overrides[d.id] = {"driver_kp": 0.0, "driver_ki": 0.0}
    case = _case("test-coast", "Coast-down", "cycle", 400.0, task, "0:0; 400:0", overrides)
    r = _run(project, case)
    if r.status == "failed":
        return [_row("Coast-down A (f0)", None, "N", "", _failure(r))]
    trace = [(t, v) for t, v in _series(r, veh.id, "sig_speed")]
    vp = _params(veh)
    mass = float(vp["mass_kg"])
    j_over_r2 = 0.0
    for el in _elements(project):
        if el.componentDefId == "propulsion.wheel":
            p = _params(el)
            j_over_r2 += float(p["inertia_kgm2"]) / float(p["radius_m"]) ** 2
    m_eff = mass + j_over_r2
    pts = []  # (v km/h, F N)
    for (t0, v0), (t1, v1) in zip(trace, trace[1:]):
        vm = (v0 + v1) / 2
        if COAST_FIT[1] <= vm <= COAST_FIT[0] and t1 > t0:
            pts.append((vm, -m_eff * (v1 - v0) / 3.6 / (t1 - t0)))
    if len(pts) < 10:
        return [_row("Coast-down A", None, "N", "", "the car did not coast from 125 to 15 km/h in 400 s")]
    a, b, c = _fit_quadratic(pts)
    how = (f"from {COAST_FROM:g} km/h with no pedal; F = (mass + Σ wheel J / r², {m_eff:,.0f} kg) "
           f"× deceleration, fitted over {COAST_FIT[0]:g}-{COAST_FIT[1]:g} km/h")
    return [_row("Coast-down A (f0)", a, "N", how),
            _row("Coast-down B (f1)", b, "N/(km/h)", how),
            _row("Coast-down C (f2)", c, "N/(km/h)²", how)]


def _fit_quadratic(pts: list[tuple[float, float]]) -> tuple[float, float, float]:
    """Least squares F = A + B·v + C·v² (normal equations, 3 x 3)."""
    s = [sum(v ** k for v, _ in pts) for k in range(5)]
    t = [sum(f * v ** k for v, f in pts) for k in range(3)]
    m = [[s[0], s[1], s[2]], [s[1], s[2], s[3]], [s[2], s[3], s[4]]]
    det = (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
           - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
           + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]))

    def solve(col: int) -> float:
        mm = [row[:] for row in m]
        for i in range(3):
            mm[i][col] = t[i]
        return (mm[0][0] * (mm[1][1] * mm[2][2] - mm[1][2] * mm[2][1])
                - mm[0][1] * (mm[1][0] * mm[2][2] - mm[1][2] * mm[2][0])
                + mm[0][2] * (mm[1][0] * mm[2][1] - mm[1][1] * mm[2][0])) / det
    return solve(0), solve(1), solve(2)
