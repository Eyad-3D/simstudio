"""The help never goes stale (LRN-12).

docs/help/checks.json lists the numbers and messages the help quotes, each
with the run or model state that produces it. These tests build those
models, run them, and fail when a quoted number or message no longer
matches the app, when a tutorial's *Check:* or an exercise's *Answer:*
quotes nothing that is checked, when a Script recipe stops doing what its
page says, or when a fact the README states about the code (the files in
its architecture tree, its API routes, the drive cycles, the examples) is
no longer true.

To change a page's numbers, change the page and its entry in checks.json
together; the failure message prints the value the app gives now.
"""
from __future__ import annotations

import copy
import json
import re
from functools import lru_cache
from pathlib import Path

import pytest
from helpers import conn, dbc, el

from app import cycles
from app.library import load_library
from app.schemas import Project, SimCase, SystemNode
from app.solver import simulate
from app.solver.scripting import compile_script, run_script
from app.storage import load_example
from app.validation import validate_project

ROOT = Path(__file__).resolve().parents[2]
HELP = ROOT / "docs" / "help"
CHECKS = json.loads((HELP / "checks.json").read_text(encoding="utf-8"))


def _text(page: str) -> str:
    """A help page's text with its line breaks and runs of spaces as one space."""
    path = ROOT / page if page.startswith(("README", "docs/")) else HELP / page
    return re.sub(r"\s+", " ", path.read_text(encoding="utf-8"))


# ---- the models the pages build or open -------------------------------------
def _build(name: str, upto: str) -> Project:
    """Model `name` from checks.json after its steps up to and including
    `upto`, as the tutorial builds it in the app: parts with the library's
    defaults, then wires, signals and values step by step."""
    spec = CHECKS["models"][name]
    ids = {label: f"e{i}" for i, (label, _) in enumerate(spec["parts"])}
    elements = {label: el(ids[label], def_id, label) for label, def_id in spec["parts"]}
    wires, signals, case = [], [], {"duration": 600.0, "timeStep": 1.0}
    for step, change in spec["steps"].items():
        for a, ap, b, bp in change.get("wires", []):
            wires.append(conn(len(wires) + 1, ids[a], ap, ids[b], bp))
        for a, ap, b, bp in change.get("signals", []):
            signals.append(dbc(len(signals) + 1, ids[a], ap, ids[b], bp))
        for label, values in change.get("set", {}).items():
            elements[label].parameterOverrides.update(values)
        case.update(change.get("case", {}))
        if step == upto:
            break
    else:
        raise KeyError(f"model {name} has no step {upto}")
    return Project(id=name, name=name, systems=[SystemNode(
        id="root", name=name, parentId=None, elements=list(elements.values()), connections=wires)],
        dataBusConnections=signals, cases=[SimCase(id="case-1", name="Case 1", **case)])


def _model(spec: dict) -> tuple[Project, str]:
    """A run's model and case id: an example (with its case's values and
    parts' values changed) or a built model at one of its steps."""
    if "example" in spec:
        project = copy.deepcopy(load_example(spec["example"]))
        case = next(c for c in project.cases if c.id == spec["case"])
        for key, value in spec.get("caseSet", {}).items():
            setattr(case, key, value)
        by_label = {e.label: e for s in project.systems for e in s.elements}
        for label, values in spec.get("set", {}).items():
            by_label[label].parameterOverrides.update(values)
            # a value the case overrides would hide the part's own
            for key in values:
                case.parameterOverrides.get(by_label[label].id, {}).pop(key, None)
        return project, case.id
    name, step = spec["model"].split("@")
    return _build(name, step), "case-1"


@lru_cache(maxsize=None)
def _run(name: str):
    project, case_id = _model(CHECKS["runs"][name])
    return simulate(project, case_id)


@lru_cache(maxsize=None)
def _checks(state: str):
    """The Data Checks of a built model at a step: "first-car@wired"."""
    name, step = state.split("@")
    return validate_project(_build(name, step))


def v(run: str, label: str) -> float:
    """A summary value of a run, by its label."""
    for row in _run(run).summary:
        if row.label == label:
            return row.value
    raise KeyError(f"run {run} has no summary row {label!r}: "
                   f"{[r.label for r in _run(run).summary]}")


def status(run: str) -> str:
    return _run(run).status


def message(run: str, text: str) -> bool:
    """Whether a run's messages include `text`."""
    return any(text in m.text for m in _run(run).messages)


def check(state: str, text: str, level: str | None = None) -> bool:
    """Whether the Data Checks of a model state include `text`."""
    return any(text in c.text and (level is None or c.level == level) for c in _checks(state))


def clear(state: str) -> bool:
    """Whether a model state has no Data Check warning or error."""
    return not [c.text for c in _checks(state) if c.level != "info"]


def count(state: str, level: str) -> int:
    return sum(c.level == level for c in _checks(state))


def rises(run: str, channel: str) -> int:
    """How often a run's 0/1 channel turns on: an engine's starts."""
    series = next(c for c in _run(run).channels if c.label == channel).timeSeries
    values = [p["value"] or 0.0 for p in series]
    return sum(a < 0.5 <= b for a, b in zip(values, values[1:]))


def cycle_phase(cycle: str, i: int) -> tuple:
    """A drive cycle's phase: (name, start s, end s)."""
    return tuple(cycles.info(cycle)["phases"][i])


def cycle_top(cycle: str, i: int) -> float:
    """The top speed in a drive cycle's phase, km/h."""
    _, a, b = cycles.info(cycle)["phases"][i]
    return max(v for t, v in cycles.trace(cycle) if a <= t <= b)


FUNCTIONS = {"v": v, "status": status, "message": message, "check": check, "clear": clear,
             "count": count, "rises": rises, "cycle_phase": cycle_phase, "cycle_top": cycle_top,
             "abs": abs, "round": round, "tuple": tuple, "__builtins__": {}}


def _fact_id(fact: dict) -> str:
    return f"{fact['page']}: {fact['text']}"


@pytest.mark.parametrize("fact", CHECKS["facts"], ids=_fact_id)
def test_the_help_quotes_what_the_app_gives(fact):
    """Each fact's text is on its page, and its expression gives that text
    (formatted with the fact's format) or is true (a band, a message)."""
    assert fact["text"] in _text(fact["page"]), f"{fact['page']} no longer says {fact['text']!r}"
    got = eval(fact["expr"], dict(FUNCTIONS))  # noqa: S307 - our own file
    if "format" in fact:
        shown = fact["format"].format(got)
        assert shown in fact["text"], (
            f"{fact['page']} says {fact['text']!r}; the app gives {shown!r} ({fact['expr']})")
    else:
        assert got is True, f"{fact['page']}: {fact['text']!r} — {fact['expr']} is {got!r}"


# ---- every check and answer a page gives is a checked fact ------------------
MARKED = re.compile(r"\*\*(Check|Answer):\*\*(.*?)(?=\n\n|\n#|\Z)", re.S)


def _marked_pages():
    for path in sorted(HELP.rglob("*.md")):
        text = path.read_text(encoding="utf-8")
        if MARKED.search(text):
            yield path.relative_to(HELP).as_posix()


@pytest.mark.parametrize("page", list(_marked_pages()))
def test_every_check_and_answer_is_tested(page):
    facts = [f["text"] for f in CHECKS["facts"] if f["page"] == page]
    unchecked = []
    for kind, body in MARKED.findall((HELP / page).read_text(encoding="utf-8")):
        flat = re.sub(r"\s+", " ", body)
        if not any(text in flat for text in facts):
            unchecked.append(f"{kind}: {flat.strip()[:80]}")
    assert not unchecked, f"{page}: no checks.json fact for " + "; ".join(unchecked)


def test_the_tutorials_and_lessons_have_checks():
    """A tutorial or lesson gives the reader a check after its steps."""
    pages = [p.relative_to(HELP).as_posix() for p in sorted((HELP / "tutorials").glob("*.md"))]
    pages += [p.relative_to(HELP).as_posix() for p in sorted((HELP / "lessons").glob("*.md"))]
    assert pages
    missing = [p for p in pages if "**Check:**" not in (HELP / p).read_text(encoding="utf-8")]
    assert not missing, f"tutorials with no **Check:**: {missing}"


def test_every_example_has_a_lesson_page_with_exercises():
    """LRN-16: each example has a page with at least two exercises, whose
    answers the facts above check."""
    for f in sorted((ROOT / "backend" / "projects").glob("*.json")):
        pid = json.loads(f.read_text(encoding="utf-8"))["id"]
        page = HELP / "examples" / f"{pid}.md"
        assert page.exists(), f"example {pid} has no page docs/help/examples/{pid}.md"
        assert page.read_text(encoding="utf-8").count("**Answer:**") >= 2, f"{pid}: fewer than 2 exercises"


# ---- the Script cookbook (LRN-15) -------------------------------------------
COOKBOOK = HELP / "reference" / "script-cookbook.md"
RECIPE = re.compile(r"^## (.+?)\n(.*?)```python\n(.*?)```", re.S | re.M)


def _recipes() -> dict[str, str]:
    return {title: code for title, _, code in RECIPE.findall(COOKBOOK.read_text(encoding="utf-8"))}


def test_every_recipe_is_tested():
    assert len(_recipes()) >= 8
    assert set(_recipes()) == set(CHECKS["recipes"]), "recipes and their tests differ"


@pytest.mark.parametrize("title", sorted(CHECKS["recipes"]))
def test_recipe_does_what_its_page_says(title):
    """Each recipe compiles under the Script rules and, called step by step
    as the solver calls it, gives the outputs its test lists."""
    fn = compile_script(_recipes()[title], title)
    state: dict = {}
    for i, call in enumerate(CHECKS["recipes"][title]):
        for _ in range(call.get("repeat", 1)):
            out = run_script(fn, title, call.get("t", 0.0), call.get("dt", 0.01), call["inputs"],
                             state, {})
        for key, want in call["expect"].items():
            assert key in out, f"{title}, call {i + 1}: no output {key!r} in {out}"
            assert out[key] == pytest.approx(want, abs=1e-6), f"{title}, call {i + 1}: {key}"


# ---- what the README says about the code ------------------------------------
README = (ROOT / "README.md").read_text(encoding="utf-8")


def test_the_readme_architecture_tree_names_files_that_exist():
    tree = README.split("## Architecture", 1)[1].split("```", 2)[1]
    missing, base = [], None
    for line in tree.splitlines():
        top = re.match(r"^(frontend|backend|desktop)/", line)
        if top:
            base = top.group(1)
            continue
        m = re.search(r"[├└]─ ((?:app/|src/|scripts/)?[\w./-]+\.(?:py|json|js|mjs|html|spec|yml))", line)
        if m and base:
            name = m.group(1)
            # the solver's modules are listed under app/solver/ by their file name
            candidates = [ROOT / base / name, ROOT / base / "app" / "solver" / name]
            if not any(c.exists() for c in candidates):
                missing.append(f"{base}/{name}")
    assert not missing, f"README's architecture tree names files that do not exist: {missing}"


def test_the_readme_api_table_matches_the_routes():
    from app.main import app

    def norm(path: str) -> str:  # /api/projects/{id} and /api/projects/{project_id}
        return re.sub(r"\{[^}]*\}", "{}", path)

    routes = {norm(r.path) for r in app.routes if getattr(r, "path", "").startswith("/api")}
    listed = set()
    for row in README.split("### API", 1)[1].split("\n## ", 1)[0].splitlines():
        for path in re.findall(r"`(?:[A-Z/]+ )+(/api/[^`]*)`", row):
            listed.add(norm(path))
    missing = sorted(p for p in listed if p not in routes)
    assert listed and not missing, f"README lists API routes the engine does not have: {missing}"


def test_the_readme_names_the_bundled_cycles_and_examples():
    cycles = json.loads((ROOT / "backend" / "app" / "cycles" / "cycles.json").read_text(encoding="utf-8"))
    assert set(cycles["cycles"]) == {"wltc-3b", "udds", "hwfet"}, "README's cycle list needs updating"
    for word in ("WLTC class 3b", "UDDS", "HWFET"):
        assert word in README
    files = sorted(f.name for f in (ROOT / "backend" / "projects").glob("*.json"))
    line = next(line for line in README.splitlines() if "example projects (" in line)
    assert all(f in line for f in files), f"README's tree lists {line.strip()}, the folder has {files}"


def test_the_glossary_has_the_words_the_results_use():
    """LRN-10: every summary row and every glossary term has a definition."""
    glossary = (HELP / "glossary.md").read_text(encoding="utf-8")
    terms = re.findall(r"^\*\*(.+?)\*\*:", glossary, re.M)
    assert len(terms) >= 60, f"the glossary has {len(terms)} terms"
    assert len(terms) == len(set(terms)), "a glossary term is defined twice"


def test_every_summary_row_is_defined():
    """LRN-10: each summary row the examples' runs give has a row in the
    Results reference (a part's name stands as *part*, a number as *N*)."""
    page = (HELP / "reference" / "results.md").read_text(encoding="utf-8")
    patterns = []
    for name in re.findall(r"^\| \*\*(.+?)\*\* \|", page, re.M):
        rx = re.escape(name).replace(r"\*part\*", ".+").replace(r"\*N\*", r"[\d.]+")
        rx = rx.replace(r"\*what\*", ".+").replace(r"\*axis\*", ".+").replace(r"\*limit\*", ".+")
        patterns.append(re.compile(f"^{rx}$"))
    assert patterns
    rows = {r.label for run in CHECKS["runs"] for r in _run(run).summary}
    undefined = sorted(r for r in rows if not any(p.match(r) for p in patterns))
    assert not undefined, f"summary rows with no definition in reference/results.md: {undefined}"


def test_every_component_is_in_the_library_page_count():
    """The help's front page and README do not quote a part count that drifted."""
    n = len(load_library())
    for text in (README, (HELP / "index.md").read_text(encoding="utf-8")):
        for said in re.findall(r"\b(\d+) (?:parts|components) in the library", text):
            assert int(said) == n, f"a page says {said} parts in the library; there are {n}"
