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
