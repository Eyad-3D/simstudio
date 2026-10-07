"""FMU parts (STD-01): reading an FMU, keeping it, allowing it, running it in
its own locked-down process, and failing safely when it misbehaves.

Most tests use LightSimTest, a tiny FMU compiled from tests/fmu/ (they skip
without a C compiler or without FMPy, the FMU pack). The last test runs every
Co-Simulation FMU of the Modelica Association's Reference FMUs (BSD-2-Clause)
against FMPy's own simulate_fmu when LIGHTSIM_REFERENCE_FMUS points at the
unpacked release (CI downloads it).
"""
from __future__ import annotations

import io
import math
import multiprocessing
import os
import sys
import time
import zipfile
from pathlib import Path
from types import SimpleNamespace

import pytest
from fastapi.testclient import TestClient
from fmu.build import build as build_test_fmu
from fmu.build import compiler
from helpers import dbc, el, project, sig_port

from app.fmu import fmpy_available, info, store
from app.fmu.block import spec_for
from app.fmu.sandbox import FmuError, FmuSandbox
from app.schemas import ElementInstance
from app.solver import simulate
from app.validation import validate_project

needs_fmpy = pytest.mark.skipif(not fmpy_available(), reason="FMU pack (FMPy) not installed")
needs_cc = pytest.mark.skipif(compiler() is None, reason="no C compiler to build the test FMU")


@pytest.fixture(autouse=True)
def user_folder(tmp_path, monkeypatch):
    """Imported FMUs and the allowed list go to a temporary user folder."""
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path / "user"))
    return tmp_path / "user"


@pytest.fixture(scope="session")
def test_fmu(tmp_path_factory) -> Path:
    if compiler() is None or not fmpy_available():
        pytest.skip("needs a C compiler and FMPy")
    return build_test_fmu(tmp_path_factory.mktemp("fmu-build"))


def _import(path: Path, allow: bool = True) -> dict:
    sha, kept = store.store_bytes(path.read_bytes())
    if allow:
        store.allow(sha, path.name)
    return {"fmu_path": str(kept), "fmu_sha256": sha, "fmu_name": path.name}


def _fmu_project(params: dict, *, u: float = 1.0, duration: float = 2.0,
                 outputs=("y", "t_fmu", "escaped")):
    """A Constant feeding the FMU's input u; the FMU's chosen outputs as pins."""
    ports = [sig_port("u", "input")] + [sig_port(o, "output") for o in outputs]
    fmu = ElementInstance(id="fmu", componentDefId="signal.fmu", label="Lag",
                          position={"x": 0, "y": 0}, parameterOverrides=params,
                          dynamicPorts=ports)
    return project([el("src", "signal.constant", "Source", value=u), fmu], [],
                   [dbc(1, "src", "sig_out", "fmu", "u")],
                   duration=duration, time_step=0.1)


def _series(result, el_id: str, port_id: str) -> list[tuple[float, float]]:
    for ch in result.channels:
        if ch.elementId == el_id and ch.portId == port_id:
            return [(p["t"], p["value"]) for p in ch.timeSeries]
    raise AssertionError(f"no channel {el_id}.{port_id}: "
                         f"{[(c.elementId, c.portId) for c in result.channels][:20]}")


def _no_new_workers(before: set) -> None:
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        if not (set(multiprocessing.active_children()) - before):
            return
        time.sleep(0.05)
    raise AssertionError("an FMU worker was left running")


# ---- platform badge (no FMPy needed) --------------------------------------------

BADGES = [
    # (fmi version, binary folders, sources?, this OS) -> badge
    ("2.0", ["win64"], False, "Windows", "runs here"),
    ("2.0", ["win64", "win32"], False, "Linux", "Windows only"),
    ("2.0", ["linux64"], False, "Linux", "runs here"),
    ("2.0", ["linux64", "darwin64"], True, "Windows", "Linux and macOS only"),
    ("2.0", [], True, "Linux", "source only"),
    ("2.0", [], False, "Linux", "no code"),
    ("3.0", ["x86_64-windows"], False, "Windows", "runs here"),
    ("3.0", ["x86_64-linux", "aarch64-linux"], False, "Windows", "Linux only"),
    ("3.0", ["aarch64-darwin", "x86_64-darwin"], False, "macOS", "runs here"),
    ("3.0", ["x86_64-windows", "x86_64-linux", "x86_64-darwin"], True, "Linux", "runs here"),
]


@pytest.mark.parametrize("version,folders,sources,os_name,badge", BADGES)
def test_platform_badge(monkeypatch, version, folders, sources, os_name, badge):
    """100 % of FMUs get the right badge: every layout the FMI standard
    allows, on each operating system."""
    here = {"Windows": ("win64", "x86_64-windows"), "Linux": ("linux64", "x86_64-linux"),
            "macOS": ("darwin64", "aarch64-darwin")}[os_name]
    monkeypatch.setattr(info, "this_os", lambda: os_name)
    monkeypatch.setattr(info, "this_folders",
                        lambda v: [here[1]] if v.startswith("3") else [here[0], "aarch64-darwin"]
                        if os_name == "macOS" else [here[0]])
    ext = {"win": ".dll", "lin": ".so", "dar": ".dylib"}
    names = ["modelDescription.xml"]
    for f in folders:
        key = f[:3] if version.startswith("2") else f.rsplit("-", 1)[1][:3]
        names.append(f"binaries/{f}/M{ext[key]}")
    names.append("binaries/linux64/Other.so")  # another model's library does not count
    if sources:
        names.append("sources/model.c")
    plat = info.platforms(names, "M", version)
    assert plat["badge"] == badge
    assert plat["runsHere"] == (badge == "runs here")


def test_data_checks_say_when_the_fmu_pack_is_missing(monkeypatch):
    from app.fmu import block

    monkeypatch.setattr(block, "fmpy_available", lambda: False)
    proj = _fmu_project({"fmu_path": "x.fmu"})
    errors = [c for c in validate_project(proj) if c.level == "error"]
    assert any("FMU support is not installed" in c.text for c in errors)
    assert any("FMU pack" in (c.fix or "") for c in errors)


# ---- store: keeping, finding and unpacking files -------------------------------

def test_store_refuses_a_file_that_is_not_a_zip():
    with pytest.raises(store.FmuFileError, match="not an FMU"):
        store.store_bytes(b"hello")


def test_locate_explains_a_missing_file(tmp_path):
    with pytest.raises(store.FmuFileError, match="Import the FMU again"):
        store.locate({"fmu_path": str(tmp_path / "gone.fmu"), "fmu_name": "Gone.fmu"})
    with pytest.raises(store.FmuFileError, match="No FMU file chosen"):
        store.locate({})


def test_locate_notices_a_changed_file(tmp_path):
    f = tmp_path / "a.fmu"
    with zipfile.ZipFile(f, "w") as zf:
        zf.writestr("modelDescription.xml", "<x/>")
    params = {"fmu_path": str(f), "fmu_sha256": "0" * 64}
    with pytest.raises(store.FmuFileError, match="contents changed"):
        store.locate(params)


@pytest.mark.parametrize("bad", ["../evil.txt", "/abs/evil.txt", "a/../../evil.txt"])
def test_unpacking_refuses_names_outside_the_folder(tmp_path, bad):
    f = tmp_path / "bad.fmu"
    with zipfile.ZipFile(f, "w") as zf:
        zf.writestr("modelDescription.xml", "<x/>")
        zf.writestr(bad, "x")
    with pytest.raises(store.FmuFileError, match="unsafe name"):
        store.unpacked(f)
    assert not (tmp_path / "evil.txt").exists()


def test_allowed_list_is_per_fingerprint():
    sha = "a" * 64
    assert not store.is_allowed(sha)
    store.allow(sha, "x.fmu")
    assert store.is_allowed(sha)
    assert not store.is_allowed("b" * 64)
    with pytest.raises(store.FmuFileError):
        store.allow("not-a-hash", "x")


# ---- reading an FMU without running it ------------------------------------------

@needs_fmpy
@needs_cc
def test_describe_lists_variables_kind_tool_and_platform(test_fmu):
    d = info.describe(test_fmu)
    assert d["ok"], d["problems"]
    assert d["fmiVersion"] == "2.0"
    assert d["kinds"] == ["Co-Simulation"]
    assert d["generationTool"] == "LightSim tests"
    assert d["platform"]["badge"] == "runs here"
    assert d["platform"]["hasSources"]
    assert d["defaultStepSize"] == pytest.approx(0.01)
    by = {v["name"]: v for v in d["variables"]}
    assert by["u"]["pin"] == "input" and by["u"]["settable"]
    assert by["y"]["pin"] == "output" and not by["y"]["settable"]
    assert by["k"]["pin"] is None and by["k"]["settable"] and by["k"]["start"] == 2
    assert by["tau"]["unit"] == "s"
    assert by["mode"]["type"] == "Integer"
    # it has code for one operating system only: colleagues on the others are told
    assert any("no code for" in w for w in d["warnings"])


@needs_fmpy
def test_describe_explains_a_model_exchange_only_fmu(tmp_path):
    f = tmp_path / "me.fmu"
    with zipfile.ZipFile(f, "w") as zf:
        zf.writestr("modelDescription.xml", """<?xml version="1.0" encoding="UTF-8"?>
<fmiModelDescription fmiVersion="2.0" modelName="ME" guid="{1}" numberOfEventIndicators="0">
  <ModelExchange modelIdentifier="ME"/>
  <ModelVariables><ScalarVariable name="x" valueReference="0" causality="output"
    variability="continuous" initial="exact"><Real start="0"/></ScalarVariable></ModelVariables>
  <ModelStructure><Outputs><Unknown index="1" dependencies=""/></Outputs></ModelStructure>
</fmiModelDescription>""")
    d = info.describe(f)
    assert not d["ok"]
    assert any("Model Exchange FMU only" in p for p in d["problems"])
    assert d["platform"]["badge"] == "no code"


@needs_fmpy
def test_describe_reports_validation_problems_in_plain_words(tmp_path):
    f = tmp_path / "bad.fmu"
    with zipfile.ZipFile(f, "w") as zf:
        zf.writestr("modelDescription.xml", """<?xml version="1.0" encoding="UTF-8"?>
<fmiModelDescription fmiVersion="2.0" modelName="B" guid="{1}">
  <CoSimulation modelIdentifier="B"/>
  <ModelVariables>
    <ScalarVariable name="x" valueReference="0" causality="input"><Real/></ScalarVariable>
  </ModelVariables>
  <ModelStructure/>
</fmiModelDescription>""")
    d = info.describe(f)
    # a warning to pass on to the supplier: FMPy's checks also flag some of
    # the standard's own Reference FMUs, which run fine. (It fails for having
    # no compiled code, the only error.)
    assert [p for p in d["problems"] if "no code for" not in p] == []
    assert any(w.startswith("Its description does not follow the FMI standard")
               for w in d["warnings"])


# ---- running it ----------------------------------------------------------------

@needs_fmpy
@needs_cc
def test_fmu_runs_with_the_rest_of_the_model(test_fmu):
    """The FMU's output follows its closed-form answer, with a start value
    from the project and its input from the Data Bus."""
    params = _import(test_fmu) | {"start:k": 3.0}
    before = set(multiprocessing.active_children())
    result = simulate(_fmu_project(params, u=2.0), "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    y = _series(result, "fmu", "y")
    t_fmu = _series(result, "fmu", "t_fmu")
    assert t_fmu[-1][1] == pytest.approx(2.0, abs=1e-9)
    # the value recorded at t is the FMU's after its step to t (the input
    # reaches it one solver step after the Constant publishes it)
    for t, v in y:
        assert v == pytest.approx(3.0 * 2.0 * (1 - math.exp(-t / 0.5)), abs=0.05)
    assert y[-1][1] == pytest.approx(6 * (1 - math.exp(-4)), abs=0.02)
    _no_new_workers(before)


@needs_fmpy
@needs_cc
def test_communication_step_holds_outputs_between_steps(test_fmu):
    params = _import(test_fmu) | {"sample_time_s": 0.25}
    result = simulate(_fmu_project(params), "case")
    assert result.status in ("success", "warning")
    t_fmu = [v for _, v in _series(result, "fmu", "t_fmu")]
    # the FMU only moves on in 0.25 s steps
    assert all(abs(v / 0.25 - round(v / 0.25)) < 1e-9 for v in t_fmu)
    assert 0.25 in t_fmu and 0.3 not in t_fmu
    assert t_fmu[-1] == pytest.approx(2.0, abs=1e-9)


@needs_fmpy
@needs_cc
def test_an_fmu_nobody_allowed_does_not_run(test_fmu):
    params = _import(test_fmu, allow=False)
    proj = _fmu_project(params)
    checks = validate_project(proj)
    assert any(c.level == "error" and "has not been allowed" in c.text for c in checks)
    before = set(multiprocessing.active_children())
    result = simulate(proj, "case")
    assert result.status == "failed"
    assert "has not been allowed" in result.messages[0].text
    _no_new_workers(before)


@needs_fmpy
@needs_cc
def test_data_checks_catch_a_pin_the_fmu_does_not_have(test_fmu):
    params = _import(test_fmu)
    proj = _fmu_project(params, outputs=("y", "nope"))
    texts = [c.text for c in validate_project(proj) if c.level == "error"]
    assert any("'nope' that is not a variable" in t for t in texts)
    proj = _fmu_project(params, outputs=("y", "k"))  # a parameter is not an output
    texts = [c.text for c in validate_project(proj) if c.level == "error"]
    assert any("'k' cannot be an output pin" in t for t in texts)


@needs_fmpy
@needs_cc
@pytest.mark.parametrize("mode,expect", [
    (1, "crashed at t = 0.5"),
    (5, "reported an error at t = 0.5"),
])
def test_a_misbehaving_fmu_fails_the_run_not_the_engine(test_fmu, mode, expect):
    params = _import(test_fmu) | {"start:mode": mode}
    before = set(multiprocessing.active_children())
    result = simulate(_fmu_project(params), "case")
    assert result.status == "failed"
    assert any(expect in m.text for m in result.messages), [m.text for m in result.messages]
    _no_new_workers(before)
    # the engine is fine: the same FMU behaving runs straight after
    ok = simulate(_fmu_project(_import(test_fmu)), "case")
    assert ok.status in ("success", "warning")


@needs_fmpy
@needs_cc
def test_a_hung_fmu_is_stopped(test_fmu):
    params = _import(test_fmu) | {"start:mode": 2}
    ports = [SimpleNamespace(name="y", direction="output")]
    spec = spec_for("fmu", "Lag", params, ports)
    sb = FmuSandbox(spec, step_limit=1.0)
    try:
        sb.step(0.0, 0.5, [])
        t0 = time.monotonic()
        with pytest.raises(FmuError, match="did not finish its step"):
            sb.step(0.5, 0.5, [])
        assert time.monotonic() - t0 < 5
    finally:
        sb.close()
    assert not sb._proc.is_alive()


@needs_fmpy
@needs_cc
def test_a_memory_hungry_fmu_hits_its_cap(test_fmu):
    params = _import(test_fmu) | {"start:mode": 4}
    spec = spec_for("fmu", "Lag", params, [SimpleNamespace(name="y", direction="output")])
    with FmuSandbox(spec, mem_bytes=512 * 1024 * 1024) as sb:
        sb.step(0.0, 0.5, [])
        try:
            sb.step(0.5, 0.5, [])  # its allocations fail at the cap, or it dies
        except FmuError:
            pass


def _landlock_available() -> bool:
    if not sys.platform.startswith("linux"):
        return False
    import ctypes
    libc = ctypes.CDLL(None, use_errno=True)
    libc.syscall.restype = ctypes.c_long
    return libc.syscall(444, None, ctypes.c_size_t(0), ctypes.c_uint32(1)) >= 0


@needs_fmpy
@needs_cc
@pytest.mark.skipif(not _landlock_available(), reason="file confinement needs Landlock")
def test_fmu_cannot_read_or_write_files_outside_itself(test_fmu):
    params = _import(test_fmu) | {"start:mode": 3}
    spec = spec_for("fmu", "Lag", params, [SimpleNamespace(name="escaped", direction="output")])
    with FmuSandbox(spec) as sb:
        sb.step(0.0, 0.5, [])
        escaped = sb.step(0.5, 0.5, [])[0]
    assert escaped == 0, "the FMU read /etc/hostname (1) or wrote a file (2)"
    assert not (Path(spec.unzip_dir) / "binaries" / "linux64" / "lightsim-fmu-escape.txt").exists()


@needs_fmpy
@needs_cc
def test_fmu_worker_sees_no_engine_environment(test_fmu, monkeypatch):
    monkeypatch.setenv("LIGHTSIM_TOKEN_TEST_SECRET", "s3cret")
    # the worker clears its environment before the FMU loads (sandbox_worker.harden)
    params = _import(test_fmu)
    spec = spec_for("fmu", "Lag", params, [SimpleNamespace(name="y", direction="output")])
    with FmuSandbox(spec) as sb:
        assert sb.step(0.0, 0.1, []) == [0.0]
    assert os.environ["LIGHTSIM_TOKEN_TEST_SECRET"] == "s3cret"  # the engine's own is untouched


# ---- API -----------------------------------------------------------------------

@needs_fmpy
@needs_cc
def test_api_imports_describes_and_allows(test_fmu):
    from app.main import app

    client = TestClient(app)
    r = client.post("/api/fmus?name=LightSimTest.fmu", content=test_fmu.read_bytes(),
                    headers={"Content-Type": "application/octet-stream"})
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["allowed"] is False  # importing alone does not allow it
    assert body["info"]["ok"] and body["name"] == "LightSimTest.fmu"
    sha = body["sha256"]
    r = client.post("/api/fmus/describe", json={"fmuPath": body["path"], "fmuSha256": sha})
    assert r.json()["found"] and not r.json()["allowed"]
    assert client.post(f"/api/fmus/{sha}/allow?name=x").json() == {"allowed": True}
    assert client.post("/api/fmus/describe", json={"fmuSha256": sha}).json()["allowed"]
    r = client.post("/api/fmus?allow=true", content=test_fmu.read_bytes())
    assert r.json()["allowed"] and r.json()["sha256"] == sha
    missing = client.post("/api/fmus/describe", json={"fmuPath": "/nowhere/x.fmu",
                                                       "fmuName": "x.fmu"}).json()
    assert missing["found"] is False and "not found" in missing["problem"]


def test_api_refuses_a_file_that_is_not_an_fmu():
    from app.main import app

    client = TestClient(app)
    r = client.post("/api/fmus", content=b"not a zip")
    assert r.status_code == 400 and "not an FMU" in r.json()["detail"]
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as zf:
        zf.writestr("readme.txt", "hi")
    r = client.post("/api/fmus", content=buf.getvalue())
    assert r.status_code == 200 and not r.json()["info"]["ok"]
    assert client.post("/api/fmus/zzz/allow").status_code == 400


# ---- the Modelica Association's Reference FMUs against FMPy ---------------------

REFERENCE = os.environ.get("LIGHTSIM_REFERENCE_FMUS")


def _reference_fmus():
    if not REFERENCE:
        return []
    root = Path(REFERENCE)
    return sorted([*root.glob("2.0/*.fmu"), *root.glob("3.0/*.fmu")])


@needs_fmpy
@pytest.mark.skipif(not REFERENCE, reason="LIGHTSIM_REFERENCE_FMUS not set (CI downloads them)")
@pytest.mark.parametrize("path", _reference_fmus(), ids=lambda p: f"{p.parent.name}/{p.stem}")
def test_reference_fmu_matches_fmpy(path):
    """Metric: every Co-Simulation Reference FMU for this OS matches FMPy's
    simulate_fmu output within 1e-6."""
    from fmpy import simulate_fmu

    d = info.describe(path)
    if "Co-Simulation" not in d.get("kinds", []):
        pytest.skip(f"not a Co-Simulation FMU ({d.get('kinds')})")
    if not d["platform"]["runsHere"]:
        pytest.skip(f"no code for this OS ({d['platform']['badge']})")
    assert d["ok"], d["problems"]
    outs = [v for v in d["variables"] if v["pin"] == "output" and v["causality"] == "output"]
    if not outs:
        pytest.skip("no numeric outputs")
    h = d["defaultStepSize"] or 0.01
    stop = min(3.0, 300 * h)
    n = int(round(stop / h))
    ref = simulate_fmu(str(path), fmi_type="CoSimulation", stop_time=n * h,
                       output_interval=h, step_size=h,
                       output=[v["name"] for v in outs])
    params = _import(path)
    spec = spec_for("ref", path.stem, params,
                    [SimpleNamespace(name=v["name"], direction="output") for v in outs])
    worst = 0.0
    with FmuSandbox(spec) as sb:
        for k in range(n):
            got = sb.step(k * h, h, [])
            row = ref[k + 1]
            assert row["time"] == pytest.approx((k + 1) * h)
            for v, value in zip(outs, got):
                worst = max(worst, abs(value - float(row[v["name"]])))
    assert worst <= 1e-6
