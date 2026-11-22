"""Junctions with as many terminals as a part lists (``terminals``), on both
engines: an Electric Node with seven terminals carries what two chained
five-terminal nodes carry, a Mechanical Node with six flanges turns as two
chained four-flange nodes, extra terminals with nothing on them change no
result, and the data checks refuse a terminal list or a wire that cannot be.
"""
from __future__ import annotations

import copy

import pytest

from app.schemas import Connection, ElementInstance
from app.solver import new_engine, simulate
from app.storage import load_example
from app.validation import validate_project

try:
    import lightsim_engine  # noqa: F401
    HAVE_NEW = True
except ImportError:
    HAVE_NEW = False

ENGINES = ["today", pytest.param("new", marks=pytest.mark.skipif(
    not HAVE_NEW, reason="the compiled module lightsim_engine is not installed"))]


# The same to: today's engine (one bus or shaft, whatever the nodes), and the
# new engine's tolerance (rtol 1e-6: the two models' equations differ, so do
# their steps)
SAME = {"today": 1e-9, "new": 1e-5}


def _run(project, case_id, engine, monkeypatch):
    with monkeypatch.context() as m:
        m.setenv(new_engine.ENV_VAR, engine)
        return simulate(project, case_id)


def _element(proj, el_id):
    return next(e for e in proj.systems[0].elements if e.id == el_id)


def _add(proj, el_id, kind, label, at=(0.0, 0.0), **extra):
    proj.systems[0].elements.append(ElementInstance(
        id=el_id, componentDefId=kind, label=label, position={"x": at[0], "y": at[1]}, **extra))


def _wire(proj, wid, a, pa, b, pb):
    proj.systems[0].connections.append(Connection(
        id=wid, sourceElementId=a, sourcePortId=pa, targetElementId=b, targetPortId=pb))


def _rewire(proj, wid, end, el_id, port):
    c = next(c for c in proj.systems[0].connections if c.id == wid)
    setattr(c, f"{end}ElementId", el_id)
    setattr(c, f"{end}PortId", port)


def _same(a, b, rel):
    """The two runs give the same figures, part energies and channels (to
    `rel`), for every part both have; how many numbers were compared."""
    assert a.status == b.status, (a.messages, b.messages)
    n = 0
    fa = {s.key: s.value for s in a.summary if s.key}
    fb = {s.key: s.value for s in b.summary if s.key}
    for k in fa.keys() & fb.keys():
        assert fa[k] == pytest.approx(fb[k], rel=rel, abs=1e-9), k
        n += 1
    pb = {(p.elementId, p.label): p for p in b.partEnergy}
    for p in a.partEnergy:
        q = pb.get((p.elementId, p.label))
        if q is None:
            continue
        for f in ("energyIn", "energyOut", "losses", "stored"):
            assert getattr(p, f) == pytest.approx(getattr(q, f), rel=rel, abs=1e-6), (p.label, f)
            n += 1
    cb = {(c.elementId, c.portId): c for c in b.channels}
    for c in a.channels:
        d = cb.get((c.elementId, c.portId))
        if d is None:
            continue
        xs = [p["value"] for p in c.timeSeries]
        ys = [p["value"] for p in d.timeSeries]
        assert len(xs) == len(ys), c.label
        # (a channel that stays near nothing, a brake's few milliwatts, is
        # judged against a thousandth of its unit, not against its own noise)
        scale = max((abs(x) for x in xs if x is not None), default=0.0, key=float)
        scale = max(scale, 1e-3)
        for x, y in zip(xs, ys):
            assert (x is None) == (y is None), c.label
            if x is not None:
                assert abs(x - y) <= rel * scale, (c.elementId, c.portId, x, y)
        n += 1
    return n


def _seven(base):
    """The electric car's HV Bus with seven terminals: its four wires (t1 to
    t4), t5 free, and two more loads on t6 and t7."""
    p = copy.deepcopy(base)
    _element(p, "el-hvbus").terminals = [f"t{k}" for k in range(1, 8)]
    for k, kw in ((1, 0.8), (2, 1.2)):
        _add(p, f"el-x{k}", "electric.constant_drive", f"Extra load {k}",
             parameterOverrides={"power_kW": kw})
        _wire(p, f"c-x{k}", "el-hvbus", f"t{5 + k}", f"el-x{k}", "pos")
        _wire(p, f"c-xr{k}", f"el-x{k}", "neg", "el-ground", "t2")
    return p


def _chained(base):
    """The same two loads on a second, five-terminal node on the HV Bus's t5."""
    p = copy.deepcopy(base)
    _add(p, "el-bus2", "electric.node", "Bus 2")
    _wire(p, "c-chain", "el-hvbus", "t5", "el-bus2", "t1")
    for k, kw in ((1, 0.8), (2, 1.2)):
        _add(p, f"el-x{k}", "electric.constant_drive", f"Extra load {k}",
             parameterOverrides={"power_kW": kw})
        _wire(p, f"c-x{k}", "el-bus2", f"t{1 + k}", f"el-x{k}", "pos")
        _wire(p, f"c-xr{k}", f"el-x{k}", "neg", "el-ground", "t2")
    return p


@pytest.mark.parametrize("engine", ENGINES)
def test_seven_terminals_carry_what_two_chained_nodes_carry(engine, monkeypatch):
    base = load_example("bev-car")
    one = _run(_seven(base), "case-city", engine, monkeypatch)
    two = _run(_chained(base), "case-city", engine, monkeypatch)
    assert one.status == "success", one.messages
    assert _same(one, two, SAME[engine]) > 30
    # the loads are on: each draws its power
    x2 = next(c for c in one.channels if (c.elementId, c.portId) == ("el-x2", "sig_power"))
    assert max(p["value"] for p in x2.timeSeries[1:]) == pytest.approx(1.2, rel=1e-9)


@pytest.mark.parametrize("engine", ENGINES)
def test_terminals_with_nothing_on_them_change_no_result(engine, monkeypatch):
    """The electric car with seven terminals on its HV Bus and six flanges on
    a front corner, nothing more wired, consumes what it did."""
    base = load_example("bev-car")
    more = copy.deepcopy(base)
    _element(more, "el-hvbus").terminals = [f"t{k}" for k in range(1, 8)]
    _element(more, "el-node-fl").terminals = [f"f{k}" for k in range(1, 7)]
    a = _run(base, "case-city", engine, monkeypatch)
    b = _run(more, "case-city", engine, monkeypatch)
    assert _same(a, b, SAME[engine]) > 30
    # and the library's own list is the library's node
    same = copy.deepcopy(base)
    _element(same, "el-hvbus").terminals = ["t1", "t2", "t3", "t4", "t5"]
    assert _same(a, _run(same, "case-city", engine, monkeypatch), 0.0) > 30


@pytest.mark.parametrize("engine", ENGINES)
def test_six_flanges_turn_as_two_chained_nodes_turn(engine, monkeypatch):
    """The front-left corner: its brake and wheel on two added flanges (f5,
    f6) of its node, or on a second, four-flange node on its free f4; and as
    the car as shipped."""
    base = load_example("bev-car")
    six = copy.deepcopy(base)
    _element(six, "el-node-fl").terminals = [f"f{k}" for k in range(1, 7)]
    _rewire(six, "c-9", "source", "el-node-fl", "f5")   # node → brake
    _rewire(six, "c-10", "source", "el-node-fl", "f6")  # node → wheel
    two = copy.deepcopy(base)
    _add(two, "el-node2", "mech.node", "Node 2")
    _wire(two, "c-chain", "el-node-fl", "f4", "el-node2", "f1")
    _rewire(two, "c-9", "source", "el-node2", "f2")
    _rewire(two, "c-10", "source", "el-node2", "f3")
    a = _run(six, "case-city", engine, monkeypatch)
    b = _run(two, "case-city", engine, monkeypatch)
    assert a.status == "success", a.messages
    assert _same(a, b, SAME[engine]) > 30
    assert _same(a, _run(base, "case-city", engine, monkeypatch), SAME[engine]) > 30


def _texts(project):
    return [c.text for c in validate_project(project) if c.level == "error"]


def test_the_data_checks_refuse_a_terminal_list_or_a_wire_that_cannot_be():
    base = load_example("bev-car")
    for bad in (["t1"], ["t1", "t1", "t2"], ["t1", "x2"], ["t1", "f2"]):
        p = copy.deepcopy(base)
        _element(p, "el-hvbus").terminals = bad
        assert any("needs at least two terminals, each named once (t1, t2, …)" in t
                   for t in _texts(p)), bad
    p = copy.deepcopy(base)
    _element(p, "el-battery").terminals = ["pos", "neg"]
    assert any("only an Electric Node or a Mechanical Node can" in t for t in _texts(p))
    # the HV Bus without its t3, which wires the motor
    p = copy.deepcopy(base)
    _element(p, "el-hvbus").terminals = ["t1", "t2", "t4", "t5"]
    assert any("missing port 't3' on 'HV Bus'" in t for t in _texts(p))
    # its own list, every wire on a terminal it has: no error about them
    assert not [t for t in _texts(_seven(base)) if "terminal" in t or "port" in t]
