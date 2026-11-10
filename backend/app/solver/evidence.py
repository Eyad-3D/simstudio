"""What a finished run's verdict, its reports and its hand calculations are
made from, whichever engine ran it.

verdict.judge (through its trace's ``ctx``), reports.make (the energy
report, the duty and what held each driveline back), energy.energy_flows
(the part energies) and references.hand_checks read one Evidence and
nothing else of the run, so no part of them is written twice. Today's
engine fills it from its RunContext once the run is over (core.py); the
new engine from lightsim_engine's run (new_engine.py): its channels as the
engine gave them (a value that is not a number is judged before the result
shows it as a gap), its trace of the Driver's target and the vehicle from
the dense output, its records of the tables it read past their data, its
per-part energy books, its integrals over every step and its events.

The part records keep today's names (a battery's ``energy_out_wh``, a
motor's ``max_rpm``, ...): today's engine passes its own state objects,
the new engine the same objects of a RunContext built for the project and
never stepped, filled from its run.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable, Optional

from .maps import MapUse
from .network import Model


@dataclass
class Evidence:
    model: Model  # the project's model (network.build_model)
    veh_id: Optional[str]
    params: Callable[[str], dict]  # a part's parameters as the case runs it
    # (air drag, rolling resistance) at a speed, N (domains.RunContext.road_load)
    road_load: Callable[[float, float, float], tuple[float, float]]
    distance: float = 0.0  # distance driven, m
    end_distance: Optional[float] = None  # where the case ends, m (None: at its duration)
    full_throttle: bool = False  # an acceleration test
    performance: bool = False  # not a cycle: the trace is sampled more often
    # each part's facts and totals, by element id (today's state objects:
    # BatteryState, MotorCache, EngineCache, FuelCellCache)
    batteries: dict = field(default_factory=dict)
    motors: dict = field(default_factory=dict)
    engines: dict = field(default_factory=dict)
    fuelcells: dict = field(default_factory=dict)
    vsource_energy_wh: dict = field(default_factory=dict)
    grip_limited_s: float = 0.0  # time a driven wheel was at the tyres' grip limit
    map_use: list[MapUse] = field(default_factory=list)  # tables run past, maximum speeds
    # the reports: each part's energy book (energy.Flow or energy.BookFlow,
    # J) and the time it covers, s; the time the duty and the lanes cover
    flows: list = field(default_factory=list)
    time_s: float = 0.0
    duty_s: float = 0.0
    # each duty part: (element id, [(quantity, unit, [∫x dt, ∫x² dt, max, min])])
    duty: list = field(default_factory=list)
    # what held each driveline back (reports.Lane: label, element_ids,
    # seconds per LIMIT_STATES, changes [(t, state)])
    lanes: list = field(default_factory=list)
    energy_report: bool = True  # the case asks for the energy report
    residual_wh: float = 0.0  # energy the books do not account for, Wh
    throughput_wh: float = 0.0  # ... against the energy that went through, Wh

    @classmethod
    def of_context(cls, ctx, **more) -> Evidence:
        """Today's engine's evidence: its RunContext at the end of its run."""
        return cls(model=ctx.model, veh_id=ctx.veh_id, params=ctx.params,
                   road_load=ctx.road_load, distance=ctx.distance,
                   end_distance=ctx.end_distance, full_throttle=ctx.full_throttle,
                   performance=ctx.performance, batteries=ctx.batteries, motors=ctx.motors,
                   engines=ctx.engines, fuelcells=ctx.fuelcells,
                   vsource_energy_wh=ctx.vsource_energy_wh, grip_limited_s=ctx.grip_limited_s,
                   map_use=ctx.map_use, residual_wh=ctx.residual_wh,
                   throughput_wh=ctx.throughput_wh, **more)
