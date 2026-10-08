"""Speed harness: how fast an engine runs the example cars' main cases.

For each case of benchmarks/targets.toml ([[speed.cases]]): one warm-up run
(untimed; it also counts the solver steps), then ``repeats`` timed runs in
the same process; the median wall-clock time gives "x real time"
(simulated seconds per wall-clock second) and steps per second. The load
of the machine is recorded around every case (load average, CPU count,
and each run's CPU time against its wall time: a share well under 1 means
the run was waiting for a CPU, and its time is not to be trusted).

Today's engine is driven through ``app.solver.simulate`` on the example
project as shipped (the call the app makes); the hybrid's Script blocks
run with script trust off (LIGHTSIM_SCRIPT_TRUST=off), as in the tests.
"""
from __future__ import annotations

import os
import platform
import statistics
import time
from dataclasses import asdict, dataclass, field

from .engines import lightsim_py
from .reference import targets


@dataclass
class CaseSpeed:
    label: str
    project: str
    case: str
    kind: str  # cycle, acceleration, lap
    simulated_s: float  # the simulated time of one run
    steps: int  # solver steps (master steps) of one run
    walls_s: list[float]
    cpus_s: list[float]
    load_before: tuple[float, float, float]
    load_after: tuple[float, float, float]
    status: str
    note: str = ""
    fast_target: bool = False

    @property
    def wall_s(self) -> float:
        return statistics.median(self.walls_s)

    @property
    def x_realtime(self) -> float:
        return self.simulated_s / self.wall_s

    @property
    def steps_per_s(self) -> float:
        return self.steps / self.wall_s

    @property
    def cpu_share(self) -> float:
        return min(c / w for c, w in zip(self.cpus_s, self.walls_s))

    def to_dict(self) -> dict:
        d = asdict(self)
        d.update(wall_s=self.wall_s, x_realtime=self.x_realtime, steps_per_s=self.steps_per_s,
                 cpu_share=self.cpu_share)
        return d


@dataclass
class SpeedReport:
    machine: dict
    cases: list[CaseSpeed] = field(default_factory=list)
    targets: dict = field(default_factory=dict)


def machine() -> dict:
    """What the timings ran on."""
    model = ""
    try:
        with open("/proc/cpuinfo", encoding="utf-8") as f:
            model = next((ln.split(":", 1)[1].strip() for ln in f if ln.startswith("model name")), "")
    except OSError:
        pass
    return {"platform": platform.platform(), "python": platform.python_version(),
            "cpu": model or platform.processor(), "cpus": os.cpu_count(),
            "load_at_start": _load()}


def _load() -> tuple[float, float, float]:
    try:
        return tuple(round(x, 2) for x in os.getloadavg())  # type: ignore[return-value]
    except OSError:
        return (float("nan"),) * 3  # type: ignore[return-value]


def _cpu() -> float:
    """CPU time of this process and of its finished children (a Script
    block's sandbox worker), s."""
    t = os.times()
    return t.user + t.system + t.children_user + t.children_system


def _count_steps(fn):
    """Run fn() counting the master's solver steps."""
    from app.solver import master

    original = master.Master.step
    count = 0

    def counting(self, t, h):
        nonlocal count
        count += 1
        return original(self, t, h)

    master.Master.step = counting
    try:
        result = fn()
    finally:
        master.Master.step = original
    return result, count


def _simulated_s(result) -> float:
    """The simulated time of a run: its last recorded time."""
    return max((ch.timeSeries[-1]["t"] for ch in result.channels if ch.timeSeries), default=0.0)


def time_case(label: str, project_id: str, case_id: str, repeats: int,
              fast_target: bool = False) -> CaseSpeed:
    from app.solver import simulate
    from app.storage import load_example

    proj = load_example(project_id)
    case = next(c for c in proj.cases if c.id == case_id)
    load_before = _load()
    result, steps = _count_steps(lambda: simulate(proj, case_id))  # warm-up
    walls, cpus = [], []
    for _ in range(repeats):
        t0, c0 = time.perf_counter(), _cpu()
        result = simulate(proj, case_id)
        walls.append(time.perf_counter() - t0)
        cpus.append(_cpu() - c0)
    note = ""
    if case.kind == "lap":
        note = "lap mode (quasi-steady, one step per stretch of track)"
    return CaseSpeed(label=label, project=project_id, case=case_id, kind=case.kind or "cycle",
                     simulated_s=_simulated_s(result), steps=steps, walls_s=walls, cpus_s=cpus,
                     load_before=load_before, load_after=_load(), status=result.status,
                     note=note, fast_target=fast_target)


def run(repeats: int | None = None, labels: list[str] | None = None,
        progress=print) -> SpeedReport:
    spec = targets()["speed"]
    repeats = repeats or int(spec.get("repeats", 5))
    report = SpeedReport(machine=machine(), targets={
        "dynamic_min_x_realtime": spec["dynamic_min_x_realtime"],
        "fast_wltc_x_realtime": spec["fast_wltc_x_realtime"], "repeats": repeats,
        "engine": lightsim_py.version()})
    for c in spec["cases"]:
        if labels and c["label"] not in labels:
            continue
        cs = time_case(c["label"], c["project"], c["case"], repeats, c.get("fast_target", False))
        report.cases.append(cs)
        progress(f"  {cs.label:24s} {cs.x_realtime:9.1f} x real time  {cs.steps_per_s:9.0f} "
                 f"steps/s  (median {cs.wall_s:.2f} s, cpu share {cs.cpu_share:.2f}, "
                 f"load {cs.load_before[0]:.2f} -> {cs.load_after[0]:.2f})")
    return report
