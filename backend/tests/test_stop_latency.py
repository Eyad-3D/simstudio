"""Stop latency on the new engine's path, on Windows (step 2's STREAM line,
benchmarks/back_to_back/acceptance.toml): the wall time from a stop asked
at a random moment to the run returning, 20 stops each, on the battery
electric car's WLTC as a live run and on a 16-point study of its WLTC
(not of its City Cycle, as registered: acceptance.toml's dated change of
31 October). Pass marks, registered before the measurement: p95 at most 0.25 s
for the live run and 1.0 s for the study; and at least half the stops must
reach a running point and end it cancelled, or nothing was measured.

Windows only (the only target, where stop latency counts): CI's Windows
engine job runs it with the module built (LIGHTSIM_REQUIRE_NEW_ENGINE=1).
Step 2's live part wired the stop into the new engine's run (its
RunControl); the numbers taken on Linux are an indication only
(benchmarks/back_to_back/baseline/README.md)."""
from __future__ import annotations

import os
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))  # the repository: benchmarks/

pytestmark = [
    pytest.mark.skipif(sys.platform != "win32", reason="stop latency is judged on Windows only"),
    pytest.mark.skipif(os.environ.get("LIGHTSIM_REQUIRE_NEW_ENGINE", "").strip() != "1",
                       reason="needs the module (CI's Windows engine job)"),
]


# STREAM S2's live mark is a known issue (engine/STATUS.md, "Step 2 closed
# without acceptance"; the owner's decision of 10 November 2026): on Windows CI
# run 43116535929 (989204b) its p95 was 0.296 s against 0.25 s, with 3 of 20
# stops over the mark. It is a timing near its mark (0.233 s on a local run),
# so a run may pass: not strict, as a pass must not turn CI red; a failed
# mark still shows as XFAIL with its numbers, and any other error still fails.
@pytest.mark.xfail(raises=AssertionError, strict=False,
                   reason="known issue: the live stop p95 is over its 0.25 s mark on Windows CI")
def test_a_live_run_stops_within_its_mark():
    from benchmarks.back_to_back import stream
    got = stream.live_stops("new", 20)
    print(got)
    assert got["state"] == "pass", got


def test_a_study_stops_within_its_mark():
    from benchmarks.back_to_back import stream
    got = stream.study_stops("new", 20)
    print(got)
    assert got["state"] == "pass", got
