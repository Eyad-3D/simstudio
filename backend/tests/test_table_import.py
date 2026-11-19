"""Tables, maps and drive profiles from CSV and Excel files (STD-10): the
data is found, read in the units its headers name and converted to the
parameter's, and a file that cannot be read says exactly where and why."""
from __future__ import annotations

import base64
import io

import openpyxl
import pytest
from fastapi.testclient import TestClient

from app.dataio import sheets, tables, units
from app.dataio.cli import main as cli_main
from app.main import app

client = TestClient(app)

OCV = tables.target_for("battery.generic", "ocv_table")
LOSS = tables.target_for("motor.emotor", "power_loss")  # [Speed 1/min, Torque N·m] -> kW
FULL_LOAD = tables.target_for("motor.emotor", "full_load_torque")  # [Voltage, Speed]
DRIVE = tables.target_for("signal.driving_task", "profile")
ROAD = tables.target_for("signal.road_profile", "profile", "distance")


def xlsx(rows: list[list], title: str = "Sheet1", more: dict | None = None) -> bytes:
    wb = openpyxl.Workbook()
    ws = wb.active
    ws.title = title
    for r in rows:
        ws.append(r)
    for name, extra in (more or {}).items():
        w = wb.create_sheet(name)
        for r in extra:
            w.append(r)
    buf = io.BytesIO()
    wb.save(buf)
    return buf.getvalue()


def read(data: bytes, name: str, target: tables.Target, sheet: int = 0, **opts):
    return tables.import_table(sheets.read_file(data, name)[sheet], target, opts)


# ---- units ----------------------------------------------------------------------

@pytest.mark.parametrize("header, name, unit", [
    ("speed [km/h]", "speed", "km/h"),
    ("n (rpm)", "n", "1/min"),
    ("Torque [Nm]", "Torque", "N·m"),
    ("Drehzahl [U/min]", "Drehzahl", "1/min"),
    ("speed_meters_per_second", "speed", "m/s"),
    ("time_seconds", "time", "s"),
    ("t_s", "t", "s"),
    ("Power loss in W", "Power loss", "W"),
    ("Temp [degC]", "Temp", "°C"),
    ("Torque", "Torque", None),
    # a header that is only a unit
    ("rad/s", "rad/s", "rad/s"),
    ("kW", "kW", "kW"),
    ("mph", "mph", "mph"),
    ("%", "%", "%"),
    # a letter alone, or "min", names a quantity as often as a unit
    ("t", "t", None),
    ("v", "v", None),
    ("min", "min", None),
])
def test_units_are_read_from_headers(header, name, unit):
    got = units.split_header(header)
    assert got[0] == name and got[1] == unit


def test_milli_and_mega_are_told_apart_by_their_case():
    assert units.canonical("MW") == "MW" and units.canonical("MWh") == "MWh"
    assert units.canonical("MJ") == "MJ" and units.canonical("mΩ") == "mΩ"
    # milliwatts and the like are not units LightSim knows: refused, never
    # read as MW (10^9 times too large)
    for u in ("mW", "mw", "mWh", "mJ", "MΩ"):
        assert units.canonical(u) is None, u
    assert units.canonical("mohm") == "mΩ" and units.canonical("kw") == "kW"


def test_a_header_that_is_only_a_unit_is_read():
    engine = tables.target_for("engine.combustion", "full_load_torque")  # 1/min -> N·m
    r = read(b"rad/s,Nm\n100,10\n200,20\n", "wot.csv", engine)
    assert r.units["x"].used == "rad/s" and r.units["x"].how == "header"
    assert r.value == {"954.929658551": 10.0, "1909.8593171": 20.0}
    # a unit of the wrong kind is refused
    r = read(b"speed,kW\n1000,10\n2000,20\n", "wot.csv", engine)
    assert r.value is None and "is in kW, which is not a unit of" in r.errors[0].text
    # in a map's corner, an axis's unit alone is not the values' unit
    rows = [["rpm", 0, 1000], [0, 0.1, 0.2], [100, 0.3, 0.4]]
    r = read(xlsx(rows), "m.xlsx", LOSS)
    assert not r.errors and r.units["value"].how == "assumed"
    rows[0][0] = "MW"
    r = read(xlsx(rows), "m.xlsx", LOSS)
    assert r.units["value"].used == "MW" and r.units["value"].how == "header"
    assert r.value["0"]["0"] == pytest.approx(100.0)


def test_unit_conversions():
    assert units.convert(36, "km/h", "m/s") == pytest.approx(10)
    assert units.convert(10, "mph", "km/h") == pytest.approx(16.09344)
    assert units.convert(100, "rad/s", "1/min") == pytest.approx(954.92966)
    assert units.convert(1500, "W", "kW") == pytest.approx(1.5)
    assert units.convert(0, "°C", "K") == pytest.approx(273.15)
    assert units.convert(32, "°F", "°C") == pytest.approx(0)
    with pytest.raises(ValueError):
        units.convert(1, "kg", "m")


# ---- files ----------------------------------------------------------------------

def test_csv_dialects_are_recognised():
    semi = "SOC [%];OCV [V]\n0;300,5\n50;350,25\n100;400\n".encode("cp1252")
    r = read(semi, "ocv.csv", OCV)
    assert r.value == {"0": 300.5, "50": 350.25, "100": 400.0}
    assert "decimal comma" in r.notes[0] and "semicolon" in r.notes[0]
    tab = "SOC\tOCV\r\n0\t300\r\n100\t400\r\n".encode("utf-16")
    assert read(tab, "ocv.txt", OCV).value == {"0": 300.0, "100": 400.0}
    bom = "﻿SOC [%],OCV [V]\n0,3.0\n100,4.2\n".encode("utf-8")
    assert read(bom, "cell.csv", OCV).value == {"0": 3.0, "100": 4.2}


ENGINE_FULL_LOAD = tables.target_for("engine.combustion", "full_load_torque")  # 1/min -> N·m


def test_thousands_separators_are_not_decimal_marks():
    # German Excel: semicolons, a decimal comma, dots between thousands
    de = "Drehzahl [U/min];Moment [Nm]\n1.000;12,5\n2.000;13,5\n3.000;14,5\n".encode("cp1252")
    r = read(de, "vl.csv", ENGINE_FULL_LOAD)
    assert r.value == {"1000": 12.5, "2000": 13.5, "3000": 14.5}
    assert "decimal comma" in r.notes[0] and "1.000 = 1000" in r.notes[0]
    assert r.decimal == "comma" and r.decimal_question is None
    # US Excel, tab-separated: commas between thousands, a decimal point
    us = b"Speed [rpm]\tTorque [Nm]\n1,000\t120.5\n2,000\t130\n3,000\t140\n"
    r = read(us, "wot.tsv", ENGINE_FULL_LOAD)
    assert r.value == {"1000": 120.5, "2000": 130.0, "3000": 140.0}
    assert "decimal comma" not in r.notes[0] and "1,000 = 1000" in r.notes[0]
    assert r.decimal == "point" and r.decimal_question is None
    assert sheets.parse_number("1.234.567,5", True) == 1234567.5
    assert sheets.parse_number("1,234,567.5") == 1234567.5
    assert sheets.parse_number("0,125", True) == 0.125
    assert sheets.parse_number("0,125") is None  # not a thousands separator


def test_a_file_that_does_not_show_its_decimal_mark_asks():
    # 1,000 alone is 1 (decimal comma) or 1000 (commas between thousands)
    us = b"Speed [rpm]\tTorque [Nm]\n1,000\t120\n2,000\t130\n3,000\t140\n"
    r = read(us, "wot.tsv", ENGINE_FULL_LOAD)
    assert r.value == {"1000": 120.0, "2000": 130.0, "3000": 140.0}
    assert r.decimal == "point" and "Cell A2 holds 1,000" in r.decimal_question
    r = tables.import_table(sheets.read_file(us, "wot.tsv", "comma")[0], ENGINE_FULL_LOAD)
    assert r.value == {"1": 120.0, "2": 130.0, "3": 140.0}
    assert r.decimal == "comma" and r.decimal_question is None
    # semicolons go with a decimal comma: 1.000 is one thousand
    de = b"Drehzahl [U/min];Moment [Nm]\n1.000;120\n2.000;130\n"
    r = read(de, "vl.csv", ENGINE_FULL_LOAD)
    assert r.value == {"1000": 120.0, "2000": 130.0} and "1.000" in r.decimal_question
    # in a comma-separated file 1.250 has a decimal point
    assert sheets.read_file(b"x,y\n1.250,2\n", "a.csv")[0].question is None
    # the dialog's choice reaches the reader
    body = {"filename": "wot.tsv", "data": base64.b64encode(us).decode(),
            "componentDefId": "engine.combustion", "paramKey": "full_load_torque"}
    got = client.post("/api/import/table", json=body).json()
    assert got["decimal"] == "point" and got["decimalQuestion"]
    got = client.post("/api/import/table", json={**body, "decimal": "comma"}).json()
    assert got["value"] == {"1": 120.0, "2": 130.0, "3": 140.0}
    assert got["decimal"] == "comma" and got["decimalQuestion"] is None


def test_fastsim_cycle_layout_with_grade():
    data = b"time_seconds,speed_meters_per_second,grade\n0,0,0\n1,0.5,0.01\n2,1.25,0.02\n"
    r = read(data, "cycle.csv", DRIVE)
    assert r.value == "0:0; 1:1.8; 2:4.5"
    assert r.units["y"].used == "m/s" and r.units["y"].how == "header"
    grade = read(data, "cycle.csv", tables.target_for("signal.road_profile", "profile", "time"))
    assert grade.y_column == 2 and grade.value == "0:0; 1:1; 2:2"
    assert grade.units["y"].how == "guessed" and grade.units["y"].question


def test_a_speed_without_a_unit_is_guessed_and_can_be_changed():
    data = b"t,v\n0,0\n10,12\n20,30\n"
    r = read(data, "trace.csv", DRIVE)
    assert r.units["y"].used == "m/s" and r.units["y"].how == "guessed"
    assert "m/s" in r.units["y"].question
    r = read(data, "trace.csv", DRIVE, units={"y": "km/h"})
    assert r.value == "0:0; 10:12; 20:30" and r.units["y"].how == "chosen"
    fast = b"t,v\n0,0\n10,60\n20,120\n"
    assert read(fast, "trace.csv", DRIVE).units["y"].used == "km/h"


def test_a_logger_layout_with_a_row_of_units():
    data = b"Time,Ground Speed,Throttle\ns,mph,%\n0,0,0\n0.1,1,20\n0.2,2.5,40\n"
    r = read(data, "motec.csv", DRIVE)
    assert r.units["y"].used == "mph" and r.columns[1]["written"] == "mph"
    assert r.value == "0:0; 0.1:1.609344; 0.2:4.02336"


def test_a_20x30_motor_loss_map_from_excel_with_axes_and_units():
    # as a supplier sends it: speeds down the rows in rpm, torques along the
    # top in N·m, losses in W; LightSim stores [speed][torque] in kW
    speeds = [i * 500 for i in range(30)]
    torques = [j * 15 for j in range(20)]
    rows = [["E-motor loss map, supplier X"], ["Loss [W]"],
            ["Speed [rpm] \\ Torque [Nm]", *torques]]
    rows += [[s, *(round(50 + s * 0.05 + t * t * 0.02, 3) for t in torques)] for s in speeds]
    r = read(xlsx(rows, "Losses"), "map.xlsx", LOSS)
    assert not r.errors, r.errors
    assert r.transpose is True  # the file's columns are torque: swapped
    assert sorted(map(float, r.value)) == speeds
    assert sorted(map(float, r.value["1500"])) == torques
    assert r.value["1500"]["30"] == pytest.approx((50 + 1500 * 0.05 + 900 * 0.02) / 1000)
    assert r.units["value"].used == "W" and r.units["rows"].used == "1/min"
    assert r.points == 600


def test_a_map_laid_out_as_lightsim_shows_it_is_not_swapped():
    rows = [["Torque [N·m] \\ Speed [1/min]", 0, 1000, 2000], [0, 0.1, 0.2, 0.3],
            [100, 0.4, 0.5, 0.6]]
    r = read(xlsx(rows), "m.xlsx", LOSS)
    assert r.transpose is False
    assert r.value == {"0": {"0": 0.1, "100": 0.4}, "1000": {"0": 0.2, "100": 0.5},
                       "2000": {"0": 0.3, "100": 0.6}}
    # labels that contradict a forced swap are an error, not a silent mix-up
    assert read(xlsx(rows), "m.xlsx", LOSS, transpose=True).errors
    rows[0][0] = None  # no labels: the user's choice decides
    swapped = read(xlsx(rows), "m.xlsx", LOSS, transpose=True)
    assert swapped.value["100"] == {"0": 0.4, "1000": 0.5, "2000": 0.6}


def test_an_axis_label_lightsim_does_not_know_does_not_stop_the_swap():
    # torques along the top, labelled; speeds down the side under a label
    # LightSim does not recognise ("Shaft")
    rows = [[None, None, "Torque"], [None, None, 0, 100, 200],
            ["Shaft", 1000, 0.5, 0.6, 0.7], [None, 3000, 0.8, 0.9, 1.0],
            [None, 6000, 1.1, 1.2, 1.3]]
    r = read(xlsx(rows), "m.xlsx", LOSS)
    assert not r.errors, r.errors
    assert r.transpose is True and any("swapped" in n for n in r.notes)
    assert r.value["1000"] == {"0": 0.5, "100": 0.6, "200": 0.7}
    assert sorted(map(float, r.value)) == [1000, 3000, 6000]
    # without the label, the same
    rows[2][0] = None
    assert read(xlsx(rows), "m.xlsx", LOSS).transpose is True
    # two labels that name the same quantity: not swapped, and the note says why
    rows[2][0] = "Torque"
    r = read(xlsx(rows), "m.xlsx", LOSS)
    assert r.transpose is False and any("disagree" in n for n in r.notes)
    # labels LightSim does not know only: as read, and the note says so
    rows[0][2], rows[2][0] = "Shaft", "Load"
    r = read(xlsx(rows), "m.xlsx", LOSS)
    assert r.transpose is False and any("do not say which axis" in n for n in r.notes)


def test_a_range_picks_one_table_of_several():
    rows = [["SOC [%]", "OCV [V]", None, "SOC [%]", "OCV [V]"],
            [0, 3.0, None, 0, 300], [100, 4.2, None, 100, 400]]
    r = read(xlsx(rows), "two.xlsx", OCV, range="D1:E3")
    assert r.value == {"0": 300.0, "100": 400.0}
    r = read(xlsx(rows), "two.xlsx", OCV, xColumn=3, yColumn=4)
    assert r.value == {"0": 300.0, "100": 400.0}


def test_sheets_are_listed_and_picked_by_name():
    data = xlsx([["notes"]], "Read me", more={"Cell": [["SOC", "OCV"], [0, 3], [100, 4]]})
    got = sheets.read_file(data, "book.xlsx")
    assert [s.name for s in got] == ["Read me", "Cell"]
    r = client.post("/api/import/table", json={
        "filename": "book.xlsx", "data": base64.b64encode(data).decode(),
        "componentDefId": "battery.generic", "paramKey": "ocv_table"}).json()
    assert r["sheet"] == "Cell" and r["ok"] and r["value"] == {"0": 3.0, "100": 4.0}
    assert [s["name"] for s in r["sheets"]] == ["Read me", "Cell"]
    assert r["cells"][0] == ["SOC", "OCV"]


# ---- ten malformed files, each with its own message ----------------------------------

MALFORMED = [
    ("empty.csv", b"", OCV, "The file is empty."),
    ("binary.csv", b"\x00\x01\x02\x03" * 20, OCV, "does not look like a text (CSV) file"),
    ("old.xls", b"\xd0\xcf\x11\xe0 old excel", OCV, "Excel 97-2003 file (.xls)"),
    ("broken.xlsx", b"PK\x03\x04 not really a zip", OCV, "could not be unzipped"),
    ("text.csv", b"SOC [%],OCV [V]\n0,300\n50,abc\n100,400\n", OCV,
     "Row 3: 'abc' in cell B3 (Open-Circuit Voltage"),
    ("hole.csv", b"SOC [%],OCV [V]\n0,300\n50,\n100,400\n", OCV,
     "Row 3: cell B3 (Open-Circuit Voltage"),
    ("order.csv", b"SOC [%],OCV [V]\n0,300\n60,350\n40,340\n100,400\n", OCV,
     "Row 4: SOC 40 % is below the row before (60 %)"),
    ("repeat.csv", b"SOC [%],OCV [V]\n0,300\n50,350\n50,351\n", OCV,
     "Row 4: SOC 50 % repeats the row before"),
    ("kind.csv", b"time [s],speed [kg]\n0,0\n10,20\n", DRIVE,
     "is in kg, which is not a unit of target speed"),
    ("unknown.csv", b"distance [furlong],grade [%]\n0,0\n100,2\n", ROAD,
     "the unit 'furlong' is not one LightSim knows"),
    ("one-column.csv", b"OCV [V]\n300\n350\n", OCV, "No data found"),
    ("no-map.csv", b"a,b\n1,2\n3,4\n", LOSS, "No map found"),
    ("map-hole.csv", b"x,0,1000\n0,1,2\n100,,3\n", LOSS,
     "Row 3: cell B3 is empty; every point of the map needs a value."),
]


@pytest.mark.parametrize("name, data, target, message", MALFORMED,
                         ids=[m[0] for m in MALFORMED])
def test_malformed_files_say_what_is_wrong(name, data, target, message):
    try:
        r = read(data, name, target)
    except sheets.SheetError as e:
        assert message in str(e)
        return
    assert r.errors, f"{name} was accepted"
    assert message in r.errors[0].text, r.errors[0].text


def test_errors_carry_their_row_and_cell():
    r = read(b"SOC [%],OCV [V]\n0,300\n50,abc\n", "t.csv", OCV)
    assert (r.errors[0].row, r.errors[0].cell) == (3, "B3")
    body = r.as_dict()
    assert body["ok"] is False and body["value"] is None


def test_the_api_refuses_unknown_parameters_and_bad_files():
    good = base64.b64encode(b"a,b\n1,2\n").decode()
    r = client.post("/api/import/table", json={"filename": "a.csv", "data": good,
                                               "componentDefId": "motor.emotor",
                                               "paramKey": "nope"})
    assert r.status_code == 400
    r = client.post("/api/import/table", json={"filename": "a.csv", "data": "%%%",
                                               "componentDefId": "battery.generic",
                                               "paramKey": "ocv_table"})
    assert r.status_code == 400
    r = client.post("/api/import/table", json={
        "filename": "a.xls", "data": base64.b64encode(b"\xd0\xcf").decode(),
        "componentDefId": "battery.generic", "paramKey": "ocv_table"})
    assert r.status_code == 400 and ".xls" in r.json()["detail"]


def test_the_xlsx_reader_reads_what_excel_writes():
    wb = openpyxl.Workbook()
    ws = wb.active
    ws["A1"] = "Gear [-]"
    ws["B1"] = "Ratio"
    ws["A2"], ws["B2"] = 1, 3.5
    ws["A3"], ws["B3"] = 2, "=B2/1.75"  # a formula: openpyxl writes no cached value
    ws["A4"], ws["B4"] = True, None
    buf = io.BytesIO()
    wb.save(buf)
    rows = sheets.read_xlsx(buf.getvalue())[0].rows
    assert rows[0] == ["Gear [-]", "Ratio"]
    assert rows[1] == [1.0, 3.5]
    assert rows[3][0] == "TRUE"


def test_xlsx_written_by_lightsim_reads_back():
    data = sheets.xlsx_bytes([sheets.OutSheet("A/B:C", [["x", "y"], [1, 2.5], [2, None]]),
                              sheets.OutSheet("A/B:C", [["second"]])])
    got = sheets.read_xlsx(data)
    assert [s.name for s in got] == ["A_B_C", "A_B_C (2)"]
    assert got[0].rows == [["x", "y"], [1.0, 2.5], [2.0]]
    wb = openpyxl.load_workbook(io.BytesIO(data))
    assert wb["A_B_C"]["B2"].value == 2.5


def test_a_workbook_with_a_dtd_is_refused():
    import zipfile
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        z.writestr("xl/workbook.xml", '<?xml version="1.0"?><!DOCTYPE x [<!ENTITY a "b">]><x/>')
    with pytest.raises(sheets.SheetError, match="DTD"):
        sheets.read_xlsx(buf.getvalue())


def test_import_table_on_the_command_line(tmp_path, capsys):
    f = tmp_path / "ocv.csv"
    f.write_bytes(b"SOC [%],OCV [V]\n0,300\n100,400\n")
    assert cli_main(["import-table", str(f), "--part", "battery.generic",
                     "--param", "ocv_table", "--json"]) == 0
    assert '"100": 400.0' in capsys.readouterr().out
    f.write_bytes(b"SOC [%],OCV [V]\n0,300\n100,x\n")
    assert cli_main(["import-table", str(f), "--part", "battery.generic",
                     "--param", "ocv_table"]) == 1
    capsys.readouterr()
    f.write_bytes(b"Speed [rpm]\tTorque [Nm]\n1,000\t120\n2,000\t130\n")
    assert cli_main(["import-table", str(f), "--part", "engine.combustion",
                     "--param", "full_load_torque"]) == 0
    out = capsys.readouterr().out
    assert "Give --decimal comma if that is wrong." in out and '"1000": 120.0' in out
    assert cli_main(["import-table", str(f), "--part", "engine.combustion",
                     "--param", "full_load_torque", "--decimal", "comma"]) == 0
    assert '"1": 120.0' in capsys.readouterr().out
