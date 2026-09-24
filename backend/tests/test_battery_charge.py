"""The battery's SOC counts the charge that flows (amp-hours), as battery
management systems, datasheets and test data do (MOD-38): a constant 1C
discharge empties it in one hour whatever the shape of its OCV table.

A constant-current load is a Power Consumer whose demand is a Lookup of the
battery's terminal voltage: P = V · I, in kW."""
import pytest
from helpers import conn, dbc, el, project, series

from app.solver import simulate
from app.solver.maps import parse_table1d
from app.solver.runtime import ocv_mean

Q_AH = 100.0
LIBRARY_OCV = {"0": 300, "10": 318, "20": 330, "40": 342, "60": 352, "80": 362, "90": 368, "100": 376}


def _values(result, port):
    return {p["t"]: p["value"] for p in series(result, "batt", port)}


def _summary(result):
    return {s.label: s.value for s in result.summary}


def _amps(i_a):
    """A Lookup that turns the battery voltage into the power (kW) that draws i_a."""
    return el("amps", "signal.lookup", "Current", table_1d={"0": 0, "1000": i_a})


def _discharge(battery, i_a, duration):
    return simulate(project(
        [el("batt", "battery.generic", "Battery", initial_soc_pct=100, min_soc_pct=0, **battery),
         el("bus", "electric.node", "Bus"),
         el("load", "electric.constant_drive", "Load", power_kW=0),
         _amps(i_a)],
        [conn(1, "batt", "pos", "bus", "t1"), conn(2, "bus", "t2", "load", "pos")],
        [dbc(1, "batt", "sig_voltage", "amps", "sig_x_in"),
         dbc(2, "amps", "sig_out", "load", "sig_demand_in")],
        duration=duration, time_step=1.0), "case")


def _time_empty(soc):
    """The time the SOC reaches 0, interpolated between recorded points."""
    points = sorted(soc.items())
    for (ta, a), (tb, b) in zip(points, points[1:]):
        if a > 1e-6 >= b:
            return ta + a / (a - b) * (tb - ta)
    return None


@pytest.mark.parametrize("ocv_table", [LIBRARY_OCV, {"0": 200, "10": 300, "100": 400}],
                         ids=["library table", "steep table"])
def test_a_1c_discharge_empties_the_battery_in_one_hour(ocv_table):
    """Before, the SOC counted energy (OCV · I), so a pack whose voltage falls
    as it empties reached 0 % late: 3,611 s on the library table and 3,666 s
    on the steep one. Coulombic efficiency must not act on discharge."""
    result = _discharge({"capacity_Ah": Q_AH, "ocv_table": ocv_table, "internal_resistance_ohm": 0.01,
                         "coulombic_efficiency_pct": 99}, Q_AH, 3610.0)
    soc, current = _values(result, "sig_soc"), _values(result, "sig_current")
    t_empty = _time_empty(soc)
    assert t_empty is not None and 3599.0 <= t_empty <= 3601.0, t_empty
    assert soc[1800.0] == pytest.approx(50.0, abs=0.03)
    for t, i_a in current.items():
        if 0 < t < 3600:
            assert i_a == pytest.approx(Q_AH, rel=1e-4), t


def test_without_a_charge_capacity_it_comes_from_the_usable_capacity():
    """Old projects have no Charge Capacity: 34.5 kWh at the library table's
    SOC-weighted mean of 345 V is 100 Ah, and a full-to-empty discharge still
    gives out the Usable Capacity."""
    assert ocv_mean(parse_table1d(LIBRARY_OCV)) == pytest.approx(345.0)
    result = _discharge({"capacity_kWh": 34.5, "internal_resistance_ohm": 1e-6}, 10 * Q_AH, 370.0)
    soc = _values(result, "sig_soc")
    assert _time_empty(soc) == pytest.approx(360.0, abs=0.2)
    assert soc[180.0] == pytest.approx(50.0, abs=0.03)
    assert _summary(result)["Battery — energy delivered"] == pytest.approx(34.5, abs=0.002)


def _charge(eta_pct):
    """10C (1000 A) into a 100 Ah battery at 10 % through a lossless DC-DC."""
    return simulate(project(
        [el("grid", "electric.voltage_source", "Grid", voltage_V=400),
         el("a", "electric.node", "Bus A"),
         el("dc", "controller.dcdc", "Charger", efficiency_pct=100),
         el("b", "electric.node", "Bus B"),
         el("batt", "battery.generic", "Battery", capacity_Ah=Q_AH, initial_soc_pct=10,
            internal_resistance_ohm=0.001, coulombic_efficiency_pct=eta_pct, max_charge_power_kW=1000),
         _amps(10 * Q_AH)],
        [conn(1, "grid", "pos", "a", "t1"), conn(2, "a", "t2", "dc", "a_pos"),
         conn(3, "dc", "b_pos", "b", "t1"), conn(4, "b", "t2", "batt", "pos")],
        [dbc(1, "batt", "sig_voltage", "amps", "sig_x_in"),
         dbc(2, "amps", "sig_out", "dc", "sig_setpoint_in")],
        duration=180.0, time_step=1.0), "case")


def test_coulombic_efficiency_counts_charge_only():
    """50 Ah go in over 180 s; at 99 % efficiency 49.5 Ah are stored, and the
    charge not stored shows up in the internal losses, so the energy
    balance still closes."""
    full, lossy = _charge(100), _charge(99)
    assert _values(full, "sig_soc")[180.0] == pytest.approx(60.0, abs=0.01)
    assert _values(lossy, "sig_soc")[180.0] == pytest.approx(59.5, abs=0.01)
    s_full, s_lossy = _summary(full), _summary(lossy)
    energy_in = s_lossy["Battery — energy recuperated"]
    extra_loss = s_lossy["Battery — internal losses"] - s_full["Battery — internal losses"]
    assert extra_loss == pytest.approx(0.01 * energy_in, rel=0.03)
    assert s_full["Electrical energy balance error"] == 0
    assert s_lossy["Electrical energy balance error"] == 0
