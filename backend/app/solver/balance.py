"""Charge-balanced runs of a hybrid (ENG-33).

A hybrid's fuel figure holds the energy its battery gave or took: start it
fuller than it ends and the fuel looks lower than the car burns. A
charge-balanced case runs the cycle, sets the battery's start SOC to the SOC
it ended with and runs it again, until the battery's stored energy changes
by less than BALANCE_SHARE of the fuel's energy (the SAE J1711-style
criterion, background knowledge, unverified) or MAX_RUNS runs (FASTSim's
default of 5 iterations). The last run is the result, with the start SOC it
found. If it does not settle, the fuel figure is also given corrected to no
change of charge, by a straight line through the runs' fuel against their
battery energy change (SAE J1711's correction, from the runs it has).

It is on by default (SimCase.chargeBalance None) for a cycle case whose
model has an engine and a battery, and off for a paced (live tuning) run.
"""
from __future__ import annotations

from typing import Callable, Optional

from ..schemas import Project, SimMessage, SimResult, SummaryValue
from .energy import FUEL_LHV_MJ
from .network import ModelError, build_model
from .runtime import ocv_mean

BALANCE_SHARE = 0.01  # battery energy change / fuel energy that counts as balanced
MAX_RUNS = 5
LHV_MJ_PER_KG = FUEL_LHV_MJ  # petrol, when no fuel tank sets one

RunFn = Callable[..., SimResult]


def run_totals(ctx, soc_start: dict[str, float]) -> dict:
    """Each battery's (start SOC, end SOC, stored energy change in J) and the
    fuel burnt (kg) and its energy (J), from a finished run's context."""
    batteries = {}
    for el_id, b in ctx.batteries.items():
        s0, s1 = soc_start.get(el_id, b.soc), b.soc
        lo, hi = sorted((max(0.0, min(1.0, s0)), max(0.0, min(1.0, s1))))
        volts = (ocv_mean(b.ocv_map.pts, b.ocv_map.linear[0], lo * 100.0, hi * 100.0)
                 if hi > lo else b.ocv_map.at(hi * 100.0))
        batteries[el_id] = (s0, s1, b.q_ah * (s1 - s0) * volts * 3600.0)
    fuel_kg = sum(ec.fuel_used_kg for ec in ctx.engines.values())
    lhv = LHV_MJ_PER_KG
    if ctx.model.fuel_tank:
        try:
            lhv = max(0.0, float(ctx.params(ctx.model.fuel_tank).get("lhv_MJ_per_kg", lhv)))
        except (TypeError, ValueError):
            pass
    return {"batteries": batteries, "fuel_kg": fuel_kg, "fuel_j": fuel_kg * lhv * 1e6}


def applies(project: Project, case) -> bool:
    """Whether the case is run charge-balanced."""
    if case.chargeBalance is not None:
        return bool(case.chargeBalance) and case.kind in ("cycle", "performance")
    if case.kind != "cycle" or (case.realtimeFactor or 0) > 0:
        return False
    types = {el.componentDefId for s in project.systems for el in s.elements}
    # a hybrid: an engine, and a battery an E-Motor can charge (a car with
    # only a 12 V battery on a load has nothing to balance)
    return "engine.combustion" in types and "battery.generic" in types and "motor.emotor" in types


def _fit_zero(points: list[tuple[float, float]]) -> Optional[float]:
    """The fuel at no battery energy change: a least-squares line through
    (energy change, fuel) read at 0; None for fewer than two distinct changes."""
    if len({x for x, _ in points}) < 2:
        return None
    n = len(points)
    mx = sum(x for x, _ in points) / n
    my = sum(y for _, y in points) / n
    sxx = sum((x - mx) ** 2 for x, _ in points)
    slope = sum((x - mx) * (y - my) for x, y in points) / sxx
    return my - slope * mx


def simulate_balanced(project: Project, case_id: str, emit, control, run: RunFn) -> SimResult:
    case = next((c for c in project.cases if c.id == case_id), None)
    if case is None or not applies(project, case):
        return run(project, case_id, emit, control)
    try:
        model = build_model(project, {}, case.parameterOverrides)
    except ModelError:
        return run(project, case_id, emit, control)
    batteries = [e for e, c in model.cdef_of.items() if c.id == "battery.generic"]
    if not batteries:
        return run(project, case_id, emit, control)
    proj = project.model_copy(deep=True)
    case = next(c for c in proj.cases if c.id == case_id)

    edited = False  # a live edit during the first run: later runs would not have it

    def watched() -> list[dict]:
        nonlocal edited
        msgs = control() if control else []
        edited = edited or any(m.get("type") == "set_param" for m in msgs)
        return msgs

    def quiet(event: dict) -> None:  # later runs: no steps or messages of their own
        return None

    history: list[tuple[dict, float, float, Optional[float]]] = []
    result: SimResult | None = None
    last_good: SimResult | None = None
    for k in range(MAX_RUNS):
        totals: dict = {}
        result = run(proj, case_id, emit if k == 0 else quiet, watched, totals)
        if result.status in ("failed", "cancelled") or not totals:
            if not history:
                return result
            # a later run was stopped or failed: report the last finished one
            why = f"run {k + 1} was {'stopped' if result.status == 'cancelled' else 'cut short'}"
            out = _annotate(last_good, model, history, done=False, why=why)
            return out.model_copy(update={"messages": [
                *out.messages, *(m for m in result.messages if m.level == "error")]})
        d_e = sum(e for _, _, e in totals["batteries"].values())
        fuel = next((s.value for s in result.summary if s.label == "Fuel consumption"), None)
        history.append((totals["batteries"], d_e, totals["fuel_j"], fuel))
        last_good = result
        if totals["fuel_j"] <= 0:
            # the engine never ran: nothing to balance, the run stands as it is
            return result.model_copy(update={"messages": [
                *result.messages, SimMessage(level="info", text=(
                    "Charge balancing: the engine burnt no fuel on this run, so there is no "
                    "fuel figure to balance; the run is reported as it is."))]})
        if abs(d_e) <= BALANCE_SHARE * totals["fuel_j"]:
            return _annotate(result, model, history, done=True, why=None)
        if edited:
            return _annotate(result, model, history, done=False,
                             why="a parameter was changed while it ran")
        for el_id, (_, s1, _) in totals["batteries"].items():
            case.parameterOverrides.setdefault(el_id, {})["initial_soc_pct"] = s1 * 100.0
        if emit and k + 1 < MAX_RUNS:
            emit({"type": "message", "level": "info",
                  "text": f"Charge balancing: run {k + 2} of up to {MAX_RUNS}, starting at the "
                          f"charge run {k + 1} ended with."})
    return _annotate(result, model, history, done=False,
                     why=f"the battery still changed after {MAX_RUNS} runs")


def _annotate(result: SimResult, model, history, done: bool, why: Optional[str]) -> SimResult:
    """The last run's result with what charge balancing found: messages, the
    start SOC it settled on, the battery's energy change as a share of the
    fuel's and, when it did not settle, the charge-corrected fuel figure."""
    label = lambda el: model.elements[el].label if el in model.elements else el  # noqa: E731
    last_b, d_e, fuel_j, _ = history[-1]
    lines = []
    for k, (bats, de, fj, _) in enumerate(history):
        socs = "; ".join(f"{label(el)} {s0 * 100:.2f} → {s1 * 100:.2f} %"
                         for el, (s0, s1, _) in bats.items())
        share = f"{100 * de / fj:+.2f} % of the fuel's energy" if fj > 0 else "no fuel burnt"
        lines.append(f"run {k + 1}: {socs} ({share})")
    head = (f"Charge-balanced in {len(history)} run{'s' if len(history) > 1 else ''}: the "
            f"battery's stored energy changed by {abs(d_e) / 3.6e6:.4g} kWh, within "
            f"{BALANCE_SHARE * 100:g} % of the fuel's energy, so the fuel figure needs no "
            f"charge correction" if done else
            f"Charge balancing did not settle ({why})")
    messages = list(result.messages)
    messages.insert(1, SimMessage(level="info" if done else "warning",
                                  text=f"{head}. " + "; ".join(lines) + "."))
    rows = [SummaryValue(label=f"{label(el)} — charge-balanced start SOC", value=s0 * 100.0,
                         unit="%", notValid=None if done else "charge balancing did not settle")
            for el, (s0, _, _) in last_b.items()]
    if fuel_j > 0:
        rows.append(SummaryValue(label="Battery energy change, share of fuel energy",
                                 value=100.0 * d_e / fuel_j, unit="%",
                                 limit=BALANCE_SHARE * 100.0, passed=done))
    rows.append(SummaryValue(label="Charge balance runs", value=float(len(history)), unit="-"))
    if not done:
        fit = _fit_zero([(de, f) for _, de, _, f in history if f is not None])
        if fit is not None:
            rows.append(SummaryValue(label="Fuel consumption, charge-corrected", value=fit,
                                     unit="l/100km"))
            messages.insert(2, SimMessage(level="info", text=(
                f"Fuel consumption corrected to no change of charge: {fit:.3f} l/100 km, from a "
                f"straight line through the {len(history)} runs' fuel against their battery "
                f"energy change (SAE J1711's method; the last run's own figure includes the "
                f"charge its battery gave or took).")))
    summary = list(result.summary)
    i = next((k for k, s in enumerate(summary) if s.label == "Simulated duration"), len(summary))
    summary[i:i] = rows
    status = result.status
    if not done and status == "success":
        status = "warning"
    return result.model_copy(update={"messages": messages, "summary": summary, "status": status})
