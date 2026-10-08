"""US window-sticker estimate from the two EPA cycles (CON-32).

A simulated estimate, never a certified value. The model runs EPA's city
cycle (UDDS) and highway cycle (HWFET); the lab figures of those two runs
are adjusted with EPA's "derived five-cycle" equations, as FASTSim's
simdrivelabel module does (FASTSim 2.1.5, Apache-2.0, Copyright 2020
Alliance for Sustainable Energy, LLC; its NOTICE is in
THIRD-PARTY-NOTICES.txt). The equations and their coefficients are EPA's
(40 CFR 600.210; FASTSim's longparams.json keeps the 2008 and 2017 sets):

    adjusted city    = 1 / (city intercept    + city slope    / lab city)
    adjusted highway = 1 / (highway intercept + highway slope / lab highway)
    combined         = 1 / (0.55 / city + 0.45 / highway)        (mpg)
    combined         = 0.55 × city + 0.45 × highway              (kWh/mi)

For an electric car the lab figure is MPGe at the battery (33.7 kWh per
gallon, FASTSim's constant), the adjustment may cut it by at most 30 %
(FASTSim's max_epa_adj, the 0.7 floor EPA allows for electric cars), and
the label energy is at the socket: ÷ the Charger Efficiency. Its range is
the battery's usable energy ÷ the combined energy at the battery.

A hybrid's lab figures use the charge-corrected fuel consumption of each
run (CON-05), so a run that ends with a different charge still compares.
A model with a fuel cell or a voltage source gets no label: their energy
is in neither the battery's Consumption nor the fuel consumption, and
EPA's hydrogen rules (1 kg of hydrogen as a gallon) are not built in.
The five-cycle method with US06, SC03 and a cold FTP needs heat and climate
models LightSim does not have yet (CON-21).
"""
from __future__ import annotations

from dataclasses import dataclass

from . import cycles
from .schemas import Project, SimCase

KWH_PER_GGE = 33.7  # FASTSim's kWh per gallon of gasoline equivalent
L_PER_GALLON = 3.785411784
KM_PER_MILE = 1.609344
MAX_ADJ = 0.3  # an electric car's figure is cut by at most 30 %
CITY_SHARE = 0.55
# EPA's derived five-cycle coefficients, by the model years they apply from
COEFFICIENTS = {
    2008: {"City Intercept": 0.003259, "City Slope": 1.1805,
           "Highway Intercept": 0.001376, "Highway Slope": 1.3466},
    2017: {"City Intercept": 0.004091, "City Slope": 1.1601,
           "Highway Intercept": 0.003191, "Highway Slope": 1.2945},
}
NOT_CERTIFIED = "Simulated estimate, not a certified value."
# energy sources the label cannot count: neither the battery's Consumption
# nor the fuel consumption holds their energy
UNCOUNTED_SOURCES = {"fuelcell.stack": "Fuel Cell Stack", "electric.voltage_source": "Voltage Source"}


def coefficients(model_year: int) -> tuple[int, dict[str, float]]:
    """FASTSim's rule: 2008 coefficients before 2017, the 2017 ones after."""
    year = 2008 if model_year < 2017 else 2017
    return year, COEFFICIENTS[year]


@dataclass
class LabFigures:
    """What the two runs give: per mile, at the battery or in fuel."""
    city_kwh_per_mi: float = 0.0  # battery (DC), electric cars
    hwy_kwh_per_mi: float = 0.0
    city_mpg: float = 0.0  # gallons of fuel, cars with an engine
    hwy_mpg: float = 0.0


def adjust(lab: LabFigures, electric: bool, model_year: int = 2017,
           charger_eff: float = 0.86, usable_kwh: float = 0.0) -> dict[str, float]:
    """The label figures from the lab ones, as FASTSim's get_label_fe
    computes them (non-plug-in branch)."""
    _, c = coefficients(model_year)
    out: dict[str, float] = {}
    if electric:
        out["labUddsKwhPerMile"] = lab.city_kwh_per_mi
        out["labHwyKwhPerMile"] = lab.hwy_kwh_per_mi
        out["labCombKwhPerMile"] = CITY_SHARE * lab.city_kwh_per_mi + (1 - CITY_SHARE) * lab.hwy_kwh_per_mi
        for key, kwh, icpt, slope in (("Udds", lab.city_kwh_per_mi, "City Intercept", "City Slope"),
                                      ("Hwy", lab.hwy_kwh_per_mi, "Highway Intercept", "Highway Slope")):
            lab_mpge = KWH_PER_GGE / kwh
            adj_mpge = max(1 / (c[icpt] + c[slope] / lab_mpge), lab_mpge * (1 - MAX_ADJ))
            out[f"adj{key}KwhPerMile"] = KWH_PER_GGE / adj_mpge / charger_eff
        out["adjCombKwhPerMile"] = (CITY_SHARE * out["adjUddsKwhPerMile"]
                                    + (1 - CITY_SHARE) * out["adjHwyKwhPerMile"])
        for key in ("Udds", "Hwy", "Comb"):
            out[f"adj{key}EssKwhPerMile"] = out[f"adj{key}KwhPerMile"] * charger_eff
            out[f"adj{key}Mpgge"] = KWH_PER_GGE / out[f"adj{key}KwhPerMile"]
        if usable_kwh > 0:
            out["netRangeMiles"] = usable_kwh / out["adjCombEssKwhPerMile"]
    else:
        out["labUddsMpgge"] = lab.city_mpg
        out["labHwyMpgge"] = lab.hwy_mpg
        out["labCombMpgge"] = 1 / (CITY_SHARE / lab.city_mpg + (1 - CITY_SHARE) / lab.hwy_mpg)
        out["adjUddsMpgge"] = 1 / (c["City Intercept"] + c["City Slope"] / lab.city_mpg)
        out["adjHwyMpgge"] = 1 / (c["Highway Intercept"] + c["Highway Slope"] / lab.hwy_mpg)
        out["adjCombMpgge"] = 1 / (CITY_SHARE / out["adjUddsMpgge"]
                                   + (1 - CITY_SHARE) / out["adjHwyMpgge"])
    return out


# ---- running the model on the two cycles ------------------------------------

def uncounted_sources(project: Project) -> list[str]:
    """The parts that supply energy the battery's Consumption and the fuel
    consumption leave out (a fuel cell, a voltage source), by label."""
    return [f"{UNCOUNTED_SOURCES[e.componentDefId]} '{e.label}'"
            for s in project.systems for e in s.elements if e.componentDefId in UNCOUNTED_SOURCES]


def _task_params(case: SimCase, task) -> dict:
    """The Driving Task's values as the case runs them: the library's
    defaults, the part's own values, then the case's (whose own profile
    wins over a cycle, as build_model has it)."""
    from .library import library_by_id
    from .solver.network import resolve_params
    params = resolve_params(task, library_by_id()[task.componentDefId])
    own = case.parameterOverrides.get(task.id) or {}
    params.update(own)
    if "profile" in own and "cycle" not in own:
        params["cycle"] = ""
    return params


def _drives(case: SimCase, task, cycle_id: str) -> bool:
    return case.kind == "cycle" and str(_task_params(case, task).get("cycle") or "") == cycle_id


def _not_whole(case: SimCase, task, cycle_id: str) -> str:
    """Why a case on the cycle does not drive it as published, start to
    end ('' when it does)."""
    why = cycles.not_as_published(_task_params(case, task), case.duration)
    if not why and case.duration < cycles.info(cycle_id)["duration_s"] - 1e-6:
        why = f"for {case.duration:g} s only"
    if not why and ((case.endDistance or 0) > 0 or (case.endLaps or 0) > 0):
        why = "up to a set distance or lap count"
    return why


def label_cases(project: Project, base_case_id: str | None = None,
                notes: list[str] | None = None) -> dict[str, SimCase]:
    """The case to run for each cycle: one of the project's own that drives
    it as published (the hybrid example has one for each, each with its
    balanced start charge), or else a copy of the base case (the first
    Cycle case) set to the cycle as published and its length. ``notes``
    gets a line for each own case passed over because it changes the cycle
    (scaled, repeated, cut short)."""
    cases = [c for c in project.cases if c.kind == "cycle"]
    base = next((c for c in cases if c.id == base_case_id), cases[0] if cases else None)
    tasks = [e for s in project.systems for e in s.elements
             if e.componentDefId == "signal.driving_task"]
    if base is None or len(tasks) != 1:
        raise ValueError("The US label estimate needs a case of kind Cycle and one Driving Task.")
    task = tasks[0]
    out: dict[str, SimCase] = {}
    for cycle_id in ("udds", "hwfet"):
        name = cycles.CYCLES[cycle_id]["name"]
        on_it = [c for c in cases if _drives(c, task, cycle_id)]
        whole = [c for c in on_it if not _not_whole(c, task, cycle_id)]
        for c in on_it if not whole else ():
            if notes is not None:
                notes.append(f"Case '{c.name}' drives {name} {_not_whole(c, task, cycle_id)}; "
                             f"the label runs the cycle as published instead.")
        own = whole[0] if whole else None
        if own is None:
            own = base.model_copy(deep=True)
            own.id = f"label-{cycle_id}"
            own.name = f"US label: {name}"
            ov = own.parameterOverrides.setdefault(task.id, {})
            ov.update({"cycle": cycle_id, "scale_pct": 100, "repeat": False, "mode": "time"})
            ov.pop("profile", None)
            own.duration = cycles.info(cycle_id)["duration_s"]
            own.endDistance = None
            own.endLaps = None
            own.realtimeFactor = 0
        out[cycle_id] = own
    return out


def estimate(project: Project, base_case_id: str | None = None, model_year: int = 2017) -> dict:
    """Run the model on UDDS and HWFET and give the label figures with every
    step. Raises ValueError when the model cannot be run on them."""
    from .solver import simulate

    others = uncounted_sources(project)
    if others:
        raise ValueError(f"The US label estimate covers battery electric cars, hybrids and cars "
                         f"with an engine only. This model also takes energy from "
                         f"{', '.join(others)}, which the label would leave out.")
    year, coef = coefficients(model_year)
    problems: list[str] = []
    chosen = label_cases(project, base_case_id, problems)
    runs: dict[str, dict[str, float]] = {}
    for cycle_id, case in chosen.items():
        trial = project.model_copy(deep=True)
        if not any(c.id == case.id for c in trial.cases):
            trial.cases.append(case)
        result = simulate(trial, case.id)
        rows = {s.label: s for s in result.summary}
        for label in ("Consumption", "Fuel consumption", "Fuel consumption, charge-corrected"):
            if label in rows and rows[label].notValid:
                problems.append(f"{cycles.CYCLES[cycle_id]['name']}: {label} is not valid "
                                f"({rows[label].notValid}).")
        if result.status == "failed":
            problems.append(f"{cycles.CYCLES[cycle_id]['name']}: the run failed.")
        runs[cycle_id] = {s.label: s.value for s in result.summary}
        runs[cycle_id]["_case"] = case.name  # type: ignore[assignment]

    def per_mile(r: dict) -> tuple[float | None, float | None, float | None]:
        km = r.get("Distance driven") or 0.0
        dc = r.get("Consumption")
        lpk = r.get("Fuel consumption, charge-corrected", r.get("Fuel consumption"))
        kwh_mi = dc / 100.0 * KM_PER_MILE if dc else None
        mpg = (100.0 / lpk) * L_PER_GALLON / KM_PER_MILE if lpk else None
        return km, kwh_mi, mpg

    _, city_kwh, city_mpg = per_mile(runs["udds"])
    _, hwy_kwh, hwy_mpg = per_mile(runs["hwfet"])
    electric = bool(city_kwh and hwy_kwh) and not (city_mpg or hwy_mpg)
    if not electric and not (city_mpg and hwy_mpg):
        raise ValueError("The runs gave neither an electric Consumption nor a fuel consumption "
                         "on both cycles, so there is nothing to put on a label.")
    charger = _charger_efficiency(project)
    usable = _usable_kwh(project)
    lab = LabFigures(city_kwh_per_mi=city_kwh or 0.0, hwy_kwh_per_mi=hwy_kwh or 0.0,
                     city_mpg=city_mpg or 0.0, hwy_mpg=hwy_mpg or 0.0)
    out = adjust(lab, electric, year, charger, usable)
    return {
        "notCertified": NOT_CERTIFIED,
        "electric": electric,
        "modelYearCoefficients": year,
        "coefficients": coef,
        "chargerEfficiency": charger if electric else None,
        "usableKwh": usable if electric else None,
        "cases": {k: runs[k]["_case"] for k in runs},
        "problems": problems,
        "figures": out,
        "steps": _steps(out, electric, coef, charger, usable),
    }


def _batteries(project: Project) -> list[dict]:
    """Each battery's parameters, library defaults filled in."""
    from .library import library_by_id
    from .solver.network import resolve_params
    cdef = library_by_id()["battery.generic"]
    return [resolve_params(e, cdef) for s in project.systems for e in s.elements
            if e.componentDefId == "battery.generic"]


def _number(params: dict, key: str) -> float | None:
    try:
        return float(params[key])
    except (KeyError, TypeError, ValueError):
        return None


def _charger_efficiency(project: Project) -> float:
    effs = [_number(p, "charger_efficiency_pct") for p in _batteries(project)]
    effs = [e for e in effs if e is not None]
    return max(0.01, min(1.0, sum(effs) / len(effs) / 100.0)) if effs else 0.86


def _usable_kwh(project: Project) -> float:
    """The batteries' Usable Capacity × their share above the Minimum SOC,
    the energy the label range assumes is driven."""
    total = 0.0
    for p in _batteries(project):
        cap, floor = _number(p, "capacity_kWh"), _number(p, "min_soc_pct")
        if cap is not None and floor is not None:
            total += cap * (1 - floor / 100.0)
    return total


def _steps(out: dict[str, float], electric: bool, coef: dict[str, float],
           charger: float, usable: float) -> list[dict]:
    """Each figure with how it was worked out, in the order a reader follows."""
    def row(what: str, value: float, unit: str, how: str) -> dict:
        return {"what": what, "value": round(value, 3), "unit": unit, "how": how}
    ci, cs = coef["City Intercept"], coef["City Slope"]
    hi, hs = coef["Highway Intercept"], coef["Highway Slope"]
    if electric:
        steps = [
            row("Lab city energy (battery)", out["labUddsKwhPerMile"] * 100, "kWh/100 mi",
                "UDDS run: Consumption at the battery, per mile"),
            row("Lab highway energy (battery)", out["labHwyKwhPerMile"] * 100, "kWh/100 mi",
                "HWFET run: Consumption at the battery, per mile"),
            row("Label city energy (socket)", out["adjUddsKwhPerMile"] * 100, "kWh/100 mi",
                f"33.7 ÷ max(1 ÷ ({ci} + {cs} ÷ lab MPGe), 0.7 × lab MPGe) ÷ charger {charger:.0%}"),
            row("Label highway energy (socket)", out["adjHwyKwhPerMile"] * 100, "kWh/100 mi",
                f"33.7 ÷ max(1 ÷ ({hi} + {hs} ÷ lab MPGe), 0.7 × lab MPGe) ÷ charger {charger:.0%}"),
            row("Label combined energy (socket)", out["adjCombKwhPerMile"] * 100, "kWh/100 mi",
                "0.55 × city + 0.45 × highway"),
            row("Label combined energy (socket)", out["adjCombKwhPerMile"] * 100 / KM_PER_MILE,
                "kWh/100 km", "the same, per 100 km"),
            row("Label city MPGe", out["adjUddsMpgge"], "MPGe", "33.7 ÷ label city kWh per mile"),
            row("Label highway MPGe", out["adjHwyMpgge"], "MPGe", "33.7 ÷ label highway kWh per mile"),
            row("Label combined MPGe", out["adjCombMpgge"], "MPGe", "33.7 ÷ label combined kWh per mile"),
        ]
        if "netRangeMiles" in out:
            steps += [
                row("Label range", out["netRangeMiles"], "mi",
                    f"usable energy {usable:.1f} kWh ÷ combined energy at the battery"),
                row("Label range", out["netRangeMiles"] * KM_PER_MILE, "km", "the same, in km"),
            ]
        return steps
    return [
        row("Lab city fuel economy", out["labUddsMpgge"], "mpg",
            "UDDS run: miles ÷ US gallons (charge-corrected for a hybrid)"),
        row("Lab highway fuel economy", out["labHwyMpgge"], "mpg",
            "HWFET run: miles ÷ US gallons (charge-corrected for a hybrid)"),
        row("Lab combined", out["labCombMpgge"], "mpg", "1 ÷ (0.55 ÷ city + 0.45 ÷ highway)"),
        row("Label city", out["adjUddsMpgge"], "mpg", f"1 ÷ ({ci} + {cs} ÷ lab city)"),
        row("Label highway", out["adjHwyMpgge"], "mpg", f"1 ÷ ({hi} + {hs} ÷ lab highway)"),
        row("Label combined", out["adjCombMpgge"], "mpg", "1 ÷ (0.55 ÷ city + 0.45 ÷ highway)"),
        row("Label combined", 100 * L_PER_GALLON / (out["adjCombMpgge"] * KM_PER_MILE), "l/100km",
            "the same, in litres per 100 km"),
    ]
