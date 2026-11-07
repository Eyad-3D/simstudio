"""The command-line tool (AI-02): one-line runs of the examples, JSON
output and the documented exit codes (0 done, 1 Data Checks failed, 2 run
not valid, 3 usage or file error). Also run on Windows in CI."""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

import lightsim as ls

BACKEND = Path(__file__).parent.parent


def cli(*args: str, cwd: Path = BACKEND, env: dict | None = None,
        unset: tuple[str, ...] = ()) -> subprocess.CompletedProcess:
    full = None
    if env or unset:
        full = {**os.environ, **(env or {})}
        for key in unset:
            full.pop(key, None)
    return subprocess.run([sys.executable, "-m", "lightsim", *args], cwd=cwd,
                          capture_output=True, text=True, encoding="utf-8", timeout=600, env=full)


def _landlock() -> bool:
    from app.solver import sandbox_worker as sw
    return sys.platform.startswith("linux") and sw.landlock_abi() >= 0


@pytest.mark.parametrize("example,case", [("bev-car", "City Cycle"),
                                          ("hybrid-car", "EPA city (UDDS)")])
def test_both_examples_run_from_one_line(example, case, tmp_path):
    out = tmp_path / "r.csv"
    r = cli("run", str(BACKEND / "projects" / f"{example}.json"), "--case", case, "--json",
            "-o", str(out))
    assert r.returncode == 0, r.stderr
    data = json.loads(r.stdout)
    assert data["status"] == "success" and data["exitCode"] == 0
    assert data["kpis"]["distance_km"] > 1
    assert out.is_file() and out.stat().st_size > 1000


def test_text_output_lists_the_summary():
    r = cli("run", "bev-car")
    assert r.returncode == 0, r.stderr
    assert "Battery Electric Car — City Cycle: success" in r.stdout
    assert "Consumption" in r.stdout


def _bev_copy(path: Path) -> Path:
    path.write_text((BACKEND / "projects" / "bev-car.json").read_text(encoding="utf-8"),
                    encoding="utf-8")
    return path


def test_edit_saves_a_set_and_an_add_to_a_new_file(tmp_path):
    # (e): the command-line tool can save an edit — today it had no save, so
    # every edit task failed in command-line mode. The operations are the AI
    # plug's model_edit operations.
    src = _bev_copy(tmp_path / "car.json")
    out = tmp_path / "edited.json"
    r = cli("edit", str(src), "--set", "Vehicle.mass_kg=2100 kg",
            "--op", '{"op": "add", "type": "mech.brake", "label": "Brake FL"}',
            "--out", str(out), "--json")
    assert r.returncode == 0, r.stderr
    data = json.loads(r.stdout)
    assert data["saved"] == str(out) and len(data["changes"]) == 2
    assert data["recoverable"].endswith("the original project is unchanged")
    saved = ls.load(out)
    vehicle = next(e for e in saved.elements if e.label == "Vehicle")
    assert vehicle.parameterOverrides["mass_kg"] == 2100
    assert any(e.label == "Brake FL" for e in saved.elements)
    assert ls.load(src).get("Vehicle.mass_kg") == 1927, "the source file is untouched"


def test_edit_in_place_keeps_the_previous_version_as_a_backup(tmp_path):
    # (f): a save is recoverable, and says which: one step in the project's
    # history, with the version it replaced kept as a backup (step 6)
    src = _bev_copy(tmp_path / "car.json")
    original = src.read_text(encoding="utf-8")
    r = cli("edit", str(src), "--set", "Vehicle.mass_kg=1800")
    assert r.returncode == 0, r.stderr
    assert "the previous versions are kept in" in r.stdout and "car.json-backups" in r.stdout
    assert "project's history" in r.stdout
    kept = [f for f in (tmp_path / "car.json-backups").glob("*.json") if f.stem[0].isdigit()]
    assert len(kept) == 1 and kept[0].read_text(encoding="utf-8") == original
    assert ls.load(src).get("Vehicle.mass_kg") == 1800


def test_edit_in_place_keeps_every_version_so_the_original_survives(tmp_path):
    # the check's defect (f): a single <file>.bak lost the original after a
    # second in-place edit. The <file>-backups folder keeps each version, so
    # an agent that saves several times while it works can still recover any.
    src = _bev_copy(tmp_path / "car.json")
    assert ls.load(src).get("Vehicle.mass_kg") == 1927  # the original
    for mass in (1500, 1600, 1700):
        assert cli("edit", str(src), "--set", f"Vehicle.mass_kg={mass}").returncode == 0
    assert ls.load(src).get("Vehicle.mass_kg") == 1700
    backups = tmp_path / "car.json-backups"
    masses = {ls.load(b).get("Vehicle.mass_kg") for b in backups.glob("*.json")}
    assert {1927, 1500, 1600} <= masses, f"the original 1927 and each step are recoverable: {masses}"


def _hybrid_with_a_new_script(tmp_path: Path) -> Path:
    """The hybrid example cut to a 2 s case, its Script block changed by a
    comment, so its code is code nobody approved (as a script an agent wrote)."""
    raw = json.loads((BACKEND / "projects" / "hybrid-car.json").read_text(encoding="utf-8"))
    raw["cases"] = [{**raw["cases"][0], "duration": 2, "realtimeFactor": 0}]
    scr = next(e for s in raw["systems"] for e in s["elements"]
               if e["componentDefId"] == "signal.script")
    scr["parameterOverrides"]["code"] = scr["parameterOverrides"].get("code", "") + "\n# nobody approved this\n"
    proj = tmp_path / "car.json"
    proj.write_text(json.dumps(raw), encoding="utf-8")
    return proj


def test_both_command_lines_hold_every_run_to_the_strict_sandbox(tmp_path, monkeypatch, capsys):
    # The command-line tool cannot tell an agent from a person, and an agent
    # must never wait on a person's approval, so `lightsim run` and the
    # engine's `lightsim-backend run` follow the MCP server's rules: a sandbox
    # that cannot be set up refuses the run (sandbox_unavailable), never a
    # weaker run. LIGHTSIM_STRICT_SANDBOX=0 gives a person the app's rules.
    import app.solver.domains as domains
    from app.dataio.cli import main as engine_main
    from lightsim.cli import main

    monkeypatch.setattr(domains, "why_no_strict_sandbox",
                        lambda platform=None: "No sandbox on this test computer [sandbox_unavailable].")
    monkeypatch.delenv("LIGHTSIM_STRICT_SANDBOX", raising=False)
    proj = _hybrid_with_a_new_script(tmp_path)
    assert main(["run", str(proj), "--case", "case-udds"]) == 2
    assert "sandbox_unavailable" in capsys.readouterr().out
    assert engine_main(["run", str(proj), "--case", "case-udds", "--json",
                        "--out", str(tmp_path / "r.csv")]) == 2
    assert "sandbox_unavailable" in capsys.readouterr().out
    monkeypatch.setenv("LIGHTSIM_STRICT_SANDBOX", "0")  # a person's opt-out: the app's rules
    assert main(["run", str(proj), "--case", "case-udds"]) == 0, capsys.readouterr().out
    assert engine_main(["run", str(proj), "--case", "case-udds", "--json",
                        "--out", str(tmp_path / "r.csv")]) == 0


@pytest.mark.skipif(not _landlock(), reason="needs the strict Linux sandbox (Landlock)")
def test_a_new_script_runs_from_the_command_line_with_no_approval(tmp_path):
    # Hands-off: a script nobody approved runs in the OS sandbox with no Run-
    # scripts step, with LightSim's script check on as an install has it, from
    # both command lines. LIGHTSIM_STRICT_SANDBOX=0 brings the app's rules
    # back: the script must be approved in the app first.
    proj = _hybrid_with_a_new_script(tmp_path)
    data = tmp_path / "d"
    data.mkdir()
    (data / ".script-trust-migrated").write_text("done", encoding="utf-8")  # no project scan
    env = {"LIGHTSIM_DATA_DIR": str(data), "LIGHTSIM_PROJECTS_DIR": str(tmp_path / "p")}
    off = ("LIGHTSIM_SCRIPT_TRUST", "LIGHTSIM_STRICT_SANDBOX")  # script_trust active, as installed
    full = {**{k: v for k, v in os.environ.items() if k not in off}, **env}

    for command in (["-m", "lightsim", "run", str(proj)],
                    [str(BACKEND / "run_backend.py"), "run", str(proj), "--json",
                     "--out", str(tmp_path / "r.csv")]):
        r = subprocess.run([sys.executable, *command, "--case", "case-udds"], cwd=BACKEND, env=full,
                           capture_output=True, text=True, encoding="utf-8", timeout=600)
        assert r.returncode == 0, r.stdout + r.stderr
        assert "success" in r.stdout and "has not been approved" not in r.stdout
        r0 = subprocess.run([sys.executable, *command, "--case", "case-udds"], cwd=BACKEND,
                            env={**full, "LIGHTSIM_STRICT_SANDBOX": "0"},
                            capture_output=True, text=True, encoding="utf-8", timeout=600)
        assert r0.returncode == 2 and "has not been approved" in r0.stdout, r0.stdout + r0.stderr


def test_edit_an_example_needs_an_out_file():
    r = cli("edit", "bev-car", "--set", "Vehicle.mass_kg=1800")
    assert r.returncode == 3 and "give --out" in r.stderr


def test_edit_reports_a_bad_operation_and_saves_nothing(tmp_path):
    src = _bev_copy(tmp_path / "car.json")
    before = src.read_text(encoding="utf-8")
    r = cli("edit", str(src), "--op", '{"op": "set", "element": "Vehicle", "param": "nope", "value": 1}')
    assert r.returncode == 3 and "nope" in r.stderr
    assert src.read_text(encoding="utf-8") == before, "nothing saved"
    assert not (tmp_path / "car.json-backups").exists(), "and no backup"


def test_exit_1_when_the_data_checks_fail(tmp_path):
    p = ls.load("bev-car")
    p.remove("E-Motor")
    path = p.save(tmp_path / "broken.json")
    r = cli("check", str(path), "--json")
    assert r.returncode == 1
    assert json.loads(r.stdout)["errors"] > 0
    r = cli("run", str(path))
    assert r.returncode == 1 and "error" in r.stdout


def test_exit_2_when_the_run_is_not_valid():
    r = cli("run", "bev-car", "--case", "WLTC Class 3b", "--time-limit", "0", "--json")
    assert r.returncode == 2, r.stderr
    assert json.loads(r.stdout)["status"] == "cancelled"


def test_exit_3_on_usage_and_file_errors(tmp_path):
    assert cli("run", str(tmp_path / "missing.json")).returncode == 3
    assert cli("run", "bev-car", "--case", "Nope").returncode == 3
    assert cli("run", "bev-car", "--bogus").returncode == 3
    assert cli("run", "bev-car", "--set", "Vehicle.mass_kg=150 kW").returncode == 3
    assert cli().returncode == 3
    bad = tmp_path / "bad.json"
    bad.write_text("{not json", encoding="utf-8")
    assert cli("check", str(bad)).returncode == 3


def test_set_changes_a_value_for_one_run():
    light = json.loads(cli("run", "bev-car", "--json", "--set", "Vehicle.mass_kg=1.2 t").stdout)
    heavy = json.loads(cli("run", "bev-car", "--json", "--set", "Vehicle.mass_kg=2400").stdout)
    key = "el-battery.energy_delivered_kwh"
    assert heavy["kpis"][key] > light["kpis"][key]


def test_set_wins_over_the_cases_own_value(tmp_path):
    # the HVAC case sets the Power Consumer's power itself (2.5 kW)
    out = tmp_path / "hvac.json"
    r = cli("run", "bev-car", "--case", "WLTC, heating/air-con on", "--time-limit", "2",
            "--set", "Power Consumer.power_kW=5", "-o", str(out))
    assert r.returncode in (0, 2), r.stderr
    drawn = json.loads(out.read_text(encoding="utf-8"))["channels"]["el-consumer:sig_power"]
    assert drawn["values"][1:4] == [5, 5, 5]

    from lightsim.cli import _apply_sets

    p = ls.load("hybrid-car")
    cases = [c.id for c in p.cases]
    _apply_sets(p, ["HV Battery.initial_soc_pct=70"], cases)
    assert {p.get("HV Battery.initial_soc_pct", case=c) for c in cases} == {70}
    assert p.get("HV Battery.initial_soc_pct") == 70


def test_export_a_stored_run(tmp_path):
    loadmat = pytest.importorskip("scipy.io").loadmat
    first = tmp_path / "run.json"
    assert cli("run", "bev-car", "-o", str(first)).returncode == 0
    mat = tmp_path / "run.mat"
    r = cli("export", str(first), "-o", str(mat))
    assert r.returncode == 0, r.stderr
    assert loadmat(mat, squeeze_me=True)["time"][-1] == 600.0


def test_listing_commands():
    assert "bev-car" in cli("examples").stdout
    shown = json.loads(cli("show", "hybrid-car", "--json").stdout)
    assert shown["hasScripts"] is True and len(shown["cases"]) >= 3
    parts = json.loads(cli("parts", "motor.emotor", "--json").stdout)
    assert parts[0]["parameters"][0]["key"]
    params = json.loads(cli("params", "bev-car", "--part", "Vehicle", "--json").stdout)
    assert any(p["key"] == "mass_kg" and p["unit"] == "kg" for p in params)
    assert json.loads(cli("version", "--json").stdout)["apiVersion"] == 1


def test_notebook_is_valid_json(tmp_path):
    out = tmp_path / "nb.ipynb"
    assert cli("notebook", "bev-car", "-o", str(out)).returncode == 0
    nb = json.loads(out.read_text(encoding="utf-8"))
    assert nb["nbformat"] == 4 and "ls.run('bev-car'" in "".join(nb["cells"][1]["source"])
    assert cli("notebook", "bev-car", "-o", str(out)).returncode == 3  # no overwrite


def test_the_engine_entry_point_takes_the_same_commands():
    r = subprocess.run([sys.executable, "run_backend.py", "examples"], cwd=BACKEND,
                       capture_output=True, text=True, timeout=120)
    assert r.returncode == 0, r.stderr
    assert "bev-car" in r.stdout


def test_out_never_replaces_a_project_and_keeps_any_other_file_it_replaces(tmp_path):
    # the attack check's defect 1: `run car.json --out car.json` (or notebook
    # --force) put results over the model, with no backup and no history line,
    # so an agent fooled into it left a change nobody could undo
    car = _bev_copy(tmp_path / "car.json")
    model = car.read_bytes()
    env = {"PYTHONPATH": str(BACKEND)}
    for argv in (["run", "car.json", "--out", "car.json"],
                 ["notebook", "car.json", "--force", "--out", "car.json"]):
        r = cli(*argv, cwd=tmp_path, env=env)
        assert r.returncode == 3 and "is a LightSim project" in r.stderr, (argv, r.stdout, r.stderr)
        assert car.read_bytes() == model, argv
    data = tmp_path / "data.csv"
    data.write_bytes(b"t,v\n0,1\n")
    assert cli("run", "car.json", "--out", "data.csv", cwd=tmp_path, env=env).returncode == 0
    kept = [f.read_bytes() for f in (tmp_path / "data.csv-backups").glob("*.json")]
    assert kept == [b"t,v\n0,1\n"] and data.read_bytes() != kept[0]
