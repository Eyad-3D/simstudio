"""The both-engines diff of step 2 (benchmarks/back_to_back/acceptance.toml).

    python -m benchmarks.back_to_back all OUT [--workers N]   every line, from scratch or resumed
    python -m benchmarks.back_to_back run OUT [--parts items,faults,meta] [--workers N]
    python -m benchmarks.back_to_back stop OUT [--stops N]     the stop-latency probe (STREAM S2)
    python -m benchmarks.back_to_back replay OUT              the replay probe (STREAM S3)
    python -m benchmarks.back_to_back mutation OUT [--max N]  mutmut on verdict.py (FAULTS), resumable
    python -m benchmarks.back_to_back noskip OUT              the new engine's tests with and without the module
    python -m benchmarks.back_to_back report OUT              OUT/report.json and OUT/report.md
    python -m benchmarks.back_to_back job SPEC PATH           one run (what `run` starts)

Run from the repository root with the backend's Python and the module
lightsim_engine installed (engine/crates/lsim-py). OUT is a folder outside
the repository: every raw result goes there (raw/<setting>/*.json), each
with its SHA-256 in OUT/SHA256SUMS once `report` has run. A stopped run
resumes where it stopped: a job whose output exists is not run again.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path


def main() -> None:
    ap = argparse.ArgumentParser(prog="python -m benchmarks.back_to_back")
    ap.add_argument("command", choices=["all", "run", "stop", "replay", "mutation", "noskip", "report", "job"])
    ap.add_argument("out")
    ap.add_argument("path", nargs="?")
    ap.add_argument("--parts", default="items,faults,meta")
    ap.add_argument("--workers", type=int, default=max(1, min(8, (os.cpu_count() or 2) // 2)))
    ap.add_argument("--stops", type=int, default=20)
    ap.add_argument("--max", type=int, default=None)
    a = ap.parse_args()
    if a.command == "job":  # (out is the spec here, path the output)
        from . import runs
        runs.child(json.loads(a.out), Path(a.path))
        return
    out = Path(a.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    if a.command in ("run", "all"):
        from . import runs
        runs.run_all(out, runs.specs(set(a.parts.split(","))), a.workers)
    if a.command in ("stop", "all"):
        from . import stream
        stream.stop_probe(out, a.stops)
    if a.command in ("replay", "all"):
        from . import stream
        stream.replay_probe(out)
    if a.command in ("mutation", "all"):
        from . import mutation
        mutation.run(out, a.max)
    if a.command in ("noskip", "all"):
        from . import report
        report.noskip_probe(out)
    if a.command in ("report", "all"):
        from . import report
        report.write(out)
    sys.stdout.flush()


if __name__ == "__main__":
    main()
