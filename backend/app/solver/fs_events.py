"""Formula Student dynamic events and their points (MOD-43).

A case can stand for one of the four dynamic events of an electric
Formula Student car (SimCase.fsEvent). Its run then gets:

- the event's time as the rules take it: acceleration from the start line to
  75 m (an Acceleration case), skidpad the mean of the timed right and left
  circles, autocross the lap, endurance the total less the driver change lap;
- for endurance, the energy as the rules count it (regeneration × 0.9) and
  the efficiency factor;
- the rule checks of the electric car's power, current and voltage, and
  whether the endurance finished on its energy;
- an estimate of the points against the case's reference values (the
  fastest team's time, the most efficient team's energy), from the rules'
  scoring formulas.

The formulas and numbers are from Formula Student Rules 2026 v1.1 (FSG),
read in the rule book (www.formulastudent.de, FS-Rules_2026_v1.1.pdf):
table 3 (maximum points), D 9.1.1 and table 11 (the dynamic events' score,
Tmax and Pmin), D 9.4 (efficiency), D 4.2.2 (skidpad time), D 7.2.5
(endurance time), D 7.9.5 (endurance energy), D 7.1.3 (22 km), D 7.5.4
(3 min driver change), EV 2.2.1 (80 kW), EV 2.2.2 (500 A), EV 4.1.1 (600 V),
D 10.4 (a violation disqualifies the run). FSUK and FSAE score differently:
check the current season's rules. The points are estimates, not official.

``efficiency_factor_2020`` and ``efficiency_score_2020`` are the FSG 2020
formulas as the FSG score calculator (github.com/philenius/
formula-student-germany-score-calculator) computes them, kept to compare
with that tool; LightSim scores with the 2026 rules.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

RULES = "FS Rules 2026 v1.1 (FSG)"
EVENTS = ("acceleration", "skidpad", "autocross", "endurance")
NAMES = {"acceleration": "Acceleration", "skidpad": "Skidpad", "autocross": "Autocross",
         "endurance": "Endurance", "efficiency": "Efficiency"}


@dataclass(frozen=True)
class Scoring:
    p_max: float  # the event's maximum points (table 3)
    t_max: float  # Tmax as a multiple of Tmin (table 11)
    p_min: float  # Pmin as a share of Pmax (table 11)


# FS Rules 2026 v1.1 (FSG), table 3 and table 11 (manual mode, CV and EV)
SCORING = {
    "skidpad": Scoring(50.0, 1.35, 0.05),
    "acceleration": Scoring(50.0, 1.7, 0.05),
    "autocross": Scoring(100.0, 1.4, 0.1),
    "endurance": Scoring(250.0, 1.5, 0.1),
}
EFFICIENCY_P_MAX = 75.0  # table 3
REGEN_FACTOR = 0.9  # D 7.9.5: regenerated energy × 0.9 is taken off the used energy
ENDURANCE_M = 22_000.0  # D 7.1.3: about 22 km
DRIVER_CHANGE_S = 180.0  # D 7.5.4: 3 min
POWER_LIMIT_KW = 80.0  # EV 2.2.1
CURRENT_LIMIT_A = 500.0  # EV 2.2.2
VOLTAGE_LIMIT_V = 600.0  # EV 4.1.1


def points(event: str, t_team: float, t_min: float) -> float:
    """The event's points, D 9.1.1: (Pmax − Pmin)·((Tmax − Tteam)/(Tmax − Tmin))²
    + Pmin, with Tteam capped to Tmax. A time below Tmin (faster than the
    reference) gives Pmax."""
    sc = SCORING[event]
    p_min = sc.p_min * sc.p_max
    t_max = sc.t_max * t_min
    t = min(max(t_team, t_min), t_max)
    return (sc.p_max - p_min) * ((t_max - t) / (t_max - t_min)) ** 2 + p_min


def efficiency_factor(t_s: float, e_kwh: float) -> float:
    """D 9.4.2: EF = T² · E (T the uncorrected driving time, E the used energy)."""
    return t_s * t_s * e_kwh


def efficiency_points(ef_team: float, ef_min: float) -> float:
    """D 9.4.1: Pmax·((EFmax − EFteam)/(EFmax − EFmin))², EFmax = 2·EFmin. An
    EF below EFmin (better than the reference) gives Pmax, one above EFmax 0."""
    ef_max = 2.0 * ef_min
    ef = min(max(ef_team, ef_min), ef_max)
    return EFFICIENCY_P_MAX * ((ef_max - ef) / (ef_max - ef_min)) ** 2


def efficiency_factor_2020(t_team: float, t_min: float, e_team: float, e_min: float) -> float:
    """FSG 2020, as the FSG score calculator computes it:
    (tMin·enMin²)/(tTeam·enTeam²)."""
    return (t_min * e_min ** 2) / (t_team * e_team ** 2)


def efficiency_score_2020(ef_team: float, ef_max: float) -> float:
    """FSG 2020, as the FSG score calculator computes it:
    100·((0.1/EFteam − 1)/(0.1/EFmax − 1))."""
    return 100.0 * ((0.1 / ef_team - 1.0) / (0.1 / ef_max - 1.0))


def endurance_laps(lap_m: float) -> int:
    """Laps of ``lap_m`` that make the endurance's 22 km (D 7.1.3), rounded."""
    return max(2, round(ENDURANCE_M / lap_m))


def endurance_energy_kwh(batteries) -> float:
    """The used energy as the rules count it, kWh (D 7.9.5): the energy out
    of the accumulators less 0.9 × the energy regenerated into them."""
    return sum(b.energy_out_wh - REGEN_FACTOR * b.energy_in_wh for b in batteries) / 1000.0


@dataclass
class EventResult:
    """An event's figures, from a run or typed in (score())."""
    event: str
    time_s: Optional[float]
    finished: bool = True
    breach: Optional[str] = None  # a rule broken: the run is disqualified (D 10.4.2)
    energy_kwh: Optional[float] = None  # endurance only


def score(res: EventResult, t_min: Optional[float], e_min: Optional[float] = None,
          t_e_min: Optional[float] = None) -> dict:
    """The points of one event: {"points", "efficiency_factor",
    "efficiency_points", "notes"}, None where they cannot be estimated.
    ``t_min`` the fastest team's time, ``e_min`` and ``t_e_min`` the most
    efficient team's energy and time (``t_e_min`` None: ``t_min``)."""
    out: dict = {"points": None, "efficiency_factor": None, "efficiency_points": None,
                 "notes": []}
    notes = out["notes"]
    ok = res.finished and res.breach is None and res.time_s is not None and res.time_s > 0
    if res.breach:
        notes.append(f"{res.breach}: the run is disqualified (D 10.4.2), 0 points.")
    elif not res.finished:
        notes.append("The car did not finish: no points (D 1.1.9).")
    if res.event == "endurance" and ok and res.energy_kwh is not None and res.energy_kwh > 0:
        out["efficiency_factor"] = efficiency_factor(res.time_s, res.energy_kwh)
    if t_min is None or t_min <= 0:
        notes.append("Set the case's Reference time (the fastest team's time) to estimate "
                     "the points.")
        return out
    out["points"] = points(res.event, res.time_s, t_min) if ok else 0.0
    if ok and res.time_s < t_min:
        notes.append(f"Faster than the reference time ({t_min:g} s): full points.")
    elif ok and res.time_s >= SCORING[res.event].t_max * t_min:
        notes.append(f"Slower than Tmax ({SCORING[res.event].t_max:g} × the reference time): "
                     f"the minimum points.")
    if res.event == "endurance":
        if e_min is None or e_min <= 0:
            notes.append("Set the case's Reference energy (the most efficient team's) to "
                         "estimate the efficiency points.")
        elif out["efficiency_factor"] is None:
            out["efficiency_points"] = 0.0  # D 7.9.2: only cars that finished
        else:
            t_e = t_e_min if t_e_min and t_e_min > 0 else t_min
            out["efficiency_points"] = efficiency_points(out["efficiency_factor"],
                                                         efficiency_factor(t_e, e_min))
    return out


# ---- a run's figures ------------------------------------------------------------


def _checks(ctx) -> tuple[list[tuple], list[str]]:
    """The electric car's rule checks over the run: (rows, breaches). Power:
    the battery's 500 ms average when its power check averages over 0.5 s or
    more (as D 10.4.1 does), else its peak over a solver step, which is
    stricter; current: the peak over a solver step (stricter than a 500 ms
    average); voltage: the open-circuit voltage at 100 % SOC or the highest
    terminal voltage the run reached."""
    batts = list(ctx.batteries.values())
    if not batts:
        return [], []
    p_kw = 0.0
    averaged = True
    v_max = 0.0
    for b in batts:
        chk = b.check
        if chk is not None and chk.window_s >= 0.5:
            p_kw += chk.avg_peak_w / 1000.0
        else:
            p_kw += b.p_peak_w / 1000.0
            averaged = False
        v_full = b.ocv_map.at(100.0)
        v_max = max(v_max, v_full, b.v_peak)
    i_a = max(b.i_peak_a for b in batts)
    rows = [
        (f"Rule check: power{', 500 ms average' if averaged else ''} (EV 2.2.1)",
         round(p_kw, 2), "kW", POWER_LIMIT_KW, p_kw <= POWER_LIMIT_KW * (1 + 1e-6)),
        ("Rule check: current (EV 2.2.2)", round(i_a, 1), "A", CURRENT_LIMIT_A,
         i_a <= CURRENT_LIMIT_A * (1 + 1e-6)),
        ("Rule check: voltage (EV 4.1.1)", round(v_max, 1), "V", VOLTAGE_LIMIT_V,
         v_max <= VOLTAGE_LIMIT_V * (1 + 1e-6)),
    ]
    what = (f"power over {POWER_LIMIT_KW:g} kW (EV 2.2.1)", f"current over {CURRENT_LIMIT_A:g} A "
            f"(EV 2.2.2)", f"voltage over {VOLTAGE_LIMIT_V:g} V (EV 4.1.1)")
    breaches = [w for w, row in zip(what, rows) if not row[4]]
    return rows, breaches


def event_time(case, lap, summary: dict) -> Optional[float]:
    """The event's time from the run: see the module's docstring."""
    ev = case.fsEvent
    if ev == "acceleration":
        d = case.endDistance or 75.0
        return summary.get(f"Time to {d:g} m")
    if lap is None or not lap.lap_times:
        return None
    if ev == "skidpad":  # D 4.2.2: the mean of the timed right and left circles
        sectors = lap.sector_times[-1]
        return sum(sectors) / len(sectors) if len(sectors) == 2 else lap.lap_times[-1] / 2.0
    if ev == "autocross":  # D 6.2.1: a run is one lap
        return min(lap.lap_times)
    # endurance, D 7.2.5: the total less the extra-long lap of the driver change
    # (here the lap that restarts from rest)
    times = list(lap.lap_times)
    if lap.stop_after is not None and lap.stop_after + 1 < len(times):
        times.pop(lap.stop_after + 1)
    return sum(times)


def event_rows(case, ctx, lap, summary: list, finished: bool) -> tuple[list[tuple], list[tuple]]:
    """(summary rows (label, value, unit, limit, passed, not valid),
    messages (level, text)) for a case with an fsEvent. ``summary`` the
    run's rows so far; ``finished``: the run went to its end."""
    ev = case.fsEvent
    by_label = {s.label: s for s in summary}
    values = {k: s.value for k, s in by_label.items()}
    rows: list[tuple] = []
    msgs: list[tuple] = []
    name = NAMES[ev]
    check_rows, breaches = _checks(ctx)
    t = event_time(case, lap, values)
    why = None  # why the event time is not valid
    src = (f"Time to {(case.endDistance or 75.0):g} m" if ev == "acceleration"
           else "Lap time")
    if src in by_label and by_label[src].notValid:
        why = by_label[src].notValid
    depleted = any(b.depleted_flagged for b in ctx.batteries.values())
    done = finished and t is not None and not (ev == "endurance" and depleted)
    energy = endurance_energy_kwh(ctx.batteries.values()) if ev == "endurance" else None

    unit_t = "s"
    if t is not None:
        rows.append((f"{name} time ({RULES})", round(t, 3), unit_t, None, None, why))
    if ev == "endurance":
        rows.append(("Endurance energy (regeneration × 0.9)", round(energy, 4), "kWh",
                     None, None, why))
        rows.append(("Endurance finished on its energy", 1.0 if not depleted else 0.0, "-",
                     None, not depleted, None))
    rows += [(*r, None) for r in check_rows]
    res = EventResult(ev, t, finished=done, breach=breaches[0] if breaches else None,
                      energy_kwh=energy)
    sc = score(res, case.referenceTime, case.referenceEnergy, case.referenceEnergyTime)
    if sc["efficiency_factor"] is not None:
        rows.append(("Efficiency factor (T² · E)", round(sc["efficiency_factor"] / 1e6, 4),
                     "10⁶ s²·kWh", None, None, why))
    if sc["points"] is not None:
        rows.append((f"{name} points (estimate)", round(sc["points"], 2), "points",
                     SCORING[ev].p_max, None, why))
    if sc["efficiency_points"] is not None:
        rows.append(("Efficiency points (estimate)", round(sc["efficiency_points"], 2), "points",
                     EFFICIENCY_P_MAX, None, why))
    for b in breaches:
        msgs.append(("warning", f"{name}: {b}. A violation disqualifies the run "
                                f"({RULES} D 10.4.2), so it scores 0 points."))
    if ev == "endurance" and depleted:
        msgs.append(("warning", "Endurance: the accumulator reached its minimum SOC before the "
                                "last lap was done, so the car does not finish (no endurance or "
                                "efficiency points). Lower the Output Power Limit or add "
                                "lift-and-coast (Race Track) to finish on the energy."))
    for n in sc["notes"]:
        if "disqualified" not in n and "did not finish" not in n:
            msgs.append(("info", f"{name}: {n}"))
    msgs.append(("info", f"{name}: the points are an estimate from the scoring formulas of "
                         f"{RULES} (D 9), not an official result; FSUK and FSAE score "
                         f"differently. Check the current season's rules."))
    return rows, msgs


def lap_check(case, laps: int, lap_m: float) -> list[tuple[str, str]]:
    """Messages for an event case's lap set-up (level, text)."""
    out = []
    if case.fsEvent == "endurance":
        km = laps * lap_m / 1000.0
        if abs(km * 1000.0 - ENDURANCE_M) > max(lap_m, 0.1 * ENDURANCE_M):
            out.append(("info", f"Endurance: {laps} laps make {km:.1f} km; the endurance is "
                                 f"about 22 km ({RULES} D 7.1.3)."))
        if laps >= 2:
            out.append(("info", f"Endurance: the car stops for the driver change after lap "
                                 f"{laps // 2} and restarts from rest ({RULES} D 7.2.3, D 7.5); "
                                 f"the 3 min stop is not driven (the tractive system is off, "
                                 f"D 7.5.5), and the restart lap is left out of the event time "
                                 f"(D 7.2.5)."))
    return out

