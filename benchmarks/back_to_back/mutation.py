"""The FAULTS line's mutation testing: mutmut 3 on the run verdict
(backend/app/solver/verdict.py), with the backend tests that exercise it.

mutmut plants one small change at a time in verdict.py (a < for a <=, a
constant off by one, a dropped call) and runs the tests against it: a
mutant the tests do not catch survives. Every survivor needs a written
reason in survivors.toml (an equivalent change, or what test would kill
it and why none is wanted), or the line fails.

It works on a copy of the backend in OUT/mutation/backend (mutmut writes
its mutants and its progress there), so a stopped run resumes: `mutmut
run` goes on with the mutants not yet tested; a copy of other code (any
file it copies, code_key) is made afresh first. The tests are the fast ones
that reach the verdict (TESTS: they call it, or run small models through
it); the whole-example tests that also reach it (test_verdict.py's
test_bundled_examples_follow_their_cycles, test_examples_plausible.py,
golden_compare.py through test_golden.py) take a minute or more a run and
are left out, which can only make the survivors more, never fewer. mutmut
runs on Linux only (it forks).
"""
from __future__ import annotations

import hashlib
import json
import os
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


IGNORE = shutil.ignore_patterns("__pycache__", ".pytest_cache", "mutants", "*.egg-info", "build", "dist",
                                ".ruff_cache", ".hypothesis")  # caches, and mutmut's own output


def copied(root: Path):
    """Every file prepare copies from ``root``, as shutil.copytree walks it with IGNORE."""
    for here, dirs, files in os.walk(root):
        ignored = IGNORE(here, dirs + files)
        dirs[:] = sorted(d for d in dirs if d not in ignored)
        yield from (Path(here) / f for f in sorted(files) if f not in ignored)


def code_key() -> str:
    """The SHA-256 of what decides which mutants the tests kill: every file
    of the copy (the app's code, verdict.py among it, its library and the
    example projects, the tests and the helpers and fixtures they read) and
    the configuration mutmut runs with."""
    h = hashlib.sha256(CONFIG.encode() + b"\0")
    for p in copied(BACKEND):
        h.update(str(p.relative_to(BACKEND)).replace("\\", "/").encode() + b"\0" + p.read_bytes() + b"\0")
    return h.hexdigest()


def prepare(out: Path) -> Path:
    """OUT/mutation/backend, the copy mutmut works in: kept (so a stopped
    run resumes) only while the code it was copied from is unchanged
    (code_key, in OUT/mutation/copy.json); otherwise made afresh."""
    work, key_file = out / "mutation" / "backend", out / "mutation" / "copy.json"
    key = code_key()
    kept = json.loads(key_file.read_text(encoding="utf-8")).get("code") if key_file.exists() else None
    if work.exists() and kept != key:
        shutil.rmtree(work)
    if not work.exists():
        shutil.copytree(BACKEND, work, ignore=IGNORE)
        key_file.write_text(json.dumps({"code": key}), encoding="utf-8")
    (work / "setup.cfg").write_text(CONFIG, encoding="utf-8")
    return work


def run(out: Path, max_children: int | None = None) -> dict:
    from .runs import stamp
    made_by, code = stamp(), code_key()  # the code it starts on
    work = prepare(out)
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
    res = {"stamp": made_by, "code": code, "tool": "mutmut " + _version(), "target": TARGET, "tests": TESTS,
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
