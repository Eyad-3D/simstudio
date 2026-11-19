"""Every run-summary number carries a stable key (AI-07): scripts, the CLI,
studies and AI tools read numbers by key, never by the label the app shows.
docs/spec/results.md lists the keys."""
from __future__ import annotations

import re

import pytest
from helpers import fs_car

from app.solver import simulate
from app.storage import load_example

KEY = re.compile(r"^(?:[^\s]+\.)?[a-z0-9_]+$")


def _cases():
    out = []
    for ex in ("bev-car", "hybrid-car", "fs-electric"):
        p = load_example(ex)
        out += [(ex, c.id) for c in p.cases if "live" not in c.id]
    return out


@pytest.mark.parametrize("example,case_id", _cases())
def test_every_summary_row_of_the_examples_has_a_unique_key(example, case_id):
    project = load_example(example)
    case = next(c for c in project.cases if c.id == case_id)
    if case.kind == "cycle":
        case.duration = min(case.duration, 120.0)  # the keys, not the numbers
    result = simulate(project, case_id)
    keys = [s.key for s in result.summary]
    assert all(keys), [s.label for s in result.summary if not s.key]
    assert len(set(keys)) == len(keys), keys
    for k in keys:
        assert KEY.match(k), k
    elements = {e.id for s in project.systems for e in s.elements}
    for k in keys:  # a part's figure is named after the part's id, not its label
        if "." in k:
            assert k.rpartition(".")[0] in elements, k


def test_a_part_figure_keeps_its_key_when_the_part_is_renamed():
    project = load_example("bev-car")
    before = {s.key: s.label for s in simulate(project, "case-city").summary}
    for s in project.systems:
        for e in s.elements:
            if e.id == "el-battery":
                e.label = "Pack"
    after = {s.key: s.label for s in simulate(project, "case-city").summary}
    assert set(before) == set(after)
    assert after["el-battery.final_soc_pct"] == "Pack — final SOC"


def test_a_lap_case_has_keys_for_its_rows():
    result = simulate(fs_car(laps=2), "case")
    keys = {s.key for s in result.summary}
    assert {"lap_time_s", "lap1_time_s", "total_time_s", "energy_per_lap_kwh",
            "time_limited_by_cornering_grip_s"} <= keys
