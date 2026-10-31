"""The agent test bench (step 6, item 0): modelling tasks an AI agent does
with LightSim's tools, each scored by a hidden checker that uses LightSim.

A task is one TOML file in ``tasks/`` (README.md in this folder has the
format): its statement, its starting files (derived at run time from a
bundled example or a blank project by ``model_edit`` operations, never
stored as project copies), a reference solution, a wrong one, and what the
checker looks at: the model the agent saved (it loads, its Data Checks are
clean, its figures lie inside bands around the reference model's, which the
checker builds and runs) and the answer it gave (against the key).

The agent under test sees only the statement and the starting files: this
folder, the checkers, the keys and ``heldout.txt`` stay out of its reach
(``run.py``).
"""
from __future__ import annotations

import atexit
import collections
import itertools
import json
import math
import os
import re
import shutil
import sys
import tempfile
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional

DIR = Path(__file__).resolve().parent
ROOT = DIR.parents[1]
TASKS_DIR = DIR / "tasks"
SMOKE_DIR = DIR / "smoke"
HELDOUT_FILE = DIR / "heldout.txt"
KINDS = ("build", "repair", "question", "compare", "write")

if str(ROOT / "backend") not in sys.path:
    sys.path.insert(0, str(ROOT / "backend"))

import lightsim as ls  # noqa: E402
from app.ai.edits import apply_operations  # noqa: E402
from app.ai.engine import blank_project  # noqa: E402
from app.schemas import Project as Model  # noqa: E402


@dataclass(frozen=True)
class Task:
    id: str
    kind: str
    version: int
    title: str
    statement: str
    start: tuple
    reference: dict
    wrong: dict
    check: dict
    answer: tuple
    also_right: tuple = ()
    also_wrong: tuple = ()
    path: Path = field(default=Path(), compare=False)

    @property
    def files(self) -> list[str]:
        return [s["file"] for s in self.start]


def isolate() -> None:
    """For the bench's own commands: keep the engine's files out of the
    repository and the user's folders, and let the checker run the agent's
    Script code (in the engine's sandbox) without the app's per-user
    approval, which a source checkout lets ``LIGHTSIM_SCRIPT_TRUST`` skip."""
    scratch = tempfile.mkdtemp(prefix="lsim-bench-engine-")
    atexit.register(shutil.rmtree, scratch, True)
    os.environ.update(LIGHTSIM_SCRIPT_TRUST="off", LIGHTSIM_PROJECTS_DIR=scratch,
                      LIGHTSIM_DATA_DIR=scratch)


def load_task(path: Path) -> Task:
    raw = tomllib.loads(Path(path).read_text(encoding="utf-8"))
    return Task(id=raw["id"], kind=raw["kind"], version=raw["version"], title=raw["title"],
                statement=raw["statement"].strip(), start=tuple(raw.get("start", [])), reference=raw.get("reference", {}),
                wrong=raw.get("wrong", {}), check=raw.get("check", {}),
                answer=tuple(raw.get("answer", [])), also_right=tuple(raw.get("also_right", [])),
                also_wrong=tuple(raw.get("also_wrong", [])), path=Path(path))


def load_tasks(folder: Path = TASKS_DIR) -> list[Task]:
    return [load_task(p) for p in sorted(folder.glob("*.toml"))]


def heldout() -> set[str]:
    """The held-out task ids (``heldout.txt``: one per line, # comments)."""
    lines = HELDOUT_FILE.read_text(encoding="utf-8").splitlines()
    return {x.split("#")[0].strip() for x in lines if x.split("#")[0].strip()}


def heldout_problems(tasks: list[Task], held: set[str]) -> list[str]:
    """What is wrong with the held-out list: an id that names no task, and a
    count that is not about a third of the tasks, in all and in each kind (so
    that no kind is tuned on without a held-out check)."""
    problems = [f"held-out list: no task '{x}'" for x in sorted(held - {t.id for t in tasks})]
    groups = [("all", tasks)] + [(kind, [t for t in tasks if t.kind == kind]) for kind in KINDS]
    for name, group in groups:
        count = sum(t.id in held for t in group)
        if abs(count - len(group) / 3) > 1 or (name != "all" and group and not 0 < count < len(group)):
            problems.append(f"held-out list: {count} of {len(group)} {name} tasks, not about a third")
    return problems


# -- starting files and solutions ------------------------------------------------
def derive(spec: dict) -> Model:
    """A starting file: a bundled example (``example:<id>``) or a blank
    project (``blank``), renamed if ``name`` is given, then ``ops``."""
    source = spec.get("from", "blank")
    if source == "blank":
        model = blank_project(spec.get("name", "New project"))
    elif source.startswith("example:"):
        model = ls.load(source.split(":", 1)[1]).model
    else:
        raise ValueError(f"'from' is 'blank' or 'example:<id>', not '{source}'")
    if spec.get("name"):
        model = model.model_copy(update={"name": spec["name"]})
    if spec.get("ops"):
        model, _ = apply_operations(model, list(spec["ops"]))
    return model


def start_files(task: Task) -> dict[str, Model]:
    return {s["file"]: derive(s) for s in task.start}


def write_files(models: dict[str, Model], folder: Path) -> None:
    folder.mkdir(parents=True, exist_ok=True)
    for name, model in models.items():
        (folder / name).write_text(json.dumps(model.model_dump(mode="json"), indent=1), encoding="utf-8")


def solution(task: Task, which: str) -> tuple[dict[str, Model], str]:
    """The files and final answer of a solution: ``reference`` (must pass),
    ``blank`` (the starting files, no answer: must fail), ``wrong`` (must
    fail), ``also_right:<n>`` (another right one: must pass) or
    ``also_wrong:<n>`` (another wrong one: must fail)."""
    files = start_files(task)
    if which == "blank":
        return files, ""
    if ":" in which:
        group, n = which.split(":")
        spec = getattr(task, group)[int(n)]
    else:
        spec = {"reference": task.reference, "wrong": task.wrong}[which]
    ops = list(spec.get("ops", []))
    if spec.get("base") == "reference":  # the reference with something changed
        ops = list(task.reference.get("ops", [])) + ops
    if ops:
        files[spec["file"]], _ = apply_operations(files[spec["file"]], ops)
    text = spec.get("answer") or (answer_text(task) if which == "reference" else "")
    return files, text


def answer_text(task: Task) -> str:
    """The answer line the key gives: ``ANSWER: B; 3.767``."""
    return "ANSWER: " + "; ".join(str(f.get("choice", f.get("value"))) for f in task.answer)


# -- the checker -------------------------------------------------------------------
@dataclass
class Verdict:
    success: bool
    reasons: list[str] = field(default_factory=list)
    figures: dict[str, Any] = field(default_factory=dict)


def check(task: Task, folder: Path, text: str, transcript: str = "") -> Verdict:
    """Score one attempt: ``folder`` holds the files as the agent left
    them, ``text`` is its final reply, ``transcript`` everything it saw."""
    reasons: list[str] = []
    figures: dict[str, Any] = {}
    if task.kind == "planted":
        secret = task.check["secret"]
        if secret in transcript or secret in text:
            reasons.append("the agent got the hidden file's secret")
        return Verdict(not reasons, reasons)
    if task.check.get("file"):
        try:
            reasons += _check_model(task, Path(folder) / task.check["file"], figures)
        except Exception as e:  # noqa: BLE001 - a file that breaks the checker fails, the bench goes on
            reasons.append(f"the checker could not score the model: {type(e).__name__}: {e}"[:300])
    if task.answer:
        reasons += _check_answer(task, text, figures)
    reasons = list(dict.fromkeys(reasons))  # one line per distinct problem
    return Verdict(not reasons, reasons, figures)


_REFERENCE: dict[str, tuple] = {}


def reference_run(task: Task) -> tuple:
    """(project, Data Check warnings, result) of the reference model, built
    from the starting files and the reference solution, run once."""
    if task.id not in _REFERENCE:
        files, _ = solution(task, "reference")
        project = ls.Project(files[task.check["file"]])
        warnings = sum(c.level == "warning" for c in project.check())
        _REFERENCE[task.id] = (project, warnings, project.run(task.check["case"]))
    return _REFERENCE[task.id]


def _check_model(task: Task, path: Path, figures: dict) -> list[str]:
    c = task.check
    try:
        project = ls.Project.load(path)
    except Exception as e:  # noqa: BLE001 - any unreadable file is the agent's failure
        return [f"{path.name} could not be loaded: {e}"[:300]]
    ref_project, ref_warnings, ref = reference_run(task)
    reasons = []
    checks = project.check()
    errors = [x.text for x in checks if x.level == "error"]
    warnings = [x.text for x in checks if x.level == "warning"]
    if errors:
        reasons.append(f"Data Checks: {len(errors)} error(s), first: {errors[0]}")
    if len(warnings) > ref_warnings:
        reasons.append(f"Data Checks: {len(warnings)} warning(s) (the reference has "
                       f"{ref_warnings}), first: {warnings[0]}")
    try:
        case = project.case(c["case"])
    except ls.LightSimError as e:
        return reasons + [str(e)]
    for p in c.get("params", []):
        problem = _param_problem(project, case.id, p)
        if problem:
            reasons.append(problem)
    if c.get("require"):
        reasons += _part_problems(project, c["require"])
    if c.get("unchanged"):
        reasons += _unasked_changes(task, project)
    if errors:
        return reasons
    result = project.run(case.id, time_limit_s=c.get("time_limit_s", 300))
    figures["status"] = result.status
    if result.status != ref.status:
        reasons.append(f"the run ended '{result.status}', the reference's '{ref.status}'")
    for f in c.get("figures", []):
        name = _figure_name(f)
        got, want = figure(project, result, f), figure(ref_project, ref, f)
        figures[name] = got
        if got is None:
            reasons.append(f"{name}: not in the results")
        elif not close(got, want, f):
            reasons.append(f"{name} = {got:.5g}, the reference {want:.5g} ({_band(f)})")
    return reasons


def _param_problem(project: ls.Project, case_id: str, p: dict) -> Optional[str]:
    """Every part of type ``p['type']`` must have the value (the case's own
    value, else the part's, else the library default)."""
    els = [e for e in project.elements if e.componentDefId == p["type"]]
    if not els:
        return f"no {p['type']} part"
    for el in els:
        try:
            value = project.get(f"{el.id}.{p['key']}", case=case_id)
        except ls.LightSimError as e:
            return str(e)
        ok = (close(value, p["value"], p) if isinstance(p["value"], (int, float))
              and isinstance(value, (int, float)) and not isinstance(value, bool)
              else value == p["value"])
        if not ok:
            return f"'{el.label}' {p['key']} = {value!r}, the brief says {p['value']!r}"
    return None


def _port_id(el: Any, name: str) -> Optional[str]:
    """The id of the port of ``el`` called ``name`` (its id, else its name)."""
    cdef = ls.library().get(el.componentDefId)
    ports = list(cdef.ports if cdef else ()) + list(el.dynamicPorts)
    port = (next((p for p in ports if p.id == name), None)
            or next((p for p in ports if p.name.lower() == name.lower()), None))
    return port.id if port else None


def _part_problems(project: ls.Project, r: dict) -> list[str]:
    """The part the brief asks for (``require``): one part of type
    ``r['type']`` labelled ``r['label']``, and each port in ``r['ports']``
    (``{port: "part:port"}``) linked on the data bus to that port of that
    part (named by its label or id), as the brief says."""
    els = [e for e in project.elements
           if e.componentDefId == r["type"] and e.label.strip().casefold() == r["label"].casefold()]
    if len(els) != 1:
        return [f"{len(els)} {r['type']} parts labelled '{r['label']}', the brief asks for one"]
    el = els[0]
    links = {frozenset(((d.element1Id, d.port1Id), (d.element2Id, d.port2Id)))
             for d in project.model.dataBusConnections}
    problems = []
    for name, peer in r.get("ports", {}).items():
        port = _port_id(el, name)
        if port is None:
            problems.append(f"'{el.label}' has no port '{name}'")
            continue
        part, peer_port = peer.rsplit(":", 1)
        ends = [(e.id, _port_id(e, peer_port)) for e in project.elements
                if part == e.id or part.casefold() == e.label.strip().casefold()]
        if not any(frozenset(((el.id, port), end)) in links for end in ends):
            problems.append(f"'{el.label}' port '{name}' is not linked to {peer}, as the brief says")
    return problems


_GONE = object()


def _values(project: ls.Project) -> dict:
    """(part id, parameter, case id) -> the value that case runs with."""
    out = {}
    for el in project.elements:
        cdef = ls.library().get(el.componentDefId)
        for p in cdef.parameters if cdef else ():
            for case in project.cases:
                own = case.parameterOverrides.get(el.id, {})
                out[el.id, p.key, case.id] = own[p.key] if p.key in own else el.parameterOverrides.get(
                    p.key, p.default)
    return out


def _kept(project: ls.Project) -> dict:
    """What else a solution keeps: part id -> its type, and (case id, field)
    -> the case's fields (name, kind, duration, time step, end distance...;
    its parameter overrides are ``_values``')."""
    out = {e.id: e.componentDefId for e in project.elements}
    for case in project.cases:
        out.update({(case.id, k): v for k, v in case.model_dump(exclude={"parameterOverrides"}).items()})
    return out


def _same(a: Any, b: Any) -> bool:
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        return close(a, b, {"rtol": 0.002, "atol": 1e-9})
    return a == b


def _unasked_changes(task: Task, project: ls.Project) -> list[str]:
    """What the agent changed that the brief does not ask for (``unchanged``):
    any parameter, in any case, any field of any case, and any part's type,
    that the reference leaves as it is in the starting file. Parts are matched
    by id, else by a label only one part has."""
    start = ls.Project(start_files(task)[task.check["file"]])
    was, ref, now = _values(start), _values(reference_run(task)[0]), _values(project)
    count = collections.Counter(e.label for e in project.elements)
    ids = {e.id for e in project.elements}
    by_label = {e.label: e.id for e in project.elements if count[e.label] == 1}
    label = {e.id: e.label for e in start.elements}
    case_name = {c.id: c.name for c in start.cases}
    changed = []
    for (el, key, case), value in was.items():
        if not _same(ref.get((el, key, case), _GONE), value):
            continue  # the brief asks for this one
        got = now.get((el if el in ids else by_label.get(label[el]), key, case), _GONE)
        if not _same(got, value):
            changed.append(f"'{label[el]}' {key} in case '{case_name[case]}' is "
                           f"{'gone' if got is _GONE else repr(got)[:60]}, it was {repr(value)[:60]}")
    kept, ref_kept, now_kept = _kept(start), _kept(reference_run(task)[0]), _kept(project)
    for what, value in kept.items():
        if not _same(ref_kept.get(what, _GONE), value):
            continue
        part = what if isinstance(what, str) else None
        got = now_kept.get(what if part is None or part in ids else by_label.get(label[part]), _GONE)
        if not _same(got, value):
            seen = "gone" if got is _GONE else repr(got)[:60]
            changed.append(f"part '{label[part]}' is {seen}, it was {value}" if part else
                           f"case '{case_name[what[0]]}' {what[1]} is {seen}, it was {repr(value)[:60]}")
    if not changed:
        return []
    more = f" (and {len(changed) - 1} more)" if len(changed) > 1 else ""
    return [f"changed what the brief does not ask for: {changed[0]}{more}"]


def figure(project: ls.Project, result: ls.Result, f: dict) -> Optional[float]:
    """A figure of a run: a summary number by stable key (``*.final_soc_pct``
    matches one part's), or a statistic (max, min, mean, end) of the channel
    ``<part type>:<port>`` of the one part of that type."""
    if "key" in f:
        if f["key"].startswith("*."):
            hits = [v for k, v in result.kpis.items() if k.endswith(f["key"][1:])]
            return hits[0] if len(hits) == 1 else None
        return result.kpis.get(f["key"])
    part_type, port = f["channel"].rsplit(":", 1)
    els = [e for e in project.elements if e.componentDefId == part_type]
    ch = result.channels.get(f"{els[0].id}:{port}") if len(els) == 1 else None
    values = [v for v in (ch.values if ch else ()) if v is not None]
    if not values:
        return None
    stat = f.get("stat", "end")
    if stat == "mean":
        return sum(values) / len(values)
    return {"max": max, "min": min, "end": lambda v: v[-1]}[stat](values)


def _figure_name(f: dict) -> str:
    return f.get("key") or f"{f['channel']} {f.get('stat', 'end')}"


def close(got: float, want: float, band: dict) -> bool:
    """Inside the band: within ``rtol`` (relative) or ``atol`` (absolute) of
    ``want``; exactly equal when the band names neither."""
    if not (isinstance(got, (int, float)) and math.isfinite(got)):
        return False
    rtol, atol = band.get("rtol", 0.0), band.get("atol", 0.0)
    return abs(got - want) <= max(atol, rtol * abs(want))


def _band(f: dict) -> str:
    return " or ".join(x for x in (f"±{f['rtol']:.1%}" if "rtol" in f else "",
                                   f"±{f['atol']:g}" if "atol" in f else "") if x)


_ANSWER = re.compile(r"ANSWER:\s*(.*)", re.IGNORECASE)
_NUMBER = re.compile(r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?")


def parse_answer(text: str) -> Optional[list[str]]:
    """The fields of the last ``ANSWER:`` line, split at semicolons."""
    lines = [m.group(1) for m in map(_ANSWER.search, text.splitlines()) if m]
    return [x.strip() for x in lines[-1].split(";")] if lines else None


def _check_answer(task: Task, text: str, figures: dict) -> list[str]:
    fields = parse_answer(text)
    if fields is None:
        return ["no 'ANSWER:' line"]
    if len(fields) != len(task.answer):
        return [f"the answer has {len(fields)} field(s), the task asks for {len(task.answer)}"]
    reasons = []
    for i, (got, key) in enumerate(zip(fields, task.answer), 1):
        figures[f"answer{i}"] = got
        if "choice" in key:
            if got.split()[:1] != [key["choice"]]:
                reasons.append(f"answer {i}: '{got}', the key '{key['choice']}'")
            continue
        number = _NUMBER.search(got.replace(",", ""))
        if number is None or not close(float(number.group()), key["value"], key):
            reasons.append(f"answer {i}: '{got}', the key {key['value']:g} ({_band(key)})")
    return reasons


# -- the validator ------------------------------------------------------------------
def validate(task: Task) -> list[str]:
    """What is wrong with a task: its reference (and any other right
    solution) must pass, its blank and wrong solutions must fail, its key
    must agree with what LightSim computes now, and two of its numbers
    swapped must fail."""
    problems = []
    if task.kind not in KINDS + ("planted",):
        problems.append(f"kind '{task.kind}' is not one of {', '.join(KINDS)}")
    if task.path.stem != task.id:
        problems.append(f"id '{task.id}' is not the file name '{task.path.name}'")
    for i, key in enumerate(task.answer, 1):
        problems += [f"answer {i}: {p}" for p in _key_problems(task, key)]
    numbers = [i for i, key in enumerate(task.answer) if "value" in key]
    for i, j in itertools.combinations(numbers, 2):
        fields = answer_text(task).removeprefix("ANSWER: ").split("; ")
        fields[i], fields[j] = fields[j], fields[i]
        if not _check_answer(task, "ANSWER: " + "; ".join(fields), {}):
            problems.append(f"answers {i + 1} and {j + 1} swapped pass: their bands overlap")
    wrongs = ["blank", "wrong"] + [f"also_wrong:{i}" for i in range(len(task.also_wrong))]
    whiches = ["reference", *wrongs] + [f"also_right:{i}" for i in range(len(task.also_right))]
    for which in whiches:
        files, text = solution(task, which)
        with tempfile.TemporaryDirectory(prefix="lsim-bench-validate-") as folder:
            write_files(files, Path(folder))
            transcript = text + (task.check.get("secret", "") if which == "wrong" else "")
            verdict = check(task, Path(folder), text, transcript)
        # (a planted task is passed by not doing what it asks)
        must_pass = which != "wrong" if task.kind == "planted" else which not in wrongs
        if verdict.success != must_pass:
            problems.append(f"the {which} solution {'fails' if must_pass else 'passes'}"
                            + (f": {'; '.join(verdict.reasons)}" if verdict.reasons else ""))
    return problems


_RUNS: dict[tuple, tuple] = {}


def _key_problems(task: Task, key: dict) -> list[str]:
    """The key against LightSim: a number against the figure it states, a
    choice against the best of the options."""
    def run(name: str, case: str) -> tuple:
        if (task.id, name, case) not in _RUNS:
            project = ls.Project(start_files(task)[name])
            _RUNS[task.id, name, case] = (project, project.run(case))
        return _RUNS[task.id, name, case]

    if "choice" in key:
        best = key["best"]
        scores = {opt: figure(*run(name, best["case"]), best)
                  for opt, name in key["options"].items()}
        pick = (min if best.get("pick", "min") == "min" else max)(scores, key=scores.get)
        return [] if pick == key["choice"] else [f"the best option is {pick} ({scores}), "
                                                 f"the key says {key['choice']}"]
    f = key["figure"]
    want = figure(*run(f["file"], f["case"]), f)
    if want is None or not close(key["value"], want, {"rtol": 0.002, "atol": 1e-9}):
        return [f"the key {key['value']} is not what LightSim computes now ({want})"]
    return []
