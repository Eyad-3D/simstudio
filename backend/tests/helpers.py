"""Builders for solver test projects."""
from __future__ import annotations

from functools import lru_cache

from app.schemas import (
    Connection,
    DataBusConnection,
    ElementInstance,
    PortDef,
    Project,
    SimCase,
    SimResult,
    SystemNode,
)
from app.solver import simulate
from app.storage import load_example


def el(id_: str, def_id: str, label: str, **overrides) -> ElementInstance:
    return ElementInstance(
        id=id_, componentDefId=def_id, label=label,
        position={"x": 0, "y": 0}, parameterOverrides=overrides,
    )


def conn(i: int, a: str, ap: str, b: str, bp: str) -> Connection:
    return Connection(id=f"c{i}", sourceElementId=a, sourcePortId=ap,
                      targetElementId=b, targetPortId=bp)


def dbc(i: int, a: str, ap: str, b: str, bp: str) -> DataBusConnection:
    return DataBusConnection(id=f"db{i}", element1Id=a, port1Id=ap,
                             element2Id=b, port2Id=bp)


def sig_port(pid: str, direction: str) -> PortDef:
    return PortDef(id=pid, name=pid, direction=direction, kind="signal", unitGroup="No Unit")


def project(elements, connections, databus, duration=30.0, time_step=0.5) -> Project:
    return Project(
        id="test", name="Test",
        systems=[SystemNode(id="root", name="Test", parentId=None,
                            elements=elements, connections=connections)],
        dataBusConnections=databus,
        cases=[SimCase(id="case", name="Case", duration=duration, timeStep=time_step)],
    )


def driver_wiring(next_dbc_id: int = 90) -> list[DataBusConnection]:
    """Standard driver loop: task → driver ← vehicle speed."""
    return [
        dbc(next_dbc_id, "task", "sig_demand", "drv", "sig_target_in"),
        dbc(next_dbc_id + 1, "veh", "sig_speed", "drv", "sig_speed_in"),
    ]


def bev_axle(locked: bool = False, mu_left: float = 1.0,
             profile: str = "0:0; 5:100; 30:100"):
    """Minimal driven axle: battery → motor → final drive → diff → two wheels."""
    elements = [
        el("veh", "vehicle.body", "Vehicle"),
        el("drv", "driver.driver", "Driver"),
        el("task", "signal.driving_task", "Task", profile=profile),
        el("batt", "battery.generic", "Battery"),
        el("hvbus", "electric.node", "HV Bus"),
        el("mot", "motor.emotor", "E-Motor"),
        el("fd", "mech.final_drive", "Final Drive"),
        el("diff", "mech.differential", "Differential", locked=locked),
        el("whl", "propulsion.wheel", "Wheel L", mu=mu_left),
        el("whr", "propulsion.wheel", "Wheel R"),
    ]
    connections = [
        conn(1, "batt", "pos", "hvbus", "t1"),
        conn(2, "hvbus", "t3", "mot", "pos"),
        conn(3, "mot", "shaft", "fd", "flange_in"),
        conn(4, "fd", "flange_out", "diff", "flange_in"),
        conn(5, "diff", "flange_out_a", "whl", "shaft"),
        conn(6, "diff", "flange_out_b", "whr", "shaft"),
    ]
    databus = [
        *driver_wiring(),
        dbc(2, "drv", "sig_traction_cmd", "mot", "sig_demand_in"),
    ]
    return project(elements, connections, databus)


# an FS-sized motor (EMRAX 228-like, typical values): 230 N·m to 4,000 1/min,
# then about 96 kW, to 6,500 1/min, with its loss map, kW
FS_FULL_LOAD = {"600": {"0": 230, "4000": 230, "4500": 204, "5000": 184, "5500": 167,
                        "6000": 153, "6500": 141}}
FS_LOSS = {"0": {"0": 0.05, "100": 0.6, "230": 2.2}, "3000": {"0": 0.25, "100": 1.2, "230": 3.4},
           "6500": {"0": 0.8, "100": 2.4, "230": 5.5}}


def fs_car(layout: str = "Autocross", laps: int = 1, mu: float = 1.5, vehicle=None, wheel=None,
           battery=None) -> Project:
    """A Formula Student-sized electric car on a Race Track, its one case a
    lap case: 280 kg, one rear motor through a 4.0 final drive (97 %) and an
    open differential (99 %), 0.23 m wheels with 45 % of the weight on the
    front axle, a brake on each, a 7 kWh, 450-588 V battery with 0.35 ohm
    and the Driver wired as in a drive cycle. Typical values, not a real
    car; ``vehicle``, ``wheel`` (all four) and ``battery`` override values."""
    ocv = {"0": 450, "10": 500, "50": 540, "90": 575, "100": 588}
    elements = [
        el("veh", "vehicle.body", "Vehicle", **{"mass_kg": 280, "cd": 1.2, "frontal_area_m2": 1.0,
                                                 "wheelbase_m": 1.53, **(vehicle or {})}),
        el("drv", "driver.driver", "Driver", regen_weight_pct=80),
        el("task", "signal.driving_task", "Task", profile="0:0; 10:50; 20:0"),
        el("batt", "battery.generic", "Accumulator", **{
            "capacity_kWh": 7.0, "ocv_table": ocv, "internal_resistance_ohm": 0.35,
            "max_charge_power_kW": 40, "initial_soc_pct": 95, "min_soc_pct": 5,
            **(battery or {})}),
        el("hvbus", "electric.node", "HV Bus"),
        el("mot", "motor.emotor", "E-Motor", full_load_torque=FS_FULL_LOAD, power_loss=FS_LOSS,
           drag_torque={"0": 0, "6500": 0.5}, max_speed_rpm=6500, inertia_kgm2=0.02),
        el("fd", "mech.final_drive", "Final Drive", ratio=4.0, efficiency_pct=97,
           inertia_in_kgm2=0.002, inertia_out_kgm2=0.01),
        el("diff", "mech.differential", "Differential", efficiency_pct=99, inertia_kgm2=0.005),
        el("trk", "track.lap", "Race Track", layout=layout, laps=laps),
    ]
    connections = [conn(1, "batt", "pos", "hvbus", "t1"), conn(2, "hvbus", "t3", "mot", "pos"),
                   conn(3, "mot", "shaft", "fd", "flange_in"),
                   conn(4, "fd", "flange_out", "diff", "flange_in")]
    databus = [*driver_wiring(), dbc(2, "drv", "sig_traction_cmd", "mot", "sig_demand_in")]
    i = 10
    for w, share, axle in (("fl", 22.5, "Front"), ("fr", 22.5, "Front"),
                           ("rl", 27.5, "Rear"), ("rr", 27.5, "Rear")):
        elements += [el(w, "propulsion.wheel", f"Wheel {w.upper()}", **{
                         "radius_m": 0.23, "inertia_kgm2": 0.3, "vehicle_load_share_pct": share,
                         "axle": axle, "mu": mu, "slip_stiffness": 20,
                         "rolling_resistance": 0.015, **(wheel or {})}),
                     el(f"b{w}", "mech.brake", f"Brake {w.upper()}", max_torque_Nm=900,
                        inertia_kgm2=0.02)]
        databus.append(dbc(i, "drv", "sig_brake_cmd", f"b{w}", "sig_demand_in"))
        if axle == "Rear":
            elements.append(el(f"n{w}", "mech.node", f"Node {w.upper()}"))
            connections += [conn(i, "diff", "flange_out_a" if w == "rl" else "flange_out_b",
                                 f"n{w}", "f1"),
                            conn(i + 1, f"n{w}", "f2", f"b{w}", "flange"),
                            conn(i + 2, f"n{w}", "f3", w, "shaft")]
        else:
            connections.append(conn(i, f"b{w}", "flange", w, "shaft"))
        i += 3
    proj = project(elements, connections, databus)
    proj.cases[0].kind = "lap"
    return proj


def coast_project(veh_kw=None, wheel_kw=None, ambient=None, grade=None,
                  v0=100.0, duration=10.0, dt=0.1, inertia=0.001) -> Project:
    """A car coasting from v0 km/h with nothing driving it: a Vehicle on four
    Wheels, each on its own Brake (never applied). Wheels and brakes get the
    rotational inertia `inertia` (small by default, so the coast is the road
    load alone; the effective mass is mass + 8 * inertia / radius²). Optional:
    an Ambient with these parameter values ({} for the library's) and a
    constant road grade (%)."""
    elements = [el("veh", "vehicle.body", "Vehicle", initial_speed_kmh=v0, **(veh_kw or {}))]
    connections = []
    for i in range(4):
        elements += [el(f"w{i}", "propulsion.wheel", f"Wheel {i}",
                        **{"inertia_kgm2": inertia, **(wheel_kw or {})}),
                     el(f"b{i}", "mech.brake", f"Brake {i}", inertia_kgm2=inertia)]
        connections.append(conn(i, f"b{i}", "flange", f"w{i}", "shaft"))
    if ambient is not None:
        elements.append(el("amb", "boundary.ambient", "Ambient", **ambient))
    databus = []
    if grade is not None:
        elements.append(el("grade", "signal.constant", "Grade", value=grade))
        databus.append(dbc(1, "grade", "sig_out", "veh", "sig_grade_in"))
    return project(elements, connections, databus, duration=duration, time_step=dt)


def series(result, el_id: str, port_id: str):
    for c in result.channels:
        if c.elementId == el_id and c.portId == port_id:
            return c.timeSeries
    raise KeyError(f"channel {el_id}:{port_id} not in result")


@lru_cache(maxsize=None)
def example_result(project_id: str, case_id: str) -> SimResult:
    """A shipped example's case, run once per test session (read-only: the
    result is shared by every test that asks for it)."""
    return simulate(load_example(project_id), case_id)
