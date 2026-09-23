"""Break the shipped examples on purpose, one fault at a time (VAL-01).

Every element, every wire and every signal link of both examples is deleted
in turn (118 faults) and Data Checks run on the result. A fault must never
get an all-clear unless it is on the reviewed list of harmless ones, and a
fault that stops the car must block the run with an error.

Which faults stop the car was measured once by running every broken model
to the end of its first case (a full run each, too slow for the suite);
redo that measurement when the examples change. Last measured for the
example rebuild (CON-02/CON-03, 2026-09): City Cycle and EPA city cycle.
"""
import copy

import pytest

from app.schemas import Project
from app.storage import load_example
from app.validation import validate_project

EXAMPLES = ("bev-car", "hybrid-car")

# Faults that do not change the model's behaviour: monitors only watch,
# negative terminals fall back to an implicit ground, and the Driver reads
# the vehicle's own speed when its Actual Speed input is not wired.
HARMLESS = {
    "bev-car": {"el-veh-monitor", "el-bms-monitor", "el-ground", "c-2", "c-18", "c-19",
                "db-7", "db-8", "db-9", "db-10", "db-11", "db-12", "db-13"},
    "hybrid-car": {"el-monitor", "el-ground", "c-14", "c-20", "c-22",
                   "db-14", "db-15", "db-16", "db-17", "db-18"},
}
# Faults that do matter but that Data Checks cannot tell from a deliberate
# model: deleting the auxiliary load is a legitimate simplification, and an
# unwired engine Enable means "always on", as in any conventional car.
KNOWN_GAPS = {
    "bev-car": {"el-consumer"},
    "hybrid-car": {"db-7", "el-aux"},
}
# Faults after which the car covers less than 5 % of the cycle distance.
STOPS_THE_CAR = {
    "bev-car": {"el-vehicle", "el-driver", "el-node-fl", "el-hvbus", "el-battery", "el-motor",
                "el-final-drive", "el-diff", "el-node-fr", "el-task", "el-wheel-fl",
                "el-wheel-fr", "c-1", "c-4", "c-5", "c-6", "c-7", "c-8", "c-10", "c-12",
                "db-1", "db-2"},
    "hybrid-car": {"el-vehicle", "el-driver", "el-task", "el-engine", "el-node", "el-gearbox",
                   "el-fd", "el-diff", "el-battery", "el-hvbus", "el-motor", "el-node-l",
                   "el-node-r", "el-wheel-l", "el-wheel-r", "c-1", "c-2", "c-3", "c-4", "c-5",
                   "c-6", "c-7", "c-8", "c-10", "c-12", "c-13", "c-15", "db-1", "db-4", "db-5"},
}
# ... of which these still leave a complete, commanded drive path: without its
# E-Motor the P2 hybrid's control script never starts the engine (it reads
# the input-shaft speed from the motor). Data Checks warn (that script input
# then reads 0) but cannot know the strategy.
STOPS_BUT_WARNING_ONLY = {"hybrid-car": {"el-motor"}}


def faults(name: str):
    """(fault id, broken project) for every single deletion."""
    base = load_example(name).model_dump()
    for s in base["systems"]:
        for el in s["elements"]:
            d = copy.deepcopy(base)
            for s2 in d["systems"]:
                s2["elements"] = [e for e in s2["elements"] if e["id"] != el["id"]]
                s2["connections"] = [c for c in s2["connections"]
                                     if el["id"] not in (c["sourceElementId"], c["targetElementId"])]
            d["dataBusConnections"] = [c for c in d["dataBusConnections"]
                                       if el["id"] not in (c["element1Id"], c["element2Id"])]
            yield el["id"], d
        for conn in s["connections"]:
            d = copy.deepcopy(base)
            for s2 in d["systems"]:
                s2["connections"] = [c for c in s2["connections"] if c["id"] != conn["id"]]
            yield conn["id"], d
    for link in base["dataBusConnections"]:
        d = copy.deepcopy(base)
        d["dataBusConnections"] = [c for c in d["dataBusConnections"] if c["id"] != link["id"]]
        yield link["id"], d


def worst(checks) -> str:
    levels = [c.level for c in checks]
    return next((lv for lv in ("error", "warning") if lv in levels), "info")


@pytest.fixture(scope="module")
def outcome():
    return {name: {fid: worst(validate_project(Project.model_validate(d)))
                   for fid, d in faults(name)}
            for name in EXAMPLES}


def test_corpus_is_complete(outcome):
    assert sum(len(v) for v in outcome.values()) == 118
    for name in EXAMPLES:
        listed = HARMLESS[name] | KNOWN_GAPS[name] | STOPS_THE_CAR[name]
        assert listed <= outcome[name].keys(), "a reviewed fault id no longer exists"


@pytest.mark.parametrize("name", EXAMPLES)
def test_no_consequential_fault_gets_an_all_clear(name, outcome):
    silent = [fid for fid, level in outcome[name].items()
              if level == "info" and fid not in HARMLESS[name] | KNOWN_GAPS[name]]
    assert silent == []


@pytest.mark.parametrize("name", EXAMPLES)
def test_faults_that_stop_the_car_block_the_run(name, outcome):
    expected_errors = STOPS_THE_CAR[name] - STOPS_BUT_WARNING_ONLY.get(name, set())
    assert [fid for fid in sorted(expected_errors) if outcome[name][fid] != "error"] == []
    for fid in STOPS_BUT_WARNING_ONLY.get(name, ()):
        assert outcome[name][fid] == "warning"


@pytest.mark.parametrize("name", EXAMPLES)
def test_harmless_faults_raise_no_alarm(name, outcome):
    assert [fid for fid in sorted(HARMLESS[name]) if outcome[name][fid] != "info"] == []


def test_check_coverage_is_at_least_90_percent(outcome):
    consequential = [(n, fid) for n in EXAMPLES for fid in outcome[n] if fid not in HARMLESS[n]]
    flagged = [(n, fid) for n, fid in consequential if outcome[n][fid] != "info"]
    assert len(consequential) == 95
    assert len(flagged) / len(consequential) >= 0.9  # 92 of 95 today


@pytest.mark.parametrize("name", EXAMPLES)
def test_the_examples_themselves_pass_cleanly(name):
    checks = validate_project(load_example(name))
    assert [(c.level, c.text) for c in checks if c.level != "info"] == []
