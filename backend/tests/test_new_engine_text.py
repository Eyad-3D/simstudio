"""Text components (step 2's item 5): a part whose element names a text file
(ElementInstance.textFile) is built by the new engine from the component in
that file, in the place of its built-in definition. Today's engine reads no
text components, so such a case runs on the new engine whatever
LIGHTSIM_ENGINE says, and one the new engine's path does not run fails with
the reason's code and NEW-TEXT (never the built-in part in its place).

These are guards; the TEXT line of step 2's acceptance
(benchmarks/back_to_back/acceptance.toml) runs every part of the examples."""
from __future__ import annotations

import hashlib
import json
import os
import sys

import pytest

from app.solver import new_engine, simulate
from app.storage import load_example

try:
    import lightsim_engine as le
    MISSING = None
except ImportError as e:
    MISSING = f"the compiled module lightsim_engine is not installed ({e})"
REQUIRED = os.environ.get("LIGHTSIM_REQUIRE_NEW_ENGINE", "").strip() == "1"
needs_engine = pytest.mark.skipif(MISSING is not None and not REQUIRED, reason=MISSING or "")

CITY = "case-city"


@pytest.fixture(autouse=True)
def user_folder(tmp_path, monkeypatch):
    # the projects folder is tmp_path: a text file must be inside it
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path))
    monkeypatch.delenv(new_engine.ENV_VAR, raising=False)
    monkeypatch.delenv(new_engine.STRICT_VAR, raising=False)


def _sha(result) -> str:
    return hashlib.sha256(json.dumps(result.model_dump(mode="json"), sort_keys=True,
                                     separators=(",", ":")).encode()).hexdigest()


def _with_text(name: str, case_id: str, el_id: str, path, text=None):
    project = load_example(name)
    if text is None:
        text = le.part_text(project.model_dump_json(), case_id, el_id)
    if text is not False:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    el = next(e for s in project.systems for e in s.elements if e.id == el_id)
    el.textFile = str(path)
    return project


@needs_engine
def test_a_part_read_from_its_text_file_gives_the_built_in_parts_result(tmp_path, monkeypatch):
    """The battery written as text (the definition the import makes for it,
    lightsim_engine.part_text) in a file: on the new path the whole result
    is the built-in part's, its SHA-256 the same."""
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    plain = simulate(load_example("bev-car"), CITY)
    proj = _with_text("bev-car", CITY, "el-battery", tmp_path / "battery.mo")
    assert new_engine.read_texts(proj) == {"el-battery": (tmp_path / "battery.mo").read_text(encoding="utf-8")}
    text = simulate(proj, CITY)
    assert text.status == plain.status == "success"
    assert _sha(text) == _sha(plain)


@needs_engine
def test_the_file_is_read_and_without_the_flag_the_new_engine_runs_it(tmp_path, monkeypatch):
    """The text is what runs (twice the drag in it moves the result), the
    file found from the projects folder when its path is not absolute, and
    with LIGHTSIM_ENGINE unset the case runs on the new engine, saying why."""
    project = load_example("bev-car")
    text = le.part_text(project.model_dump_json(), CITY, "el-vehicle")
    doubled = text.replace("max(cd, 0)", "2 * max(cd, 0)", 1)
    assert doubled != text
    proj = _with_text("bev-car", CITY, "el-vehicle", tmp_path / "parts" / "vehicle.mo", doubled)
    el = next(e for s in proj.systems for e in s.elements if e.id == "el-vehicle")
    el.textFile = "parts/vehicle.mo"
    result = simulate(proj, CITY)
    assert result.status != "failed", [m.text for m in result.messages]
    assert result.engine == "new"
    assert "today's engine reads no text components" in result.messages[0].text
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    plain = simulate(load_example("bev-car"), CITY)

    def used(r):
        return next(s.value for s in r.summary if s.key == "el-battery.energy_delivered_kwh")
    assert used(result) > used(plain) * 1.01


@needs_engine
def test_a_file_that_is_not_there_fails_the_run_with_new_text(tmp_path):
    proj = _with_text("bev-car", CITY, "el-battery", tmp_path / "gone.mo", text=False)
    result = simulate(proj, CITY)
    assert result.status == "failed" and not result.channels
    text = result.messages[0].text
    assert "[NEW-TEXT]" in text and "could not be read" in text and "HV Battery Pack" in text



@needs_engine
def test_a_file_over_the_size_cap_fails_the_run_with_new_text(tmp_path):
    """The part's own text, padded past 1 MB with comments (a text the
    engine would read): refused unread, NEW-TEXT naming the cap."""
    project = load_example("bev-car")
    text = le.part_text(project.model_dump_json(), CITY, "el-battery") + "// padding\n" * 100_000
    assert len(text.encode()) > new_engine.TEXT_LIMIT_BYTES
    result = simulate(_with_text("bev-car", CITY, "el-battery", tmp_path / "big.mo", text), CITY)
    assert result.status == "failed" and not result.channels
    said = result.messages[0].text
    assert "[NEW-TEXT]" in said and "is over 1000 kB" in said and "HV Battery Pack" in said, said

@needs_engine
def test_a_text_that_does_not_fit_its_part_fails_with_the_engines_code(tmp_path):
    proj = _with_text("bev-car", CITY, "el-battery", tmp_path / "bad.mo", "model Broken\nend Broken;\n")
    result = simulate(proj, CITY)
    assert result.status == "failed"
    text = result.messages[0].text
    assert "[NEW-FAILED] [NEW-TEXT]" in text and "TEXT-" in text, text


@needs_engine
def test_a_case_the_new_path_does_not_run_fails_with_both_codes(tmp_path, monkeypatch):
    """A lap case (NEW-LAP) with a text component: today's engine would run
    the built-in part in its place without a word, so the case fails with
    NEW-LAP and NEW-TEXT, in either mode."""
    proj = _with_text("fs-electric", "case-accel-75m", "el-battery", tmp_path / "acc.mo")
    for strict in ("", "1"):
        monkeypatch.setenv(new_engine.STRICT_VAR, strict)
        result = simulate(proj, "case-autocross")
        assert result.status == "failed" and not result.channels
        assert result.messages[0].text.startswith("Not run [NEW-LAP] [NEW-TEXT]")


def test_without_the_module_a_text_component_never_runs_on_today_engine(tmp_path, monkeypatch):
    """No new engine: the case fails with NEW-MISSING and NEW-TEXT."""
    import builtins

    real = builtins.__import__

    def hidden(name, *args, **kwargs):
        if name == "lightsim_engine":
            raise ImportError("hidden")
        return real(name, *args, **kwargs)
    monkeypatch.setattr(builtins, "__import__", hidden)
    project = load_example("bev-car")
    el = next(e for s in project.systems for e in s.elements if e.id == "el-battery")
    el.textFile = str(tmp_path / "any.mo")
    result = simulate(project, CITY)
    assert result.status == "failed"
    assert result.messages[0].text.startswith("Not run [NEW-MISSING] [NEW-TEXT]")


# ---- where a text file may be (merge follow-up N3) ---------------------------------
# One rule for every run, read in new_engine.read_texts: the file's path, taken
# from the projects folder, must lie inside it once links are followed. Each
# case runs with LIGHTSIM_ENGINE unset and =new (a text component runs on the
# new engine either way) and as an agent's strict run (STRICT, set by app/ai).

RUNS = [("", False), ("new", False), ("", True), ("new", True)]


def _run(proj, engine: str, agent: bool, monkeypatch):
    from app.solver.sandbox import STRICT
    if engine:
        monkeypatch.setenv(new_engine.ENV_VAR, engine)
    token = STRICT.set(agent)
    try:
        return simulate(proj, CITY)
    finally:
        STRICT.reset(token)


@pytest.fixture
def outside(tmp_path_factory):
    """The battery's own text in a folder outside the projects folder: read,
    it would run as well as a good file does."""
    folder = tmp_path_factory.mktemp("outside")
    text = le.part_text(load_example("bev-car").model_dump_json(), CITY, "el-battery")
    (folder / "battery.mo").write_text(text, encoding="utf-8")
    return folder


def _escapes(tmp_path, outside, monkeypatch):
    """Each way out of the projects folder (tmp_path), as a textFile."""
    monkeypatch.setenv("HOME", str(outside))  # (expanduser: HOME, USERPROFILE on Windows)
    monkeypatch.setenv("USERPROFILE", str(outside))
    out = {"absolute": str(outside / "battery.mo"), "home": "~/battery.mo",
           "dot-dot": os.path.relpath(outside / "battery.mo", tmp_path)}
    try:
        (tmp_path / "link.mo").symlink_to(outside / "battery.mo")
        (tmp_path / "linked").symlink_to(outside, target_is_directory=True)
        out |= {"file link": "link.mo", "folder link": "linked/battery.mo"}
    except OSError:  # (Windows without the right to make links)
        pass
    return out


@needs_engine
@pytest.mark.parametrize("engine, agent", RUNS)
def test_a_text_file_outside_the_projects_folder_is_never_read(engine, agent, tmp_path, outside,
                                                               monkeypatch):
    escapes = _escapes(tmp_path, outside, monkeypatch)
    assert escapes["dot-dot"].startswith("..")
    if sys.platform != "win32":
        assert {"file link", "folder link"} <= set(escapes)
    for how, name in escapes.items():
        proj = load_example("bev-car")
        next(e for s in proj.systems for e in s.elements if e.id == "el-battery").textFile = name
        result = _run(proj, engine, agent, monkeypatch)
        assert result.status == "failed" and not result.channels, how
        said = result.messages[0].text
        assert said.startswith("Not run [NEW-TEXT]: the text file of 'HV Battery Pack' "
                               f"({name}) is outside the projects folder"), (how, said)
        if "link" in how:  # (a link's target is not told)
            assert str(outside) not in said, (how, said)


@needs_engine
@pytest.mark.parametrize("engine, agent", RUNS)
def test_a_text_file_inside_the_projects_folder_runs(engine, agent, tmp_path, monkeypatch):
    """A relative path from the projects folder, one with a harmless ``..``
    that stays inside, an absolute path inside it, and a link that stays
    inside: each runs, the result the built-in part's."""
    plain = _run(load_example("bev-car"), "new", False, monkeypatch)
    _with_text("bev-car", CITY, "el-battery", tmp_path / "parts" / "battery.mo")
    names = ["parts/battery.mo", "parts/../parts/battery.mo", str(tmp_path / "parts" / "battery.mo")]
    try:
        (tmp_path / "inside.mo").symlink_to(tmp_path / "parts" / "battery.mo")
        names.append("inside.mo")
    except OSError:  # (Windows without the right to make links)
        pass
    for name in names:
        proj = load_example("bev-car")
        next(e for s in proj.systems for e in s.elements if e.id == "el-battery").textFile = name
        result = _run(proj, engine, agent, monkeypatch)
        assert result.status == plain.status == "success", (name, [m.text for m in result.messages])
        assert result.engine == "new"
        assert [s.value for s in result.summary] == [s.value for s in plain.summary], name
