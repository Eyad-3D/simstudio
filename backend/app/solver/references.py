"""Expected values and hand calculations (VAL-35).

A case can carry *reference values*: a number the user trusts (a maker's
0-100 km/h time, last year's measured 75 m time, a hand calculation) for one
summary value, with a tolerance and a note on where it comes from. After
every run each one gets a grade:

- *within*: the gap is at most the tolerance (green),
- *near*: at most twice the tolerance (amber),
- *outside*: more than that (red),
- *missing*: the run has no summary value of that name (grey),

and *not valid* when the run itself rules the value out.

Two automatic hand calculations come with every run, marked as such:

- *top speed*: no vehicle speed above what each E-Motor's maximum speed
  allows through the gears (its maximum speed ÷ the overall ratio × the
  wheel radius, the ratio read from the run at its fastest point),
- *energy*: the energy the batteries, voltage sources and fuel cells gave
  is at least the car's gain in motion and height plus the road-load work
  over the speed it drove, summed by hand from the recorded speed (every
  other loss only adds to what the sources must give).

They catch a car that drives on energy from nowhere or a motor run past
its map, which the summary alone would not show.
"""
from __future__ import annotations

import math

from ..schemas import Grade, ReferenceCheck, ReferenceValue

# the sum by hand works on the recorded points (1 s by default), a few
# tenths of a percent off the solver's own sum; a bound is failed only
# beyond this share
HAND_SUM_MARGIN = 0.03
G = 9.80665


def grade_gap(diff: float, tol: float) -> Grade:
    """Green within the tolerance, amber within twice it, red beyond.
    frontend/src/references.ts grades the same way."""
    d = abs(diff)
    if d <= tol * (1 + 1e-9):
        return "within"
    return "near" if d <= 2 * tol * (1 + 1e-9) else "outside"


def check_references(references, summary) -> list[ReferenceCheck]:
    """Each reference value against the summary row with its label."""
    rows = {s.label: s for s in summary}
    out: list[ReferenceCheck] = []
    for ref in references:
        r = ref if isinstance(ref, ReferenceValue) else ReferenceValue.model_validate(ref)
        tol = abs(r.tolerance) * (abs(r.value) / 100.0 if r.tolerancePct else 1.0)
        row = rows.get(r.kpi)
        if row is None:
            out.append(ReferenceCheck(label=r.kpi, reference=r.value, tolerance=tol,
                                      grade="missing", source=r.source,
                                      note="this run has no summary value of that name"))
            continue
        diff = row.value - r.value
        out.append(ReferenceCheck(
            label=r.kpi, value=row.value, reference=r.value, unit=row.unit,
            difference=round(diff, 6),
            differencePct=round(100.0 * diff / r.value, 3) if r.value else None,
            tolerance=round(tol, 6), source=r.source,
            grade="not valid" if row.notValid else grade_gap(diff, tol),
            note=f"not valid: {row.notValid}" if row.notValid else None))
    return out


def _bound(label: str, value: float, ref: float, unit: str, bound: str, margin: float,
           note: str) -> ReferenceCheck:
    ok = value <= ref * (1 + margin) if bound == "at most" else value >= ref * (1 - margin)
    return ReferenceCheck(
        label=label, value=round(value, 4), reference=round(ref, 4), unit=unit,
        difference=round(value - ref, 4),
        differencePct=round(100.0 * (value - ref) / ref, 2) if ref else None,
        tolerance=round(abs(ref) * margin, 4), grade="within" if ok else "outside",
        source="hand calculation", automatic=True, bound=bound, note=note)  # type: ignore[arg-type]


def hand_checks(ctx, channels) -> list[ReferenceCheck]:
    """The automatic hand calculations for a finished run (see the module)."""
    model = ctx.model
    series = {(c.elementId, c.portId): c.timeSeries for c in channels}
    out: list[ReferenceCheck] = []
    veh = ctx.veh_id
    speed = series.get((veh, "sig_speed")) if veh else None
    if not speed or len(speed) < 2:
        return out
    ts = [p["t"] for p in speed]
    v = [max(0.0, (p["value"] or 0.0) / 3.6) for p in speed]  # m/s
    i_top = max(range(len(v)), key=v.__getitem__)

    # -- top speed against each motor's maximum speed through the gears -------
    wheels = [(w.el_id, w.radius) for dl in model.drivelines for seg in dl.segments
              for w in seg.wheels]
    for mc in ctx.motors.values():
        n_m = series.get((mc.el_id, "sig_speed"))
        if not n_m or v[i_top] < 1.0 or not math.isfinite(mc.max_rpm):
            continue
        n_motor = abs(n_m[i_top]["value"] or 0.0)
        best = None  # the wheel it turns slowest relative to (its own axle)
        for w_id, radius in wheels:
            n_w = series.get((w_id, "sig_speed"))
            n_wheel = abs((n_w[i_top]["value"] or 0.0)) if n_w else 0.0
            if n_wheel > 1.0 and n_motor > 1.0:
                ratio = n_motor / n_wheel
                if best is None or ratio < best[0]:
                    best = (ratio, radius)
        if best is None:
            continue
        ratio, radius = best
        allowed = mc.max_rpm / ratio * 2 * math.pi / 60 * radius * 3.6
        label = model.elements[mc.el_id].label
        out.append(_bound(
            f"Top speed allowed by '{label}' (hand calculation)", v[i_top] * 3.6, allowed,
            "km/h", "at most", 0.02,
            f"its maximum speed {mc.max_rpm:,.0f} 1/min ÷ the overall ratio {ratio:.3f} "
            f"(read at the run's fastest point) × the wheel radius {radius:g} m"))

    # -- energy: the sources gave at least the motion, height and road load ----
    if ctx.engines or not (ctx.motors or ctx.batteries):
        return out  # fuel's energy is not summed here
    vp = ctx.params(veh)
    mass = float(vp.get("mass_kg", 0) or 0)
    w_all = [w for dl in model.drivelines for seg in dl.segments for w in seg.wheels]
    share = sum(w.load_share for w in w_all) or 1.0
    c_rr = sum(w.c_rr * w.load_share for w in w_all) / share if w_all else 0.0
    grade = series.get((next((e for e, c in model.cdef_of.items()
                              if c.id == "signal.road_profile"), ""), "sig_grade"))
    work = height = 0.0
    for i in range(1, len(v)):
        dt = ts[i] - ts[i - 1]
        f = []
        for j in (i - 1, i):
            theta = math.atan((grade[j]["value"] or 0.0) / 100.0) if grade else 0.0
            aero, roll = ctx.road_load(v[j], mass * G * math.cos(theta) * c_rr, math.cos(theta))
            f.append((aero + roll) * v[j])
            if j == i and grade:
                height += math.sin(theta) * 0.5 * (v[i] + v[i - 1]) * dt
        work += 0.5 * (f[0] + f[1]) * dt
    motion = 0.5 * mass * (v[-1] ** 2 - v[0] ** 2)
    need_kwh = (motion + mass * G * height + work) / 3.6e6
    gave_kwh = (sum(b.energy_out_wh - b.energy_in_wh for b in ctx.batteries.values())
                + sum(ctx.vsource_energy_wh.values())
                + sum(fc.energy_wh for fc in ctx.fuelcells.values())) / 1000.0
    if need_kwh > 1e-4:
        out.append(_bound(
            "Energy from the sources at least the motion and road load (hand calculation)",
            gave_kwh, need_kwh, "kWh", "at least", HAND_SUM_MARGIN,
            f"½·m·v² change {motion / 3.6e6:.4f} kWh + height {mass * G * height / 3.6e6:.4f} kWh"
            f" + road load {work / 3.6e6:.4f} kWh, summed over the recorded speed; every other "
            f"loss adds to it"))
    return out
