#!/usr/bin/env python3
"""LightSim against FASTSim on LightSim's EPA reference suite.

A benchmarking tool only: nothing in LightSim imports it, and FASTSim is
never a LightSim dependency. It compares the two simulators on the four
electric cars and two EPA cycles (UDDS, HWFET) of ``backend/validation/``:

1. how far each one's energy at the wall is from EPA's published figure;
2. how long each one takes (median of warm runs, as a multiple of real time).

LightSim and FASTSim live in different Python environments, so the work is
split in three steps that only exchange small JSON files. See README.md.

    # LightSim's own interpreter (never needs fastsim)
    <lightsim python> compare.py lightsim --out results.lightsim.json
    # the FASTSim virtualenv (never needs LightSim's packages)
    <fastsim python> compare.py fastsim --vehicles <fastsim>/cal_and_val/f2-vehicles \
        --out results.fastsim.json
    # any Python 3.11+, standard library only
    python compare.py report results.lightsim.json results.fastsim.json --out results.md

or all three in one call: ``compare.py all --fastsim-python <venv python>
--vehicles <dir>``.

Both tools are post-processed the same way, the way the suite does it
(``validation/suite.py``, ``run_cycle``): energy at the wall = the net energy
out of the battery's chemistry (terminal energy, regeneration subtracted,
plus the battery's own internal losses) per km actually driven, divided by
a charger efficiency of 0.86. The EPA figure is the *unadjusted* mpge turned
into Wh/km, so no EPA 0.7 label factor is involved.
"""
from __future__ import annotations

import argparse
import bisect
import csv
import datetime
import gc
import hashlib
import json
import math
import os
import platform
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
BACKEND = REPO / "backend"
VALIDATION = BACKEND / "validation"
CYCLE_DIR = BACKEND / "app" / "cycles"

CYCLES = ("udds", "hwfet")

# The suite's constants (validation/suite.py). They are repeated here so the
# FASTSim side needs nothing from LightSim; the LightSim side checks them
# against the suite and stops if they ever drift apart.
LB_KG = 0.45359237
HP_KW = 0.745699872
LBF_N = 4.448222
MPH_KMH = 1.609344
WH_PER_GALLON_EQ = 33705.0
INERTIA_SHARE = 0.015  # rules: rotating_inertia_pct, all of it in the four wheels
OCV_V = 350.0
BATTERY_LOSS_AT_1C = 0.015  # rules: battery
G = 9.81  # FASTSim's gravity
RHO_FASTSIM = 1.2  # FASTSim's constant air density, kg/m3 (f2_const_air_density)


# ---- shared, standard library only ------------------------------------------------------

def load_suite() -> list[dict]:
    """The suite's cases, in the suite's order."""
    suite = json.loads((VALIDATION / "suite.json").read_text(encoding="utf-8"))
    return [json.loads((VALIDATION / p).read_text(encoding="utf-8")) for p in suite["cases"]]


def pick(cases: list[dict], wanted: list[str] | None) -> list[dict]:
    if not wanted:
        return cases
    chosen = [c for c in cases if any(w.lower() in c["id"] for w in wanted)]
    if not chosen:
        sys.exit(f"no case id contains any of {wanted}; the ids are {[c['id'] for c in cases]}")
    return chosen


def mpge_to_wh_per_km(mpge: float) -> float:
    return WH_PER_GALLON_EQ / (mpge * MPH_KMH)


def cycle_points(cycle_id: str, case: dict) -> list[tuple[float, float]]:
    """LightSim's cycle file as (s, km/h); the case's SHA-256 must match."""
    path = CYCLE_DIR / f"{cycle_id}.csv"
    if hashlib.sha256(path.read_bytes()).hexdigest() != case["cycles"][cycle_id]:
        sys.exit(f"{path} is not the cycle file case {case['id']} was written for")
    with path.open(encoding="utf-8", newline="") as f:
        return [(float(r["t_s"]), float(r["speed_kmh"])) for r in csv.DictReader(f)]


def cycle_1hz(points: list[tuple[float, float]]) -> list[float]:
    """The cycle's speeds in km/h at every whole second from 0, by the same
    straight-line interpolation LightSim's Driving Task uses between points."""
    times = [t for t, _ in points]
    out = []
    for t in range(int(times[-1]) + 1):
        j = bisect.bisect_left(times, t)
        if times[j] == t:
            out.append(points[j][1])
        else:
            (t0, v0), (t1, v1) = points[j - 1], points[j]
            out.append(v0 + (v1 - v0) * (t - t0) / (t1 - t0))
    return out


def epa_inputs(case: dict) -> dict:
    """The case's values after the suite's rules (suite.inputs_of), SI."""
    epa, fs = case["epa"], case["fastsim"]
    a, b, c = epa["target_abc_lbf"]
    return {
        "mass_kg": epa["test_weight_lb"] * LB_KG,
        "abc": (a * LBF_N, b * LBF_N / MPH_KMH, c * LBF_N / MPH_KMH ** 2),
        "wheel_radius_m": fs["wheel_radius_m"],
        "motor_kw": epa["rated_hp"] * HP_KW if "rated_hp" in epa else fs["motor_kW"],
        "battery_kwh": fs["battery_kWh"],
        "battery_max_kw": fs["battery_max_kW"],
        "aux_kw": fs["aux_kW"],
        "charger": fs["charger_efficiency"],
    }


def road_load_n(abc: tuple[float, float, float], kmh: float) -> float:
    return abc[0] + abc[1] * kmh + abc[2] * kmh * kmh


def median(xs: list[float]) -> float:
    return statistics.median(xs)


def machine() -> dict:
    return {
        "date": datetime.date.today().isoformat(),
        "time_utc": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d %H:%M:%S"),
        "cpus": os.cpu_count(),
        "loadavg": [round(x, 2) for x in os.getloadavg()],
        "platform": platform.platform(),
        "python": platform.python_version(),
    }


def git_head(where: Path) -> str | None:
    try:
        out = subprocess.run(["git", "-C", str(where), "rev-parse", "--short=12", "HEAD"],
                             capture_output=True, text=True, timeout=20)
    except (OSError, subprocess.SubprocessError):
        return None
    return out.stdout.strip() or None if out.returncode == 0 else None


def timed(make, run, runs: int):
    """Call ``run(make())`` ``runs`` times; only ``run`` is timed. Returns
    (wall seconds, CPU seconds, the last run's output)."""
    wall, cpu, out = [], [], None
    for _ in range(runs):
        x = make()
        gc.collect()
        w0, c0 = time.perf_counter(), time.process_time()
        out = run(x)
        wall.append(time.perf_counter() - w0)
        cpu.append(time.process_time() - c0)
    return wall, cpu, out


def log(msg: str) -> None:
    print(msg, file=sys.stderr, flush=True)


# ---- the LightSim side --------------------------------------------------------------------

def run_lightsim(args: argparse.Namespace) -> None:
    sys.path.insert(0, str(BACKEND))
    os.chdir(BACKEND)  # LightSim is run from backend/
    from app.solver import simulate  # noqa: PLC0415
    from validation import suite  # noqa: PLC0415

    cases = pick(load_suite(), args.cars)
    start = machine()
    results = []
    for case in cases:
        i = suite.inputs_of(case)
        mine = epa_inputs(case)
        for got, want, what in [
                (i.mass_kg, mine["mass_kg"], "mass"), (i.abc[0], mine["abc"][0], "A"),
                (i.abc[1], mine["abc"][1], "B"), (i.abc[2], mine["abc"][2], "C"),
                (i.motor_kw, mine["motor_kw"], "motor power"),
                (i.battery_kwh, mine["battery_kwh"], "battery size"),
                (i.max_kw, mine["battery_max_kw"], "battery limit"),
                (i.aux_kw, mine["aux_kw"], "auxiliaries"), (i.charger, mine["charger"], "charger"),
                (i.radius_m, mine["wheel_radius_m"], "wheel radius"),
                (i.wheel_inertia, INERTIA_SHARE * i.mass_kg * i.radius_m ** 2 / 4, "wheel inertia"),
                (i.r0_ohm, BATTERY_LOSS_AT_1C * OCV_V ** 2 / (i.battery_kwh * 1000), "resistance")]:
            if abs(got - want) > 1e-9 * max(1.0, abs(want)):
                sys.exit(f"{case['id']}: the suite's {what} is {got}, this script's is {want}; "
                         f"update the constants at the top of compare.py")
        if abs(suite.mpge_to_wh_per_km(case["target"]["udds_mpge"])
               - mpge_to_wh_per_km(case["target"]["udds_mpge"])) > 1e-9:
            sys.exit("the suite's mpge conversion changed; update compare.py")
        for cycle in CYCLES:
            load_before = os.getloadavg()[0]
            # the first run is the warm-up, and gives the result by the suite's own function
            r = suite.run_cycle(case, cycle)
            model_build, _, _ = timed(lambda: None, lambda _: suite.build_project(case, cycle), 3)
            wall, cpu, res = timed(lambda: suite.build_project(case, cycle),
                                   lambda proj: simulate(proj, "case"), args.runs)
            summary = {s.label: s.value for s in res.summary}
            duration = int(suite.cycles.info(cycle)["duration_s"])
            results.append({
                "case": case["id"], "name": case["name"], "cycle": cycle,
                "duration_s": duration, "distance_km": summary["Distance driven"],
                "wall_wh_per_km": r.wh_per_km, "terminal_wh_per_km": r.wh_per_km_battery,
                "target_wh_per_km": r.target, "gap_pct": r.gap_pct, "status": r.status,
                "solver_notes": [m.text for m in res.messages if "Solver step" in m.text],
                "wall_s": wall, "cpu_s": cpu, "median_s": median(wall), "median_cpu_s": median(cpu),
                "x_real_time": duration / median(wall), "model_build_s": median(model_build),
                "loadavg_1min": [round(load_before, 2), round(os.getloadavg()[0], 2)],
            })
            log(f"lightsim {case['id']:30s} {cycle:5s} {r.wh_per_km:7.2f} Wh/km "
                f"{r.gap_pct:+5.1f} %  median {median(wall):6.2f} s ({duration / median(wall):5.1f}x)")
    inputs = {}
    for case in cases:
        e = epa_inputs(case)
        inputs[case["id"]] = {
            "mass_kg": e["mass_kg"], "abc_si": list(e["abc"]), "wheel_radius_m": e["wheel_radius_m"],
            "rotating_inertia_equiv_kg": INERTIA_SHARE * e["mass_kg"], "driveline_eff": 1.0,
            "motor_kw": e["motor_kw"], "battery_kwh": e["battery_kwh"],
            "battery_max_kw": e["battery_max_kw"], "aux_kw": e["aux_kw"], "charger": e["charger"],
            "battery_loss_at_1c_pct": 100 * BATTERY_LOSS_AT_1C,
        }
    out = {
        "tool": "lightsim", "runs": args.runs, "machine": start,
        "machine_end": machine(), "lightsim_version": (REPO / "VERSION").read_text().strip(),
        "lightsim_commit": git_head(REPO), "suite_version": suite.SUITE["version"],
        "timed": "simulate(project, 'case') only; the project is built outside the timer",
        "solver_step_s": 0.01, "recorded_step_s": 1.0, "inputs": inputs, "results": results,
    }
    Path(args.out).write_text(json.dumps(out, indent=1), encoding="utf-8")
    log(f"wrote {args.out}")


# ---- the FASTSim side ---------------------------------------------------------------------

def sha256_of(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fastsim_cycle(fsim, speeds_kmh: list[float], where: Path, name: str):
    """A FASTSim cycle from 1 Hz speeds (flat road, FASTSim's default ambient)."""
    path = where / f"{name}.csv"
    with path.open("w", encoding="utf-8") as f:
        f.write("time_seconds,speed_meters_per_second\n")
        for t, v in enumerate(speeds_kmh):
            f.write(f"{t},{v / 3.6!r}\n")
    return fsim.Cycle.from_file(path)


def fastsim_inputs(veh_dict: dict) -> dict:
    """The vehicle values that matter for the comparison, from FASTSim's own dict."""
    ch, bev = veh_dict["chassis"], veh_dict["pt_type"]["BEV"]
    r = ch["wheel_radius_meters"]
    eff = bev["res"]["eff_interp"]
    return {
        "mass_kg": veh_dict["mass_kilograms"], "drag_coef": ch["drag_coef"],
        "frontal_area_m2": ch["frontal_area_square_meters"], "rolling_coef": ch["wheel_rr_coef"],
        "air_density": RHO_FASTSIM,
        "wheel_radius_m": r, "rotating_inertia_equiv_kg":
            ch["num_wheels"] * ch["wheel_inertia_kilogram_square_meters"] / r ** 2,
        "driveline_eff": bev["transmission"]["eff_interp"],
        "motor_kw": bev["em"]["pwr_out_max_watts"] / 1000,
        "battery_kwh": bev["res"]["energy_capacity_joules"] / 3.6e6,
        "battery_max_kw": bev["res"]["pwr_out_max_watts"] / 1000,
        "aux_kw": veh_dict["pwr_aux_base_watts"] / 1000,
        "battery_eff": eff["Constant"] if "Constant" in eff else "C-rate table",
    }


def source_check(case: dict, base) -> dict:
    """Does the FASTSim file hold the values the suite says it took from it?
    True/False per item; None where the suite does not take the value from
    the file (the Model 3's motor power is EPA's rated horsepower)."""
    import yaml  # noqa: PLC0415  (a FASTSim dependency)

    fs, f = case["fastsim"], fastsim_inputs(base.to_dict())

    def same(a: float, b: float, rel: float = 5e-4) -> bool:
        return abs(a - b) <= rel * max(1.0, abs(b))

    em = yaml.safe_load(base.to_yaml())["pt_type"]["BEV"]["em"]["eff_interp_achieved"]["data"]
    grid, values = em["grid"][0], em["values"]
    curve = fs["motor_efficiency"]
    return {
        "wheel radius": same(f["wheel_radius_m"], fs["wheel_radius_m"]),
        "auxiliaries": same(f["aux_kw"], fs["aux_kW"]),
        "motor efficiency curve": (
            len(grid) == len(curve["power_out_pct"])
            and all(same(100 * g, p) for g, p in zip(grid, curve["power_out_pct"]))
            and all(same(100 * v, p) for v, p in zip(values, curve["efficiency_pct"]))),
        "motor power": same(f["motor_kw"], fs["motor_kW"]) if "motor_kW" in fs else None,
        "battery size": same(f["battery_kwh"], fs["battery_kWh"]),
        "battery power limit": same(f["battery_max_kw"], fs["battery_max_kW"]),
    }


def fit_road_load(abc: tuple[float, float, float], speeds_kmh: list[float]) -> tuple[float, float, float]:
    """FASTSim has a rolling term (constant) and an air term (v squared), no
    term in v. Fit a + c v^2 to EPA's A + B v + C v^2 over the cycle's speeds,
    weighted by speed (so the energy counts), and return (a in N, c in N per
    (km/h)^2, the fit's road-load energy over EPA's, over the cycle)."""
    v = speeds_kmh
    s0, s2, s4 = (sum(x ** k for x in v) for k in (1, 3, 5))
    t0 = sum(x * road_load_n(abc, x) for x in v)
    t2 = sum(x ** 3 * road_load_n(abc, x) for x in v)
    det = s0 * s4 - s2 * s2
    a, c = (t0 * s4 - s2 * t2) / det, (s0 * t2 - s2 * t0) / det
    e_fit = sum((a + c * x * x) * x for x in v)
    e_epa = sum(road_load_n(abc, x) * x for x in v)
    return a, c, e_fit / e_epa


def aligned_vehicle(fsim, base, case: dict, speeds_kmh: list[float]):
    """FASTSim's vehicle for the case, with LightSim's inputs where FASTSim
    has a place for them (the suite's rules, validation/README.md):
    EPA test weight, with the 1.5 % rotating inertia in the wheels; EPA's road
    load refitted to FASTSim's two terms; no driveline loss (the road load
    already holds it); the suite's motor power, battery size and limit and
    auxiliaries; and the suite's battery loss (1.5 % of the power at 1C) as
    FASTSim's efficiency against C-rate. FASTSim's motor efficiency curve is
    the one the suite took, so it stays."""
    import yaml  # noqa: PLC0415  (a FASTSim dependency)

    e = epa_inputs(case)
    a, c, ratio = fit_road_load(e["abc"], speeds_kmh)
    y = yaml.safe_load(base.to_yaml())
    ch, bev = y["chassis"], y["pt_type"]["BEV"]
    m, r = e["mass_kg"], ch["wheel_radius_meters"]
    cda = 2 * c * 3.6 ** 2 / RHO_FASTSIM
    y["mass_kilograms"] = m
    ch["drag_coef"] = cda / ch["frontal_area_square_meters"]
    ch["wheel_rr_coef"] = a / (m * G)
    ch["wheel_inertia_kilogram_square_meters"] = INERTIA_SHARE * m * r * r / ch["num_wheels"]
    bev["transmission"]["eff_interp"] = 1.0
    bev["em"]["pwr_out_max_watts"] = e["motor_kw"] * 1000
    bev["res"]["energy_capacity_joules"] = e["battery_kwh"] * 3.6e6
    bev["res"]["pwr_out_max_watts"] = e["battery_max_kw"] * 1000
    y["pwr_aux_base_watts"] = e["aux_kw"] * 1000
    crate = [k * 0.5 for k in range(-20, 21)]  # 1/h; the sign is the power's
    bev["res"]["eff_interp"] = {"CRate": {
        "data": {"grid": [crate], "values": [
            1 / (1 + BATTERY_LOSS_AT_1C * x) if x > 0 else 1 - BATTERY_LOSS_AT_1C * -x
            for x in crate]},
        "strategy": "Linear", "extrapolate": "Clamp"}}
    return fsim.Vehicle.from_yaml(yaml.safe_dump(y)), {"road_load_energy_ratio": ratio}


def run_fastsim(args: argparse.Namespace) -> None:
    try:
        import fastsim as fsim  # noqa: PLC0415  (only here: this is the FASTSim side)
    except ImportError:
        sys.exit("fastsim is not installed in this Python. Run the 'fastsim' step with the "
                 "FASTSim virtualenv's interpreter (see README.md).")
    cases = pick(load_suite(), args.cars)
    vehicles = Path(args.vehicles).resolve()
    start = machine()
    results, inputs, files = [], {}, {}
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        speeds, cycles = {}, {}
        for cycle in CYCLES:
            speeds[cycle] = cycle_1hz(cycle_points(cycle, cases[0]))
            cycles[cycle] = fastsim_cycle(fsim, speeds[cycle], tmp, cycle)
        for case in cases:
            for cycle in CYCLES:
                cycle_points(cycle, case)  # the same file this case was written for
            path = vehicles / case["fastsim"]["file"]
            if not path.is_file():
                sys.exit(f"{path} is missing: --vehicles must hold FASTSim's cal_and_val "
                         f"f2-vehicles (or f3-vehicles) files, see README.md")
            files[case["fastsim"]["file"]] = sha256_of(path)
            base = fsim.Vehicle.from_file(path)
            e = epa_inputs(case)
            inputs[case["id"]] = {"own": fastsim_inputs(base.to_dict()),
                                  "file": case["fastsim"]["file"],
                                  "source_check": source_check(case, base)}
            for cycle in CYCLES:
                variants = {"own": (base, {})}
                if not args.no_aligned:
                    variants["aligned"] = aligned_vehicle(fsim, base, case, speeds[cycle])
                for name, (veh, extra) in variants.items():
                    load_before = os.getloadavg()[0]
                    if name == "aligned":
                        d = fastsim_inputs(veh.to_dict())
                        d["fit_road_load_energy_ratio"] = extra["road_load_energy_ratio"]
                        inputs[case["id"]]["aligned_" + cycle] = d
                    status, sd = "success", None
                    try:
                        sd = fsim.SimDrive(veh, cycles[cycle])  # warm-up, and the result
                        sd.run()
                        wall, cpu, sd = timed(lambda: None,
                                              lambda _: _run_once(fsim, veh, cycles[cycle]),
                                              args.runs)
                    except Exception as err:  # FASTSim raises when the cycle is missed
                        status = f"error: {str(err).splitlines()[0][:200]}"
                        wall = cpu = [float("nan")]
                    d = sd.to_dict() if sd is not None else None
                    km = d["veh"]["state"]["dist_meters"] / 1000 if d else float("nan")
                    res = d["veh"]["pt_type"]["BEV"]["res"]["state"] if d else {}
                    chemical = res.get("energy_out_chemical_joules", float("nan")) / 3600 / km
                    terminal = res.get("energy_out_electrical_joules", float("nan")) / 3600 / km
                    target = mpge_to_wh_per_km(case["target"][f"{cycle}_mpge"])
                    wall_wh = chemical / e["charger"]
                    duration = len(speeds[cycle]) - 1
                    if d and not d["veh"]["state"]["cyc_met_overall"]:
                        status = "cycle not met"
                    results.append({
                        "case": case["id"], "name": case["name"], "cycle": cycle, "variant": name,
                        "duration_s": duration, "distance_km": km, "status": status,
                        "chemical_wh_per_km": chemical, "terminal_wh_per_km": terminal,
                        "wall_wh_per_km": wall_wh, "target_wh_per_km": target,
                        "gap_pct": 100 * (wall_wh - target) / target,
                        "wall_s": wall, "cpu_s": cpu, "median_s": median(wall),
                        "median_cpu_s": median(cpu), "x_real_time": duration / median(wall),
                        "loadavg_1min": [round(load_before, 2), round(os.getloadavg()[0], 2)],
                    })
                    log(f"fastsim  {case['id']:30s} {cycle:5s} {name:7s} {wall_wh:7.2f} Wh/km "
                        f"{results[-1]['gap_pct']:+5.1f} %  median {median(wall) * 1000:7.2f} ms "
                        f"({duration / median(wall):8.0f}x) {status}")
    out = {
        "tool": "fastsim", "runs": args.runs, "machine": start, "machine_end": machine(),
        "fastsim_version": getattr(fsim, "__version__", "?"),
        "vehicles_dir": str(vehicles), "vehicles_commit": git_head(vehicles),
        "vehicle_files_sha256": files,
        "timed": "SimDrive(vehicle, cycle) and .run(); the vehicle's own history saving (every step) "
                 "stays on",
        "inputs": inputs, "results": results,
    }
    Path(args.out).write_text(json.dumps(out, indent=1), encoding="utf-8")
    log(f"wrote {args.out}")


def _run_once(fsim, veh, cyc):
    sd = fsim.SimDrive(veh, cyc)
    sd.run()
    return sd


# ---- the report -----------------------------------------------------------------------------

def signed(x: float) -> str:
    text = f"{x:+.1f} %"
    return "0.0 %" if text in ("+0.0 %", "-0.0 %") else text.replace("-", "−")


def geomean(xs: list[float]) -> float:
    return math.exp(sum(math.log(x) for x in xs) / len(xs))


def fmt_time(s: float) -> str:
    return f"{s * 1000:.1f} ms" if s < 1 else f"{s:.1f} s"


def build_report(ls: dict, fs: dict) -> str:
    lres = {(r["case"], r["cycle"]): r for r in ls["results"]}
    fres = {(r["case"], r["cycle"], r["variant"]): r for r in fs["results"]}
    keys = [(c, y) for c in ls["inputs"] if c in fs["inputs"] for y in CYCLES
            if (c, y) in lres and (c, y, "own") in fres]
    names = {r["case"]: r["name"] for r in ls["results"]}
    for c, y in keys:  # the same EPA figure on both sides
        if abs(lres[c, y]["target_wh_per_km"] / fres[c, y, "own"]["target_wh_per_km"] - 1) > 1e-9:
            sys.exit(f"{c} {y}: the two sides do not hold the same EPA figure")
    has_aligned = all((c, y, "aligned") in fres for c, y in keys)
    date = ls["machine"]["date"]
    L = []
    add = L.append

    add("# LightSim against FASTSim, on EPA's reference cars")
    add("")
    add(f"Measured on {date} (LightSim {ls['lightsim_version']}, commit `{ls['lightsim_commit']}`, "
        f"reference suite v{ls['suite_version']}; FASTSim {fs['fastsim_version']}). "
        "Produced by `benchmarks/fastsim/compare.py`; how to repeat it and what the comparison "
        "does not show are in [README.md](README.md). The raw numbers are in "
        "`results.lightsim.json` and `results.fastsim.json`.")
    add("")
    add("## 1. Energy at the wall against EPA")
    add("")
    add("Same cycle files (LightSim's `udds.csv` and `hwfet.csv`, 1 Hz, flat road), same EPA "
        "figure (the *unadjusted* mpge of the car's own EPA test, as Wh/km at the wall socket), "
        "same post-processing (net battery chemical energy per km driven, divided by 0.86). "
        "**Each tool drives its own vehicle definition**: LightSim the suite's model (EPA test "
        "weight and road load, FASTSim's powertrain values), FASTSim the vehicle file the "
        "suite's powertrain values came from. Section 3 lists what differs.")
    add("")
    add("| Car | Cycle | EPA, Wh/km | LightSim | Gap | FASTSim | Gap |")
    add("|---|---|---|---|---|---|---|")
    last = None
    for c, y in keys:
        lr, fr = lres[c, y], fres[c, y, "own"]
        add(f"| {names[c] if c != last else ''} | {y.upper()} | {lr['target_wh_per_km']:.1f} | "
            f"{lr['wall_wh_per_km']:.1f} | {signed(lr['gap_pct'])} | {fr['wall_wh_per_km']:.1f} | "
            f"{signed(fr['gap_pct'])}{'' if fr['status'] == 'success' else ' (' + fr['status'] + ')'} |")
        last = c
    lg = [abs(lres[k]["gap_pct"]) for k in keys]
    fg = [abs(fres[k[0], k[1], "own"]["gap_pct"]) for k in keys]
    add(f"| **Mean absolute gap** | | | | **{statistics.mean(lg):.1f} %** | | "
        f"**{statistics.mean(fg):.1f} %** |")
    add(f"| Largest absolute gap | | | | {max(lg):.1f} % | | {max(fg):.1f} % |")
    add(f"| Cases within 5 % / 8 % of EPA | | | | {sum(g <= 5 for g in lg)} / "
        f"{sum(g <= 8 for g in lg)} of {len(lg)} | | {sum(g <= 5 for g in fg)} / "
        f"{sum(g <= 8 for g in fg)} of {len(fg)} |")
    add("")
    add("Battery-terminal energy before the internal losses and the charger, Wh/km "
        "(for reference):")
    add("")
    add("| Car | Cycle | LightSim | FASTSim |")
    add("|---|---|---|---|")
    last = None
    for c, y in keys:
        add(f"| {names[c] if c != last else ''} | {y.upper()} | {lres[c, y]['terminal_wh_per_km']:.1f} | "
            f"{fres[c, y, 'own']['terminal_wh_per_km']:.1f} |")
        last = c
    add("")
    add("The gap is (tool − EPA) / EPA. EPA's own repeat tests of one car differ by up to "
        "6 %, and the charger efficiency (0.86, FASTSim's value, used for both) moves every "
        "figure of both tools by about ±5 % if it is really 0.82 or 0.90. Differences "
        "between the two tools smaller than that are not significant.")
    add("")

    if has_aligned:
        add("### 1b. The same, with FASTSim given LightSim's inputs")
        add("")
        add("Section 1 compares two different vehicle definitions. To see how much of the "
            "difference comes from the inputs and how much from the models, FASTSim is run "
            "again with LightSim's suite values wherever FASTSim has a place for them (EPA "
            "test weight; EPA road load refitted to FASTSim's two terms; no driveline loss; "
            "the suite's motor power, battery size and limit, auxiliaries; the suite's "
            "battery loss as an efficiency against C-rate; FASTSim's motor efficiency curve "
            "is already the suite's). Nothing is tuned to EPA's results. What cannot be "
            "aligned is in section 3.")
        add("")
        add("| Car | Cycle | EPA, Wh/km | LightSim | Gap | FASTSim, LightSim's inputs | Gap | "
            "LightSim − FASTSim |")
        add("|---|---|---|---|---|---|---|---|")
        last, diffs = None, []
        for c, y in keys:
            lr, fr = lres[c, y], fres[c, y, "aligned"]
            d = 100 * (lr["wall_wh_per_km"] - fr["wall_wh_per_km"]) / fr["wall_wh_per_km"]
            diffs.append(abs(d))
            add(f"| {names[c] if c != last else ''} | {y.upper()} | {lr['target_wh_per_km']:.1f} | "
                f"{lr['wall_wh_per_km']:.1f} | {signed(lr['gap_pct'])} | {fr['wall_wh_per_km']:.1f} | "
                f"{signed(fr['gap_pct'])} | {signed(d)} |")
            last = c
        fa = [abs(fres[c, y, "aligned"]["gap_pct"]) for c, y in keys]
        add(f"| **Mean absolute gap / difference** | | | | **{statistics.mean(lg):.1f} %** | | "
            f"**{statistics.mean(fa):.1f} %** | **{statistics.mean(diffs):.1f} %** |")
        add(f"| Largest | | | | {max(lg):.1f} % | | {max(fa):.1f} % | {max(diffs):.1f} % |")
        add("")

    add("## 2. Run time")
    add("")
    add(f"Median of {ls['runs']} warm runs per car and cycle (one untimed warm-up run first), "
        "wall-clock, one process at a time, \"× real time\" = the cycle's length divided by "
        "the run time. LightSim: its Python engine, `simulate()` on the prepared project "
        "(10 ms solver step, a point recorded every second; building the project, "
        f"about {median([r['model_build_s'] for r in ls['results']]) * 1000:.0f} ms, is outside "
        "the timer). FASTSim: its compiled core, constructing the `SimDrive` and `run()` "
        "(1 s steps; the vehicle files save history at every step, left on).")
    add("")
    add("| Car | Cycle | Cycle length | LightSim | × real time | FASTSim | × real time | "
        "FASTSim is faster by |")
    add("|---|---|---|---|---|---|---|---|")
    last, ratios, cpu_ratios = None, [], []
    for c, y in keys:
        lr, fr = lres[c, y], fres[c, y, "own"]
        ratio = lr["median_s"] / fr["median_s"]
        ratios.append(ratio)
        cpu_ratios.append(lr["median_cpu_s"] / fr["median_cpu_s"])
        add(f"| {names[c] if c != last else ''} | {y.upper()} | {lr['duration_s']} s | "
            f"{fmt_time(lr['median_s'])} | {lr['x_real_time']:,.0f} | {fmt_time(fr['median_s'])} | "
            f"{fr['x_real_time']:,.0f} | {ratio:,.0f}× |")
        last = c
    add(f"| **Geometric mean** | | | | {geomean([lres[k]['x_real_time'] for k in keys]):,.0f} | | "
        f"{geomean([fres[k[0], k[1], 'own']['x_real_time'] for k in keys]):,.0f} | "
        f"**{geomean(ratios):,.0f}×** |")
    add("")
    spread = lambda rs: max(max(r["wall_s"]) / min(r["wall_s"]) for r in rs)  # noqa: E731
    add(f"Spread inside a car and cycle (slowest of the {ls['runs']} runs over the fastest): "
        f"LightSim up to {spread(list(lres.values())):.2f}×, FASTSim up to "
        f"{spread([r for r in fs['results'] if r['variant'] == 'own']):.2f}×. "
        f"Using CPU time instead of wall-clock gives a geometric-mean ratio of "
        f"{geomean(cpu_ratios):,.0f}×.")
    add("")
    add("Machine and load while measuring (the 1-minute load average includes the benchmark "
        "itself, which is one busy process):")
    add("")
    add("| | LightSim run | FASTSim run |")
    add("|---|---|---|")
    for label, get in [
            ("Started (UTC)", lambda d: d["machine"]["time_utc"]),
            ("CPUs", lambda d: str(d["machine"]["cpus"])),
            ("Load average (1, 5, 15 min) at start", lambda d: ", ".join(map(str, d["machine"]["loadavg"]))),
            ("Load average (1, 5, 15 min) at end", lambda d: ", ".join(map(str, d["machine_end"]["loadavg"]))),
            ("Highest 1-minute load seen between runs",
             lambda d: str(max(max(r["loadavg_1min"]) for r in d["results"]))),
            ("Python", lambda d: d["machine"]["python"]),
            ("Platform", lambda d: d["machine"]["platform"])]:
        add(f"| {label} | {get(ls)} | {get(fs)} |")
    add("")
    busiest = max(max(r["loadavg_1min"]) for r in ls["results"] + fs["results"])
    add(f"Other agents share these {ls['machine']['cpus']} CPUs. The times are not corrected for "
        f"that: the highest 1-minute load seen between runs was {busiest:g} (this benchmark is "
        "one of the busy processes in it), and a busy neighbour on a shared core makes a time "
        "longer, never shorter. The spread above shows how much the repeats moved.")
    add("")

    add("## 3. What differs between the two vehicle definitions")
    add("")
    add("Per car: LightSim's value (the suite's rules applied to EPA's data and FASTSim's "
        "powertrain values) against FASTSim's own vehicle file. The road load is the force at "
        "constant speed on a flat road; for FASTSim it is rolling coefficient × mass × "
        f"9.81 + ½ × {RHO_FASTSIM} kg/m³ × Cd × A × v².")
    add("")
    ids = [c for c in ls["inputs"] if c in fs["inputs"]]
    add("Every cell reads LightSim / FASTSim.")
    add("")
    add("| | " + " | ".join(names[c] for c in ids) + " |")
    add("|---|" + "---|" * len(ids))

    def row(label: str, cell) -> None:
        add(f"| {label} | " + " | ".join(cell(ls["inputs"][c], fs["inputs"][c]["own"]) for c in ids) + " |")

    def fs_road(f: dict, kmh: float) -> float:
        return (f["rolling_coef"] * f["mass_kg"] * G
                + 0.5 * f["air_density"] * f["drag_coef"] * f["frontal_area_m2"] * (kmh / 3.6) ** 2)

    add("| FASTSim's vehicle file | " + " | ".join(
        "`" + fs["inputs"][c]["file"].removesuffix(".yaml") + "`" for c in ids) + " |")
    row("Mass, kg", lambda a, b: f"{a['mass_kg']:.0f} / {b['mass_kg']:.0f}")
    row("Road load at 48 km/h, N", lambda a, b: f"{road_load_n(a['abc_si'], 48.28):.0f} / "
                                                 f"{fs_road(b, 48.28):.0f}")
    row("Road load at 97 km/h, N", lambda a, b: f"{road_load_n(a['abc_si'], 96.56):.0f} / "
                                                 f"{fs_road(b, 96.56):.0f}")
    row("Rotating inertia, as extra mass, kg", lambda a, b: f"{a['rotating_inertia_equiv_kg']:.0f} / "
                                                            f"{b['rotating_inertia_equiv_kg']:.0f}")
    row("Driveline efficiency", lambda a, b: f"{a['driveline_eff']:.2f} / {b['driveline_eff']:.2f}")
    row("Motor power, kW", lambda a, b: f"{a['motor_kw']:.1f} / {b['motor_kw']:.1f}")
    row("Battery, kWh", lambda a, b: f"{a['battery_kwh']:.1f} / {b['battery_kwh']:.1f}")
    row("Battery power limit, kW", lambda a, b: f"{a['battery_max_kw']:.0f} / {b['battery_max_kw']:.0f}")
    row("Auxiliaries, kW", lambda a, b: f"{a['aux_kw']:.2f} / {b['aux_kw']:.2f}")
    add("")
    checks = {c: fs["inputs"][c].get("source_check", {}) for c in ids}
    items = list(next(iter(checks.values()), {}))
    if items:
        add("The suite says it took these values from the FASTSim file. Checked against the "
            "file on this run:")
        add("")
        add("| Value | Identical in the file | Differs (the suite's own choice, see the table) |")
        add("|---|---|---|")
        for item in items:
            same = [names[c] for c in ids if checks[c].get(item) is True]
            diff = [names[c] for c in ids if checks[c].get(item) is False]
            na = [names[c] for c in ids if checks[c].get(item) is None]
            add(f"| {item} | {len(same)} of {len(ids)} | "
                + ("; ".join(diff + [n + " (EPA's rated horsepower)" for n in na]) or "none") + " |")
        add("")
    add("Differences that remain even in section 1b, because the models differ, not the data:")
    add("")
    add("- **Road load.** LightSim takes EPA's A + B·v + C·v² as it is. FASTSim has "
        "only a rolling term and a v² term, so in 1b the pair is fitted to EPA's curve over "
        "each cycle's speeds, weighted by speed; the fitted curve's road-load energy over the "
        "cycle matches EPA's to "
        + (f"{max(abs(1 - v['fit_road_load_energy_ratio']) for i in fs['inputs'].values() for k, v in i.items() if k.startswith('aligned_')) * 100:.3f} %"
           if has_aligned else "n/a") + " but not the shape.")
    add("- **Battery.** LightSim: a flat 350 V with a resistance (loss grows with the square of "
        "the power). FASTSim's own files: a constant 98.49 % each way (97 % round trip) at any "
        "power; in 1b, the suite's loss written as an efficiency against C-rate.")
    add("- **Motor.** LightSim has a torque-speed envelope and a loss map built from the "
        "suite's efficiency curve; FASTSim has that efficiency curve against the fraction of "
        "its rated power, with no speed dependence.")
    add("- **Regeneration.** LightSim's driver uses 98 % of the motor's generator torque before "
        "the friction brakes and fades recuperation out below about 11 km/h. FASTSim 3.1.0 "
        "has no such setting for a battery electric car (the 98 % in the FASTSim 2 files is "
        "not read, and its source lists the low-speed fade and the friction/regeneration "
        "split as not yet done): it recovers whatever the motor and battery limits allow, "
        "down to a stop.")
    met = ("all runs meet the trace" if all(r["status"] == "success" for r in fs["results"])
           else "some runs did not meet the trace, see section 1")
    add("- **Following the cycle.** LightSim's driver is a controller chasing the trace at "
        f"10 ms steps; FASTSim solves each 1 s step for the trace speed directly ({met}).")
    add("- **Charger.** FASTSim 3.1.0 vehicle files carry no charger efficiency; 0.86 "
        "(FASTSim's own default, the suite's value) is applied to both tools outside the "
        "tools.")
    add("")

    add("## 4. Files and versions")
    add("")
    add("- FASTSim vehicle files, from `" + "/".join(Path(fs["vehicles_dir"]).parts[-2:])
        + "` of a FASTSim checkout"
        + (f" at git `{fs['vehicles_commit']}`" if fs.get("vehicles_commit") else "") + ":")
    for f, h in fs["vehicle_files_sha256"].items():
        add(f"  - `{f}`, SHA-256 `{h}`")
    add(f"- LightSim timing note: {ls['timed']}; solver step {ls['solver_step_s'] * 1000:g} ms, "
        f"recorded step {ls['recorded_step_s']:g} s.")
    add(f"- FASTSim timing note: {fs['timed']}.")
    notes = sorted({n for r in ls["results"] for n in r["solver_notes"]})
    if notes:
        add("- LightSim reduced its solver step in some runs: " + "; ".join(notes))
    add("")
    return "\n".join(L)


def run_report(args: argparse.Namespace) -> None:
    ls = json.loads(Path(args.lightsim_json).read_text(encoding="utf-8"))
    fs = json.loads(Path(args.fastsim_json).read_text(encoding="utf-8"))
    text = build_report(ls, fs)
    if args.out:
        Path(args.out).write_text(text, encoding="utf-8")
        log(f"wrote {args.out}")
    else:
        print(text)


def run_all(args: argparse.Namespace) -> None:
    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    common = ["--runs", str(args.runs)] + (["--cars", *args.cars] if args.cars else [])
    ls_json, fs_json = out / "results.lightsim.json", out / "results.fastsim.json"
    me = str(Path(__file__).resolve())
    subprocess.run([args.lightsim_python, me, "lightsim", "--out", str(ls_json), *common],
                   check=True, cwd=BACKEND)
    subprocess.run([args.fastsim_python, me, "fastsim", "--out", str(fs_json),
                    "--vehicles", args.vehicles, *common], check=True)
    run_report(argparse.Namespace(lightsim_json=ls_json, fastsim_json=fs_json,
                                  out=str(out / "results.md")))


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="mode", required=True)

    def common(sp: argparse.ArgumentParser) -> None:
        sp.add_argument("--runs", type=int, default=5, help="timed runs per car and cycle (5)")
        sp.add_argument("--cars", nargs="*", help="only cases whose id contains one of these")

    a = sub.add_parser("lightsim", help="run LightSim's suite and time it (LightSim's Python)")
    a.add_argument("--out", required=True)
    common(a)
    b = sub.add_parser("fastsim", help="run FASTSim and time it (the FASTSim virtualenv's Python)")
    b.add_argument("--out", required=True)
    b.add_argument("--vehicles", required=True,
                   help="FASTSim's cal_and_val/f2-vehicles (or f3-vehicles) folder")
    b.add_argument("--no-aligned", action="store_true",
                   help="skip the run with LightSim's inputs (section 1b)")
    common(b)
    c = sub.add_parser("report", help="turn the two JSON files into results.md (standard library)")
    c.add_argument("lightsim_json")
    c.add_argument("fastsim_json")
    c.add_argument("--out")
    d = sub.add_parser("all", help="run the three steps in one go")
    d.add_argument("--fastsim-python", required=True)
    d.add_argument("--lightsim-python", default=sys.executable)
    d.add_argument("--vehicles", required=True)
    d.add_argument("--out-dir", default=str(HERE))
    common(d)
    args = p.parse_args()
    {"lightsim": run_lightsim, "fastsim": run_fastsim, "report": run_report,
     "all": run_all}[args.mode](args)


if __name__ == "__main__":
    main()
