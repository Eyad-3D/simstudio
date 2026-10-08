"""Today's engine as a benchmark engine: LightSim's Python causal
power-flow solver (backend/app/solver), driven through the ``lightsim``
package from project files built here.

Each reference problem today's engine can express has one or more
*expressions*: a LightSim project (the project file's JSON) made of the
library's parts, and how its channels and energy books map back onto the
problem's quantities. The problems it cannot express are in
:data:`CANNOT`, with the reason; that is information about the engine,
not a failure of the benchmark.

What the expressions have to work around (each says so in ``how``):

- no part holds an initial speed except through a Vehicle and its wheels,
  so a problem that starts spinning gets a *preamble*: the E-Motor spins
  its inertia up at constant torque (exact for explicit Euler), and the
  problem's clock starts when the preamble ends. Energies over the
  problem's window are the books of the full run minus those of a run
  that stops at the end of the preamble;
- there is no current-controlled load: a constant current is a Power
  Consumer asking for I × the terminal voltage of the step before (the
  test suite's own trick, tests/test_numerics.py);
- a part's behaviour comes from tables: a DC motor's straight torque-speed
  line and a constant power's hyperbola are written as full-load curves.
"""
from __future__ import annotations

import math
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

ROOT = Path(__file__).resolve().parents[2]
BACKEND = ROOT / "backend"
if str(BACKEND) not in sys.path:
    sys.path.insert(0, str(BACKEND))
os.environ.setdefault("LIGHTSIM_SCRIPT_TRUST", "off")

import lightsim  # noqa: E402
from lightsim._engine import engine  # noqa: E402

from ..reference import Problem  # noqa: E402
from ..reference.compare import Trace  # noqa: E402

NAME = "lightsim-py"
RPM = 60.0 / (2.0 * math.pi)  # 1/min per rad/s
KWH = 3.6e6  # J
EPS_T = 1e-7  # a profile's step is placed this far before its time, so the
# solver step that starts at that time already sees the new value
MAX_STEP = engine("solver.runtime").MAX_SUBSTEP  # its default solver step, s

CANNOT = {
    "elec_rc_step": "no resistor or capacitor part; the battery's RC pair cannot be charged "
                    "from a voltage source (an electrical bus has exactly one source)",
    "elec_rl_step": "no inductor or resistor part",
    "motor_dc_spinup": "no winding inductance: the E-Motor is a torque-demand map model with no "
                       "electrical state (the L = 0 case, motor_dc_spinup_l0, is expressed)",
    "therm_lumped_mass": "no thermal model (no heat capacity or conductance parts; MOD-09)",
    "therm_two_masses": "no thermal model (no heat capacity or conductance parts; MOD-09)",
}


def _git(*args: str) -> str:
    try:
        return subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True,
                              timeout=10).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return ""


def version() -> dict:
    """The engine's version, the last commit that changed its code
    (backend/app, backend/lightsim), and the commit the benchmarks ran
    from ("+" when the working tree had changes)."""
    head = _git("rev-parse", "--short", "HEAD")
    if head and _git("status", "--porcelain", "--", "backend", "benchmarks"):
        head += "+"
    code = _git("log", "-1", "--format=%h", "--", "backend/app", "backend/lightsim")
    return {"engine": NAME, "version": lightsim.__version__, "engine_commit": code,
            "commit": head, "default_step_s": MAX_STEP}


# ---- building project files ------------------------------------------------------------

class Build:
    """A one-system project file, as JSON (docs/spec/project.md)."""

    def __init__(self, name: str):
        self.name = name
        self.elements: list[dict] = []
        self.connections: list[dict] = []
        self.bus: list[dict] = []

    def part(self, el_id: str, component: str, **params) -> "Build":
        self.elements.append({"id": el_id, "componentDefId": component, "label": el_id,
                              "position": {"x": 0, "y": 0}, "parameterOverrides": params})
        return self

    def wire(self, a: str, a_port: str, b: str, b_port: str) -> "Build":
        self.connections.append({"id": f"c{len(self.connections) + 1}", "sourceElementId": a,
                                 "sourcePortId": a_port, "targetElementId": b,
                                 "targetPortId": b_port})
        return self

    def route(self, a: str, a_port: str, b: str, b_port: str) -> "Build":
        self.bus.append({"id": f"d{len(self.bus) + 1}", "element1Id": a, "port1Id": a_port,
                         "element2Id": b, "port2Id": b_port})
        return self

    def project(self, duration: float, step: float) -> dict:
        return {"id": f"bench-{self.name}", "name": self.name,
                "systems": [{"id": "root", "name": self.name, "parentId": None,
                             "elements": self.elements, "connections": self.connections}],
                "dataBusConnections": self.bus,
                "cases": [{"id": "case", "name": "case", "duration": duration,
                           "timeStep": step}]}


def _step_profile(before: float, after: float, at: float) -> str:
    """A Driving Task profile that is ``before`` until ``at``, then ``after``."""
    return f"0:{before!r}; {at - EPS_T!r}:{before!r}; {at - EPS_T!r}:{after!r}"


def _flat(torque: float, n_max: float) -> dict:
    """A full-load curve of ``torque`` at every speed, at any voltage."""
    row = {"0": torque, f"{n_max:.6g}": torque}
    return {"100": row, "1000": row}


def _no_loss(n_max: float, t_max: float) -> dict:
    row = {"0": 0.0, f"{t_max:.6g}": 0.0}
    return {"0": row, f"{n_max:.6g}": row}


def _supply(b: Build, volts: float, motor: str = "mot") -> None:
    b.part("src", "electric.voltage_source", voltage_V=volts)
    b.part("bus", "electric.node")
    b.wire("src", "pos", "bus", "t1").wire("bus", "t2", motor, "pos")


# ---- running ------------------------------------------------------------------------------

@dataclass
class Run:
    result: object  # lightsim.Result
    wall_s: float
    steps: int | None

    def ch(self, el_id: str, port: str) -> list[float | None]:
        return list(self.result.channel(f"{el_id}:{port}").values)

    @property
    def t(self) -> list[float]:
        return list(self.result.time)

    def kpi(self, key: str) -> float:
        return float(self.result.kpis[key])

    def part(self, label: str) -> dict:
        """A part's (or 'Rotating parts'') energy book, J."""
        for f in self.result.raw.partEnergy:
            if f.label == label or f.elementId == label:
                return {k: getattr(f, k) * KWH for k in
                        ("energyIn", "energyOut", "losses", "stored")}
        return {"energyIn": 0.0, "energyOut": 0.0, "losses": 0.0, "stored": 0.0}

    def closure_j(self) -> float | None:
        """The engine's own balance residual, J (its percentage of the energy
        its sources gave)."""
        energy = self.result.raw.energy
        pct = self.result.kpis.get("energy_balance_residual_pct")
        if energy is None:
            return None
        if pct is None:  # (no summary row: the energy report's rounded remainder)
            return energy.remainderKWh * KWH
        return pct / 100.0 * energy.sourceKWh * KWH


SOLVED = re.compile(r"solved: (\d+)(?: of \d+)? steps × \S+ s(?:, ended[^(]*)? \((\d+) sub-steps")


class DataChecksRefused(RuntimeError):
    """The Data Checks found an error in a reference model."""


def run_project(data: dict) -> Run:
    """Check and run a project file's one case; time only the run.

    A reference model goes through the Data Checks like any user's model: an
    error in it stops the benchmark (it is the check or the model that is
    wrong, not something to run past)."""
    schemas = engine("schemas")
    proj = lightsim.Project(schemas.Project.model_validate(data))
    # the checks Project.run(check=True) makes, here outside the timer
    refused = [c.text for c in proj.check() if c.level == "error" and c.case_id in (None, "case")]
    if refused:
        raise DataChecksRefused(f"{data['name']}: Data Check failed: " + " ".join(refused))
    t0 = time.perf_counter()
    result = proj.run("case", check=False)  # (checked just above)
    wall = time.perf_counter() - t0
    steps = None
    for m in result.messages:
        found = SOLVED.search(m.text)
        if found:
            steps = int(found.group(1)) * int(found.group(2))
    return Run(result, wall, steps)


def _trace(run: Run, signals: dict[str, list[float | None]], energy: dict[str, float],
           shift: float = 0.0, closure: float | None = None,
           kinetic: tuple[list[tuple[str, float]], float] | None = None) -> Trace:
    """A Trace on the problem's clock (engine time − shift), keeping t ≥ 0.

    ``kinetic`` is ([(speed signal, its inertia or mass)], the kinetic
    energy the engine's books say was stored): E_kin is taken from the
    engine's own speeds (what it actually holds at the end), and a
    message says so when its books disagree."""
    t_all = run.t
    keep = [k for k, t in enumerate(t_all) if t >= shift - 1e-9]
    times = [round(t_all[k] - shift, 9) for k in keep]
    sig = {name: [vals[k] for k in keep] for name, vals in signals.items()}
    msgs = [f"{m.level}: {m.text}" for m in run.result.messages if m.level != "info"]
    if kinetic is not None:
        pairs, booked = kinetic
        held = sum(0.5 * m * (sig[n][-1] ** 2 - sig[n][0] ** 2) for n, m in pairs)
        energy = {**energy, "E_kin": held}
        if abs(booked - held) > 1e-3 * max(abs(held), 1.0):
            msgs.append(f"Energy books: the stored kinetic energy changed by {booked:.6g} J; "
                        f"the engine's own speeds say {held:.6g} J (E_kin is taken from the "
                        f"speeds)")
    return Trace(times=times, signals=sig, energy=energy,
                 closure_j=run.closure_j() if closure is None else closure,
                 wall_s=run.wall_s, sim_s=t_all[-1] if t_all else 0.0, steps=run.steps,
                 status=run.result.status, messages=msgs)


def _scaled(values, factor: float, offset: float = 0.0) -> list[float | None]:
    return [None if v is None else v * factor + offset for v in values]


# ---- the expressions ---------------------------------------------------------------------

@dataclass
class Expression:
    label: str
    how: str
    run: Callable[[Problem, float], Trace]  # (problem, solver step) → Trace


def _battery(p: dict, x0: dict, flat_ocv: float | None = None, **extra) -> dict:
    ocv = ({"0": flat_ocv, "100": flat_ocv} if flat_ocv is not None
           else {"0": p["ocv_a"], "100": p["ocv_a"] + p["ocv_b"]})
    return {"pack_model": "Pack values", "capacity_Ah": p["Q_Ah"],
            "capacity_kWh": p["Q_Ah"] * 0.37, "ocv_table": ocv,
            "internal_resistance_ohm": p["R0"], "rc_resistance_ohm": p["R1"],
            "rc_time_constant_s": p["tau1"], "initial_soc_pct": 100.0 * x0["SOC"],
            "min_soc_pct": 1, "max_charge_power_kW": 1000, "coulombic_efficiency_pct": 100,
            **extra}


def _battery_signals(run: Run, p: dict, flat_ocv: float | None = None) -> dict:
    soc = _scaled(run.ch("batt", "sig_soc"), 0.01)
    v, i = run.ch("batt", "sig_voltage"), run.ch("batt", "sig_current")

    def ocv(s):
        return flat_ocv if flat_ocv is not None else p["ocv_a"] + p["ocv_b"] * s

    v1 = [None if None in (s, a, b) else ocv(s) - a - p["R0"] * b for s, a, b in zip(soc, v, i)]
    return {"V": v, "I": i, "SOC": soc, "v1": v1}


def _battery_energy(run: Run) -> dict:
    delivered = (run.kpi("batt.energy_delivered_kwh") - run.kpi("batt.energy_recuperated_kwh"))
    return {"E_terminal": delivered * KWH,
            "E_internal": run.kpi("batt.internal_losses_kwh") * KWH,
            "E_chem": -run.part("batt")["stored"]}


def batt_constant_current(problem: Problem, step: float, v_min: float = 0.0) -> Trace:
    p, x0 = problem.parameters, problem.initial
    b = Build(problem.id)
    b.part("batt", "battery.generic", **_battery(p, x0, min_voltage_V=v_min))
    b.part("bus", "electric.node").part("load", "electric.constant_drive")
    b.part("lut", "signal.lookup", mode="1D", sample_time_s=0,
           table_1d={"0": 0, "1000": p["I"]})  # kW at 1000 V: I × V
    b.wire("batt", "pos", "bus", "t1").wire("bus", "t2", "load", "pos")
    b.route("batt", "sig_voltage", "lut", "sig_x_in").route("lut", "sig_out", "load",
                                                              "sig_demand_in")
    run = run_project(b.project(problem.t_end, step))
    return _trace(run, _battery_signals(run, p), _battery_energy(run))


def batt_constant_power(problem: Problem, step: float) -> Trace:
    p, x0 = problem.parameters, problem.initial
    b = Build(problem.id)
    b.part("batt", "battery.generic", **_battery(p, x0, flat_ocv=p["E"]))
    b.part("bus", "electric.node").part("load", "electric.constant_drive",
                                        power_kW=p["P"] / 1000.0)
    b.wire("batt", "pos", "bus", "t1").wire("bus", "t2", "load", "pos")
    run = run_project(b.project(problem.t_end, step))
    return _trace(run, _battery_signals(run, p, p["E"]), _battery_energy(run))


def batt_voltage_limit(problem: Problem, step: float) -> Trace:
    return batt_constant_current(problem, step, v_min=problem.parameters["V_min"])


def dc_motor_l0(problem: Problem, step: float) -> Trace:
    """The motor's straight torque-speed line at full demand, and its loss
    R i² + b ω² as a function of speed alone (it never leaves the line)."""
    p, x0 = problem.parameters, problem.initial
    if x0["omega"] != 0:
        raise ValueError("this expression starts at rest")
    V, R, k, J, b_v = p["V"], p["R"], p["k"], p["J"], p["b"]
    w0 = k * V / (k * k + R * b_v)  # where the net torque is 0
    line = {"0": k * V / R - p["T_load"], f"{w0 * RPM:.9g}": -p["T_load"]}
    loss = {}
    for j in range(401):
        w = w0 * 1.2 * j / 400
        kw = ((V - k * w) ** 2 / R + b_v * w * w) / 1000.0
        loss[f"{w * RPM:.9g}"] = {"0": kw, "100": kw}
    b = Build(problem.id)
    _supply(b, V)
    b.part("mot", "motor.emotor", inertia_kgm2=J, full_load_torque={"10": line, "100": line},
           power_loss=loss, drag_torque={"0": 0, f"{w0 * 1.2 * RPM:.9g}": 0},
           max_speed_rpm=round(w0 * 1.2 * RPM, 3))
    b.part("load", "propulsion.propeller", torque_ref_Nm=0, inertia_kgm2=0)
    b.part("dem", "signal.constant", value=1.0)
    b.wire("mot", "shaft", "load", "shaft").route("dem", "sig_out", "mot", "sig_demand_in")
    run = run_project(b.project(problem.t_end, step))
    w = _scaled(run.ch("mot", "sig_speed"), 1.0 / RPM)
    i = _scaled(run.ch("mot", "sig_elec_power"), 1000.0 / V)
    energy = {"E_in": run.kpi("src.energy_supplied_kwh") * KWH}
    return _trace(run, {"omega": w, "i": i}, energy,
                  kinetic=([("omega", J)], run.part("Rotating parts")["stored"]))


def _preamble(problem: Problem, step: float, build: Callable[[float], Build],
              t_pre: float) -> tuple[Run, Run]:
    """The full run (preamble + problem) and a run of the preamble alone."""
    full = run_project(build(t_pre).project(t_pre + problem.t_end, step))
    pre = run_project(build(t_pre).project(t_pre, step))
    return full, pre


def _window(full: Run, pre: Run, label: str, key: str) -> float:
    return full.part(label)[key] - pre.part(label)[key]


def inertia_coastdown(problem: Problem, step: float) -> Trace:
    """Spun up by the E-Motor (1 s at constant torque), then let go: the
    motor's drag table (c ω, applied while its inverter is off) is the
    viscous friction, a Brake held at T_c the Coulomb friction."""
    p, x0 = problem.parameters, problem.initial
    J, c, tc, w0 = p["J"], p["c"], p["T_c"], x0["omega"]
    t_pre = 1.0
    n_max = 1.5 * w0 * RPM

    def build(t_pre: float) -> Build:
        b = Build(problem.id)
        _supply(b, 400.0)
        b.part("mot", "motor.emotor", inertia_kgm2=J, full_load_torque=_flat(J * w0 / t_pre, n_max),
               power_loss=_no_loss(n_max, 2 * J * w0 / t_pre),
               drag_torque={"0": 0.0, f"{n_max:.9g}": c * n_max / RPM}, max_speed_rpm=n_max)
        b.part("brk", "mech.brake", max_torque_Nm=tc, inertia_kgm2=0)
        b.part("dem", "signal.driving_task", profile=_step_profile(1.0, 0.0, t_pre))
        b.part("cmd", "signal.driving_task", profile=_step_profile(0.0, 1.0, t_pre))
        b.wire("mot", "shaft", "brk", "flange")
        b.route("dem", "sig_demand", "mot", "sig_demand_in")
        b.route("cmd", "sig_demand", "brk", "sig_demand_in")
        return b

    full, pre = _preamble(problem, step, build, t_pre)
    w = _scaled(full.ch("mot", "sig_speed"), 1.0 / RPM)
    energy = {"E_viscous": _window(full, pre, "mot", "losses"),
              "E_coulomb": _window(full, pre, "brk", "losses")}
    return _trace(full, {"omega": w}, energy, shift=t_pre, closure=_closure_window(full, pre),
                  kinetic=([("omega", J)], _window(full, pre, "Rotating parts", "stored")))


def _closure_window(full: Run, pre: Run) -> float | None:
    a, b = full.closure_j(), pre.closure_j()
    return None if a is None or b is None else a - b


def clutch_lockup(problem: Problem, step: float) -> Trace:
    """The E-Motor (J1) spins up with the Clutch open (0.5 s at constant
    torque), then drives T_drive while the Clutch closes onto a
    Propeller of zero torque (J2)."""
    p, x0 = problem.parameters, problem.initial
    J1, J2, tc, t1, w10 = p["J1"], p["J2"], p["T_c"], p["T_drive"], x0["omega1"]
    if x0["omega2"] != 0 or p["T_load"] != 0:
        raise ValueError("this expression starts J2 at rest with no load torque")
    t_pre, t_max = 0.5, 400.0
    n_max = 1.5 * w10 * RPM

    def build(t_pre: float) -> Build:
        b = Build(problem.id)
        _supply(b, 400.0)
        b.part("mot", "motor.emotor", inertia_kgm2=J1, full_load_torque=_flat(t_max, n_max),
               power_loss=_no_loss(n_max, t_max), drag_torque={"0": 0, f"{n_max:.9g}": 0},
               max_speed_rpm=n_max)
        b.part("clu", "mech.clutch", max_torque_Nm=tc)
        b.part("load", "propulsion.propeller", torque_ref_Nm=0, inertia_kgm2=J2)
        b.part("dem", "signal.driving_task",
               profile=_step_profile(J1 * w10 / t_pre / t_max, t1 / t_max, t_pre))
        b.part("eng", "signal.driving_task", profile=_step_profile(0.0, 1.0, t_pre))
        b.wire("mot", "shaft", "clu", "flange_a").wire("clu", "flange_b", "load", "shaft")
        b.route("dem", "sig_demand", "mot", "sig_demand_in")
        b.route("eng", "sig_demand", "clu", "sig_engage_in")
        return b

    full, pre = _preamble(problem, step, build, t_pre)
    sig = {"omega1": _scaled(full.ch("mot", "sig_speed"), 1.0 / RPM),
           "omega2": _scaled(full.ch("load", "sig_speed"), 1.0 / RPM),
           "slip": _scaled(full.ch("clu", "sig_slip_speed"), 1.0 / RPM),
           "T_clutch": full.ch("clu", "sig_torque")}
    energy = {"E_drive": _window(full, pre, "mot", "energyOut"),
              "E_clutch": _window(full, pre, "clu", "losses"), "E_load": 0.0}
    return _trace(full, sig, energy, shift=t_pre, closure=_closure_window(full, pre),
                  kinetic=([("omega1", J1), ("omega2", J2)],
                           _window(full, pre, "Rotating parts", "stored")))


def _gear_parts(b: Build, p: dict, n_max: float) -> None:
    _supply(b, 400.0)
    b.part("mot", "motor.emotor", inertia_kgm2=p["J1"], full_load_torque=_flat(p["T"], n_max),
           power_loss=_no_loss(n_max, p["T"]), drag_torque={"0": 0, f"{n_max:.9g}": 0},
           max_speed_rpm=n_max)
    b.part("gb", "mech.gearbox", ratios={"1": p["i1"], "2": p["i2"]}, efficiency_pct=100,
           inertia_in_kgm2=0, inertia_out_kgm2=0)
    b.part("dem", "signal.constant", value=1.0)
    b.part("gear", "signal.driving_task", profile=_step_profile(1.0, 2.0, p["t_shift"]))
    b.wire("mot", "shaft", "gb", "flange_in")
    b.route("dem", "sig_out", "mot", "sig_demand_in")
    b.route("gear", "sig_demand", "gb", "sig_gear_in")


def gear_change_rotational(problem: Problem, step: float) -> Trace:
    p = problem.parameters
    if problem.initial["omega2"] != 0:
        raise ValueError("this expression starts at rest")
    n_max = 20000.0
    b = Build(problem.id)
    _gear_parts(b, p, n_max)
    b.part("load", "propulsion.propeller", torque_ref_Nm=0, inertia_kgm2=p["J2"])
    b.wire("gb", "flange_out", "load", "shaft")
    run = run_project(b.project(problem.t_end, step))
    sig = {"omega1": _scaled(run.ch("mot", "sig_speed"), 1.0 / RPM),
           "omega2": _scaled(run.ch("load", "sig_speed"), 1.0 / RPM)}
    energy = {"E_drive": run.part("mot")["energyOut"]}
    return _trace(run, sig, energy, kinetic=([("omega1", p["J1"]), ("omega2", p["J2"])],
                                             run.part("Rotating parts")["stored"]))


def gear_change_vehicle(problem: Problem, step: float) -> Trace:
    """J2 as a vehicle: m = J2 / r² on one 0.3 m wheel of no inertia, a
    stiff tyre (slip stiffness 100, μ 2: the solver step falls to 1.43 ms)."""
    p = problem.parameters
    r = 0.3
    n_max = 20000.0
    b = Build(problem.id)
    _gear_parts(b, p, n_max)
    b.part("whl", "propulsion.wheel", radius_m=r, inertia_kgm2=0, rolling_resistance=0,
           slip_stiffness=100, mu=2.0, vehicle_load_share_pct=100)
    b.part("veh", "vehicle.body", mass_kg=p["J2"] / r ** 2, cd=0, frontal_area_m2=0,
           initial_speed_kmh=problem.initial["omega2"] * r * 3.6)
    b.wire("gb", "flange_out", "whl", "shaft")
    run = run_project(b.project(problem.t_end, step))
    sig = {"omega1": _scaled(run.ch("mot", "sig_speed"), 1.0 / RPM),
           "omega2": _scaled(run.ch("veh", "sig_speed"), 1.0 / 3.6 / r)}
    energy = {"E_drive": run.part("mot")["energyOut"]}
    booked = run.part("Rotating parts")["stored"] + run.part("veh")["stored"]
    return _trace(run, sig, energy,
                  kinetic=([("omega1", p["J1"]), ("omega2", p["J2"])], booked))


def vehicle_coastdown(problem: Problem, step: float) -> Trace:
    """A Vehicle body alone, its road load given as coefficients A, B = 0, C."""
    p, x0 = problem.parameters, problem.initial
    b = Build(problem.id)
    b.part("veh", "vehicle.body", mass_kg=p["m"], road_load_mode="Coefficients A/B/C",
           road_load_a_N=p["A"], road_load_b_N_per_kmh=0.0,
           road_load_c_N_per_kmh2=p["C"] / 3.6 ** 2, initial_speed_kmh=x0["v"] * 3.6)
    run = run_project(b.project(problem.t_end, step))
    sig = {"v": _scaled(run.ch("veh", "sig_speed"), 1.0 / 3.6), "x": run.ch("veh", "sig_distance")}
    veh = next(f for f in run.result.raw.partEnergy if f.elementId == "veh")
    terms = {k: v * KWH for k, v in veh.terms.items()}
    energy = {}
    for key, name in (("air drag", "E_aero"), ("rolling", "E_roll")):
        found = [v for k, v in terms.items() if key in k.lower()]
        if found:
            energy[name] = found[0]
    return _trace(run, sig, energy, kinetic=([("v", p["m"])], veh.stored * KWH))


def vehicle_constant_power(problem: Problem, step: float) -> Trace:
    """The E-Motor's full-load curve is P/ω (400 points from 90 % of the
    start speed to 60 m/s), through a 9:1 lossless final drive to one 0.3 m
    wheel of no inertia; a stiff tyre (slip stiffness 100, μ 2: the solver
    step falls to 1.43 ms) on a Vehicle of mass m with no drag."""
    p, x0 = problem.parameters, problem.initial
    m, P, v0 = p["m"], p["P"], x0["v"]
    r, ratio = 0.3, 9.0
    w_lo, w_hi = 0.9 * v0 / r * ratio, 60.0 / r * ratio
    curve = {"0": P / w_lo}
    for j in range(401):
        w = w_lo + (w_hi - w_lo) * j / 400
        curve[f"{w * RPM:.9g}"] = P / w
    n_max = 1.1 * w_hi * RPM
    b = Build(problem.id)
    _supply(b, 400.0)
    b.part("mot", "motor.emotor", inertia_kgm2=0, full_load_torque={"100": curve, "1000": curve},
           power_loss=_no_loss(n_max, P / w_lo), drag_torque={"0": 0, f"{n_max:.9g}": 0},
           max_speed_rpm=n_max)
    b.part("fd", "mech.final_drive", ratio=ratio, efficiency_pct=100, inertia_in_kgm2=0,
           inertia_out_kgm2=0)
    b.part("whl", "propulsion.wheel", radius_m=r, inertia_kgm2=0, rolling_resistance=0,
           slip_stiffness=100, mu=2.0, vehicle_load_share_pct=100)
    b.part("veh", "vehicle.body", mass_kg=m, cd=0, frontal_area_m2=0, initial_speed_kmh=v0 * 3.6)
    b.part("dem", "signal.constant", value=1.0)
    b.wire("mot", "shaft", "fd", "flange_in").wire("fd", "flange_out", "whl", "shaft")
    b.route("dem", "sig_out", "mot", "sig_demand_in")
    run = run_project(b.project(problem.t_end, step))
    sig = {"v": _scaled(run.ch("veh", "sig_speed"), 1.0 / 3.6), "x": run.ch("veh", "sig_distance")}
    energy = {"E_supplied": run.part("mot")["energyOut"]}
    return _trace(run, sig, energy, kinetic=([("v", m)], run.part("veh")["stored"]))


EXPRESSIONS: dict[str, list[Expression]] = {
    "batt_cc_rc": [Expression(
        "as stated", "Battery (pack values, linear OCV table, R0, one RC pair) feeding a Power "
        "Consumer whose demand a Lookup sets to I x the terminal voltage of the step before",
        batt_constant_current)],
    "batt_cp_rc": [Expression(
        "as stated", "Battery (flat OCV) feeding a Power Consumer at a constant 60 kW",
        batt_constant_power)],
    "batt_voltage_limit": [Expression(
        "as stated", "the constant-current set-up with the pack's Min Voltage at V_min: the "
        "source-limit handshake cuts the load back to hold it", batt_voltage_limit)],
    "motor_dc_spinup_l0": [Expression(
        "as stated", "Voltage Source -> E-Motor whose full-load curve is the DC motor's "
        "torque-speed line (k(V - k w)/R - b w) and whose loss map is R i^2 + b w^2, at full "
        "demand, on its own rotor inertia J", dc_motor_l0)],
    "mech_inertia_coastdown": [Expression(
        "as stated", "E-Motor (inertia J) spun up for 1 s, then off: its drag table c w is "
        "the viscous friction and a Brake held at T_c the Coulomb friction",
        inertia_coastdown)],
    "mech_clutch_lockup": [Expression(
        "as stated", "E-Motor (J1) spun up for 0.5 s with the Clutch open, then T_drive with "
        "the Clutch closed onto a Propeller of zero torque (J2)", clutch_lockup)],
    "mech_gear_change": [
        Expression("rotational load", "E-Motor (J1) -> Gearbox (12, 7; lossless) -> "
                   "Propeller of zero torque (J2); the gear signal steps from 1 to 2 at t_shift",
                   gear_change_rotational),
        Expression("vehicle load", "E-Motor (J1) -> Gearbox -> one wheel (0.3 m, no inertia, "
                   "stiff tyre) -> Vehicle of mass J2 / r^2", gear_change_vehicle)],
    "veh_coastdown": [Expression(
        "as stated", "a Vehicle body alone, road load A + C v^2 as coefficients A/B/C",
        vehicle_coastdown)],
    "veh_constant_power": [Expression(
        "as stated", "E-Motor with a P / w full-load curve -> 9:1 final drive -> one wheel "
        "(stiff tyre) -> Vehicle of mass m without drag", vehicle_constant_power)],
}


def run(problem: Problem, expression: Expression, step: float | None = None) -> Trace:
    """Run one expression of a problem at a solver step (default: the
    engine's own, 10 ms). The case records every ``step``, so the solver
    takes exactly that step unless its stability check asks for less."""
    return expression.run(problem, min(step or MAX_STEP, problem.output_dt))
