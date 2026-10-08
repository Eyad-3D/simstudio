"""The Python package (AI-02): load, check, run, read and edit projects
in-process; units are checked; results export to CSV, MAT and JSON, and
Parquet when pyarrow is installed."""
from __future__ import annotations

import csv
import json

import pytest

import lightsim as ls
from lightsim import units
from lightsim.result import read_run


@pytest.fixture(scope="module")
def city():
    return ls.run("bev-car", case="City Cycle")


def test_the_one_liner_runs_an_example_and_gives_stable_keys(city):
    assert city.status == "success" and city.ok and city.valid
    assert city.kpis["consumption_kwh_per_100km"] == pytest.approx(11.12, abs=0.02)
    assert city.kpis["el-battery.final_soc_pct"] == pytest.approx(88.76, abs=0.05)
    assert city.units["distance_km"] == "km"
    assert city.channel("el-battery:sig_soc").unit == "%"
    assert len(city.time) == 601 and city.time[-1] == 600.0


def test_a_case_is_picked_by_name_or_id_and_the_first_by_default():
    p = ls.load("bev-car")
    assert p.case("city cycle").id == p.case("case-city").id == p.case().id
    with pytest.raises(ls.LightSimError, match="No case 'Nope'.*City Cycle"):
        p.case("Nope")


def test_csv_has_units_in_the_header_and_a_row_per_point(city, tmp_path):
    path = tmp_path / "city.csv"
    city.to_csv(path)
    rows = list(csv.reader(path.open(encoding="utf-8")))
    assert rows[0][0] == "Time [s]"
    assert "HV Battery Pack · SOC [%]" in rows[0]
    assert len(rows) == 1 + len(city.time)
    col = rows[0].index("HV Battery Pack · SOC [%]")
    assert float(rows[-1][col]) == pytest.approx(city.channel("el-battery:sig_soc").values[-1])


def test_mat_file_round_trips_through_scipy(city, tmp_path):
    loadmat = pytest.importorskip("scipy.io").loadmat
    path = tmp_path / "city.mat"
    city.to_mat(path)
    m = loadmat(path, squeeze_me=True)
    assert m["time"][-1] == 600.0
    soc = m["HV_Battery_Pack_SOC"]
    assert soc[-1] == pytest.approx(city.channel("el-battery:sig_soc").values[-1])
    assert m["units"]["HV_Battery_Pack_SOC"].item() == "%"
    assert m["channel_keys"]["HV_Battery_Pack_SOC"].item() == "el-battery:sig_soc"
    assert m["kpis"]["consumption_kwh_per_100km"].item() == city.kpis["consumption_kwh_per_100km"]
    assert m["info"]["status"].item() == "success"


def test_json_export_matches_its_schema_and_reads_back(city, tmp_path):
    jsonschema = pytest.importorskip("jsonschema")
    path = tmp_path / "city.json"
    city.to_json(path)
    data = json.loads(path.read_text(encoding="utf-8"))
    jsonschema.validate(data, ls.schemas()["lightsim-result"])
    back = read_run(path)
    assert back.kpis == city.kpis and back.status == city.status
    assert back.channel("el-battery:sig_soc").values == city.channel("el-battery:sig_soc").values


def test_parquet_round_trips_through_pyarrow(city, tmp_path):
    """STD-09: Parquet, when pyarrow is installed (LightSim does not ship it)."""
    pq = pytest.importorskip("pyarrow.parquet")
    path = tmp_path / "city.parquet"
    city.to_parquet(path)
    table = pq.read_table(path)
    assert table.column("time").to_pylist() == city.time
    soc = table.schema.field("HV Battery Pack · SOC")
    assert soc.metadata == {b"unit": b"%", b"key": b"el-battery:sig_soc"}
    assert table.column("HV Battery Pack · SOC").to_pylist() == list(
        city.channel("el-battery:sig_soc").values)
    about = json.loads(table.schema.metadata[b"lightsim"])
    assert about["kpis"] == city.kpis and about["units"] == city.units
    assert about["caseName"] == "City Cycle" and about["status"] == "success"
    assert table.num_columns == 1 + len(city.channels)


def test_parquet_without_pyarrow_says_what_it_needs(city, tmp_path, monkeypatch):
    """No pyarrow: the export says how to get it, and the command line
    refuses the file as a usage error (exit 3) without writing it."""
    from lightsim import cli

    monkeypatch.setitem(__import__("sys").modules, "pyarrow", None)
    with pytest.raises(ImportError, match=r"pip install pyarrow"):
        city.to_parquet(tmp_path / "city.parquet")
    run = tmp_path / "city.json"
    city.to_json(run)
    out = tmp_path / "out.parquet"
    assert cli.main(["export", str(run), "-o", str(out)]) == cli.EXIT_USAGE
    assert not out.exists()


def test_dataframe_carries_units(city):
    pytest.importorskip("pandas")
    df = city.df
    assert df.index.name == "Time [s]"
    assert df.attrs["units"]["HV Battery Pack · SOC"] == "%"
    assert df["HV Battery Pack · SOC"].iloc[-1] == pytest.approx(88.76, abs=0.05)


def test_set_checks_and_converts_units():
    p = ls.load("bev-car")
    p.set("Vehicle.mass_kg", "1.9 t")
    assert p.get("Vehicle.mass_kg") == pytest.approx(1900)
    p.set("Vehicle.mass_kg", 2000)
    assert p.get("Vehicle.mass_kg") == 2000
    with pytest.raises(ls.LightSimError, match="does not measure"):
        p.set("Vehicle.mass_kg", "150 kW")
    with pytest.raises(ls.LightSimError, match="above 0"):
        p.set("Vehicle.mass_kg", -5)
    with pytest.raises(ls.LightSimError, match="has no parameter 'mass'.*mass_kg"):
        p.set("Vehicle.mass", 1)
    with pytest.raises(ls.LightSimError, match="No part 'Truck'"):
        p.set("Truck.mass_kg", 1)


def test_units_convert_common_alternatives():
    assert units.to_unit("100 km/h", "m/s") == pytest.approx(27.7778, rel=1e-4)
    assert units.to_unit("0.15 MW", "kW") == pytest.approx(150)
    assert units.to_unit("3000 rpm", "1/min") == 3000
    assert units.to_unit("20 °C", "K") == pytest.approx(293.15)
    assert units.to_unit("1 bar", "kPa") == pytest.approx(100)
    with pytest.raises(units.UnitError):
        units.to_unit("3 parsecs", "m")


def test_a_case_value_changes_only_that_case():
    p = ls.load("bev-car")
    p.set("Vehicle.mass_kg", 2500, case="City Cycle")
    assert p.get("Vehicle.mass_kg", case="City Cycle") == 2500
    assert p.get("Vehicle.mass_kg") != 2500
    assert p.get("Vehicle.mass_kg", case="WLTC Class 3b") != 2500


def test_variants_in_a_loop_give_different_answers():
    base = ls.load("bev-car")
    base.case("City Cycle").duration = 120
    use = {}
    for mass in (1500, 2500):
        p = base.copy().set("Vehicle.mass_kg", f"{mass} kg")
        use[mass] = p.run("City Cycle").kpis["el-battery.energy_delivered_kwh"]
    assert use[2500] > use[1500]


def test_build_a_model_from_parts_and_run_it(tmp_path):
    p = ls.load("bev-car")
    # drop the auxiliary consumer and check the wiring is gone with it
    p.remove("Power Consumer")
    assert all(c.sourceElementId != "el-consumer" and c.targetElementId != "el-consumer"
               for s in p.model.systems for c in s.connections)
    aux = p.add("electric.constant_drive", label="Heater", power_kW="2000 W")

    def free(part, ports):
        used = {(c.sourceElementId, c.sourcePortId) for s in p.model.systems
                for c in s.connections} | {(c.targetElementId, c.targetPortId)
                                           for s in p.model.systems for c in s.connections}
        el = p.element(part)
        return f"{part}.{next(x for x in ports if (el.id, x) not in used)}"

    p.connect(free("HV Bus", ["t1", "t2", "t3", "t4", "t5"]), f"{aux}.pos")
    p.connect(f"{aux}.neg", free("Ground", ["t1", "t2"]))
    case = p.add_case("Short city", duration=60, values={"Vehicle.mass_kg": "1800 kg"})
    assert p.get("Vehicle.mass_kg", case=case) == 1800
    path = p.save(tmp_path / "variant.json")
    again = ls.load(path)
    assert again.element("Heater").parameterOverrides["power_kW"] == 2.0
    assert not [c for c in again.check() if c.level == "error"]
    assert again.run(case).status in ("success", "warning")
    with pytest.raises(ls.LightSimError, match="Signal ports are linked with route"):
        again.connect("Driver.sig_traction_cmd", "E-Motor.sig_demand_in")
    link = again.route("Driver.sig_traction_cmd", "E-Motor.sig_demand_in")
    assert sum(1 for d in again.model.dataBusConnections
               if (d.element2Id, d.port2Id) == ("el-motor", "sig_demand_in")) == 1
    again.disconnect(link)


def test_check_reports_errors_and_run_stops_on_them():
    p = ls.load("bev-car")
    p.remove("E-Motor")
    checks = p.check()
    assert any(c.level == "error" for c in checks)
    r = p.run("City Cycle")
    assert r.status == "failed" and r.checks and not r.channels


def test_a_time_limit_stops_a_run():
    r = ls.run("bev-car", case="WLTC Class 3b", time_limit_s=0.0)
    assert r.status == "cancelled"


def test_read_a_stored_run_as_the_app_keeps_it(city, tmp_path):
    import gzip

    from app.schemas import StoredRun

    stored = StoredRun(id="r1", caseId=city.case_id, caseName="City Cycle", startedAt=0,
                       status=city.status, result=city.raw)
    path = tmp_path / "r1.json.gz"
    path.write_bytes(gzip.compress(stored.model_dump_json().encode()))
    back = read_run(path)
    assert back.kpis == city.kpis and back.case_name == "City Cycle"


def test_the_package_opens_no_network_socket(monkeypatch):
    import socket

    def refuse(*a, **k):
        raise AssertionError("LightSim opened a network socket")

    monkeypatch.setattr(socket.socket, "bind", refuse)
    monkeypatch.setattr(socket.socket, "listen", refuse)
    monkeypatch.setattr(socket.socket, "connect", refuse)
    r = ls.run("bev-car", case="City Cycle")
    assert r.ok


def test_a_live_case_runs_without_waiting_and_gives_the_apps_figures(city):
    # 'City Cycle (live, 10×)' is City Cycle paced at 10× for watching in
    # the app: 60 s of waiting for its 600 s
    import time

    t0 = time.monotonic()
    live = ls.run("bev-car", case="case-city-live")
    assert time.monotonic() - t0 < 30
    assert live.status == "success" and live.kpis == city.kpis
    p = ls.load("bev-car")
    assert p.case("case-city-live").realtimeFactor == 10, "the project keeps its pace"


def test_an_unpaced_live_case_stays_unbalanced_as_in_the_app():
    from lightsim._engine import engine

    balance = engine("solver.balance")
    p = ls.load("hybrid-car")
    live, plain = p.case("Mixed Cycle (live, 10×)"), p.case("Mixed Cycle")
    fast = balance.unpaced(live)
    assert fast.realtimeFactor == 0 and live.realtimeFactor == 10
    assert balance.applies(p.model, fast) is balance.applies(p.model, live) is False
    assert balance.unpaced(plain) is plain and balance.applies(p.model, plain)
