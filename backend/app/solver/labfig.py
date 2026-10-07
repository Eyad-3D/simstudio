"""Consumption figures as test labs and regulators report them (CON-05).

Next to the battery-side "Consumption" (DC energy out of the battery per
distance) the run summary gives, where they apply:

- the energy at the charging socket (AC): DC energy ÷ the battery's Charger
  Efficiency, the figure EPA and WLTP labels quote for electric cars;
- the fuel-economy equivalent in MPGe: 33.705 kWh of electricity count as
  one US gallon of petrol (EPA), so MPGe = 33.705 ÷ (AC kWh per mile);
- the range this consumption gives: the battery's usable energy (open-
  circuit, from 100 % SOC to its Minimum SOC) ÷ the DC consumption;
- for a hybrid, the fuel corrected for the battery's change in charge: the
  energy the battery gave (or took) is counted at the engine's own average
  efficiency over the run, fuel × (1 + net battery energy ÷ engine work),
  and the change is given as a share of the fuel's energy (SAE J1711 calls
  a run charge-balanced when that share is under 1 %);
- per phase of the drive cycle the case drives (WLTC Low to Extra High,
  FTP bags): distance, consumption and fuel consumption over that phase;
- for the FTP-75, the bags weighted as EPA does: 0.43 × (bag 1 + bag 2) +
  0.57 × (bag 3 + bag 2), each a consumption over its two bags' distance.

The battery charge balancing itself (repeating a cycle until the charge
closes) is the engine area's ENG-33; this module only reports.
"""
from __future__ import annotations

from dataclasses import dataclass, field

from .. import cycles
from ..schemas import SummaryValue
from .runtime import ocv_mean

KWH_PER_GALLON = 33.705  # EPA's petrol-equivalent energy of a US gallon
KM_PER_MILE = 1.609344
LHV_PETROL_J_PER_KG = 42.9e6  # the Fuel Tank's default (petrol)
BALANCED_SHARE = 1.0  # %, SAE J1711's net-energy-change tolerance
FTP_WEIGHTS = (0.43, 0.57)  # cold (bags 1+2) and hot (bags 3+2) halves


@dataclass
class Snapshot:
    t: float
    distance_m: float
    net_wh: float  # battery energy out minus in, all batteries
    fuel_kg: float


@dataclass
class LabLog:
    """Totals at the start and at the end of each phase of the cycle the
    case drives, logged by the run loop (sample) at the end of each step."""
    phases: list[tuple[str, float, float]] = field(default_factory=list)
    marks: list[Snapshot] = field(default_factory=list)

    @classmethod
    def for_run(cls, model, case_kind: str) -> LabLog:
        """Phases only for a Cycle case whose one Driving Task drives a
        bundled cycle that has them."""
        tasks = [el_id for el_id, c in model.cdef_of.items() if c.id == "signal.driving_task"]
        if case_kind != "cycle" or len(tasks) != 1:
            return cls()
        cycle_id = str(model.params_of[tasks[0]].get("cycle") or "")
        if cycle_id not in cycles.CYCLES:
            return cls()
        return cls(phases=cycles.phases(cycle_id))

    def sample(self, t: float, ctx) -> None:
        if not self.phases:
            return
        while not self.marks or (len(self.marks) <= len(self.phases)
                                 and t >= self.phases[len(self.marks) - 1][2] - 1e-9):
            self.marks.append(Snapshot(
                t, ctx.distance,
                sum(b.energy_out_wh - b.energy_in_wh for b in ctx.batteries.values()),
                sum(ec.fuel_used_kg for ec in ctx.engines.values())))


def _num(params: dict, key: str, default: float) -> float:
    try:
        return float(params.get(key, default))
    except (TypeError, ValueError):
        return default


def lab_rows(ctx, model, log: LabLog, density_kg_per_l: float,
             balanced: bool = False) -> tuple[list[SummaryValue], dict[str, str]]:
    """The rows, and for each the base row whose validity it shares
    ("Consumption" or "Fuel consumption"). ``balanced``: the case runs
    charge-balanced (balance.py, ENG-33), which gives the battery's share of
    the fuel energy and, when needed, the charge-corrected fuel itself."""
    rows: list[SummaryValue] = []
    base: dict[str, str] = {}
    km = ctx.distance / 1000.0
    if km <= 0.1:
        return rows, base

    def add(label: str, value: float, unit: str, of: str) -> None:
        rows.append(SummaryValue(label=label, value=value, unit=unit))
        base[label] = of

    net_wh = sum(b.energy_out_wh - b.energy_in_wh for b in ctx.batteries.values())
    fuel_kg = sum(ec.fuel_used_kg for ec in ctx.engines.values())
    electric = net_wh > 0 and fuel_kg <= 0 and not ctx.fuelcells

    if electric:
        dc = net_wh / 10.0 / km  # kWh/100 km
        effs = [_num(ctx.params(b.el_id), "charger_efficiency_pct", 86.0)
                for b in ctx.batteries.values()]
        eff = max(1.0, min(100.0, sum(effs) / len(effs))) / 100.0
        ac = dc / eff
        add("Consumption at the socket (AC)", ac, "kWh/100km", "Consumption")
        add("Fuel-economy equivalent (MPGe, AC)",
            KWH_PER_GALLON / (ac / 100.0 * KM_PER_MILE), "MPGe", "Consumption")
        usable_wh = sum(b.q_ah * (1.0 - min(1.0, max(0.0, b.min_soc)))
                        * ocv_mean(b.ocv_map.pts, b.ocv_map.linear[0], b.min_soc * 100.0, 100.0)
                        for b in ctx.batteries.values() if b.min_soc < 1.0)
        if usable_wh > 0:
            add("Range at this consumption", usable_wh / (dc * 10.0), "km", "Consumption")

    if fuel_kg > 0 and ctx.batteries and not balanced:
        work_wh = sum(ec.work_wh for ec in ctx.engines.values())
        lhv = LHV_PETROL_J_PER_KG
        if model.fuel_tank:
            lhv = 1e6 * _num(ctx.params(model.fuel_tank), "lhv_MJ_per_kg", lhv / 1e6)
        # the battery's stored energy change (+ when it ends fuller), as
        # charge balancing gives it (ENG-33)
        share = -100.0 * net_wh * 3600.0 / (fuel_kg * lhv)
        add("Battery energy change, share of fuel energy", share, "%", "Fuel consumption")
        if work_wh > 0:
            corrected = fuel_kg * (1.0 + net_wh / work_wh)
            add("Fuel consumption, charge-corrected",
                corrected / density_kg_per_l * 100.0 / km, "l/100km", "Fuel consumption")

    # per phase, from the totals logged at each phase end
    done = log.marks[1:]
    per_phase: list[tuple[str, float, float, float]] = []  # name, km, kWh, kg
    for (name, _, _), a, b in zip(log.phases, log.marks, done):
        d_km = (b.distance_m - a.distance_m) / 1000.0
        per_phase.append((name, d_km, (b.net_wh - a.net_wh) / 1000.0, b.fuel_kg - a.fuel_kg))
        if d_km <= 0:
            continue
        add(f"Phase {name} — distance", d_km, "km", "Consumption" if electric else "Fuel consumption")
        if electric or (net_wh > 0 and fuel_kg <= 0):
            add(f"Phase {name} — consumption", (b.net_wh - a.net_wh) / 10.0 / d_km,
                "kWh/100km", "Consumption")
        if fuel_kg > 0:
            add(f"Phase {name} — fuel consumption",
                (b.fuel_kg - a.fuel_kg) / density_kg_per_l * 100.0 / d_km,
                "l/100km", "Fuel consumption")

    # EPA's FTP weighting of its three bags
    if len(per_phase) == 3 and all(p[0].startswith("Bag ") for p in per_phase):
        (_, d1, e1, f1), (_, d2, e2, f2), (_, d3, e3, f3) = per_phase
        wc, wh = FTP_WEIGHTS
        if d1 + d2 > 0 and d3 + d2 > 0:
            if electric:
                add("FTP weighted consumption",
                    100.0 * (wc * (e1 + e2) / (d1 + d2) + wh * (e3 + e2) / (d3 + d2)),
                    "kWh/100km", "Consumption")
            if fuel_kg > 0:
                add("FTP weighted fuel consumption",
                    100.0 / density_kg_per_l
                    * (wc * (f1 + f2) / (d1 + d2) + wh * (f3 + f2) / (d3 + d2)),
                    "l/100km", "Fuel consumption")
    return rows, base
