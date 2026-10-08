"""The command-line tool (AI-02): one-line runs of the examples, JSON
output and the documented exit codes (0 done, 1 Data Checks failed, 2 run
not valid, 3 usage or file error). Also run on Windows in CI."""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

import lightsim as ls

BACKEND = Path(__file__).parent.parent


def cli(*args: str, cwd: Path = BACKEND) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, "-m", "lightsim", *args], cwd=cwd,
                          capture_output=True, text=True, encoding="utf-8", timeout=600)


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
