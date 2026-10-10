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
    with pytest.raises(store.FmuFileError, match="not found on this computer"):
        store.locate({"fmu_sha256": "0" * 64, "fmu_name": "Gone.fmu"})
    with pytest.raises(store.FmuFileError, match="No FMU file chosen"):
        store.locate({})


def _zip(entries: dict[str, bytes | str], *, stored: bool = False) -> bytes:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_STORED if stored else zipfile.ZIP_DEFLATED) as zf:
        for name, data in entries.items():
            zf.writestr(name, data)
    return buf.getvalue()


def test_locate_notices_a_damaged_copy():
    sha = "0" * 64
    path = store.stored_path(sha)
    path.parent.mkdir(parents=True)
    path.write_bytes(_zip({"modelDescription.xml": "<x/>"}))
    with pytest.raises(store.FmuFileError, match="damaged"):
        store.locate({"fmu_sha256": sha, "fmu_name": "A.fmu"})


def test_a_path_in_the_project_is_never_touched(tmp_path, monkeypatch):
    """A project from someone else names its FMU by a network path: checking
    whether it exists would make Windows connect to that host (and send the
    user's sign-in), so LightSim never looks at it; only its own copies, by
    fingerprint."""
    touched = []
    real_stat = os.stat

    def spy(path, *args, **kwargs):
        touched.append(str(path))
        return real_stat(path, *args, **kwargs)

    monkeypatch.setattr(os, "stat", spy)
    for where in ("//attacker.example/share/battery.fmu", r"\\attacker.example\share\b.fmu",
                  "/net/attacker.example/b.fmu"):
        params = {"fmu_path": where, "fmu_sha256": "ab" * 32, "fmu_name": "battery.fmu"}
        checks = validate_project(_fmu_project(params))
        assert any(c.level == "error" for c in checks)
        with pytest.raises(store.FmuFileError, match="not found on this computer"):
            store.locate(params)
    assert not [p for p in touched if "attacker" in p]

    # nor is a real file elsewhere used, even one with the right fingerprint:
    # only a file the user imported in the app (its bytes uploaded) runs
    monkeypatch.setattr(os, "stat", real_stat)
    elsewhere = tmp_path / "shared" / "battery.fmu"
    elsewhere.parent.mkdir()
    elsewhere.write_bytes(_zip({"modelDescription.xml": "<x/>"}))
    sha = store.sha256_of(elsewhere)
    store.allow(sha, "battery.fmu")
    with pytest.raises(store.FmuFileError, match="not found on this computer"):
        store.locate({"fmu_path": str(elsewhere), "fmu_sha256": sha})
    kept = store.store_bytes(elsewhere.read_bytes())[1]
    assert store.locate({"fmu_path": str(elsewhere), "fmu_sha256": sha}) == kept


def test_a_kept_fmu_swapped_after_its_check_does_not_run():
    """The file that is unpacked is hashed as it is copied: a kept FMU whose
    bytes changed after Data Checks hashed it (same size and time, so the
    cached hash still says it is the allowed one) is refused, not unpacked
    under the allowed fingerprint."""
    good = _zip({"modelDescription.xml": "<good/>", "binaries/x": "A" * 64}, stored=True)
    sha, kept = store.store_bytes(good)
    store.allow(sha, "a.fmu")
    params = {"fmu_sha256": sha, "fmu_name": "a.fmu"}
    assert store.locate(params) == kept  # Data Checks: hashed and cached
    st = kept.stat()
    evil = good.replace(b"A" * 64, b"B" * 64)
    assert len(evil) == len(good) and evil != good
    kept.write_bytes(evil)
    os.utime(kept, ns=(st.st_atime_ns, st.st_mtime_ns))
    assert store.locate(params) == kept  # the cache cannot tell
    with pytest.raises(store.FmuFileError, match="does not match the FMU that was allowed"):
        store.unpacked(sha)
    assert not (store.fmu_dir() / "unpacked" / sha).exists()
    assert [p.name for p in (store.fmu_dir() / "unpacked").iterdir()] == []  # no copies left
    # the real file unpacks
    kept.write_bytes(good)
    assert (store.unpacked(sha) / "modelDescription.xml").read_text(encoding="utf-8") == "<good/>"


@pytest.mark.parametrize("bad", ["../evil.txt", "/abs/evil.txt", "a/../../evil.txt",
                                 "binaries/D:/evil.dll", "C:evil.txt", "binaries/C:x/evil.dll",
                                 "a/b:c.dll"])
def test_unpacking_refuses_names_outside_the_folder(tmp_path, bad):
    sha, _ = store.store_bytes(_zip({"modelDescription.xml": "<x/>", bad: "x"}))
    with pytest.raises(store.FmuFileError, match="unsafe name"):
        store.unpacked(sha)
    assert not (tmp_path / "evil.txt").exists()


def test_unpacking_reads_windows_names_as_windows_does():
    # binaries/D:/evil.dll would land on drive D: on Windows (D:evil.dll)
    for name in ("binaries/D:/evil.dll", "D:/x", "binaries/x:y", "a//b/..", "/x", "a/../b"):
        assert store._unsafe(name), name
    for name in ("modelDescription.xml", "binaries/win64/Model.dll", "resources/a b.txt",
                 "binaries/x86_64-windows/M.dll", "documentation/index.html"):
        assert not store._unsafe(name), name


def test_a_damaged_fmu_fails_with_a_plain_message():
    """A bad CRC (or another fault in the archive) is a plain FmuFileError,
    which Data Checks and runs report, never an unhandled exception."""
    data = bytearray(_zip({"modelDescription.xml": "<x/>",
                           "sources/model.c": "#include <stdio.h>\n" * 10}, stored=True))
    i = data.find(b"#include")
    data[i] ^= 0x01
    sha, _ = store.store_bytes(bytes(data))
    with pytest.raises(store.FmuFileError, match="damaged"):
        store.unpacked(sha)
    assert not (store.fmu_dir() / "unpacked" / sha).exists()


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


@needs_fmpy
def test_describe_does_not_read_other_files_through_the_xml(tmp_path):
    """An FMU's XML cannot pull a file from the computer into what LightSim
    shows (and saves, as the block's name)."""
    f = tmp_path / "xxe.fmu"
    secret = tmp_path / "secret.txt"
    secret.write_text("TOP-SECRET")
    with zipfile.ZipFile(f, "w") as zf:
        zf.writestr("modelDescription.xml", f"""<?xml version="1.0"?>
<!DOCTYPE fmiModelDescription [<!ENTITY x SYSTEM "{secret.as_uri()}">]>
<fmiModelDescription fmiVersion="2.0" modelName="&x;" guid="{{1}}">
  <CoSimulation modelIdentifier="B"/><ModelVariables/><ModelStructure/>
</fmiModelDescription>""")
    d = info.describe(f)
    assert "TOP-SECRET" not in repr(d)
    assert not d["ok"]


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
    """With a Communication Step the FMU exchanges values only at its sample
    instants and its outputs hold in between; it is never ahead of the
    engine (it catches up to the end of the solver step at each exchange)."""
    params = _import(test_fmu) | {"sample_time_s": 0.25}
    result = simulate(_fmu_project(params), "case")
    assert result.status in ("success", "warning")
    t_fmu = _series(result, "fmu", "t_fmu")
    h = 0.01  # the solver step
    for t, v in t_fmu:
        assert v <= t + 1e-9, f"at t = {t} the FMU is already at {v}"
        assert t - v < 0.25 + h + 1e-9, f"at t = {t} the FMU is still at {v}"
    # the FMU only moves on at the sample instants: 0, 0.25, 0.5 … 1.75
    moved = sorted({round(v, 9) for _, v in t_fmu if v > 0})
    assert moved == pytest.approx([k * 0.25 + h for k in range(8)], abs=1e-9)
    # it is not set back when the engine's samples fall between its steps
    values = [v for _, v in t_fmu]
    assert values == sorted(values)


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
def test_a_damaged_fmu_fails_the_run_with_a_message(test_fmu):
    """An archive that Data Checks can read but that fails to unpack (a bad
    CRC) fails the run with a plain message, over REST too, and leaves no
    worker behind."""
    data = bytearray(_zip({i.filename: zipfile.ZipFile(test_fmu).read(i.filename)
                           for i in zipfile.ZipFile(test_fmu).infolist()}, stored=True))
    i = data.find(b"#include")
    data[i] ^= 0x01
    bad = test_fmu.parent / "Damaged.fmu"
    bad.write_bytes(bytes(data))
    good = _fmu_project(_import(test_fmu)).systems[0].elements[1]
    good.id, good.label = "good", "Good"
    proj = _fmu_project(_import(bad))
    proj.systems[0].elements.insert(1, good)  # started first, so it must be stopped
    assert not [c for c in validate_project(proj) if c.level == "error"]
    before = set(multiprocessing.active_children())
    result = simulate(proj, "case")
    assert result.status == "failed"
    assert any("damaged" in m.text for m in result.messages), [m.text for m in result.messages]
    _no_new_workers(before)

    from app.main import app
    r = TestClient(app).post("/api/simulate", json={"project": proj.model_dump(), "caseId": "case"})
    assert r.status_code == 200 and r.json()["status"] == "failed"


@needs_fmpy
@needs_cc
def test_an_unexpected_error_while_starting_an_fmu_fails_the_run(test_fmu, monkeypatch):
    from app.fmu import sandbox

    def boom(*args, **kwargs):
        raise RuntimeError("no more processes")

    monkeypatch.setattr(sandbox, "FmuSandbox", boom)
    result = simulate(_fmu_project(_import(test_fmu)), "case")
    assert result.status == "failed"
    assert any("could not be set up: no more processes" in m.text for m in result.messages)


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
