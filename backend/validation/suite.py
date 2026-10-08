"""LightSim's reference suite (VAL-05): real vehicles with official test
results, run through LightSim and compared automatically.

Each case file in ``cases/`` holds one car's facts in the units they were
published in (EPA's pounds, pound-force and mpge; FASTSim's values) with
their sources; ``suite.json`` holds the fixed rules that turn them into a
LightSim model, and ``exact.json`` the exact-answer tier. This module builds
the models, runs them and reports the gaps; backend/tests/test_reference_suite.py
runs it in CI, and ``python -m validation.suite`` (from backend/) prints the
table that docs/VALIDATION-STATUS.md quotes.
"""
from __future__ import annotations

import copy
import hashlib
import json
import math
from dataclasses import dataclass, field
from pathlib import Path

from app import cycles
from app.schemas import Project, SimCase
from app.solver import simulate
from app.storage import load_example

DIR = Path(__file__).parent
SUITE = json.loads((DIR / "suite.json").read_text(encoding="utf-8"))
EXACT = json.loads((DIR / SUITE["exact"]).read_text(encoding="utf-8"))
RULES = SUITE["rules"]

LB_KG = 0.45359237
HP_KW = 0.745699872  # a mechanical (SAE) horsepower, as EPA rates engines and motors
LBF_N = 4.448222
MPH_KMH = 1.609344
WH_PER_GALLON_EQ = 33705.0
# the motor's maximum speed, in mph of road speed (rules: motor_full_load)
TOP_MPH = 110.0
OCV_V = 350.0
INERTIA_SHARE = RULES["rotating_inertia_pct"] / 100.0
CHARGER_SPREAD = 0.04  # rules: wall_energy


def load_cases() -> list[dict]:
    return [json.loads((DIR / p).read_text(encoding="utf-8")) for p in SUITE["cases"]]


def mpge_to_wh_per_km(mpge: float) -> float:
    return WH_PER_GALLON_EQ / (mpge * MPH_KMH)


def cycle_sha256(cycle_id: str) -> str:
    return hashlib.sha256((cycles.DIR / f"{cycle_id}.csv").read_bytes()).hexdigest()


def abc_si(lbf: list[float]) -> tuple[float, float, float]:
    """EPA's A (lbf), B (lbf/mph), C (lbf/mph²) in N, N/(km/h), N/(km/h)²."""
    a, b, c = lbf
    return a * LBF_N, b * LBF_N / MPH_KMH, c * LBF_N / MPH_KMH ** 2


@dataclass
class Inputs:
    """A case's model values after the suite's rules."""
    mass_kg: float
    abc: tuple[float, float, float]
    radius_m: float
    wheel_inertia: float
    ratio: float
    max_rpm: float
    motor_kw: float
    battery_kwh: float
    r0_ohm: float
    max_kw: float
    aux_kw: float
    regen_pct: float
    charger: float
    efficiency: dict = field(default_factory=dict)


def motor_kw(case: dict) -> float:
    """The motor's power (rules: motor_power): EPA's rated horsepower where
    the case has it, else FASTSim's figure."""
    epa, fs = case["epa"], case["fastsim"]
    return epa["rated_hp"] * HP_KW if "rated_hp" in epa else fs["motor_kW"]


def inputs_of(case: dict) -> Inputs:
    epa, fs = case["epa"], case["fastsim"]
    mass = epa["test_weight_lb"] * LB_KG
    r = fs["wheel_radius_m"]
    ratio = epa["n_per_v_rpm_per_mph"] * 2 * math.pi * r / (60 * 0.44704)
    return Inputs(
        mass_kg=mass, abc=abc_si(epa["target_abc_lbf"]), radius_m=r,
        # four wheels carry the 1.5 %: 4 J / r² = 0.015 m
        wheel_inertia=INERTIA_SHARE * mass * r * r / 4, ratio=ratio,
        max_rpm=epa["n_per_v_rpm_per_mph"] * TOP_MPH, motor_kw=motor_kw(case),
        battery_kwh=fs["battery_kWh"], r0_ohm=0.015 * OCV_V ** 2 / (fs["battery_kWh"] * 1000),
        max_kw=fs["battery_max_kW"], aux_kw=fs["aux_kW"], regen_pct=fs["regen_pct"],
        charger=fs["charger_efficiency"], efficiency=fs["motor_efficiency"])


def motor_maps(i: Inputs) -> tuple[dict, dict]:
    """Full-load torque (V → 1/min → N·m) and loss (1/min → N·m → kW) from
    the motor's power and FASTSim's efficiency against output power."""
    p_max = i.motor_kw * 1000
    w_base = i.max_rpm / 3 * 2 * math.pi / 60
    t_max = p_max / w_base
    speeds = [round(i.max_rpm * k / 20) for k in range(21)]
    full = {str(n): round(t_max if n * 2 * math.pi / 60 <= w_base
                          else p_max / (n * 2 * math.pi / 60), 3) for n in speeds}
    pct, eff = i.efficiency["power_out_pct"], i.efficiency["efficiency_pct"]

    def efficiency(p_w: float) -> float:
        x = min(100.0, 100.0 * p_w / p_max)
        for k in range(1, len(pct)):
            if x <= pct[k]:
                f = (x - pct[k - 1]) / (pct[k] - pct[k - 1])
                return (eff[k - 1] + f * (eff[k] - eff[k - 1])) / 100.0
        return eff[-1] / 100.0

    torques = sorted({0, 2, 5, 10, 20, 35, 50, 75, 100, 150, 200, 300, round(t_max, 1)}
                     | {round(t_max * k / 8, 1) for k in range(1, 8)})
    torques = [t for t in torques if t <= t_max + 1e-9]
    loss = {}
    for n in speeds:
        w = n * 2 * math.pi / 60
        row = {}
        for t in torques:
            p = t * w
            row[str(t)] = round((p / efficiency(p) - p) / 1000.0, 5) if p > 0 else 0.0
        loss[str(n)] = row
    return {"250": full, "450": full}, loss


def build_project(case: dict, cycle_id: str, step: float = 1.0, every: int = 1) -> Project:
    """The Battery Electric Car example's diagram with the case's values."""
    i = inputs_of(case)
    full, loss = motor_maps(i)
    proj = load_example("bev-car")
    proj.id, proj.name, proj.description = f"suite-{case['id']}", case["name"], None
    a, b, c = i.abc
    values = {
        "vehicle.body": {"mass_kg": round(i.mass_kg, 3), "road_load_mode": "Coefficients A/B/C",
                         "road_load_a_N": round(a, 4), "road_load_b_N_per_kmh": round(b, 6),
                         "road_load_c_N_per_kmh2": round(c, 7),
                         "abc_include_driveline_losses": True},
        "propulsion.wheel": {"radius_m": i.radius_m, "inertia_kgm2": round(i.wheel_inertia, 5)},
        "mech.brake": {"inertia_kgm2": 0},
        "motor.emotor": {"full_load_torque": full, "power_loss": loss,
                         "drag_torque": {"0": 0, str(round(i.max_rpm)): 0},
                         "max_speed_rpm": round(i.max_rpm), "inertia_kgm2": 0},
        "mech.final_drive": {"ratio": round(i.ratio, 5), "efficiency_pct": 100,
                             "inertia_in_kgm2": 0, "inertia_out_kgm2": 0},
        "mech.differential": {"inertia_kgm2": 0},
        "battery.generic": {"capacity_kWh": i.battery_kwh, "ocv_table": {"0": OCV_V, "100": OCV_V},
                            "internal_resistance_ohm": round(i.r0_ohm, 6),
                            "max_charge_power_kW": i.max_kw, "output_power_limit_kW": i.max_kw,
                            "initial_soc_pct": 90, "min_soc_pct": 5},
        "electric.constant_drive": {"power_kW": i.aux_kw},
        "driver.driver": {"regen_weight_pct": i.regen_pct},
        "signal.driving_task": {"cycle": cycle_id},
    }
    for el in proj.systems[0].elements:
        el.parameterOverrides.update(copy.deepcopy(values.get(el.componentDefId, {})))
    duration = cycles.info(cycle_id)["duration_s"]
    proj.cases = [SimCase(id="case", name=cycle_id, duration=duration, timeStep=step,
                          outputEvery=every)]
    return proj


def _summary(result) -> dict[str, float]:
    return {s.label: s.value for s in result.summary}


@dataclass
class CycleResult:
    cycle: str
    wh_per_km: float  # at the wall
    wh_per_km_battery: float  # at the battery terminals
    target: float  # Wh/km at the wall
    status: str
    messages: list[str]

    @property
    def gap_pct(self) -> float:
        return 100.0 * (self.wh_per_km - self.target) / self.target


def run_cycle(case: dict, cycle_id: str, step: float = 1.0, every: int = 1) -> CycleResult:
    i = inputs_of(case)
    result = simulate(build_project(case, cycle_id, step, every), "case")
    s = _summary(result)
    km = s["Distance driven"]
    battery = next(k.split(" — ")[0] for k in s if k.endswith(" — energy delivered"))
    net = s[f"{battery} — energy delivered"] - s[f"{battery} — energy recuperated"]
    terminal = 1000.0 * net / km
    chemical = 1000.0 * (net + s[f"{battery} — internal losses"]) / km
    return CycleResult(
        cycle=cycle_id, wh_per_km=chemical / i.charger, wh_per_km_battery=terminal,
        target=mpge_to_wh_per_km(case["target"][f"{cycle_id}_mpge"]), status=result.status,
        messages=[m.text for m in result.messages if m.level != "info"])


def coastdown(case: dict) -> dict:
    """A virtual coast-down: the car let go at 130 km/h with no drive and no
    brakes; A, B and C fitted to −m_eff·dv/dt against v and compared with the
    target's force at 20 to 120 km/h. Returns the fit and the worst gap, %."""
    i = inputs_of(case)
    proj = build_project(case, "udds")
    for el in proj.systems[0].elements:
        if el.componentDefId == "vehicle.body":
            el.parameterOverrides["initial_speed_kmh"] = 130
    # nothing commands the motor or the brakes
    proj.dataBusConnections = [d for d in proj.dataBusConnections
                               if d.port1Id not in ("sig_traction_cmd", "sig_brake_cmd")]
    proj.cases[0].duration = 400
    result = simulate(proj, "case")
    speed = next(c for c in result.channels if c.portId == "sig_speed"
                 and c.elementId == "el-vehicle").timeSeries
    m_eff = i.mass_kg * (1 + INERTIA_SHARE)
    pts = []
    for k in range(1, len(speed) - 1):
        v = speed[k]["value"]
        if v is None or not 15.0 <= v <= 125.0:
            continue
        dv = (speed[k + 1]["value"] - speed[k - 1]["value"]) / 3.6
        dt = speed[k + 1]["t"] - speed[k - 1]["t"]
        pts.append((v, -m_eff * dv / dt))
    fit = _quadratic_fit(pts)
    a, b, c = i.abc
    worst = max(abs((fit[0] + fit[1] * v + fit[2] * v * v) / (a + b * v + c * v * v) - 1)
                for v in range(20, 121, 10))
    return {"fit": fit, "target": i.abc, "worst_pct": 100.0 * worst, "points": len(pts)}


def _quadratic_fit(pts: list[tuple[float, float]]) -> tuple[float, float, float]:
    """Least squares y = a + b x + c x² (normal equations, 3 × 3)."""
    s = [sum(x ** k for x, _ in pts) for k in range(5)]
    t = [sum(y * x ** k for x, y in pts) for k in range(3)]
    m = [[s[0], s[1], s[2], t[0]], [s[1], s[2], s[3], t[1]], [s[2], s[3], s[4], t[2]]]
    for col in range(3):  # Gauss-Jordan
        piv = max(range(col, 3), key=lambda r: abs(m[r][col]))
        m[col], m[piv] = m[piv], m[col]
        for r in range(3):
            if r != col:
                f = m[r][col] / m[col][col]
                m[r] = [x - f * y for x, y in zip(m[r], m[col])]
    return tuple(m[r][3] / m[r][r] for r in range(3))  # type: ignore[return-value]


# ---- exact-answer tier ---------------------------------------------------------------

def _coast_project(mass: float, v0: float, a: float, b: float, c: float, duration: float):
    """A car with no motor in action: the Battery Electric Car with nothing
    commanding the motor or the brakes, no rolling inertia and the given
    road-load coefficients."""
    proj = load_example("bev-car")
    for el in proj.systems[0].elements:
        d = el.componentDefId
        if d == "vehicle.body":
            el.parameterOverrides.update({
                "mass_kg": mass, "initial_speed_kmh": v0, "road_load_mode": "Coefficients A/B/C",
                "road_load_a_N": a, "road_load_b_N_per_kmh": b, "road_load_c_N_per_kmh2": c})
        if d in ("propulsion.wheel", "mech.brake", "motor.emotor", "mech.differential"):
            el.parameterOverrides["inertia_kgm2"] = 0
        if d == "mech.final_drive":
            el.parameterOverrides.update({"inertia_in_kgm2": 0, "inertia_out_kgm2": 0})
        if d == "motor.emotor":
            el.parameterOverrides["drag_torque"] = {"0": 0, "16000": 0}
            el.parameterOverrides["power_loss"] = {"0": {"0": 0, "310": 0}, "16000": {"0": 0, "310": 0}}
        if d == "electric.constant_drive":
            el.parameterOverrides["power_kW"] = 0
    proj.dataBusConnections = [d for d in proj.dataBusConnections
                               if d.port1Id not in ("sig_traction_cmd", "sig_brake_cmd")]
    proj.cases = [SimCase(id="case", name="coast", duration=duration, timeStep=0.1)]
    return proj


def _speed(result) -> list[tuple[float, float]]:
    ch = next(c for c in result.channels if c.portId == "sig_speed" and c.elementId == "el-vehicle")
    return [(p["t"], p["value"]) for p in ch.timeSeries]


def run_exact(ex: dict) -> dict:
    """LightSim against the closed form; the gap in % of the exact value."""
    m, v0 = ex["mass_kg"], ex["v0_kmh"]
    if ex["id"] == "coast-air-drag-only":
        proj = _coast_project(m, v0, 0, 0, ex["c_N_per_kmh2"], ex["t_s"])
        k = ex["c_N_per_kmh2"] * 3.6 ** 2 / m
        exact = v0 / 3.6 / (1 + k * v0 / 3.6 * ex["t_s"]) * 3.6
        got = _speed(simulate(proj, "case"))[-1][1]
    elif ex["id"] == "coast-constant-force":
        proj = _coast_project(m, v0, ex["a_N"], 0, 0, ex["t_s"])
        exact = (v0 / 3.6 - ex["a_N"] * ex["t_s"] / m) * 3.6
        got = _speed(simulate(proj, "case"))[-1][1]
    else:  # time from v0 to v1 under A + B v + C v²
        a, b, c, v1 = ex["a_N"], ex["b_N_per_kmh"], ex["c_N_per_kmh2"], ex["v1_kmh"]
        n = 20000  # midpoint rule on dt = m dv / F
        h = (v0 - v1) / n
        exact = sum(m * h / 3.6 / (a + b * (v1 + (j + .5) * h) + c * (v1 + (j + .5) * h) ** 2)
                    for j in range(n))
        pts = _speed(simulate(_coast_project(m, v0, a, b, c, exact * 1.5), "case"))
        j = next(j for j, (_, v) in enumerate(pts) if v <= v1)
        (t0, s0), (t1, s1) = pts[j - 1], pts[j]
        got = t0 + (s0 - v1) / (s0 - s1) * (t1 - t0)
    return {"id": ex["id"], "name": ex["name"], "exact": exact, "lightsim": got,
            "gap_pct": 100.0 * (got - exact) / exact}


def report() -> str:
    """The suite's results as a Markdown table (slow: runs every case)."""
    lines = ["| Case | Cycle | EPA, Wh/km at the wall | LightSim | Gap | Battery terminals, Wh/km |",
             "|---|---|---|---|---|---|"]
    for case in load_cases():
        for cyc in ("udds", "hwfet"):
            r = run_cycle(case, cyc)
            lines.append(f"| {case['name']} | {cyc.upper()} | {r.target:.1f} | {r.wh_per_km:.1f} "
                         f"| {r.gap_pct:+.1f} % | {r.wh_per_km_battery:.1f} |")
    lines += ["", "| Case | Coast-down: worst force gap 20-120 km/h |", "|---|---|"]
    for case in load_cases():
        lines.append(f"| {case['name']} | {coastdown(case)['worst_pct']:.2f} % |")
    lines += ["", "| Exact case | Exact | LightSim | Gap |", "|---|---|---|---|"]
    for ex in EXACT["cases"]:
        r = run_exact(ex)
        lines.append(f"| {r['name']} | {r['exact']:.4f} | {r['lightsim']:.4f} | {r['gap_pct']:+.3f} % |")
    return "\n".join(lines)


if __name__ == "__main__":
    print(report())

