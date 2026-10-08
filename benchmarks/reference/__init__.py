"""Engine-neutral reference problems with exact answers (LightSim Stage 0).

Each problem is one TOML file in ``problems/``: the statement, equations,
parameters and initial state (SI units), the derivation of its exact
answer, the quantities to compare with their tolerances, and the terms of
its energy balance. ``exact.py`` computes the answers; ``verify.py``
checks them against a very fine numerical integration of the stated
equations. Any engine (today's Python one, the Rust one) is compared with
:func:`compare.compare` through an adapter in ``benchmarks/engines/``.
"""
from __future__ import annotations

import math
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

DIR = Path(__file__).parent
PROBLEMS_DIR = DIR / "problems"
TARGETS_FILE = DIR.parent / "targets.toml"
KINDS = ("signal", "event", "energy")


def targets() -> dict:
    """benchmarks/targets.toml: the default tolerances and the speed targets."""
    return tomllib.loads(TARGETS_FILE.read_text(encoding="utf-8"))


@dataclass(frozen=True)
class Compare:
    """One compared quantity. ``signal``: the largest error over the output
    times (t > 0, outside ``breaks``) against atol + rtol × scale, the
    scale being the largest |exact value| there unless given. ``event``:
    |t − t_exact| against atol + rtol × |t_exact|. ``energy``: the value at
    t_end against atol + rtol × the problem's energy scale."""

    name: str
    kind: str
    unit: str
    rtol: float
    atol: float
    scale: float | None = None
    note: str = ""
    # an event: the signal and level whose first crossing it is ("rising",
    # "falling" or "either"), for an engine that does not report it itself
    signal: str | None = None
    level: float | None = None
    direction: str = "either"


@dataclass
class Problem:
    id: str
    model: str
    title: str
    domain: str
    version: int
    summary: str
    statement: str
    equations: list[str]
    assumptions: list[str]
    solution: str
    parameters: dict[str, float]
    initial: dict[str, float]
    units: dict[str, str]
    notes: dict[str, str]
    t_end: float
    output_dt: float
    compare: list[Compare]
    sources: list[str]
    sinks: list[str]
    closure_rtol: float
    checkpoints: dict = field(default_factory=dict)
    path: Path | None = None

    def output_times(self) -> list[float]:
        """0, dt, 2 dt, …, t_end (t_end a whole number of steps)."""
        n = round(self.t_end / self.output_dt)
        if abs(n * self.output_dt - self.t_end) > 1e-9 * self.t_end:
            raise ValueError(f"{self.id}: t_end is not a whole number of output steps")
        return [round(k * self.output_dt, 12) for k in range(n + 1)]

    def compared(self, kind: str) -> list[Compare]:
        return [c for c in self.compare if c.kind == kind]

    @property
    def energy_terms(self) -> list[str]:
        return self.sources + self.sinks


def _values(table: dict, where: str) -> tuple[dict, dict, dict]:
    values, units, notes = {}, {}, {}
    for key, entry in table.items():
        if not isinstance(entry, dict) or "value" not in entry or "unit" not in entry:
            raise ValueError(f"{where}.{key}: needs {{ value = …, unit = … }}")
        values[key] = float(entry["value"])
        units[key] = str(entry["unit"])
        notes[key] = str(entry.get("note", ""))
    return values, units, notes


def load(path: Path | str) -> Problem:
    """A problem from its TOML file (or its id)."""
    path = Path(path)
    if not path.suffix:
        path = PROBLEMS_DIR / f"{path}.toml"
    raw = tomllib.loads(path.read_text(encoding="utf-8"))
    acc = targets()["accuracy"]
    params, p_units, p_notes = _values(raw["parameters"], "parameters")
    initial, i_units, i_notes = _values(raw["initial"], "initial")
    compare = []
    for c in raw["compare"]:
        kind = c["kind"]
        if kind not in KINDS:
            raise ValueError(f"{path.name}: compare '{c['name']}' has kind '{kind}'")
        default_rtol = {"signal": acc["signal_rtol"], "event": acc["event_rtol"],
                        "energy": acc["energy_rtol"]}[kind]
        default_atol = acc["event_atol_s"] if kind == "event" else 0.0
        compare.append(Compare(name=c["name"], kind=kind, unit=c["unit"],
                               rtol=float(c.get("rtol", default_rtol)),
                               atol=float(c.get("atol", default_atol)),
                               scale=float(c["scale"]) if "scale" in c else None,
                               note=c.get("note", ""), signal=c.get("signal"),
                               level=float(c["level"]) if "level" in c else None,
                               direction=c.get("direction", "either")))
        if kind == "event" and (compare[-1].signal is None or compare[-1].level is None):
            raise ValueError(f"{path.name}: event '{c['name']}' needs its signal and level")
    energy = raw["energy"]
    prob = Problem(
        id=raw["id"], model=raw["model"], title=raw["title"], domain=raw["domain"],
        version=int(raw["version"]), summary=raw["summary"].strip(),
        statement=raw["statement"].strip(), equations=list(raw["equations"]),
        assumptions=list(raw.get("assumptions", [])), solution=raw["solution"].strip(),
        parameters=params, initial=initial, units={**p_units, **i_units},
        notes={**p_notes, **i_notes}, t_end=float(raw["run"]["t_end"]),
        output_dt=float(raw["run"]["output_dt"]), compare=compare,
        sources=list(energy["sources"]), sinks=list(energy["sinks"]),
        closure_rtol=float(energy.get("closure_rtol", acc["closure_rtol"])),
        checkpoints=raw.get("checkpoints", {}), path=path)
    if prob.path.stem != prob.id:
        raise ValueError(f"{path.name}: the file name must be the id '{prob.id}'")
    if not math.isfinite(prob.t_end) or prob.t_end <= 0:
        raise ValueError(f"{path.name}: t_end must be positive")
    prob.output_times()  # t_end a whole number of steps
    return prob


def load_all() -> list[Problem]:
    """Every problem, in file-name order."""
    return [load(p) for p in sorted(PROBLEMS_DIR.glob("*.toml"))]
