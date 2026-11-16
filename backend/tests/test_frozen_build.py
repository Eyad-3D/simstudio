"""The frozen backend carries the new engine (the Rust module lightsim_engine).

The app runs every case on that module, so the installer must hold it: a
build that has no module installed stops at the freeze (backend/lightsim-
backend.spec, switch LIGHTSIM_FREEZE_ENGINE=1), and the desktop build, the
licence check and the Windows sandbox test all build and freeze it in. These
tests read what a unit test can see of that: the spec run with a stand-in for
PyInstaller's helpers, the workflows' steps and the frozen smoke test. What
the frozen build really holds is judged by scripts/smoke-backend.mjs and the
notices step, in the desktop build."""
from __future__ import annotations

import sys
import types
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SPEC = ROOT / "backend" / "lightsim-backend.spec"
WORKFLOWS = ROOT / ".github" / "workflows"


def freeze(monkeypatch, *, engine: bool, module: bool = True, fmu_pack: bool = False) -> dict:
    """Run the spec up to the Analysis call with the switches set, and return
    what it decided: its names (excludes, hiddenimports). The module is a
    stand-in that imports, or None, which makes ``import`` fail."""
    hooks = types.ModuleType("PyInstaller.utils.hooks")
    hooks.collect_submodules = lambda name: []
    hooks.collect_data_files = lambda name, **kw: []
    monkeypatch.setitem(sys.modules, "PyInstaller", types.ModuleType("PyInstaller"))
    monkeypatch.setitem(sys.modules, "PyInstaller.utils", types.ModuleType("PyInstaller.utils"))
    monkeypatch.setitem(sys.modules, "PyInstaller.utils.hooks", hooks)
    monkeypatch.setitem(sys.modules, "lightsim_engine", types.ModuleType("lightsim_engine") if module else None)
    monkeypatch.setitem(sys.modules, "fmpy", types.ModuleType("fmpy"))  # (the pack's own check)
    monkeypatch.setenv("LIGHTSIM_FREEZE_ENGINE", "1" if engine else "0")
    monkeypatch.setenv("LIGHTSIM_FREEZE_FMU_PACK", "1" if fmu_pack else "0")
    source = SPEC.read_text(encoding="utf-8")
    scope: dict = {"__file__": str(SPEC)}
    exec(compile(source[:source.index("\na = Analysis(")], str(SPEC), "exec"), scope)  # noqa: S102
    return scope


def test_the_engine_switch_keeps_the_module_and_numpy_in(monkeypatch):
    on = freeze(monkeypatch, engine=True)
    assert "lightsim_engine" in on["hiddenimports"] and "numpy" in on["hiddenimports"]
    assert "numpy" not in on["excludes"]
    # what stays out either way: the FMU pack's packages without its switch, NumPy's tests
    assert {"fmpy", "lxml", "pandas", "numpy.testing"} <= set(on["excludes"])


def test_without_the_switch_the_freeze_is_as_before(monkeypatch):
    off = freeze(monkeypatch, engine=False)
    assert "lightsim_engine" not in off["hiddenimports"]
    assert {"fmpy", "numpy", "lxml", "pandas"} <= set(off["excludes"])


def test_the_engine_switch_and_the_fmu_pack_together(monkeypatch):
    both = freeze(monkeypatch, engine=True, fmu_pack=True)
    assert "lightsim_engine" in both["hiddenimports"] and "numpy" not in both["excludes"]
    assert "fmpy.fmi2" in both["hiddenimports"]


def test_a_freeze_asked_to_carry_the_engine_stops_when_it_is_not_installed(monkeypatch):
    with pytest.raises(SystemExit) as stop:
        freeze(monkeypatch, engine=True, module=False)
    assert "lightsim_engine is not installed" in str(stop.value)
    # a freeze that did not ask for it does not look for it
    freeze(monkeypatch, engine=False, module=False)


def step(workflow: str, name: str, job: str = "") -> str:
    """The text of a workflow step, from "- name: ..." to the next step; with
    ``job``, the step of that job (a workflow can name two steps alike)."""
    text = (WORKFLOWS / workflow).read_text(encoding="utf-8")
    if job:
        text = text[text.index(f"\n  {job}:\n"):]
    block = text[text.index(f"- name: {name}"):]
    return block[:block.index("\n      - ")] if "\n      - " in block else block


@pytest.mark.parametrize("workflow, job, name, freeze_step", [
    ("desktop-build.yml", "", "The new engine's module (Rust build)", "Freeze the backend"),
    ("ci.yml", "backend-windows", "The new engine's module (Rust build)", "The frozen engine's sandbox"),
    ("ci.yml", "licences", "The new engine's module (Rust build)", "Freeze the engine"),
])
def test_every_workflow_that_freezes_the_backend_builds_the_module_first(workflow, job, name, freeze_step):
    """The desktop build, the Windows sandbox test (which runs the frozen
    engine's command line) and the licence check (which reads the frozen
    engine) each build the module with the Rust release engine/ pins, install
    its wheel into the Python that freezes, and freeze with
    LIGHTSIM_FREEZE_ENGINE=1, which stops the freeze when it is missing."""
    text = (WORKFLOWS / workflow).read_text(encoding="utf-8")
    if job:
        text = text[text.index(f"\n  {job}:\n"):]
    build = step(workflow, name, job)
    assert "working-directory: engine" in build  # (rust-toolchain.toml applies there)
    assert "rustup toolchain install" in build
    assert "maturin build --release -m crates/lsim-py/Cargo.toml --out wheels" in build
    assert "pip install --no-index --find-links wheels lightsim-engine" in build
    freezing = step(workflow, freeze_step, job)
    assert 'LIGHTSIM_FREEZE_ENGINE: "1"' in freezing
    assert text.index(f"- name: {name}") < text.index(f"- name: {freeze_step}")


def test_the_sandbox_test_gets_the_module_only_after_the_backend_tests():
    """backend-windows' tests run from source without the module, as before;
    only the frozen engine below them carries it."""
    text = (WORKFLOWS / "ci.yml").read_text(encoding="utf-8")
    text = text[text.index("\n  backend-windows:\n"):]
    assert text.index("- name: Tests") < text.index("- name: The new engine's module")


def test_the_desktop_build_reruns_when_the_engine_changes():
    text = (WORKFLOWS / "desktop-build.yml").read_text(encoding="utf-8")
    paths = text[text.index("  pull_request:"):text.index("  workflow_dispatch:")]
    assert '"engine/**"' in paths and '"backend/lightsim-backend.spec"' in paths


def test_the_desktop_build_checks_the_notices_and_the_frozen_runs_after_freezing():
    text = (WORKFLOWS / "desktop-build.yml").read_text(encoding="utf-8")
    order = [text.index(f"- name: {n}") for n in
             ("Freeze the backend", "Smoke-test the frozen backend", "Third-party notices")]
    assert order == sorted(order)


def test_the_local_build_scripts_build_the_module_too():
    for script, build, freeze_ in (("build-desktop.ps1", "maturin build --release", "LIGHTSIM_FREEZE_ENGINE"),
                                  ("build-desktop.sh", "maturin build --release", "LIGHTSIM_FREEZE_ENGINE=1")):
        text = (ROOT / "scripts" / script).read_text(encoding="utf-8")
        assert build in text and freeze_ in text, script
        assert text.index(build) < text.index("PyInstaller"), script
        assert "rustup toolchain install" in text, script


def test_the_frozen_smoke_test_holds_every_run_to_the_new_engine():
    """scripts/smoke-backend.mjs starts the frozen backend with the new
    engine and no fall-back, and fails any result (REST, live, scripted,
    command line) whose engine is not "new"."""
    text = (ROOT / "scripts" / "smoke-backend.mjs").read_text(encoding="utf-8")
    assert 'LIGHTSIM_ENGINE: "new"' in text and 'LIGHTSIM_ENGINE_STRICT: "1"' in text
    assert 'result.engine !== "new"' in text
    for run in ('onNewEngine("REST simulate"', 'onNewEngine("live run"', "onNewEngine(`scripted run",
                "onNewEngine(`'lightsim-backend run"):
        assert run in text, run
    # the command line reports the engine that made the run
    assert '"engine": result.engine' in (ROOT / "backend" / "app" / "dataio" / "cli.py").read_text(encoding="utf-8")
