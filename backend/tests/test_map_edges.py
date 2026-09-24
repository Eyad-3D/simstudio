"""Maps and machines at the edge of their data (MOD-18).

Every table axis has an "outside the data" setting: Error stops the run,
Clamp holds the edge value (what every table did before 0.3), Linear
extends the edge slope. Motor and engine speed and torque axes stop the run
by default. An E-Motor has a maximum speed (0 = its full-load curve's last
point): its drive torque falls to zero over the last 2 % below it and its
inverter is off above it. The run counts the time each table and machine
spends outside its data (RunContext.map_use), and the summary lists it when
it happened. Data Checks say beforehand where the library data do not fit.
"""
import pytest
from helpers import bev_axle, conn, dbc, el, example_result, project, series

from app.library import library_by_id
from app.solver import (
    Map,
    MapUse,
    OutsideDataError,
    core,
    interp1,
    interp2,
    parse_table1d,
    parse_table2d,
    simulate,
)
from app.storage import load_example
from app.validation import validate_project

EDGE_ROWS = ("— time outside its", "— furthest", "— time above maximum speed",
             "— highest speed")


def _summary(result) -> dict[str, float]:
    return {s.label: s.value for s in result.summary}


def _texts(result, level: str | None = None) -> list[str]:
    return [m.text for m in result.messages if level is None or m.level == level]


def _max(result, el_id: str, port: str) -> float:
    return max(p["value"] for p in series(result, el_id, port))


def _el(proj, el_id: str):
    return next(e for e in proj.systems[0].elements if e.id == el_id)


def _map(pts, policy: list[str], name: str = "T") -> Map:
    axes = ("X", "Y")[:len(policy)]
    return Map(pts, name, policy, [MapUse("e", "'T' table", a, "-", 0.0) for a in axes])


# ---- the three settings ---------------------------------------------------------

def test_policies():
    pts1 = parse_table1d({"0": 0, "10": 10})
    pts2 = parse_table2d({"0": {"0": 0, "10": 10}, "10": {"0": 10, "10": 20}})

    clamp = _map(pts1, ["clamp"])
    for x in (-5.0, 3.0, 12.0):
        assert clamp.at(x) == interp1(pts1, x)
    clamp2 = _map(pts2, ["clamp", "clamp"])
    for x, y in ((-5.0, 3.0), (12.0, 14.0), (5.0, -1.0)):
        assert clamp2.at(x, y) == interp2(pts2, x, y)

    linear = _map(pts1, ["linear"])
    assert linear.at(12.0) == pytest.approx(12.0)
    assert linear.at(-2.0) == pytest.approx(-2.0)
    linear2 = _map(pts2, ["linear", "linear"])
    assert linear2.at(12.0, 14.0) == pytest.approx(26.0)  # 12 + 14 on a plane
    assert linear2.at(-2.0, 5.0) == pytest.approx(3.0)
    assert _map(pts2, ["linear", "clamp"]).at(12.0, 14.0) == pytest.approx(22.0)

    error = _map(pts2, ["clamp", "error"], name="E-Motor 'M' Power Loss")
    assert error.at(15.0, 5.0) == pytest.approx(15.0)  # the Clamp axis holds
    with pytest.raises(OutsideDataError) as e:
        error.at(5.0, 12.5)
    assert str(e.value) == "E-Motor 'M' Power Loss: Y 12.5 - is outside its data (0 to 10 -)"

    # a single point is a constant: never outside, even under Error
    const = _map(parse_table2d({"0": {"0": 0}}), ["error", "error"])
    assert const.at(1e6, -1e6) == 0.0
    assert const.count(0.0, 0.01, 1e6, -1e6) == []

    # counting: dt per step outside, whichever edge; the furthest point and
    # its time kept; the first time returns the record
    m = _map(pts1, ["clamp"])
    use = m.uses[0]
    assert m.count(0.0, 0.01, 5.0) == []
    assert m.count(1.0, 0.01, 11.0) == [use]
    assert m.count(2.0, 0.01, -4.0) == []
    assert m.count(3.0, 0.01, 12.0) == []
    assert use.outside_s == pytest.approx(0.03)
    assert (use.value, use.edge, use.t) == (-4.0, 0.0, 2.0)
    # an Error axis never counts: the run stopped before
    assert _map(pts1, ["error"]).count(0.0, 0.01, 50.0) == []


# ---- E-Motor maximum speed ----------------------------------------------------------

def test_300_kmh_target_stops_at_the_motors_maximum_speed():
    """Before, the motor ran to 21,238 1/min (268.9 km/h) on a full-load map
    that ends at 12,000, and no message mentioned it."""
    proj = bev_axle(profile="0:0; 3:300; 300:300")
    proj.cases[0].duration = 300
    result = simulate(proj, "case")
    rpm = _max(result, "mot", "sig_speed")
    assert 0.97 * 12000 <= rpm <= 12000
    assert result.status == "warning"  # the car cannot follow 300 km/h
    assert any("maximum speed" in t and "12,000 1/min" in t for t in _texts(result, "info"))
    assert not [s for s in _summary(result) if any(r in s for r in EDGE_ROWS)]


def test_maximum_speed_below_the_map_is_a_speed_limiter():
    proj = bev_axle(profile="0:0; 3:300; 60:300")
    proj.cases[0].duration = 60
    _el(proj, "mot").parameterOverrides["max_speed_rpm"] = 9000
    result = simulate(proj, "case")
    assert 0.97 * 9000 <= _max(result, "mot", "sig_speed") <= 9000
    assert any("maximum speed (9,000 1/min)" in t for t in _texts(result, "info"))


def test_maximum_speed_beyond_the_map_stops_the_run():
    proj = bev_axle(profile="0:0; 3:300; 300:300")
    proj.cases[0].duration = 300
    _el(proj, "mot").parameterOverrides["max_speed_rpm"] = 14000
    result = simulate(proj, "case")
    assert result.status == "failed"
    [error] = _texts(result, "error")
    assert error.startswith("E-Motor 'E-Motor' Full-Load Torque: Speed 12")
    assert "is outside its data (0 to 12000 1/min) at t = " in error
    assert "set to stop the run (Error)" in error
    # the channels are kept up to the stop
    speed = series(result, "veh", "sig_speed")
    assert 10 < speed[-1]["t"] < 300
    assert speed[-1]["value"] > 140


def test_a_motor_driven_past_its_maximum_speed_is_flagged():
    """200 km/h turns the motor at 15,700 1/min: above its 12,000 1/min
    maximum the inverter is off, so it gives no drive torque until the car
    has slowed down."""
    proj = bev_axle(profile="0:200; 30:200")
    proj.cases[0].duration = 30
    _el(proj, "veh").parameterOverrides["initial_speed_kmh"] = 200
    result = simulate(proj, "case")
    assert any("was driven above its maximum speed (12,000 1/min) at t = 0.00 s" in t
               for t in _texts(result, "info"))
    summary = _summary(result)
    assert 0 < summary["E-Motor — time above maximum speed"] < 100
    assert summary["E-Motor — highest speed"] > 15000
    for n, tq in zip(series(result, "mot", "sig_speed"), series(result, "mot", "sig_torque")):
        if n["value"] > 12000:
            assert tq["value"] <= 0.0, n["t"]
    assert _max(result, "mot", "sig_speed") > 15000  # it started up there


@pytest.mark.parametrize("kind, params", [
    ("motor", {}),  # at 12,000 1/min to float noise
    ("motor", {"max_speed_rpm": 3000, "inertia_kgm2": 0.02}),  # a step jumps the 2 % taper
    ("engine", {"inertia_kgm2": 0.05}),  # a step overshoots the rev limit by 3 %
])
def test_a_machine_held_at_its_limiter_is_not_over_speed(kind, params):
    """A free-spinning machine at full command: its own last step can carry
    it past its limit before the limiter acts. Before, a light one had most
    of the run booked as over speed (16.5 of 20 s at 3,000 1/min)."""
    if kind == "motor":
        machine = el("m", "motor.emotor", "Machine", **params)
        supply = [el("batt", "battery.generic", "Battery"), el("bus", "electric.node", "Bus")]
        wires = [conn(2, "batt", "pos", "bus", "t1"), conn(3, "bus", "t3", "m", "pos")]
        port, n_max = "sig_demand_in", params.get("max_speed_rpm", 12000)
    else:
        machine = el("m", "engine.combustion", "Machine", **params)
        supply, wires, port, n_max = [], [], "sig_throttle_in", 6000
    proj = project([machine, el("sh", "mech.shaft", "Shaft"),
                    el("c", "signal.constant", "Command", value=1), *supply],
                   [conn(1, "m", "shaft", "sh", "flange_a"), *wires],
                   [dbc(1, "c", "sig_out", "m", port)], duration=5, time_step=0.5)
    result = simulate(proj, "case")
    assert _max(result, "m", "sig_speed") > 0.99 * n_max
    assert not [s for s in _summary(result) if any(r in s for r in EDGE_ROWS)]
    assert not [t for t in _texts(result) if "driven above" in t]


def test_max_speed_is_live_tunable():
    proj = bev_axle(profile="0:0; 3:300; 60:300")
    proj.cases[0].duration = 60
    proj.cases[0].timeStep = 1.0
    steps = {"n": 0}

    def control():
        steps["n"] += 1
        if steps["n"] == 20:  # at t = 19 s, near 12,000 1/min
            return [{"type": "set_param", "elementId": "mot", "key": "max_speed_rpm",
                     "value": 9000}]
        return []

    result = simulate(proj, "case", control=control)
    rpm = series(result, "mot", "sig_speed")
    assert max(p["value"] for p in rpm if p["t"] <= 19) > 11000
    # above its new maximum it coasts down to it, and stays there
    assert max(p["value"] for p in rpm if p["t"] > 50) <= 9000


# ---- tables outside their data -------------------------------------------------------

def _lookup(policy: list[str] | None, sample_time: float = 0.0):
    lk = el("lk", "signal.lookup", "Lookup", sample_time_s=sample_time)
    if policy:
        lk.tableOutside = {"table_1d": policy}
    proj = project([el("c", "signal.constant", "Input", value=2.5), lk], [],
                   [dbc(1, "c", "sig_out", "lk", "sig_x_in")], duration=4, time_step=0.5)
    return simulate(proj, "case")


@pytest.mark.parametrize("policy, out", [(None, 1.0), (["clamp"], 1.0), (["linear"], 2.5)])
def test_clamp_and_linear_lookup_blocks_count_their_time_outside(policy, out):
    result = _lookup(policy)
    assert series(result, "lk", "sig_out")[-1]["value"] == pytest.approx(out)
    summary = _summary(result)
    assert summary["Lookup — time outside its '1D Table' table (Input)"] == 100.0
    assert summary["Lookup — furthest Input outside its '1D Table' table"] == 2.5
    assert any(t.startswith("Lookup Table 'Lookup' 1D Table: Input 2.5 - is past the edge "
                            "of its data (1 -) at t = 0.00 s") for t in _texts(result, "info"))


def test_a_sampled_lookup_counts_the_time_its_output_holds():
    summary = _summary(_lookup(["clamp"], sample_time=0.3))
    assert summary["Lookup — time outside its '1D Table' table (Input)"] == 100.0


def test_a_lookup_set_to_error_stops_the_run():
    result = _lookup(["error"])
    assert result.status == "failed"
    assert any(t.startswith("Lookup Table 'Lookup' 1D Table: Input 2.5 - is outside its data "
                            "(0 to 1 -) at t = 0.00 s") for t in _texts(result, "error"))


def test_a_motor_on_a_voltage_past_its_map_is_counted_not_warned():
    """The voltage axis holds its edge (Clamp). Before, this was a warning at
    first touch, which made the run 'warning' however briefly it happened;
    now it is counted and the verdict can judge the time."""
    proj = bev_axle(profile="0:0; 5:60; 30:60")
    _el(proj, "batt").parameterOverrides["ocv_table"] = {"0": 430, "100": 440}
    result = simulate(proj, "case")
    assert result.status == "success", _texts(result)
    assert any("Full-Load Torque: Voltage 4" in t and "(396 V)" in t
               for t in _texts(result, "info"))
    summary = _summary(result)
    # once per solver step: the handshake's and the Driver's trial reads of
    # the map would take it past 100 %
    assert 80 < summary["E-Motor — time outside its 'Full-Load Torque' table (Voltage)"] <= 100
    assert summary["E-Motor — furthest Voltage outside its 'Full-Load Torque' table"] > 420


def test_a_loss_map_narrower_than_the_full_load_map_stops_the_run():
    """Before, the loss was held at its 200 N·m value up to 345 N·m."""
    proj = bev_axle()
    _el(proj, "mot").parameterOverrides["power_loss"] = {
        "0": {"0": 0.1, "100": 1.6, "200": 3.2}, "12000": {"0": 1.9, "100": 5.6, "200": 9.6}}
    assert any("loss map covers 0–200 N·m" in c.text for c in validate_project(proj)
               if c.level == "warning")
    result = simulate(proj, "case")
    assert result.status == "failed"
    [error] = _texts(result, "error")
    assert error.startswith("E-Motor 'E-Motor' Power Loss (Motor + Inverter): Torque ")
    assert "(0 to 200 N·m)" in error


# ---- the examples and the counters -------------------------------------------------

def test_a_run_counts_every_table_it_reads(monkeypatch):
    """RunContext.map_use is where the summary (and the verdict) read how far
    a run went: a record per axis of each table, one per machine for its
    maximum speed."""
    seen = []

    class Seen(core.RunContext):
        def __init__(self, *a, **k):
            super().__init__(*a, **k)
            seen.append(self)

    monkeypatch.setattr(core, "RunContext", Seen)
    simulate(bev_axle(), "case")
    records = {(u.el_id, u.what, u.axis): u for u in seen[0].map_use}
    assert set(records) == {
        ("batt", "'Open-Circuit Voltage' table", "SOC"),
        ("mot", "'Full-Load Torque' table", "Voltage"),
        ("mot", "'Full-Load Torque' table", "Speed"),
        ("mot", "'Power Loss (Motor + Inverter)' table", "Speed"),
        ("mot", "'Power Loss (Motor + Inverter)' table", "Torque"),
        ("mot", "'Drag Torque (unpowered)' table", "Speed"),
        ("mot", "maximum speed", "Speed"),
    }
    assert all(u.outside_s == 0.0 for u in records.values())
    assert records[("mot", "maximum speed", "Speed")].edge == 12000


@pytest.mark.parametrize("project_id, case_ids", [
    ("bev-car", ["case-city", "case-wltc", "case-wltc-hvac"]),
    ("hybrid-car", ["case-udds", "case-hwfet", "case-mixed"]),
])
def test_every_example_case_stays_inside_its_maps(project_id, case_ids):
    """The hybrid's engine fires below its first full-load speed while it
    starts: the start-up rule reads that point, so no map is left."""
    proj = load_example(project_id)
    live = {c.id for c in proj.cases if c.realtimeFactor}
    assert set(case_ids) == {c.id for c in proj.cases} - live
    for case_id in case_ids:
        result = example_result(project_id, case_id)
        assert result.status == "success", case_id
        assert not [s for s in _summary(result) if any(r in s for r in EDGE_ROWS)], case_id
        assert not [t for t in _texts(result) if "maximum speed" in t or "edge of its data" in t]


# ---- Data Checks ---------------------------------------------------------------------

def _map_warnings(proj) -> list[str]:
    return [c.text for c in validate_project(proj) if c.level != "info" and any(
        s in c.text for s in ("full-load data", "loss map", "fuel map", "polarization curve"))]


def _with_source(source) -> object:
    """bev_axle with its battery swapped for another source (same id)."""
    proj = bev_axle()
    els = proj.systems[0].elements
    els[[e.id for e in els].index("batt")] = source
    return proj


def _dcdc_fed():
    proj = bev_axle()
    sys0 = proj.systems[0]
    sys0.elements += [el("lv", "electric.node", "Battery Bus"),
                      el("dc", "controller.dcdc", "DC-DC")]
    sys0.connections[0] = conn(1, "batt", "pos", "lv", "t1")
    sys0.connections += [conn(10, "lv", "t2", "dc", "a_pos"), conn(11, "dc", "b_pos", "hvbus", "t1")]
    return proj


def test_library_defaults_fit_together():
    """Before, the voltage source (400 V), the DC-DC (800 V) and the fuel cell
    (264-420 V) fed the default E-Motor outside its 250-396 V map, and the
    default engine's fuel map (1,000-6,000 1/min, up to 175 N·m) did not
    cover its full-load curve (800-6,000 1/min, 178 N·m peak)."""
    lib = {cid: {p.key: p.default for p in c.parameters} for cid, c in library_by_id().items()}
    volts = [v for v, _ in parse_table2d(lib["motor.emotor"]["full_load_torque"])]
    pol = parse_table1d(lib["fuelcell.stack"]["polarization"])
    for v in (lib["electric.voltage_source"]["voltage_V"], lib["controller.dcdc"]["output_voltage_V"],
              interp1(pol, lib["fuelcell.stack"]["max_current_A"]), interp1(pol, 0.0)):
        assert volts[0] <= v <= volts[-1]
    full = parse_table1d(lib["engine.combustion"]["full_load_torque"])
    fuel = parse_table2d(lib["engine.combustion"]["fuel_map"])
    assert fuel[0][0] <= full[0][0] and fuel[-1][0] >= full[-1][0]
    assert min(p[-1][0] for _, p in fuel) >= max(t for _, t in full)

    engine = project([el("eng", "engine.combustion", "Engine")], [], [])
    for proj in (bev_axle(), _dcdc_fed(), engine,
                 _with_source(el("batt", "electric.voltage_source", "Source")),
                 _with_source(el("batt", "fuelcell.stack", "Fuel Cell"))):
        assert _map_warnings(proj) == []


def test_mismatches_are_warned_about():
    proj = _with_source(el("batt", "electric.voltage_source", "Source", voltage_V=420))
    assert _map_warnings(proj) == [
        "E-Motor 'E-Motor' is fed 420 V by 'Source', outside the 250–396 V of its full-load data."]

    proj = bev_axle()
    _el(proj, "mot").parameterOverrides["max_speed_rpm"] = 14000
    assert _map_warnings(proj) == [
        "E-Motor 'E-Motor': its Maximum Speed (14,000 1/min) is beyond its full-load data, "
        "which ends at 12,000 1/min — lower it or extend the map.",
        "E-Motor 'E-Motor': its loss map covers 0–12,000 1/min but the motor runs from 0 to its "
        "maximum speed of 14,000 1/min — extend the map."]

    # a map measured from 1,000 1/min: before, the run held its edge value;
    # now the Error axis stops it at the first step
    proj = bev_axle()
    full_load = next(p.default for p in library_by_id()["motor.emotor"].parameters
                     if p.key == "full_load_torque")
    _el(proj, "mot").parameterOverrides["full_load_torque"] = {
        v: {k: t for k, t in sheet.items() if k != "0"} for v, sheet in full_load.items()}
    assert _map_warnings(proj) == [
        "E-Motor 'E-Motor': its full-load data starts at 1,000 1/min but the motor starts from "
        "0 — extend the map down to 0 1/min."]

    proj = _with_source(el("batt", "fuelcell.stack", "Fuel Cell", max_current_A=450,
                           polarization={"20": 396, "100": 349, "400": 250}))
    assert _map_warnings(proj) == [
        "Fuel cell 'Fuel Cell': its polarization curve starts at 20 A but the stack starts from "
        "0 A — extend the curve down to 0 A.",
        "Fuel cell 'Fuel Cell': its polarization curve covers 20–400 A but its Maximum Current "
        "is 450 A — extend the curve or lower the Maximum Current."]

    engine = el("eng", "engine.combustion", "Engine")
    fuel_map = next(p.default for p in library_by_id()["engine.combustion"].parameters
                    if p.key == "fuel_map")
    engine.parameterOverrides["fuel_map"] = {k: v for k, v in fuel_map.items() if k != "800"}
    assert _map_warnings(project([engine], [], [])) == [
        "Engine 'Engine': its fuel map covers 1,000–6,000 1/min but its full-load curve runs "
        "800–6,000 1/min — extend the map."]

    proj = bev_axle()
    _el(proj, "mot").tableOutside = {"power_loss": ["clamp"], "full_load_torque": ["clamp", "clamp"]}
    checks = [(c.level, c.text) for c in validate_project(proj) if "outside" in c.text]
    assert checks == [
        ("error", "'E-Motor' has an outside-the-data setting for 'power_loss' that does not fit "
                  "its tables (one setting per axis of a table) — the run ignores it."),
        ("info", "'E-Motor.Full-Load Torque' does not stop the run outside its Speed (Clamp) "
                 "data, as the library does — the run summary says how long and how far it "
                 "went outside."),
    ]
