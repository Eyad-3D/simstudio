"""The solver step is checked before a run (ENG-14): a part too stiff for
the 10 ms step gets a smaller step, said in Data Checks and the run's
messages, and no tyre slips more than 0.1 on a gentle drive because of the
step."""
from __future__ import annotations

import pytest

from app.solver import simulate
from app.solver.network import build_model
from app.solver.runtime import MAX_SUBSTEP
from app.solver.stability import MIN_SUBSTEP, solver_step
from app.storage import load_example
from app.validation import validate_project
from tests.helpers import bev_axle

GENTLE = "0:0; 15:40; 25:40; 35:0; 40:0"


def axle(stiffness: float):
    p = bev_axle(profile=GENTLE)
    for e in p.systems[0].elements:
        if e.componentDefId == "propulsion.wheel":
            e.parameterOverrides["slip_stiffness"] = stiffness
    p.cases[0].duration, p.cases[0].timeStep = 40.0, 0.5
    return p


@pytest.mark.parametrize("pid, step", [("bev-car", 0.01), ("hybrid-car", 0.01),
                                       ("fs-electric", 0.005)])  # its tyres: Slip Stiffness 20
def test_the_examples_steps(pid, step):
    assert solver_step(build_model(load_example(pid))).step == pytest.approx(step)


@pytest.mark.parametrize("stiffness", [10, 30, 100, 300, 1000])
def test_no_numerical_slip_across_the_stiffness_range(stiffness):
    p = axle(stiffness)
    r = simulate(p, "case")
    slip = max(abs(v) for c in r.channels if c.portId == "sig_slip"
               for v in (c.min or []) + (c.max or []) if v is not None)
    choice = solver_step(build_model(p))
    reduced = choice.step < 0.01
    warned = any("too stiff" in m.text for m in r.messages)
    assert slip <= 0.1 or warned, (stiffness, slip)
    if stiffness >= 30:
        assert reduced
        assert any("Solver step reduced" in m.text for m in r.messages)
    if stiffness == 1000:  # beyond the smallest step: warned, not hidden
        assert choice.step == MIN_SUBSTEP and warned


def test_no_data_check_speaks_of_the_older_engine_s_step():
    """The app's engine has no fixed 10 ms step to shrink for a stiff part
    (app pass #21: the FS example's tyres at a Slip Stiffness of 20, inside
    the range the check itself named, were called too stiff for it)."""
    for p in (load_example("fs-electric"), axle(300)):
        texts = [c.text for c in validate_project(p)]
        assert not [t for t in texts if "10 ms step" in t or "too stiff" in t], texts


def test_a_clutch_neither_rings_nor_asks_for_a_smaller_step():
    """A clutch slips at its capacity and sticks in the step its slip would
    pass through zero, so a strong clutch on a light shaft cannot overshoot
    lock-up: the P2 Hybrid Car's (300 N·m on 0.06 kg·m², 65 rad/s of slip a
    step) runs at 10 ms with no note. Before, Data Checks noted that it
    could ring at the 10 ms step."""
    p = load_example("hybrid-car")
    assert not any("can ring" in c.text for c in validate_project(p))
    choice = solver_step(build_model(p))
    assert choice.step == MAX_SUBSTEP and not choice.warnings
