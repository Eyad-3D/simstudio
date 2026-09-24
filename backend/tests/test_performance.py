"""Speed: an example run may not get more than 10 % slower than the same run
of the base commit. Both are timed on the same machine in alternating fresh
processes, best of ROUNDS each, so the runner's speed and its noise cancel
and no timing is stored.

Opt-in: CI's perf job checks the base commit out and sets LIGHTSIM_PERF_BASE
to its backend folder; without it the test is skipped. Locally, from backend/:
  git worktree add --detach /tmp/lightsim-base main
  LIGHTSIM_PERF_BASE=/tmp/lightsim-base/backend python -m pytest tests/test_performance.py -q -s"""
import os
import subprocess
import sys
from pathlib import Path

import pytest

BASE = os.environ.get("LIGHTSIM_PERF_BASE")
HEAD = Path(__file__).parent.parent
SLICES = [("bev-car", "case-city", 120), ("hybrid-car", "case-mixed", 120)]
ROUNDS = 5  # if it ever flakes, raise this before the budget
BUDGET = 1.10  # a 20 % slow-down measured 1.15 to 1.22; the same code 1.00 to 1.04

# Run with the head's or the base's backend folder as the working directory,
# so `app` is that commit's engine.
CHILD = """
import sys, time
def main():
    from app.solver import simulate
    from app.storage import load_example
    p = load_example(sys.argv[1])
    case = next(c for c in p.cases if c.id == sys.argv[2])
    case.duration = float(sys.argv[3])
    t0 = time.perf_counter()
    simulate(p, case.id)
    print(time.perf_counter() - t0)
if __name__ == "__main__":
    main()
"""


def _time(backend: Path, *args) -> float:
    out = subprocess.run([sys.executable, "-c", CHILD, *map(str, args)], cwd=backend,
                         capture_output=True, text=True)
    assert out.returncode == 0, f"{backend}: {out.stderr}"
    return float(out.stdout.split()[-1])


@pytest.mark.skipif(not BASE, reason="set LIGHTSIM_PERF_BASE to the base commit's backend folder")
@pytest.mark.parametrize("project_id,case_id,seconds", SLICES)
def test_example_run_is_not_slower_than_the_base(project_id, case_id, seconds):
    head, base = [], []
    for _ in range(ROUNDS):
        head.append(_time(HEAD, project_id, case_id, seconds))
        base.append(_time(Path(BASE), project_id, case_id, seconds))
    ratio = min(head) / min(base)
    print(f"{project_id}/{case_id} {seconds} s: {min(head):.3f} s vs base {min(base):.3f} s ({ratio:.3f})")
    assert ratio <= BUDGET, f"{ratio:.2f}x the base commit's time (budget {BUDGET}x)"
