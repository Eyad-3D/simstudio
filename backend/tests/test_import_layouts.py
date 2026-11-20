"""Table data in every layout, from a file or pasted text (layouts.py)."""
from __future__ import annotations

import base64
import itertools
import random

from app.dataio import layouts
from app.dataio.layouts import DataTarget, data_target, parse_label, read_data, split_title
from app.dataio.params import _long_rows
from app.dataio.sheets import OutSheet, read_csv, read_file, xlsx_bytes
from app.solver.maps import parse_grid
from fastapi.testclient import TestClient

LOSS = ("motor.emotor", "power_loss")  # Speed [1/min] x Torque [N·m] -> kW


def sheets(text: str) -> list:
    return [read_csv(text.encode("utf-8"), "Pasted text")]


def run(text: str, target=LOSS, **opts) -> dict:
    t = target if isinstance(target, DataTarget) else data_target(*target)
    return read_data(sheets(text) if isinstance(text, str) else text, t, opts)


def errors(res: dict) -> list[str]:
    return [p["text"] for p in res["problems"] if p["level"] == "error"]


def three_axes(speed_unit: str = "1/min") -> DataTarget:
    """A Map block of three inputs and two outputs."""
    return data_target("signal.map", "maps", [
        {"id": "temp", "name": "Temperature", "direction": "input", "unitGroup": "Temperature"},
        {"id": "speed", "name": "Speed", "direction": "input", "unitGroup": "Rotational Speed"},
        {"id": "torque", "name": "Torque", "direction": "input", "unitGroup": "Torque"},
        {"id": "loss", "name": "Loss", "direction": "output", "unitGroup": "Power"},
        {"id": "eff", "name": "Efficiency", "direction": "output", "unitGroup": "Percent"},
    ])


# ---- titles and labels -------------------------------------------------------------

def test_titles_say_names_and_units_the_way_people_write_them():
    assert split_title("Speed [rpm]") == ("Speed", "1/min", "rpm")
    assert split_title("Speed (1/min)") == ("Speed", "1/min", "1/min")
    assert split_title("Speed / rpm") == ("Speed", "1/min", "rpm")
    assert split_title("Speed in rad/s") == ("Speed", "rad/s", "rad/s")
    assert split_title("Torque_Nm") == ("Torque", "N·m", "Nm")
    assert split_title("T [degC]")[1] == "°C"


def test_labels_in_three_spellings_and_with_two_axes():
    assert parse_label("Temperature = 25 °C") == [("Temperature", 25.0, "°C", "°C")]
    assert parse_label("T [°C]: 25") == [("T", 25.0, "°C", "°C")]
    assert parse_label("T=25degC") == [("T", 25.0, "°C", "degC")]
    assert parse_label("25 °C") == [("", 25.0, "°C", "°C")]
    assert parse_label("T = 25,5 °C") == [("T", 25.5, "°C", "°C")]
    assert parse_label("T = 25 °C, U = 400 V") == [("T", 25.0, "°C", "°C"), ("U", 400.0, "V", "V")]
    assert parse_label("Sheet1") is None
    assert parse_label("Loss map of the motor") is None


# ---- two columns -------------------------------------------------------------------

def test_two_columns_with_a_title_and_a_note_beside():
    res = run("Supplier data, 2024\n\nSpeed [rpm]\tLoss [kW]\t\tmeasured on rig 3\n"
              "0\t0\n1000\t1.5\n2000\t3\n",
              data_target("signal.map", "maps", [
                  {"id": "n", "name": "Speed", "direction": "input",
                   "unitGroup": "Rotational Speed"},
                  {"id": "p", "name": "Loss", "direction": "output", "unitGroup": "Power"}]))
    assert res["ok"], res["problems"]
    assert res["layout"].startswith("Two columns")
    assert res["tables"] == {"p": {"0": 0.0, "1000": 1.5, "2000": 3.0}}
    assert [f["how"] for f in res["fields"]] == ["name and unit", "name and unit"]


# ---- a matrix ----------------------------------------------------------------------

MATRIX = """Loss map, motor A
Torque [Nm] \\ Speed [rpm]\t1000\t2000\t3000
10\t0.1\t0.2\t0.3
20\t0.4\t0.5\t0.6
"""


def test_a_matrix_with_names_in_the_corner():
    res = run(MATRIX)
    assert res["ok"], res["problems"]
    assert res["layout"].startswith("A matrix (pivot table)")
    loss = res["tables"]["value"]
    assert loss["2000"] == {"10": 0.2, "20": 0.5}  # Speed outer, Torque inner
    names = {f["name"]: f["target"] for f in res["fields"] if f["role"] == "axis"}
    assert names == {"Speed": "Speed", "Torque": "Torque"}


def test_a_matrix_the_other_way_round_is_matched_by_its_names():
    res = run("Speed [rpm] \\ Torque [Nm]\t10\t20\n1000\t0.1\t0.4\n2000\t0.2\t0.5\n")
    assert res["ok"], res["problems"]
    assert res["tables"]["value"]["2000"] == {"10": 0.2, "20": 0.5}


def test_a_matrix_with_no_names_is_read_in_order_and_the_preview_asks():
    res = run("\t1000\t2000\n10\t0.1\t0.2\n20\t0.4\t0.5\n")
    assert res["ok"]
    asked = [p["text"] for p in res["problems"] if p["level"] == "question"]
    assert len(asked) == 2 and "was read as Speed" in asked[0]
    # the user swaps them
    res = run("\t1000\t2000\n10\t0.1\t0.2\n20\t0.4\t0.5\n", fields={"0": "axis:1", "1": "axis:0"})
    assert res["tables"]["value"]["10"] == {"1000": 0.1, "2000": 0.2}


def test_a_matrix_in_other_units_is_converted_and_says_so():
    res = run("Torque [kNm] \\ Speed [rad/s]\t100\t200\n0.01\t100\t200\n0.02\t300\t400\n"
              "\nLoss in W\n")
    assert res["ok"], res["problems"]
    by = {f["name"]: f for f in res["fields"]}
    assert (by["Speed"]["used"], by["Speed"]["stored"]) == ("rad/s", "1/min")
    assert "954.929658551" in res["tables"]["value"]


def test_decimal_commas_with_semicolons_and_a_units_row():
    res = run("Speed;Loss\nrpm;kW\n0;0,5\n1.000;1,25\n2.000;2,5\n",
              data_target("signal.map", "maps", [
                  {"id": "n", "name": "Speed", "direction": "input",
                   "unitGroup": "Rotational Speed"},
                  {"id": "p", "name": "Loss", "direction": "output", "unitGroup": "Power"}]))
    assert res["ok"], res["problems"]
    assert res["tables"]["p"] == {"0": 0.5, "1000": 1.25, "2000": 2.5}


# ---- a list (long form) --------------------------------------------------------------

def test_a_three_axis_list_in_shuffled_order_with_two_value_columns():
    rows = [(t, n, m, t + n / 1000 + m, 90 - m / 10) for t in (25, 50) for n in (1000, 2000, 3000)
            for m in (10, 20)]
    random.Random(4).shuffle(rows)
    text = "Temperature [°C]\tSpeed [rpm]\tTorque [Nm]\tLoss [kW]\tEfficiency [%]\n" + "".join(
        "\t".join(f"{v:g}" for v in r) + "\n" for r in rows)
    res = run(text, three_axes())
    assert res["ok"], res["problems"]
    assert res["layout"].startswith("A list, a column per axis")
    assert res["tables"]["loss"]["50"]["2000"]["20"] == 72.0
    assert res["tables"]["eff"]["25"]["1000"] == {"10": 89.0, "20": 88.0}
    assert [a["count"] for a in res["axes"]] == [2, 3, 2]


def test_a_hole_is_refused_or_filled_from_the_nearest_breakpoint():
    text = "T [°C]\tn [rpm]\tM [Nm]\tLoss [kW]\tEff [%]\n" + "".join(
        f"{t}\t{n}\t{m}\t{t + m}\t90\n" for t, n, m in itertools.product((25, 50), (1000, 2000),
                                                                        (10, 20))
        if (t, n, m) != (50, 2000, 20))
    res = run(text, three_axes())
    assert not res["ok"] and res["holes"] == 1
    assert "1 point of the grid is missing, the first at Temperature 50 °C, Speed 2000 1/min, " \
           "Torque 20 N·m" in errors(res)[0]
    res = run(text, three_axes(), fill="nearest")
    assert res["ok"] and res["filled"] == 2  # one point, in each of the two outputs
    # nearest: one step along any axis; on a tie, the first in the grid's
    # order (Temperature 25 before Speed 1000 and Torque 10)
    assert res["tables"]["loss"]["50"]["2000"]["20"] == 45.0


def test_a_point_twice_is_named_with_its_cells():
    res = run("Speed [rpm]\tTorque [Nm]\tLoss [kW]\n1000\t10\t1\n1000\t20\t2\n2000\t10\t3\n"
              "2000\t20\t4\n1000\t10\t5\n")
    assert not res["ok"]
    assert "Cell C6: the point Speed 1000 1/min, Torque 10 N·m is in the data twice, as 1 (C2) " \
           "and 5" in errors(res)[0]


def test_a_cell_that_is_not_a_number_and_a_unit_of_another_kind():
    res = run("Speed [rpm]\tTorque [Nm]\tLoss [kW]\n1000\t10\t1\n1000\t20\tn/a\n2000\t10\t3\n"
              "2000\t20\t4\n")
    assert errors(res) == ["Cell C3: 'n/a' is not a number."]
    res = run("Speed [rpm]\tTorque [Nm]\tLoss [bar]\n1000\t10\t1\n1000\t20\t2\n2000\t10\t3\n"
              "2000\t20\t4\n")
    assert "Loss [bar] (Power Loss (Motor + Inverter)) is in bar, which is not a unit of " \
           "Power Loss (Motor + Inverter) (kW, W, MW, hp, PS)" in errors(res)[0]
    # the user reads it in W instead
    res = run("Speed [rpm]\tTorque [Nm]\tLoss [bar]\n1000\t10\t1\n1000\t20\t2\n2000\t10\t3\n"
              "2000\t20\t4\n", units={"2": "W"})
    assert res["ok"] and res["tables"]["value"]["2000"]["20"] == 0.004


def test_an_axis_the_target_does_not_have():
    res = run("T [°C]\tSpeed [rpm]\tTorque [Nm]\tLoss [kW]\n" + "".join(
        f"{t}\t{n}\t{m}\t1\n" for t, n, m in itertools.product((25, 50), (1000, 2000), (10, 20))))
    assert "The data has an axis T [°C] (column A, 2 breakpoints) that Power Loss (Motor + " \
           "Inverter) does not have (its axes: Speed, Torque)" in errors(res)[0]


# ---- several matrices, one per breakpoint ------------------------------------------------

def _block(t: float, label: str) -> str:
    return (f"{label}\nTorque [Nm] \\ Speed [rpm]\t1000\t2000\n"
            f"10\t{t + 1}\t{t + 2}\n20\t{t + 3}\t{t + 4}\n\n")


def test_matrices_one_under_another_with_labels_in_three_spellings():
    text = _block(25, "Temperature = 25 °C") + _block(50, "T [°C]: 50") + _block(75, "T=75degC")
    t = data_target("signal.map", "maps", [
        {"id": "temp", "name": "Temperature", "direction": "input", "unitGroup": "Temperature"},
        {"id": "speed", "name": "Speed", "direction": "input", "unitGroup": "Rotational Speed"},
        {"id": "torque", "name": "Torque", "direction": "input", "unitGroup": "Torque"},
        {"id": "loss", "name": "Loss", "direction": "output", "unitGroup": "Power"}])
    res = run(text, t)
    assert res["ok"], res["problems"]
    assert res["layout"].startswith("3 tables, each a matrix (pivot table), one per Temperature")
    assert res["tables"]["loss"]["75"]["2000"] == {"10": 77.0, "20": 79.0}


def test_matrices_side_by_side_with_two_label_axes():
    rows = [["T = 25 °C, U = 400 V", None, None, None, "T = 50 °C, U = 400 V"],
            ["Torque [Nm] \\ Speed [rpm]", 1000.0, 2000.0, None, "Torque [Nm] \\ Speed [rpm]",
             1000.0, 2000.0],
            [10.0, 1.0, 2.0, None, 10.0, 5.0, 6.0],
            [20.0, 3.0, 4.0, None, 20.0, 7.0, 8.0]]
    from app.dataio.sheets import Sheet
    t = data_target("signal.map", "maps", [
        {"id": "temp", "name": "T", "direction": "input", "unitGroup": "Temperature"},
        {"id": "speed", "name": "Speed", "direction": "input", "unitGroup": "Rotational Speed"},
        {"id": "torque", "name": "Torque", "direction": "input", "unitGroup": "Torque"},
        {"id": "loss", "name": "Loss", "direction": "output", "unitGroup": "Power"}])
    res = read_data([Sheet("S", rows)], t, {})
    # U has one breakpoint (400 V) and the map no such input: left out, and said
    assert res["ok"], res["problems"]
    assert any("U [V] has one breakpoint (400)" in n for n in res["notes"])
    assert res["tables"]["loss"]["50"]["1000"] == {"10": 5.0, "20": 7.0}


def test_an_excel_file_with_a_sheet_per_breakpoint():
    def sheet(t: float) -> OutSheet:
        return OutSheet(f"{t:g} °C", [["Loss [kW]", "Speed [rpm]"], [None, 1000, 2000],
                                       [10, t, t + 1], [20, t + 2, t + 3]])
    data = xlsx_bytes([sheet(25), sheet(50), OutSheet("Notes", [["from the supplier"]])])
    res = read_data(read_file(data, "map.xlsx"), three_axes(), {})
    # a Loss table only; the Efficiency output has no column
    assert "No values found for Efficiency" in errors(res)[0]
    two = data_target("signal.map", "maps", [
        {"id": "temp", "name": "Temperature", "direction": "input", "unitGroup": "Temperature"},
        {"id": "speed", "name": "Speed", "direction": "input", "unitGroup": "Rotational Speed"},
        {"id": "torque", "name": "Torque", "direction": "input", "unitGroup": "Torque"},
        {"id": "loss", "name": "Loss", "direction": "output", "unitGroup": "Power"}])
    res = read_data(read_file(data, "map.xlsx"), two, {})
    assert res["ok"], res["problems"]
    assert res["sheet"] == "*" and res["layout"].startswith("2 worksheets")
    assert res["tables"]["loss"]["50"]["2000"] == {"10": 51.0, "20": 53.0}


def test_unlabelled_tables_ask_which_one():
    text = MATRIX + "\n\n" + MATRIX.replace("0.1", "9")
    res = run(text)
    assert res["ok"] and res["block"] == 0
    assert any(p["level"] == "question" and "2 tables were found" in p["text"]
               for p in res["problems"])
    assert run(text, block=1)["tables"]["value"]["1000"]["10"] == 9.0


# ---- a pivot table with two key columns ---------------------------------------------------

def test_a_pivot_table_with_two_key_columns_blank_where_they_repeat():
    text = ("T [°C]\tSpeed [rpm]\t10\t20\n25\t1000\t1\t2\n\t2000\t3\t4\n50\t1000\t5\t6\n"
            "\t2000\t7\t8\n")
    t = data_target("signal.map", "maps", [
        {"id": "temp", "name": "T", "direction": "input", "unitGroup": "Temperature"},
        {"id": "speed", "name": "Speed", "direction": "input", "unitGroup": "Rotational Speed"},
        {"id": "torque", "name": "Torque", "direction": "input", "unitGroup": "Torque"},
        {"id": "loss", "name": "Loss", "direction": "output", "unitGroup": "Power"}])
    res = run(text, t)
    assert res["ok"], res["problems"]
    assert res["tables"]["loss"]["50"]["2000"] == {"10": 7.0, "20": 8.0}


# ---- a Map block built from the data ---------------------------------------------------

def test_a_map_block_is_built_from_the_data_with_the_user_s_yes():
    text = "Temperature [°C]\tSpeed [rad/s]\tTorque [Nm]\tLoss [kW]\tEfficiency [%]\tNote [bar]\n" + \
        "".join(f"{t}\t{n}\t{m}\t1\t90\t2\n"
                for t, n, m in itertools.product((25, 50), (100, 200), (10, 20)))
    empty = data_target("signal.map", "maps", [])
    res = run(text, empty)
    assert not res["ok"] and res["canBuild"]
    res = run(text, empty, build=True)
    assert res["ok"], res["problems"]
    ports = res["ports"]
    assert [(p["id"], p["direction"], p["unitGroup"], p.get("unitText"), p["display"])
            for p in ports] == [
        ("temperature", "input", "Temperature", None, None),
        ("speed", "input", "Rotational Speed", None, "rad/s"),
        ("torque", "input", "Torque", None, None),
        ("loss", "output", "Power", None, None),
        ("efficiency", "output", "Percent", None, None),
        ("note", "output", "No Unit", "bar", None)]
    # rad/s stored as 1/min, shown in rad/s
    assert list(res["tables"]["loss"]["25"]) == ["954.929658551", "1909.8593171"]
    assert res["tables"]["note"]["50"]["954.929658551"]["20"] == 2.0


# ---- the round trip ---------------------------------------------------------------------

def test_the_parameter_sheet_s_long_form_comes_back_the_same():
    axes = [("Temperature", "°C"), ("Speed", "1/min"), ("Torque", "N·m")]
    table = {f"{t:g}": {f"{n:g}": {f"{m:g}": t + n / 1000 + m / 7 for m in (0, 50, 100)}
                        for n in (0, 4000)} for t in (-20, 25, 60)}
    rows = _long_rows("Map", (axes, [(None, "Loss", "kW")]), table)
    data = xlsx_bytes([OutSheet("Map", rows)])
    t = DataTarget("Map", [layouts._slot(str(i), n, u) for i, (n, u) in enumerate(axes)],
                   [layouts._slot("value", "Loss", "kW")])
    res = read_data(read_file(data, "x.xlsx"), t, {})
    assert res["ok"], res["problems"]
    assert parse_grid(res["tables"]["value"], 3, ["T", "n", "M"]) == \
        parse_grid(table, 3, ["T", "n", "M"])


# ---- the endpoint ------------------------------------------------------------------------

def test_the_endpoint_reads_a_file_and_pasted_text_alike():
    from app.main import app
    c = TestClient(app)
    body = {"componentDefId": "motor.emotor", "paramKey": "power_loss"}
    pasted = c.post("/api/import/data", json={**body, "text": MATRIX}).json()
    filed = c.post("/api/import/data", json={
        **body, "filename": "m.csv",
        "data": base64.b64encode(MATRIX.encode()).decode()}).json()
    assert pasted["ok"] and pasted["source"] == "text"
    assert pasted["tables"] == filed["tables"]
    assert c.post("/api/import/data", json={**body, "text": "  "}).status_code == 400
