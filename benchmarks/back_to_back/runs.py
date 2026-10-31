"""The runs of the both-engines diff: what each item, fault and probe is,
and running it in a process of its own.

A job is one run of one project's case on one engine at one setting. The
parent (``run_all``) starts each job as ``python -m benchmarks.back_to_back
job SPEC OUT`` with the engine's environment, a few at a time, and skips a
job whose output exists and was made by this same code (so a stopped run
resumes; a record of other code is run again). The child builds the
project, runs it through the app's own ``app.solver.simulate`` and writes
the SimResult with what the harness needs beside it: the code that made
it (``stamp``), the wall time, the Data Checks' worst level, the new
engine's own build warnings (with their codes), energy totals and Script
runners, and the streamed points' check.
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
import time
import tomllib
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
BACKEND = REPO / "backend"
sys.path.insert(0, str(BACKEND))  # the app itself: today's engine and the new engine's path
ACCEPTANCE = tomllib.loads((HERE / "acceptance.toml").read_text(encoding="utf-8"))
LINES = {line["id"]: line for line in ACCEPTANCE["line"]}
FAULT_EXAMPLES = {"bev-car": "case-city", "fs-electric": "case-accel-75m", "hybrid-car": "case-udds"}
LIVE_PACE = 1000.0  # a live item's pace in the diff (pace does not change a result)
SUBSTEP = {"normal": 0.01, "fine": 0.001}  # today's engine's solver step, s
# the META settings beside the base run: (output step factor, rtol)
META = {"meta-x0.1": (0.1, 1e-6), "meta-x5": (5.0, 1e-6), "meta-rtol1e-8": (1.0, 1e-8)}
PARTS = ("items", "faults", "meta", "energy")  # every part of the runs: what `run` and `all` run
# fast mode's runner for the ENERGY line's E3: the built example
# engine/crates/lsim-project/examples/fast.rs (`cargo build --release -p
# lsim-project --example fast`); without it E3 is "not measured"
FAST_BIN = "LSIM_FAST_BIN"


def scratch() -> Path | None:
    """The lane's scratch folder, $LSIM_SCRATCH: the owner's decisions
    (decisions/) and the judges' cache (judge-cache/); None where it is not
    set (CI, another machine)."""
    value = os.environ.get("LSIM_SCRATCH", "").strip()
    return Path(value) if value else None


def cache() -> Path | None:
    """The judges' cache of the sources' copies: $LSIM_JUDGE_CACHE, else
    $LSIM_SCRATCH/judge-cache; None where neither is set."""
    value = os.environ.get("LSIM_JUDGE_CACHE", "").strip()
    return Path(value) if value else scratch() / "judge-cache" if scratch() else None


def example(name: str) -> dict:
    return json.loads((BACKEND / "projects" / f"{name}.json").read_text(encoding="utf-8"))


def _case(project: dict, case_id: str) -> dict:
    return next(c for c in project["cases"] if c["id"] == case_id)


def _element(project: dict, el_id: str) -> dict:
    return next(e for s in project["systems"] for e in s["elements"] if e["id"] == el_id)


# ---- items ------------------------------------------------------------------

def items() -> list[dict]:
    return ACCEPTANCE["item"]


def item_project(item: dict, scratch: Path) -> tuple[dict, str, dict]:
    """(project JSON, case id, how to run it) of an item of the diff."""
    iid = item["id"]
    how: dict = {}
    base = item.get("base", iid)
    name, case_id = base.split("/")
    project = example(name)
    case = _case(project, case_id)
    if (case.get("realtimeFactor") or 0) > 0:
        case["realtimeFactor"] = LIVE_PACE
        how["live"] = True
    if iid.endswith("-single"):
        case["chargeBalance"] = False
    elif iid == "probe/bev-performance":
        case.update(kind="performance", duration=40.0)
        case.setdefault("parameterOverrides", {})["el-task"] = {"cycle": "", "profile": "0:100; 40:100"}
    elif iid == "probe/fs-event-acceleration":
        case["fsEvent"] = "acceleration"
    elif iid == "probe/bev-cells":
        _element(project, "el-battery").setdefault("parameterOverrides", {})["pack_model"] = "Cells"
    elif iid == "probe/bev-fmu":
        _add_test_fmu(project, scratch)
    elif iid == "probe/no-module":
        how["hide_module"] = True
    elif iid in CHECKED:  # (the META line's battery checks)
        _element(project, "el-battery").setdefault("parameterOverrides", {}).update(CHECKED[iid])
    return project, case_id, how


# the items with a battery's Output Power Limit only checked, which its
# verdict judges on the averaged peak (META, since the evidence part's review)
CHECKED = {
    "bev-car/case-city-checked": {"output_power_limit_kW": 23.0, "power_limit_window_s": 0.5,
                                  "power_limit_enforced": False},
    "bev-car/case-city-checked-w0": {"output_power_limit_kW": 23.6, "power_limit_window_s": 0.0,
                                     "power_limit_enforced": False},
}


def _add_test_fmu(project: dict, scratch: Path) -> None:
    """The backend's test FMU (a first-order lag, tests/fmu), fed by a
    Constant, its output unwired; built and kept in the job's own folder."""
    sys.path.insert(0, str(BACKEND / "tests"))
    from app.fmu import store
    from fmu.build import build as build_test_fmu  # the backend's own test FMU

    (scratch / "fmu-build").mkdir(parents=True, exist_ok=True)
    path = build_test_fmu(scratch / "fmu-build")
    sha, kept = store.store_bytes(path.read_bytes())
    store.allow(sha, path.name)
    system = project["systems"][0]
    sig = {"kind": "signal", "unitGroup": "No Unit"}
    system["elements"] += [
        {"id": "el-fmu-src", "componentDefId": "signal.constant", "label": "FMU input",
         "position": {"x": 0, "y": 0}, "parameterOverrides": {"value": 1.0}},
        {"id": "el-fmu", "componentDefId": "signal.fmu", "label": "Lag", "position": {"x": 0, "y": 0},
         "parameterOverrides": {"fmu_path": str(kept), "fmu_sha256": sha, "fmu_name": path.name},
         "dynamicPorts": [{"id": "u", "name": "u", "direction": "input", **sig},
                          {"id": "y", "name": "y", "direction": "output", **sig}]},
    ]
    project["dataBusConnections"].append(
        {"id": "db-fmu", "element1Id": "el-fmu-src", "port1Id": "sig_out",
         "element2Id": "el-fmu", "port2Id": "u"})


# ---- the ENERGY line's sine probe ---------------------------------------------

SINE = {"P0": 5.0, "A": 3.0, "T": 1.0, "N": 40, "periods": 4}  # kW, kW, s, samples a period


def sine_project() -> tuple[dict, str]:
    """An ideal battery (no resistance) feeding a Power Consumer whose demand
    a Script block sets to P0 + A sin(2 pi t / T), held between its N ticks a
    period: over whole periods its RMS is sqrt(P0^2 + A^2/2) exactly, and the
    battery, its only supply, carries the same power."""
    q = SINE
    code = ("import math\n\n"
            "def step(t, dt, inputs, state, params):\n"
            f"    return {{'p': {q['P0']!r} + {q['A']!r} * math.sin(2 * math.pi * t / {q['T']!r})}}\n")
    sig = {"kind": "signal", "unitGroup": "No Unit"}
    pos = {"x": 0, "y": 0}
    elements = [
        {"id": "el-battery", "componentDefId": "battery.generic", "label": "Battery", "position": pos,
         "parameterOverrides": {"capacity_kWh": 10.0, "internal_resistance_ohm": 0.0,
                                "rc_resistance_ohm": 0.0, "initial_soc_pct": 80.0}},
        {"id": "el-load", "componentDefId": "electric.constant_drive", "label": "Load", "position": pos,
         "parameterOverrides": {"power_kW": 0.0}},
        {"id": "el-sine", "componentDefId": "signal.script", "label": "Sine", "position": pos,
         "parameterOverrides": {"code": code, "sample_time_s": q["T"] / q["N"]},
         "dynamicPorts": [{"id": "p", "name": "p", "direction": "output", **sig}]},
        {"id": "el-ground", "componentDefId": "boundary.ground", "label": "Ground", "position": pos},
    ]
    wire = [("el-battery", "pos", "el-load", "pos"), ("el-battery", "neg", "el-load", "neg"),
            ("el-battery", "neg", "el-ground", "t1")]
    project = {
        "id": "sine", "name": "Sine probe",
        "systems": [{"id": "root", "name": "Sine probe", "parentId": None, "elements": elements,
                     "connections": [{"id": f"c-{k}", "sourceElementId": a, "sourcePortId": ap,
                                      "targetElementId": b, "targetPortId": bp}
                                     for k, (a, ap, b, bp) in enumerate(wire, 1)]}],
        "dataBusConnections": [{"id": "db-1", "element1Id": "el-sine", "port1Id": "p",
                                "element2Id": "el-load", "port2Id": "sig_demand_in"}],
        "cases": [{"id": "case-sine", "name": "Sine", "duration": q["T"] * q["periods"],
                   "timeStep": q["T"] / q["N"]}],
    }
    return project, "case-sine"


def fast_items() -> list[str]:
    """The ENERGY line's E3 cases, "the BEVs' cycle cases against fast mode":
    the battery-electric cars' cycle items the new path runs (not the live
    one, which it streams, nor one changed from its example's case)."""
    out = []
    for item in items():
        name, case_id = item.get("base", item["id"]).split("/")
        if name not in ("bev-car", "aero-bev") or item["id"].startswith("probe/") or "base" in item:
            continue
        case = _case(example(name), case_id)
        if case.get("kind", "cycle") == "cycle" and not (case.get("realtimeFactor") or 0) > 0:
            out.append(item["id"])
    return out


def fast_run(iid: str, scratch: Path) -> dict:
    """Fast mode on an item's case, by its runner (FAST_BIN): its result
    (the run's figures and its flags' stretches), or why there is none."""
    binary = os.environ.get(FAST_BIN, "").strip()
    if not binary:
        return {"error": f"not measured: {FAST_BIN} is not set"}
    item = next(i for i in items() if i["id"] == iid)
    project, case_id, _ = item_project(item, scratch)
    scratch.mkdir(parents=True, exist_ok=True)
    path = scratch / "project.json"
    path.write_text(json.dumps(project), encoding="utf-8")
    got = subprocess.run([binary, f"{path}/{case_id}"], capture_output=True, text=True, timeout=1800)
    lines = [ln for ln in got.stdout.splitlines() if ln.startswith("{")]
    if got.returncode or not lines:
        return {"error": f"the runner failed ({got.returncode}): {got.stderr[-500:]}"}
    return json.loads(lines[-1]).get("fast") or {"error": "no fast run in the runner's output"}


# ---- faults -----------------------------------------------------------------

def _deletions(name: str):
    sys.path.insert(0, str(BACKEND / "tests"))
    from test_broken_models import faults  # the 160-fault corpus itself
    yield from faults(name)


def fault_ids() -> list[str]:
    """Every planted fault, in corpus order: '<example>:<deleted id>',
    '<example>:<element>.<parameter>:<variant>' (the block's value) and
    '<example>:case/<element>.<parameter>:<variant>' (the case's own)."""
    out = [f"{name}:{fid}" for name in FAULT_EXAMPLES for fid, _ in _deletions(name)]
    for p in LINES["FAULTS"]["parameters"]:
        out += [f"{p}:{v}" for v in LINES["FAULTS"]["variants"]]
    for p in LINES["FAULTS"]["case_values"]:
        name, target = p.split(":")
        out += [f"{name}:case/{target}:{v}" for v in LINES["FAULTS"]["variants"]]
    return out


def _library_default(cdef_id: str, key: str):
    lib = json.loads((BACKEND / "app" / "library" / "components.json").read_text(encoding="utf-8"))
    cdef = next(c for c in lib["components"] if c["id"] == cdef_id)
    return next(p.get("default") for p in cdef["parameters"] if p["key"] == key)


def fault_project(fault: str) -> tuple[dict, str]:
    """(project JSON, case id) of a planted fault, on its example's first case."""
    name, rest = fault.split(":", 1)
    case_id = FAULT_EXAMPLES[name]
    if rest.count(":") == 1:  # a parameter fault: the block's value, or the case's own (case/)
        target, variant = rest.split(":")
        on_case = target.startswith("case/")
        el_id, key = target.removeprefix("case/").split(".", 1)
        project = example(name)
        el = _element(project, el_id)
        over = el.setdefault("parameterOverrides", {})
        value = float(over.get(key, _library_default(el["componentDefId"], key)))
        if on_case:  # (the case's own value first, where it has one)
            case = _case(project, case_id)
            values = case["parameterOverrides"] = case.get("parameterOverrides") or {}
            over = values.setdefault(el_id, {})
            value = float(over.get(key, value))
        over[key] = {"zero": 0.0, "negative": -value, "x100": value * 100.0,
                     "div100": value / 100.0}[variant]
    else:
        project = next(d for fid, d in _deletions(name) if fid == rest)
    if name == "hybrid-car":  # a single run, so that both engines judge it
        _case(project, case_id)["chargeBalance"] = False
    return project, case_id


# ---- jobs -------------------------------------------------------------------

def job_path(out: Path, spec: dict) -> Path:
    return out / "raw" / spec["setting"] / (spec["id"].replace("/", "__").replace(":", "~") + ".json")


def specs(what: set[str]) -> list[dict]:
    """The jobs of the parts asked for (PARTS): 'items', 'faults', 'meta', 'energy'."""
    jobs = []
    if "items" in what:
        for item in items():
            for setting in ("today-normal", "today-fine", "new"):
                jobs.append({"kind": "item", "id": item["id"], "setting": setting})
    if "faults" in what:
        for fault in fault_ids():
            for setting in ("today-normal", "new"):
                jobs.append({"kind": "fault", "id": fault, "setting": setting})
    if "energy" in what:
        jobs += [{"kind": "probe", "id": "energy/sine", "setting": s} for s in ("today-normal", "new")]
        jobs += [{"kind": "probe", "id": f"fast/{iid}", "setting": "fast"} for iid in fast_items()]
    if "meta" in what:
        cases, faults = meta_set()
        for setting in META:
            jobs += [{"kind": "item", "id": c, "setting": setting} for c in cases]
            jobs += [{"kind": "fault", "id": f, "setting": setting} for f in faults]
    return jobs


def meta_set() -> tuple[list[str], list[str]]:
    """The META line's cases and faults, as its `cases` text names them."""
    text = LINES["META"]["cases"]
    cases = [i["id"] for i in items() if i["id"] in text and i["id"].count("/") == 1
             and not i["id"].startswith("probe/")]
    faults = [f for f in fault_ids() if re.search(re.escape(f) + r"(?![\w.:])", text)]
    return cases, faults


def env_of(spec: dict, out: Path) -> dict:
    env = {k: v for k, v in os.environ.items()
           if k not in ("LIGHTSIM_ENGINE", "LIGHTSIM_ENGINE_STRICT")}
    user = out / "user"
    env.update(LIGHTSIM_DATA_DIR=str(user), LIGHTSIM_PROJECTS_DIR=str(user / "projects"),
               PYTHONHASHSEED="0")
    if spec["setting"].startswith(("new", "meta")):
        env.update(LIGHTSIM_ENGINE="new", LIGHTSIM_ENGINE_STRICT="1")
    if spec["kind"] == "probe":  # the probes' own Script code, which no user approved
        env["LIGHTSIM_SCRIPT_TRUST"] = "off"
    return env


def fallback_note(project, case_id: str, emit) -> str | None:
    """The message the path writes when it hands a case it refuses before
    running it to today's engine (not strict): STRICT checks it carries the
    code."""
    from app.schemas import SimResult
    from app.solver import new_engine
    if new_engine.unsupported(project, case_id) is None:
        return None
    saved = os.environ.pop(new_engine.STRICT_VAR, None)
    try:
        res = new_engine.simulate(project, case_id, emit, None, lambda *a: SimResult(
            caseId=case_id, status="success", channels=[], messages=[]))
    finally:
        if saved is not None:
            os.environ[new_engine.STRICT_VAR] = saved
    return res.messages[0].text


def _git(*args: str) -> bytes:
    try:
        return subprocess.run(["git", *args], cwd=REPO, capture_output=True).stdout
    except OSError:
        return b""


def _sha(path: Path) -> str | None:
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None


def stamp() -> dict:
    """Which code made a record: the commit; the change to it not committed
    (the SHA-256 of `git diff HEAD` and of the untracked files' names, None
    when there is none); the module lightsim_engine's version and the
    SHA-256 of its compiled file; and the SHA-256 of the path, the verdict
    and the harness's own files."""
    import importlib.util
    from importlib.metadata import PackageNotFoundError, version
    change = _git("diff", "HEAD", "--binary") + _git("ls-files", "--others", "--exclude-standard")
    try:
        engine = version("lightsim_engine")
        found = importlib.util.find_spec("lightsim_engine")
        binary = sorted(p for p in Path(found.origin).parent.iterdir() if p.suffix in (".so", ".pyd"))
        engine_sha = _sha(binary[0]) if binary else None
    except (PackageNotFoundError, ImportError, AttributeError, TypeError, ValueError):
        engine, engine_sha = None, None
    files = [BACKEND / "app" / "solver" / "new_engine.py", BACKEND / "app" / "solver" / "verdict.py",
             BACKEND / "app" / "solver" / "evidence.py",
             *sorted(HERE.glob("*.py")), *sorted(HERE.glob("*.toml"))]
    fast = os.environ.get(FAST_BIN, "").strip()
    return {"commit": _git("rev-parse", "HEAD").decode().strip(),
            "dirty": hashlib.sha256(change).hexdigest() if change else None,
            "lightsim_engine": engine, "lightsim_engine_sha256": engine_sha,
            "fast_runner_sha256": _sha(Path(fast)) if fast else None,
            "sha256": {str(p.relative_to(REPO)).replace("\\", "/"): _sha(p) for p in files}}


def todo(out: Path, jobs: list[dict], now: dict) -> list[dict]:
    """The jobs to run: those with no record, and those whose record another
    code made (its stamp is not ``now``): a rerun never reuses a stale one."""
    def stale(j):
        rec = load(out, j)
        return rec is None or rec.get("stamp") != now
    return [j for j in jobs if stale(j)]


def run_all(out: Path, jobs: list[dict], workers: int, timeout: float = 3600.0) -> None:
    now = stamp()
    todo_ = todo(out, jobs, now)
    stale = sum(job_path(out, j).exists() for j in todo_)
    print(f"{len(jobs)} jobs, {len(jobs) - len(todo_)} done before at this code, {len(todo_)} to run "
          f"({stale} of them made by other code)", flush=True)
    logs = out / "logs"
    logs.mkdir(parents=True, exist_ok=True)

    def one(spec: dict) -> None:
        path = job_path(out, spec)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.unlink(missing_ok=True)  # another code's record: replaced, never mixed in
        log = logs / (path.parent.name + "__" + path.stem + ".log")
        t0 = time.monotonic()
        with open(log, "w", encoding="utf-8") as f:
            try:
                code = subprocess.run(
                    [sys.executable, "-m", "benchmarks.back_to_back", "job", json.dumps(spec),
                     str(path)], cwd=REPO, env=env_of(spec, out), stdout=f, stderr=subprocess.STDOUT,
                    timeout=timeout).returncode
            except subprocess.TimeoutExpired:
                code = "timeout"
        if not path.exists():  # the child died or timed out: say so, as a result
            path.write_text(json.dumps({"spec": spec, "crashed": str(code), "stamp": now,
                                        "wall_s": time.monotonic() - t0}), encoding="utf-8")
        print(f"{spec['setting']:>14} {spec['id']} ({time.monotonic() - t0:.0f} s)", flush=True)

    # the longest first (today's fine runs, the hybrid), so the pool ends together
    todo_.sort(key=lambda j: (j["setting"] != "today-fine", "hybrid" not in j["id"]))
    with ThreadPoolExecutor(max_workers=workers) as pool:
        list(pool.map(one, todo_))


def load(out: Path, spec: dict) -> dict | None:
    path = job_path(out, spec)
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else None


# ---- the child ----------------------------------------------------------------

class _Engine:
    """lightsim_engine.build wrapped, to keep what the app's path drops:
    the build's warnings with their codes, and the run's energy totals; to
    run at another rtol (the META line); and, for SCRIPTS a, which runner
    served the Script parts' steps: each step the engine asks of a part
    (script_calls), each step today's worker (sandbox.ScriptSandbox) ran
    for it (script_served), and the worker's process beside the run's own
    (script_worker)."""

    def __init__(self, rtol: float | None):
        import lightsim_engine
        from app.solver import sandbox
        self.real, self.rtol = lightsim_engine.build, rtol
        self.kept = {"script_served": {}, "script_worker": {"own_pid": os.getpid(), "pids": []}}
        lightsim_engine.build = self.build
        real_run, kept = sandbox.ScriptSandbox.run, self.kept

        def run(box, el_id, *args, **kwargs):  # a step the worker serves
            kept["script_served"][el_id] = kept["script_served"].get(el_id, 0) + 1
            if box._proc.pid not in kept["script_worker"]["pids"]:
                kept["script_worker"]["pids"].append(box._proc.pid)
            return real_run(box, el_id, *args, **kwargs)
        sandbox.ScriptSandbox.run = run

    def build(self, *args, **kwargs):
        model = self.real(*args, **kwargs)
        self.kept["build_warnings"] = [dict(d) for d in model.report["warnings"]]
        return _Model(model, self)


class _Model:
    def __init__(self, model, engine: _Engine):
        self.model, self.engine = model, engine

    @property
    def report(self):
        return self.model.report

    def __getattr__(self, name):  # the rest of the model as it is (var_names, params, ...)
        return getattr(self.model, name)

    def simulate(self, **kwargs):
        if self.engine.rtol is not None:
            kwargs["rtol"] = self.engine.rtol
        calls = self.engine.kept["script_calls"] = {}

        def counted(el_id, step):
            calls[el_id] = 0

            def ask(*args, **kw):
                calls[el_id] += 1
                return step(*args, **kw)
            return ask
        if kwargs.get("scripts"):
            kwargs["scripts"] = {el_id: counted(el_id, step) for el_id, step in kwargs["scripts"].items()}
        run = self.model.simulate(**kwargs)
        # which runner the module used for each Script part ("python": the caller's function)
        self.engine.kept["scripts"] = dict(run.report["scripts"])
        energy = run.energy
        self.engine.kept["energy"] = None if energy is None else {
            k: v for k, v in energy.items() if isinstance(v, (int, float, str, bool))} | {
            # what the rigid engagements took at the parts the books name (gear shifts)
            "parts_impulse_lost_J": sum(p["impulse_lost_J"] for p in energy["parts"])}
        return run


def child(spec: dict, path: Path) -> None:
    from app.schemas import Project
    from app.solver import core, simulate
    from app.validation import validate_project

    made_by = stamp()
    scratch = path.parent / (path.stem + ".work")
    if spec["kind"] == "probe" and spec["id"].startswith("fast/"):  # E3: fast mode, not the app
        t0 = time.perf_counter()
        fast = fast_run(spec["id"][len("fast/"):], scratch)
        record = {"spec": spec, "stamp": made_by, "wall_s": time.perf_counter() - t0, "fast": fast}
        tmp = path.with_suffix(".part")
        tmp.write_text(json.dumps(record), encoding="utf-8")
        tmp.replace(path)
        return
    if spec["kind"] == "item":
        item = next(i for i in items() if i["id"] == spec["id"])
        raw, case_id, how = item_project(item, scratch)
    elif spec["kind"] == "probe":
        raw, case_id = sine_project()
        how = {}
    else:
        raw, case_id = fault_project(spec["id"])
        how = {}
    setting = spec["setting"]
    rtol = None
    if setting in META:
        scale, rtol = META[setting]
        case = _case(raw, case_id)
        case["timeStep"] = case.get("timeStep", 1.0) * scale
    if setting.startswith("today"):
        core.MAX_SUBSTEP = SUBSTEP[setting.split("-")[1]]
        if setting == "today-fine":  # Script blocks keep today's 10 ms tick (golden/reference.py)
            for s in raw["systems"]:
                for el in s["elements"]:
                    if el["componentDefId"] == "signal.script":
                        po = el.setdefault("parameterOverrides", {})
                        if not po.get("sample_time_s"):
                            po["sample_time_s"] = 0.01
    engine = None
    if how.get("hide_module"):
        sys.modules["lightsim_engine"] = None  # import lightsim_engine raises ImportError
    elif setting.startswith(("new", "meta")):
        engine = _Engine(rtol)
    project = Project.model_validate(raw)
    stream: list[dict] = []
    emit = stream.append if how.get("live") else None
    t0 = time.perf_counter()
    result = simulate(project, case_id, emit)
    wall = time.perf_counter() - t0
    levels = {c.level for c in validate_project(project)}
    record = {
        "spec": spec, "stamp": made_by, "case": case_id, "wall_s": wall,
        "checks": next((lv for lv in ("error", "warning") if lv in levels), "info"),
        "result": result.model_dump(mode="json"),
        "engine": engine.kept if engine else None,
        "stream": streamed(stream, result) if how.get("live") else None,
        "fallback_note": fallback_note(project, case_id, emit) if engine or how.get("hide_module")
        else None,
    }
    tmp = path.with_suffix(".part")
    tmp.write_text(json.dumps(record), encoding="utf-8")
    tmp.replace(path)


def streamed(stream: list[dict], result) -> dict:
    """The S1 check: every streamed point ("step" message) against the final
    result's channels at that point, bit for bit. A message is unequal when
    its time or a value differs, or when it lacks a channel the result has
    a value for there (today's engine streams every channel it records);
    S1 also needs one message for every recorded point."""
    steps = [m for m in stream if m.get("type") == "step"]
    final = {f"{c.elementId}:{c.portId}": [p["value"] for p in c.timeSeries] for c in result.channels}
    times = [p["t"] for p in result.channels[0].timeSeries] if result.channels else []
    bad, lacking, first = 0, 0, None
    for k, m in enumerate(steps):
        values = m.get("values") or {}
        lacks = [name for name, vs in final.items()
                 if k < len(vs) and vs[k] is not None and name not in values]
        same = k < len(times) and m["t"] == times[k] and not lacks and all(
            name in final and k < len(final[name]) and final[name][k] == value
            for name, value in values.items())
        lacking += bool(lacks)
        if not same:
            bad += 1
            first = first or {"index": k, "t": m["t"], "lacks": lacks[:5]}
    return {"points": len(steps), "recorded": len(times), "unequal": bad, "lacking_channels": lacking,
            "first_unequal": first}

