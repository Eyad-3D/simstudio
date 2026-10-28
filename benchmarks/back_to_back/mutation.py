"""The FAULTS line's mutation testing: mutmut 3 on the run verdict
(backend/app/solver/verdict.py), with the backend tests that exercise it.

mutmut plants one small change at a time in verdict.py (a < for a <=, a
constant off by one, a dropped call) and runs the tests against it: a
mutant the tests do not catch survives. Every survivor needs a written
reason in survivors.toml (an equivalent change, or what test would kill
it and why none is wanted), or the line fails.

It works on a copy of the backend in OUT/mutation/backend (mutmut writes
its mutants and its progress there), so a stopped run resumes: `mutmut
run` goes on with the mutants not yet tested. The tests are the fast ones
that reach the verdict (TESTS: they call it, or run small models through
it); the whole-example tests that also reach it (test_verdict.py's
test_bundled_examples_follow_their_cycles, test_examples_plausible.py,
golden_compare.py through test_golden.py) take a minute or more a run and
are left out, which can only make the survivors more, never fewer. mutmut
runs on Linux only (it forks).
"""
from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

from .runs import BACKEND, HERE

MEMORY = 2_500_000_000  # bytes of address space a test process may take: a mutant that loops
#                         growing a list (one of _RangeMinMax's did, to 5.5 GB) fails there, killed
TARGET = "app/solver/verdict.py"
TESTS = ["tests/test_verdict.py", "tests/test_distance_profile.py", "tests/test_map_edges.py",
         "tests/test_acceleration.py", "tests/test_run_warnings.py", "tests/test_power_limit.py",
         # a minute a run, every bundled example to its end: left out (see above)
         "--deselect=tests/test_verdict.py::test_bundled_examples_follow_their_cycles"]
_LIST = "".join(f"    {t}\n" for t in TESTS)  # setup.cfg's lists: one item a line
CONFIG = f"""[mutmut]
source_paths = {TARGET}
pytest_add_cli_args_test_selection =
{_LIST}also_copy =
    app/
    projects/
    tests/
"""


def survivors_file() -> dict:
    return tomllib.loads((HERE / "survivors.toml").read_text(encoding="utf-8")).get("survivor", {})


def run(out: Path, max_children: int | None = None) -> dict:
    work = out / "mutation" / "backend"
    if not work.exists():
        shutil.copytree(BACKEND, work, ignore=shutil.ignore_patterns(
            "__pycache__", ".pytest_cache", "mutants", "*.egg-info", "build", "dist", ".ruff_cache"))
    (work / "setup.cfg").write_text(CONFIG, encoding="utf-8")
    mutmut = str(Path(sys.executable).with_name("mutmut"))
    log = out / "mutation" / "run.log"
    with open(log, "a", encoding="utf-8") as f:
        subprocess.run([mutmut, "run", "--max-children", str(max_children or 2)], cwd=work,
                       stdout=f, stderr=subprocess.STDOUT, preexec_fn=_limit)
    listing = subprocess.run([mutmut, "results", "--all", "true"], cwd=work, capture_output=True,
                             text=True).stdout
    states: dict[str, str] = {}
    for line in listing.splitlines():
        m = re.match(r"\s*(\S+__mutmut_\d+): (.+)$", line)
        if m:
            states[m.group(1)] = m.group(2).strip()
    survived = sorted(k for k, v in states.items() if v == "survived")
    reasons = survivors_file()
    res = {"tool": "mutmut " + _version(), "target": TARGET, "tests": TESTS,
           "mutants": len(states), "by_state": _count(states.values()),
           "survived": survived, "without_reason": [s for s in survived if not reasons.get(s)],
           "complete": bool(states) and not any(v in ("not checked", "running") for v in states.values())}
    (out / "mutation.json").write_text(json.dumps(res, indent=1), encoding="utf-8")
    print(f"mutmut: {res['mutants']} mutants, {res['by_state']}", flush=True)
    return res


def _limit() -> None:
    import resource
    resource.setrlimit(resource.RLIMIT_AS, (MEMORY, MEMORY))


def _count(values) -> dict:
    out: dict = {}
    for v in values:
        out[v] = out.get(v, 0) + 1
    return out


def _version() -> str:
    from importlib.metadata import version
    return version("mutmut")
