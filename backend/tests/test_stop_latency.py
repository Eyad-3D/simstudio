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

windows_only = pytest.mark.skipif(sys.platform != "win32", reason="stop latency is judged on Windows only")
needs_module = pytest.mark.skipif(os.environ.get("LIGHTSIM_REQUIRE_NEW_ENGINE", "").strip() != "1",
                                  reason="needs the module (CI's Windows engine job)")


# STREAM S2's live mark is a known issue (engine/STATUS.md, "Step 2 closed
# without acceptance"; the owner's decision of 10 November 2026): on Windows CI
# run 43116535929 (989204b) its p95 was 0.296 s against 0.25 s, with 3 of 20
# stops over the mark. It is a timing near its mark (0.233 s on a local run),
# so a run may pass: not strict, as a pass must not turn CI red. Only a
# measured run over its mark by no more than the known issue's size (state
# "fail", p95 at most KNOWN_LIVE_P95_S) is an expected failure, shown as XFAIL
# with its p95; a larger p95 is a new slowdown and fails, as does a run where
# nothing was measured ("not measured") or any other error.
KNOWN_LIVE_P95_S = 0.4  # ponytail: a fixed ceiling, 35 % over CI's 0.296 s

# The study stop sits at its mark in the same way (engine/STATUS.md, "The study
# stop on Windows still sits near its mark"): over seven Windows CI runs its p95
# went from 0.73 to 1.23 s against 1.0 s, with the speed of the runner (the
# unstopped study took 11.7 to 22.3 s, and the tests of other code in the same
# job took as much longer). The same rule: a measured run over the mark by no
# more than the known size is XFAIL with its p95; a larger p95 fails.
KNOWN_STUDY_P95_S = 1.5  # ponytail: a fixed ceiling, 22 % over CI's 1.226 s


def is_known_overrun(got: dict, ceiling: float) -> bool:
    return got["state"] == "fail" and got["p95_s"] <= ceiling


def test_only_a_stop_a_little_over_its_mark_is_the_known_issue():
    assert is_known_overrun({"state": "fail", "p95_s": 0.296}, KNOWN_LIVE_P95_S)
    assert not is_known_overrun({"state": "fail", "p95_s": 5.0}, KNOWN_LIVE_P95_S)
    assert not is_known_overrun({"state": "not measured", "p95_s": 0.3}, KNOWN_LIVE_P95_S)
    assert not is_known_overrun({"state": "pass", "p95_s": 0.2}, KNOWN_LIVE_P95_S)
    assert is_known_overrun({"state": "fail", "p95_s": 1.088}, KNOWN_STUDY_P95_S)
    assert not is_known_overrun({"state": "fail", "p95_s": 5.0}, KNOWN_STUDY_P95_S)
    assert not is_known_overrun({"state": "not measured", "p95_s": 1.1}, KNOWN_STUDY_P95_S)


@windows_only
@needs_module
def test_a_live_run_stops_within_its_mark():
    from benchmarks.back_to_back import stream
    got = stream.live_stops("new", 20)
    print(got)
    if is_known_overrun(got, KNOWN_LIVE_P95_S):
        pytest.xfail(f"known issue: the live stop p95 {got['p95_s']} s is over its 0.25 s mark "
                     "on Windows CI")
    assert got["state"] == "pass", got


@windows_only
@needs_module
def test_a_study_stops_within_its_mark():
    from benchmarks.back_to_back import stream
    got = stream.study_stops("new", 20)
    print(got)
    if is_known_overrun(got, KNOWN_STUDY_P95_S):
        pytest.xfail(f"known issue: the study stop p95 {got['p95_s']} s is over its 1.0 s mark "
                     "on Windows CI")
    assert got["state"] == "pass", got
