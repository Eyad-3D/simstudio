"""FMU parts on the new engine's path (step 2's item 5): an FMU block runs as
a sampled block of the new engine, its code in today's locked-down FMPy
worker (app/fmu/sandbox.py), stepped as an FMU host steps a co-simulation
FMU.

The guards here run LightSim's own test FMU (tests/fmu, compiled with the
system's C compiler) against FMPy's simulate_fmu at the same communication
step. The last test is the FMU line of step 2's acceptance
(benchmarks/back_to_back/acceptance.toml): every co-simulation Reference FMU
v0.0.41 inside LightSim's new path against fmusim v0.7.0 run alone and FMPy
0.3.32, at every communication point, to the last digit. It needs
LIGHTSIM_REFERENCE_FMUS (the unpacked release) and LIGHTSIM_FMUSIM (fmusim's
executable), which the Windows engine job of CI sets.
"""
from __future__ import annotations

import csv
import math
import multiprocessing
import os
import struct
import subprocess
import time
from pathlib import Path

import pytest
from fmu.build import build as build_test_fmu
from fmu.build import compiler
from helpers import dbc, el, project, sig_port

from app.fmu import fmpy_available, info, store
from app.schemas import ElementInstance, Project, SimCase, SystemNode
from app.solver import new_engine, simulate

try:
    import lightsim_engine  # noqa: F401
    MISSING = None
except ImportError as e:
    MISSING = f"the compiled module lightsim_engine is not installed ({e})"
REQUIRED = os.environ.get("LIGHTSIM_REQUIRE_NEW_ENGINE", "").strip() == "1"
needs_engine = pytest.mark.skipif(MISSING is not None and not REQUIRED, reason=MISSING or "")


@pytest.fixture(autouse=True)
def new_path(tmp_path, monkeypatch):
    """The new engine's path, strict (no fall-back to today's engine);
    imported FMUs and the allowed list in a temporary user folder."""
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path / "user"))


@pytest.fixture(scope="session")
def test_fmu(tmp_path_factory) -> Path:
    if compiler() is None or not fmpy_available():
        pytest.skip("needs a C compiler and FMPy (the FMU pack)")
    return build_test_fmu(tmp_path_factory.mktemp("fmu-build"))


def _import(path: Path, allow: bool = True) -> dict:
    sha, kept = store.store_bytes(path.read_bytes())
    if allow:
        store.allow(sha, path.name)
    return {"fmu_path": str(kept), "fmu_sha256": sha, "fmu_name": path.name}


def _lag_project(params: dict, u: float, h: float, stop: float) -> Project:
    """A Constant into the test FMU's input u; its outputs y and t_fmu as pins;
    the case's output step the FMU's Communication Step h."""
    ports = [sig_port("u", "input"), sig_port("y", "output"), sig_port("t_fmu", "output")]
    fmu = ElementInstance(id="fmu", componentDefId="signal.fmu", label="Lag",
                          position={"x": 0, "y": 0},
                          parameterOverrides=params | {"sample_time_s": h}, dynamicPorts=ports)
    return project([el("src", "signal.constant", "Source", value=u), fmu], [],
                   [dbc(1, "src", "sig_out", "fmu", "u")], duration=stop, time_step=h)


def _series(result, el_id: str, port_id: str) -> list[tuple[float, float]]:
    ch = next(c for c in result.channels if c.elementId == el_id and c.portId == port_id)
    return [(p["t"], p["value"]) for p in ch.timeSeries]


def _bits(x: float) -> bytes:
    return struct.pack("<d", float(x))


@needs_engine
def test_the_test_fmu_on_the_new_path_is_fmpys_run_bit_for_bit(test_fmu):
    """Guard: LightSim's test FMU (a first-order lag, its input from a
    Constant) on the new path gives, at every communication point k h, the
    outputs FMPy's simulate_fmu records there for the same input, to the
    bit: the same calls (the inputs set at a point, the step to the next,
    the outputs read there) in today's worker."""
    import numpy as np
    from fmpy import simulate_fmu

    h, stop, u = 0.01, 2.0, 2.0
    params = _import(test_fmu) | {"start:k": 3.0}
    before = set(multiprocessing.active_children())
    result = simulate(_lag_project(params, u, h, stop), "case")
    assert result.status == "success", [m.text for m in result.messages]
    assert result.messages[0].text.startswith("Run on the new engine")
    signal = np.array([(0.0, u), (stop, u)], dtype=[("time", np.float64), ("u", np.float64)])
    ref = simulate_fmu(str(test_fmu), fmi_type="CoSimulation", start_time=0.0, stop_time=stop,
                       output_interval=h, input=signal, start_values={"k": 3.0},
                       output=["y", "t_fmu"])
    for name in ("y", "t_fmu"):
        got = _series(result, "fmu", name)
        assert len(got) == len(ref) == 201
        for (t, v), row in zip(got, ref):
            assert _bits(t) == _bits(row["time"]), (name, t, row["time"])
            assert _bits(v) == _bits(row[name]), (name, t, v, row[name])
    deadline = time.monotonic() + 3
    while set(multiprocessing.active_children()) - before and time.monotonic() < deadline:
        time.sleep(0.05)
    assert not set(multiprocessing.active_children()) - before, "an FMU worker was left running"


@needs_engine
def test_an_fmu_with_a_communication_step_holds_its_outputs_between_its_points(test_fmu):
    """Guard: with a Communication Step of 0.25 s on a 0.05 s output grid
    the FMU's outputs change only at multiples of 0.25 s and hold in
    between; its own time at each point is that point's (never ahead)."""
    params = _import(test_fmu)
    proj = _lag_project(params, 1.0, 0.25, 2.0)
    proj.cases[0].timeStep = 0.05
    result = simulate(proj, "case")
    assert result.status == "success", [m.text for m in result.messages]
    for t, v in _series(result, "fmu", "t_fmu"):
        k = int(t / 0.25 + 1e-9)
        assert v == k * 0.25, (t, v)


@needs_engine
def test_an_agents_fmu_runs_in_a_strict_worker_or_is_refused_before_any_starts(test_fmu, monkeypatch):
    """An agent's run (STRICT) on the new path starts its FMU's worker
    OS-confined; where that cannot be set up it is refused
    [sandbox_unavailable] before any FMU worker starts. A person's run's
    worker is the ordinary one."""
    from app.fmu import sandbox as fmu_sandbox
    from app.solver.sandbox import SANDBOX_UNAVAILABLE, STRICT, why_no_strict_sandbox

    built = []

    class Recorded(fmu_sandbox.FmuSandbox):
        def __init__(self, spec, *a, **k):
            built.append(k.get("strict", False))
            super().__init__(spec, *a, **k)
    monkeypatch.setattr(fmu_sandbox, "FmuSandbox", Recorded)
    proj = _lag_project(_import(test_fmu), 1.0, 0.1, 1.0)

    def agent_run():
        token = STRICT.set(True)
        try:
            return simulate(proj, "case")
        finally:
            STRICT.reset(token)
    if why_no_strict_sandbox() is None:  # (Linux with Landlock, Windows with the AppContainer)
        result = agent_run()
        assert result.status == "success", [m.text for m in result.messages]
        assert result.messages[0].text.startswith("Run on the new engine")
        assert built == [True]
    built.clear()
    monkeypatch.setattr(new_engine, "why_no_strict_sandbox",
                        lambda *a: f"no sandbox here [{SANDBOX_UNAVAILABLE}]")
    result = agent_run()
    assert result.status == "failed"
    assert SANDBOX_UNAVAILABLE in " ".join(m.text for m in result.messages)
    assert result.messages[0].text.startswith("Run on the new engine")
    assert built == [], "no FMU worker started"
    result = simulate(proj, "case")  # a person's run
    assert result.status == "success", [m.text for m in result.messages]
    assert built == [False]


@needs_engine
def test_an_fmu_nobody_allowed_is_refused_in_todays_words(test_fmu, monkeypatch):
    """Guard: an FMU not allowed on this computer does not run on the new
    path either, with today's engine's message."""
    proj = _lag_project(_import(test_fmu, allow=False), 1.0, 0.1, 1.0)
    new = simulate(proj, "case")
    monkeypatch.delenv(new_engine.ENV_VAR)
    today = simulate(proj, "case")
    assert new.status == today.status == "failed"
    assert new.messages[0].text.startswith("Run on the new engine")
    assert [m.text for m in new.messages[1:]] == [m.text for m in today.messages]
    assert "has not been allowed" in new.messages[1].text


@needs_engine
@pytest.mark.parametrize("mode,what", [(1, "crashed"), (5, "reported an error")])
def test_a_misbehaving_fmu_fails_the_run_with_the_workers_words(test_fmu, mode, what):
    """Guard: an FMU that crashes or fails a step at 0.5 s fails the new
    path's run (strict: its code) with the worker's message, and leaves no
    worker running."""
    params = _import(test_fmu) | {"start:mode": mode}
    before = set(multiprocessing.active_children())
    result = simulate(_lag_project(params, 1.0, 0.01, 1.0), "case")
    assert result.status == "failed"
    text = " ".join(m.text for m in result.messages)
    assert "[NEW-FAILED]" in text and what in text and "Lag" in text, text
    assert not set(multiprocessing.active_children()) - before


@needs_engine
def test_an_fmu_fed_a_changing_signal_steps_as_fmpy_steps_it(test_fmu):
    """Guard: the test FMU fed the BEV's speed on its city cycle (a changing
    input, Communication Step 1 s) gives at each communication point what
    FMPy's simulate_fmu gives for the speed the run recorded at those
    points: each step taken with the inputs set at its start, as FMPy and
    fmusim take it (the speed at its end would put the lag a step ahead)."""
    import numpy as np
    from fmpy import simulate_fmu

    from app.storage import load_example

    h = 1.0
    proj = load_example("bev-car")
    next(c for c in proj.cases if c.id == "case-city").duration = 60.0
    ports = [sig_port("u", "input"), sig_port("y", "output"), sig_port("t_fmu", "output")]
    proj.systems[0].elements.append(ElementInstance(
        id="fmu", componentDefId="signal.fmu", label="Lag", position={"x": 0, "y": 0},
        parameterOverrides=_import(test_fmu) | {"sample_time_s": h, "start:k": 1.0, "start:tau": 2.0},
        dynamicPorts=ports))
    proj.dataBusConnections.append(dbc(99, "el-vehicle", "sig_speed", "fmu", "u"))
    result = simulate(proj, "case-city")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    speed = dict(_series(result, "el-vehicle", "sig_speed"))
    ours = dict(_series(result, "fmu", "y"))
    ticks = sorted(t for t in ours if abs(t / h - round(t / h)) < 1e-9)
    assert len(ticks) == 61 and len({speed[t] for t in ticks}) > 10  # (it changes)
    signal = np.array([(t, speed[t]) for t in ticks], dtype=[("time", np.float64), ("u", np.float64)])
    ref = simulate_fmu(str(test_fmu), fmi_type="CoSimulation", start_time=0.0, stop_time=ticks[-1],
                       output_interval=h, input=signal, start_values={"k": 1.0, "tau": 2.0},
                       output=["y"])
    assert [float(row["time"]) for row in ref] == ticks
    for row in ref:
        t, want = float(row["time"]), float(row["y"])
        assert abs(ours[t] - want) <= 1e-12 * max(1.0, abs(want)), (t, ours[t], want)


def _with_fmu(project: Project, params: dict, h: float) -> Project:
    """An example with the test FMU added, fed by a Constant, its outputs
    unwired (as the harness's probe/bev-fmu)."""
    ports = [sig_port("u", "input"), sig_port("y", "output"), sig_port("t_fmu", "output")]
    project.systems[0].elements += [
        el("fmu-src", "signal.constant", "FMU input", value=1.0),
        ElementInstance(id="fmu", componentDefId="signal.fmu", label="Lag", position={"x": 0, "y": 0},
                        parameterOverrides=params | {"sample_time_s": h}, dynamicPorts=ports)]
    project.dataBusConnections.append(dbc(99, "fmu-src", "sig_out", "fmu", "u"))
    return project


@needs_engine
def test_an_acceleration_tests_second_run_starts_the_fmu_again(test_fmu):
    """Guard: the new engine runs an acceleration test twice (the second
    run to the finish); the FMU starts again from its start for the second,
    so its own time at each recorded point is that point's, never the first
    run's end carried on."""
    from app.storage import load_example

    proj = _with_fmu(load_example("fs-electric"), _import(test_fmu), 0.01)
    result = simulate(proj, "case-accel-75m")
    assert result.status != "failed", [m.text for m in result.messages]
    got = _series(result, "fmu", "t_fmu")
    assert len(got) > 10
    for t, v in got:
        assert v == pytest.approx(math.floor(t / 0.01 + 1e-6) * 0.01, abs=1e-9), (t, v)


@needs_engine
@pytest.mark.parametrize("key,words", [("start:k", "FMU start values take effect on the next run"),
                                       ("sample_time_s", "Communication Step takes effect on the next run")])
def test_a_live_edit_of_an_fmu_is_for_the_next_run(test_fmu, key, words):
    """Guard: the FMU's worker read its start values (and the engine its
    Communication Step) at the start, so a live edit of one is not made:
    the result lists it with its note [NEW-LIVE-NEXT-RUN], and the run's
    outputs are a plain run's."""
    proj = _lag_project(_import(test_fmu), 2.0, 0.01, 1.0)
    plain = simulate(proj, "case")
    sent = []

    def control():
        if not sent:
            sent.append(1)
            return [{"type": "set_param", "elementId": "fmu", "key": key, "value": 0.05}]
        return []
    live = simulate(proj, "case", emit=lambda m: None, control=control)
    assert live.status == "success", [m.text for m in live.messages]
    (edit,) = live.liveEdits
    assert edit.key == key and words in (edit.note or "") and "[NEW-LIVE-NEXT-RUN]" in edit.note
    assert _series(live, "fmu", "y") == _series(plain, "fmu", "y")


# ---- the FMU line: the Reference FMUs v0.0.41 against fmusim and FMPy ---------

REFERENCE = os.environ.get("LIGHTSIM_REFERENCE_FMUS")
FMUSIM = os.environ.get("LIGHTSIM_FMUSIM")
# the co-simulation FMUs of v0.0.41 and each one's (communication step,
# stop time): its default experiment's, else T / 500 (fmusim's default);
# Roberts to 1 s (acceptance.toml, the FMU line's change of 31 October)
CASES = {
    "2.0/BouncingBall": (1e-2, 3.0), "2.0/Dahlquist": (0.1, 10.0), "2.0/Feedthrough": (2.0 / 500, 2.0),
    "2.0/Resource": (1.0 / 500, 1.0), "2.0/Stair": (0.2, 10.0), "2.0/VanDerPol": (1e-2, 20.0),
    "3.0/BouncingBall": (1e-2, 3.0), "3.0/Dahlquist": (0.1, 10.0), "3.0/Feedthrough": (2.0 / 500, 2.0),
    "3.0/Resource": (1.0 / 500, 1.0), "3.0/Roberts": (1.0 / 500, 1.0), "3.0/Stair": (0.2, 10.0),
    "3.0/StateSpace": (10.0 / 500, 10.0), "3.0/VanDerPol": (1e-2, 20.0),
}


def reference_project(path: Path, h: float, stop: float) -> tuple[Project, list[str]]:
    """The FMU in a one-block project: every output LightSim can pin as an
    output pin (ids as the app makes them), no input pin, its Communication
    Step h, the case from 0 to stop at the output step h."""
    d = info.describe(path)
    assert d["ok"], d["problems"]
    names = [v["name"] for v in d["variables"] if v["pin"] == "output" and v["causality"] == "output"]
    ports = [sig_port("fmu_" + "".join(c if c.isalnum() else "_" for c in n).strip("_"), "output")
             for n in names]
    for p, n in zip(ports, names):
        p.name = n
    params = _import(path) | {"sample_time_s": h}
    fmu = ElementInstance(id="el-fmu", componentDefId="signal.fmu", label=path.stem,
                          position={"x": 0, "y": 0}, parameterOverrides=params, dynamicPorts=ports)
    proj = Project(id="ref", name=path.stem,
                   systems=[SystemNode(id="root", name="Main", parentId=None, elements=[fmu],
                                       connections=[])],
                   dataBusConnections=[],
                   cases=[SimCase(id="case", name="Case", duration=stop, timeStep=h)])
    return proj, names


def compare_reference(path: Path, h: float, stop: float, fmusim: str, out_dir: Path) -> dict:
    """One case of the FMU line: the pinned outputs at every communication
    point on the three hosts, and every place they differ."""
    from fmpy import simulate_fmu

    n = round(stop / h)
    assert n * h == stop, (path, h, stop)
    proj, names = reference_project(path, h, stop)
    rec = {"fmu": f"{path.parent.name}/{path.stem}", "h": h, "stop": stop, "outputs": names,
           "points": n + 1, "differences": []}
    if not names:
        rec["state"] = "nothing to compare"
        return rec
    result = simulate(proj, "case")
    rec["status"] = result.status
    if result.status == "failed":
        rec["state"] = "fail"
        rec["why"] = " ".join(m.text for m in result.messages)
        return rec
    ours = {p.name: _series(result, "el-fmu", p.id) for p in proj.systems[0].elements[0].dynamicPorts}
    fmpy = simulate_fmu(str(path), fmi_type="CoSimulation", start_time=0.0, stop_time=stop,
                        output_interval=h, output=names)
    csv_path = out_dir / f"{path.parent.name}-{path.stem}.csv"
    cmd = [fmusim, "simulate", "--interface-type", "cs", "--start-time", "0", "--stop-time",
           repr(stop), "--output-interval", repr(h), "--output-file", str(csv_path)]
    for name in names:
        cmd += ["--output-variable", name]
    subprocess.run([*cmd, str(path)], check=True, capture_output=True, timeout=600)
    with csv_path.open(newline="") as f:
        rows = list(csv.DictReader(f))
    for host, table in (("FMPy", [{"time": r["time"], **{k: r[k] for k in names}} for r in fmpy]),
                        ("fmusim", rows)):
        if len(table) != n + 1:
            rec["differences"].append({"host": host, "what": "points", "ours": n + 1, "theirs": len(table)})
            continue
        for k, row in enumerate(table):
            t_theirs = float(row["time"])
            for name in names:
                t, v = ours[name][k]
                raw = row[name]
                theirs = {"true": 1.0, "false": 0.0}.get(str(raw).lower(), None)
                theirs = float(raw) if theirs is None else theirs
                if _bits(t) != _bits(t_theirs) or _bits(v) != _bits(theirs):
                    rec["differences"].append({"host": host, "output": name, "k": k, "t": t,
                                               "t_theirs": t_theirs, "ours": v, "theirs": theirs})
    rec["state"] = "pass" if not rec["differences"] else "fail"
    return rec


@needs_engine
@pytest.mark.skipif(not (REFERENCE and FMUSIM),
                    reason="LIGHTSIM_REFERENCE_FMUS and LIGHTSIM_FMUSIM not set (Windows CI sets them)")
@pytest.mark.parametrize("which", sorted(CASES))
def test_reference_fmus_on_the_new_path_equal_fmusim_and_fmpy(which, tmp_path):
    """The FMU line (acceptance.toml): each co-simulation Reference FMU
    v0.0.41 in a one-block project on LightSim's new path equals fmusim run
    alone and FMPy at the same communication step, every pinned output at
    every point, to the last digit."""
    path = Path(REFERENCE) / f"{which}.fmu"
    h, stop = CASES[which]
    rec = compare_reference(path, h, stop, FMUSIM, tmp_path)
    if rec["state"] == "nothing to compare":
        pytest.skip(f"{which}: no output LightSim can pin (its outputs are arrays)")
    assert rec["state"] == "pass", (rec.get("why"), rec["differences"][:5])
