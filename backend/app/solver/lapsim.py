"""Lap mode (SimCase.kind "lap"): the lap time on the model's Race Track and
the energy it takes, from the same motor, gear and battery models as a
drive cycle.

A quasi-steady-state lap simulation, re-derived from the published method
(Heilmeier et al., EVER 2019; the point-mass lap simulators such as OpenLAP),
without their code:

1. the track (load_track): its curvature and slope every ~1 m;
2. each point's cornering speed (LapRun.apex): the highest speed at which
   the tyres' lateral grip holds the car on the curvature, with downforce,
   load transfer and load sensitivity, after the grip the road load takes
   (friction ellipse);
3. a forward pass from the lap's start speed at full acceleration, within
   the driven tyres' grip the corner leaves and the powertrain's force at
   that speed (the motors' full-load curves through the gears, cut to what
   the battery can deliver: LapRun.envelope), capped at each point's
   cornering speed;
4. a backward pass from the lap's end at full braking, within all the
   tyres' grip and the brakes. The speed is the lower of the two. On a
   closed track the backward pass runs on over the next lap's corners, so
   the lap ends at a speed the next one can take.

The energy pass then drives the speed trace one co-simulation step per
track point: LapSlave commands the motors through the source-limit
handshake and their maps, with the Driver's regeneration blending, and the
ElectricalSlave integrates the battery, so the energy, the limits and the
channels come from the drive cycles' own code. It books where the energy
went (the lap energy balance). Each lap re-reads the battery's state for
its powertrain limit.
"""
from __future__ import annotations

import bisect
import json
import math
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from typing import Callable, Optional

from .domains import (
    ControlSlave,
    DriverSlave,
    ElectricalSlave,
    RunContext,
    SourceLimitSlave,
    _CtxSlave,
    full_torque,
    through_gears,
)
from .maps import TableError, interp1, parse_table1d
from .master import Master
from .network import Model
from .runtime import GRAVITY, MAX_SUBSTEP, RPM, axle_load_shift, lateral_load_shift, tyre_mu
from .slave import StepResult

TRACKS_PATH = Path(__file__).resolve().parent.parent / "library" / "tracks.json"
# what held the car back on a stretch of track: its code (Race Track Limit
# channel) is the position here + 1
LIMITS = ("cornering grip", "traction grip", "motor", "battery", "power cap", "braking",
          "lift-and-coast")
CORNER, TRACTION, MOTOR, BATTERY, CAP, BRAKING, COAST = range(1, 8)
KAPPA_MAX = 0.5  # 1/m: a 2 m radius; a tighter one is a noisy or wrong table
BALANCE_PCT = 0.5  # lap energy balance error above which the energy per lap is not valid
# braking short of the trace, as a share of the brakes' and regeneration's
# force, that is the lap solver's discretisation (0.04 % at most on the FS
# car), not a limit of the car
BRAKE_TOL = 0.005
MAX_POINTS = 50_000  # recorded points above which a lap case asks for Store every N
ENVELOPE_DV = 0.25  # m/s: the powertrain force's speed grid
# a speed this close below a point's cornering speed counts as limited by it
# (a car approaches it slowly: the grip left to speed up shrinks as it nears)
AT_APEX = 0.005
# the Race Track's channels, in LapSlave.publish_point's order
TRACK_PORTS = ("sig_lap_distance", "sig_lap", "sig_curvature", "sig_long_accel",
               "sig_lat_accel", "sig_limit", "sig_x", "sig_y", "sig_elevation")


class LapError(Exception):
    """The car cannot drive the lap (it stops on the track)."""


@lru_cache(maxsize=1)
def layouts() -> dict:
    return json.loads(TRACKS_PATH.read_text(encoding="utf-8"))["layouts"]


@dataclass
class Track:
    name: str
    closed: bool
    length: float
    ds: float  # point spacing, m
    kappa: list[float]  # curvature at each point, 1/m (+ left); n + 1 points from 0 to length
    sin_t: list[float]  # sine of the slope angle (+ uphill)
    x: list[float]  # the map, m (starting at 0, 0 heading along +x)
    y: list[float]
    z: list[float]  # elevation, m
    heading: float  # heading change over the track, rad
    sector_ends: list[float]  # each sector's end, m; the last is the length


def _from_segments(segs: list, closed: bool) -> list[tuple[float, float]]:
    """A layout's segments [length m, radius m (+ left, 0 straight)] as
    distance → curvature points, the curvature changing linearly over 1 m
    centred on each join, as a car's line does (and a table holds one value
    per distance). On a closed lap the change from its last segment to its
    first is centred on the finish line: half ends the lap, half starts it."""
    ks = [(float(length), 1.0 / r if r else 0.0) for length, r in segs]
    joins = list(zip(ks, ks[1:] + ks[:1])) if closed else list(zip(ks, ks[1:]))
    half = [min(0.5, a[0] / 2.0, b[0] / 2.0) for a, b in joins]
    wrap = closed and ks[-1][1] != ks[0][1]
    mid = 0.5 * (ks[-1][1] + ks[0][1])
    pts = [(0.0, mid), (half[-1], ks[0][1])] if wrap else [(0.0, ks[0][1])]
    s = 0.0
    for i, ((length, k), (_, k_next)) in enumerate(joins):
        s += length
        if i == len(ks) - 1:  # the finish line
            pts += [(s - half[i], k), (s, mid)] if wrap else [(s, k)]
        elif k_next != k:
            pts += [(s - half[i], k), (s + half[i], k_next)]
    if not closed:
        pts.append((s + ks[-1][0], ks[-1][1]))
    out: list[tuple[float, float]] = []
    for x, k in pts:  # two changes a whole ramp apart share a point
        if not out or x - out[-1][0] > 1e-9:
            out.append((x, k))
    return out


def _sector_ends(text: str) -> list[float]:
    ends = []
    for part in text.replace(",", ";").split(";"):
        try:
            ends.append(float(part))
        except ValueError:
            continue
    return ends


def load_track(params: dict, spacing: float = 1.0) -> Track:
    """The Race Track as points about ``spacing`` apart (an equal spacing
    that fits the length); raises TableError, KeyError or ValueError for a
    track problems() reports."""
    name = str(params.get("layout", "Autocross"))
    if name == "Custom":
        pts = parse_table1d(params.get("curvature_table"))
        closed = bool(params.get("closed", True))
        ends = _sector_ends(str(params.get("sector_ends", "") or ""))
        elev = parse_table1d(params.get("elevation_table") or {"0": 0})
    else:
        lay = layouts()[name]
        closed = bool(lay["closed"])
        pts = _from_segments(lay["segments"], closed)
        ends, elev = list(lay["sector_ends"]), [(0.0, 0.0)]
    length = pts[-1][0]
    if length <= 0 or len(pts) < 2:
        raise ValueError("the track has no length")
    n = max(2, round(length / spacing))
    ds = length / n
    s = [i * ds for i in range(n + 1)]
    # the heading: the exact integral of the piecewise-linear curvature
    xs = [x for x, _ in pts]
    cum = [0.0]
    for (xa, ka), (xb, kb) in zip(pts, pts[1:]):
        cum.append(cum[-1] + 0.5 * (ka + kb) * (xb - xa))

    def heading(d: float) -> float:
        j = max(1, min(len(pts) - 1, bisect.bisect_right(xs, d)))
        (xa, ka), (xb, kb) = pts[j - 1], pts[j]
        k_d = ka if xb == xa else ka + (kb - ka) * (d - xa) / (xb - xa)
        return cum[j - 1] + 0.5 * (ka + k_d) * (d - xa)

    kappa = [interp1(pts, si) for si in s]
    heads = [heading(si) for si in s]
    z = [interp1(elev, si) for si in s]
    sin_t = [max(-1.0, min(1.0, (interp1(elev, si + 0.5 * ds) - interp1(elev, si - 0.5 * ds)) / ds))
             for si in s]
    x, y = [0.0], [0.0]  # the map steps along the mean heading of each spacing
    for i in range(n):
        mid = 0.5 * (heads[i] + heads[i + 1])
        x.append(x[-1] + ds * math.cos(mid))
        y.append(y[-1] + ds * math.sin(mid))
    ends = sorted(e for e in ends if 0 < e < length) + [length]
    return Track(name, closed, length, ds, kappa, sin_t, x, y, z, heads[-1], ends)


def problems(model: Model, output_every: int = 1) -> list[tuple[str, str, tuple[str, ...]]]:
    """(level, text, parts) for a lap case of this model: an error refuses
    the run (and fails Data Checks), a warning or an info says what lap mode
    does differently. `parts` are the element ids it is about; () means the
    Race Track. Shared by simulate() and Data Checks."""
    E, cdef_of, params_of = model.elements, model.cdef_of, model.params_of
    out: list[tuple[str, str, tuple[str, ...]]] = []

    def name(el_id: str) -> str:
        return f"{cdef_of[el_id].name} '{E[el_id].label}'"

    if model.track is None:
        return [("error", "A lap case needs a Race Track: add one from Driver & Signals and "
                          "choose its layout.", (model.vehicle,) if model.vehicle else ())]
    if model.vehicle is None:
        out.append(("error", "A lap case needs a Vehicle.", ()))
    if model.driver is None:  # (and Data Checks want the E-Motors commanded by one)
        out.append(("error", "A lap case needs a Driver: its Recuperation Weight sets the "
                             "regeneration when braking.", ()))
    wheeled = [dl for dl in model.drivelines if any(seg.wheels for seg in dl.segments)]
    for dl in model.drivelines:  # (a series hybrid's engine and generator)
        if any(dl is w for w in wheeled):
            continue
        for el_id in dl.element_group:
            if cdef_of[el_id].id in ("engine.combustion", "mech.clutch", "motor.emotor"):
                out.append(("error", f"{name(el_id)} drives no wheels: a lap case does not run "
                                     f"drivelines without wheels (run it as a drive cycle "
                                     f"instead).", (el_id,)))
    motors = []
    for dl in wheeled:
        for el_id in dl.element_group:
            t = cdef_of[el_id].id
            if t in ("engine.combustion", "mech.clutch"):
                out.append(("error", f"{name(el_id)} is in a driveline with wheels: a lap case "
                                     f"drives E-Motors only, without engines and clutches (run "
                                     f"it as a drive cycle instead).", (el_id,)))
            elif t == "motor.emotor":
                motors.append(el_id)
            elif t == "mech.gearbox" and len(params_of[el_id].get("ratios") or {}) > 1:
                gear = float(params_of[el_id].get("default_gear", 1) or 1)
                out.append(("warning", f"{name(el_id)} stays in gear {gear:g} for the whole lap: "
                                       f"a lap case does not shift gears.", (el_id,)))
    if not motors:
        out.append(("error", "A lap case needs an E-Motor that drives the wheels.", ()))
    wheels = [w for dl in wheeled for seg in dl.segments for w in seg.wheels]
    if wheels and len({w.axle for w in wheels}) < 2:
        out.append(("error", "A lap case needs wheels on both axles for its load transfer: set "
                             "Axle to Rear on the rear wheels and Front on the front ones.",
                    tuple(w.el_id for w in wheels)))
    driver = model.driver
    for m in motors:
        src = model.signal_route.get((m, "sig_demand_in"))
        if src != (driver, "sig_traction_cmd"):
            out.append(("info", f"{name(m)} is not commanded straight by the Driver's Traction "
                                f"Command: a lap case commands every E-Motor itself, with one "
                                f"demand for all.", (m,)))
    if model.vehicle is not None and (model.vehicle, "sig_grade_in") in model.signal_route:
        out.append(("info", "The Vehicle's Road Grade input is not used in a lap case: the Race "
                            "Track's elevation sets the slope.", (model.vehicle,)))

    tp, label = params_of[model.track], E[model.track].label
    layout = str(tp.get("layout", "Autocross"))
    try:
        laps = float(tp.get("laps", 1))
    except (TypeError, ValueError):
        laps = 0.0
    if math.isfinite(laps) and laps != int(laps):  # 1 to 500: the catalogue's limits
        out.append(("error", f"Race Track '{label}': Laps must be a whole number.", ()))
        laps = 1
    if layout != "Custom" and layout not in layouts():
        out.append(("error", f"Race Track '{label}' has no layout '{layout}'.", ()))
        return out
    try:
        pts = None
        if layout == "Custom":
            pts = parse_table1d(tp.get("curvature_table"))
            parse_table1d(tp.get("elevation_table") or {"0": 0})
    except TableError as e:
        return out + [("error", f"Race Track '{label}' Custom tables: {e}", ())]
    if pts is not None:
        bad = False
        if pts[0][0] != 0:
            out.append(("error", f"Race Track '{label}': its Curvature table must start at 0 m "
                                 f"(it starts at {pts[0][0]:g} m).", ()))
            bad = True
        if pts[-1][0] - pts[0][0] < 10:
            out.append(("error", f"Race Track '{label}': its Curvature table is shorter than "
                                 f"10 m.", ()))
            bad = True
        k_max = max(abs(k) for _, k in pts)
        if k_max > KAPPA_MAX:
            out.append(("error", f"Race Track '{label}': its Curvature reaches {k_max:g} 1/m (a "
                                 f"{1 / k_max:.2g} m radius); a car's line has at most "
                                 f"{KAPPA_MAX:g} 1/m. Check the unit (1/m) or smooth a logged "
                                 f"curvature.", ()))
            bad = True
        ends = _sector_ends(str(tp.get("sector_ends", "") or ""))
        outside = [e for e in ends if not 0 < e < pts[-1][0]]
        if outside:
            out.append(("error", f"Race Track '{label}': Sector Ends "
                                 f"{', '.join(f'{e:g}' for e in outside)} m are not on the "
                                 f"track (0 to {pts[-1][0]:g} m).", ()))
            bad = True
        if not bad and bool(tp.get("closed", True)):
            tr = load_track(tp)
            turn = math.degrees(tr.heading)
            gap = math.hypot(tr.x[-1] - tr.x[0], tr.y[-1] - tr.y[0])
            # a closed lap ends where it starts, heading the same way: a circuit
            # turns ±360°, a figure eight (the skidpad) 0°
            if abs(turn - 360.0 * round(turn / 360.0)) > 5.0 or gap > 0.02 * tr.length:
                out.append(("warning", f"Race Track '{label}' is a Closed Circuit, but its "
                                       f"curvature turns the car {turn:.0f}° and ends {gap:.0f} m "
                                       f"from its start (a closed lap ends where it starts, "
                                       f"heading the same way): check the table, or untick "
                                       f"Closed Circuit.", ()))
        length = pts[-1][0]
    else:
        length = sum(float(seg_len) for seg_len, _ in layouts()[layout]["segments"])
    points = laps * length / max(1, output_every)  # a point about every metre
    if points > MAX_POINTS:
        out.append(("info", f"This lap case records about {points:,.0f} points; set the case's "
                            f"Store every to 5 or more to keep its result small.", ()))
    return out


def ellipse_left(use: float, n: float) -> float:
    """The share of a tyre's grip left one way while ``use`` of it is taken
    the other way: (1 − use^n)^(1/n) (n = 2: an ellipse)."""
    if use >= 1.0:
        return 0.0
    return (1.0 - use ** n) ** (1.0 / n) if use > 0 else 1.0


@dataclass
class Profile:
    """One lap's speed at each track point and what limited each stretch."""
    v: list[float]
    t: list[float]  # time from the lap's start, s
    code: list[int]  # per stretch i → i + 1 (LIMITS position + 1)


@dataclass
class LapBook:
    """What the energy pass booked, J (for the lap energy balance)."""
    kinetic: float = 0.0
    road: float = 0.0
    grade: float = 0.0
    friction: float = 0.0
    gears: float = 0.0
    motors: float = 0.0
    consumers: float = 0.0
    shortfall: float = 0.0
    # the braking the trace asked for that the friction brakes (at their
    # Max Torque) and the regeneration could not give, J, and for how long, s
    brake_short: float = 0.0
    brake_short_s: float = 0.0


class LapRun:
    """A lap case's car and track, read from the RunContext: the effective
    masses, the tyres and their loads, the powertrain's force at any speed;
    solve() gives a lap's speed profile."""

    def __init__(self, ctx: RunContext, spacing: float = 1.0):
        self.ctx = ctx
        model = ctx.model
        tp = ctx.params(model.track)
        self.track = load_track(tp, spacing)
        self.laps = int(float(tp.get("laps", 1)))
        # the lap (from 0) that ends at a standstill for the endurance's driver
        # change (FS Rules 2026 v1.1 (FSG) D 7.2.3, D 7.5); the next lap starts
        # from rest. None: no stop
        self.stop_after: Optional[int] = None
        # lift-and-coast (MOD-44): the share of each stretch of acceleration
        # before braking that the driver coasts, and the energy the laps may
        # take, J (0: none), which sets each lap's share instead
        self.coast = min(1.0, max(0.0, float(tp.get("coast_pct", 0) or 0) / 100.0))
        self.energy_target_j = max(0.0, float(tp.get("energy_target_kWh", 0) or 0)) * 3.6e6
        self.coast_used = self.coast > 0 or self.energy_target_j > 0
        self.coast_laps: list[float] = []  # each lap's share
        self.strategy: Optional[EnergyStrategy] = None
        self.m = ctx.veh_mass
        self.dp = ctx.params(model.driver) if model.driver else {}

        # every driveline with wheels, rolling without slip: its segments'
        # speeds and its coordinates per m/s of vehicle speed
        self.dls = [st for st in ctx.dls if not st.plan.over_constrained and st.plan.n
                    and any(seg.wheels for seg in st.dl.segments)]
        v0, ctx.v = ctx.v, 1.0
        self.unit: dict[int, list[float]] = {}
        self.x_unit: dict[int, list[float]] = {}
        for st in self.dls:
            ctx.start_at_vehicle_speed(st)
            self.x_unit[id(st)] = list(st.plan.x)
            self.unit[id(st)] = [ctx.seg_speed(st, s) for s in range(len(st.dl.segments))]
        ctx.v = v0
        self.set_speed(v0)
        m_und = self.m_eff = self.m
        self.drives = []  # (driveline, segment index, segment, its motors)
        self.brakes = []  # (brake, its force at the road per N·m, 1/m)
        driven = set()
        for st in self.dls:
            unit = self.unit[id(st)]
            j = sum(max(1e-4, seg.inertia) * unit[s] ** 2 for s, seg in enumerate(st.dl.segments))
            self.m_eff += j
            srcs = [(s, seg, [src for src in seg.sources if src.el_id in ctx.motors])
                    for s, seg in enumerate(st.dl.segments)]
            if any(ms for _, _, ms in srcs):
                driven |= {w.el_id for seg in st.dl.segments for w in seg.wheels}
            else:
                m_und += j  # undriven wheels: their tyres spin them up
            self.drives += [(st, s, seg, ms) for s, seg, ms in srcs if ms]
            self.brakes += [(br, abs(br.m * unit[s]))
                            for s, seg in enumerate(st.dl.segments) for br in seg.brakes]
        self.m_und = m_und
        self.motors = [(st, s, src, ctx.motors[src.el_id]) for st, s, seg, ms in self.drives
                       for src in ms]
        self.wheels = ctx.wheels
        self.driven = [w.el_id in driven for w in self.wheels]
        # the wheels of an axle in pairs across the car: in a corner the first
        # of each pair takes the load the second gives (a wheel left alone in
        # the middle keeps its own)
        self.pairs = []
        index = {id(w): i for i, w in enumerate(self.wheels)}
        for a, ws in enumerate(ctx.axle_wheels):
            k = len(ws) // 2
            self.pairs += [(a, index[id(p)], index[id(q)], k)
                           for p, q in zip(ws[:k], ws[len(ws) - k:])]
        self.apex_at: dict[tuple[float, float], float] = {}
        self.read_car()
        self.env_v: list[float] = []
        self.env_f: list[float] = []
        self.env_code: list[int] = []
        self.env_gen: list[float] = []
        self.book = LapBook()
        self.lap_times: list[float] = []
        self.sector_times: list[list[float]] = []
        self.limit_s = [0.0] * len(LIMITS)
        self.p_sq = 0.0  # ∫ (battery terminal power)² dt, W²·s
        # the lap being driven, and the stretch of it the energy pass is on
        self.k = 0
        self.i = 0
        self.prof: Optional[Profile] = None
        root_of = {bus.id: root_id for root_id, nodes in ctx.bus_tree.items() for bus, _, _ in nodes}
        self.root_of_motor = {mc.el_id: root_of.get(ctx.motor_bus[mc.el_id].id)
                              for _, _, _, mc in self.motors if mc.el_id in ctx.motor_bus}

    # ---- the car ---------------------------------------------------------------

    def read_car(self) -> None:
        """The car's values a live edit can change (the Vehicle's, the wheels'
        load shares and friction, the brakes' torque), read at each lap's
        start; the cornering speeds are then solved again."""
        ctx = self.ctx
        vp = ctx.params(ctx.veh_id)
        self.h = max(0.0, float(vp.get("cg_height_m", 0)))
        self.wheelbase = float(vp.get("wheelbase_m", 2.7))
        self.cza = float(vp.get("downforce_cza_m2", 0))
        self.aero_front = min(1.0, max(0.0, float(vp.get("aero_balance_front_pct", 50)) / 100.0))
        self.track_f = float(vp.get("track_front_m", 1.55))
        self.track_r = float(vp.get("track_rear_m", 1.55))
        self.fr_cap = sum(br.max_torque * lever for br, lever in self.brakes)  # friction brakes, N
        front, _ = ctx.axle_wheels
        self.front_share = sum(w.load_share for w in front)
        # each wheel's axle and its part of that axle's load
        index = {id(w): i for i, w in enumerate(self.wheels)}
        self.axle_of = []
        for a, ws in enumerate(ctx.axle_wheels):
            share = sum(w.load_share for w in ws)
            self.axle_of += [(index[id(w)], a, w.load_share / share if share > 0 else 1.0 / len(ws))
                             for w in ws]
        self.n_ell = sum(w.n_ell for w in self.wheels) / max(1, len(self.wheels))
        self.apex_at.clear()

    def set_speed(self, v: float) -> None:
        """Every driveline with wheels rolling at v without slip."""
        for st in self.dls:
            st.plan.x[:] = [x * v for x in self.x_unit[id(st)]]

    def loads(self, v: float, ax: float, ay: float, sin_t: float, cos_t: float) -> list[float]:
        """Each wheel's normal load, N: its share of the weight normal to
        the road, its axle's part of the longitudinal transfer and the
        downforce (runtime.axle_load_shift) split by the wheels' shares, and
        the lateral transfer from the inner to the outer wheels (shared
        between the axles as the static weight is), a lifting inner wheel
        passing on no more than it carries."""
        mg = self.m * GRAVITY * cos_t
        fz = [w.load_share * mg for w in self.wheels]
        axle = [0.0, 0.0]
        for i, a, _ in self.axle_of:
            axle[a] += fz[i]
        d = axle_load_shift(self.m, self.h, self.wheelbase, ax, sin_t, self.cza, self.aero_front,
                            self.ctx.rho, v, axle[0], axle[1])
        for i, a, part in self.axle_of:
            fz[i] = max(0.0, fz[i] + part * d[a])
        if self.h and ay:
            lat = lateral_load_shift(self.m, self.h, ay, self.track_f, self.track_r,
                                     self.front_share)
            for a, p, q, k in self.pairs:
                moved = min(lat[a] / k, fz[q])
                fz[p] += moved
                fz[q] -= moved
        return fz

    def grip(self, fz: list[float]) -> tuple[float, float, float, float]:
        """(the driven wheels' longitudinal grip, all wheels' longitudinal
        grip, their lateral grip, their rolling resistance), N."""
        drive = total = lateral = roll = 0.0
        for w, f, driven in zip(self.wheels, fz, self.driven):
            gx = tyre_mu(w, f) * f
            total += gx
            if driven:
                drive += gx
            lateral += tyre_mu(w, f, lateral=True) * f
            roll += w.c_rr * f
        return drive, total, lateral, roll

    def resist(self, v: float, roll: float, sin_t: float, cos_t: float) -> float:
        """Road load and the slope's pull, N (the Vehicle's own road load)."""
        aero, f_roll = self.ctx.road_load(v, roll, cos_t)
        return aero + f_roll * max(0.0, min(1.0, v / 0.3)) + self.m * GRAVITY * sin_t

    # ---- the powertrain ------------------------------------------------------

    def force(self, torques: dict[str, float], out: Optional[dict] = None) -> float:
        """The force the motors' shaft torques give at the road, N, through
        each driveline's gears and splits (the mechanics' own path); ``out``
        gets each driven segment's output torque, keyed (driveline, segment)."""
        f = 0.0
        for st, s, seg, ms in self.drives:
            t_at = [0.0] * len(seg.stages)
            for src in ms:
                t_at[src.region] += torques.get(src.el_id, 0.0) * src.m
            unit = self.unit[id(st)][s]
            t_out = through_gears(seg, t_at, unit)
            if out is not None:
                out[(id(st), s)] = t_out
            eff = st.plan.eff_chain[s]
            f += (t_out * eff if t_out * unit >= 0 else t_out / eff) * unit
        return f

    def _full(self, v: float, volts: dict[int, float], k: float, drive: bool) -> tuple[dict, dict]:
        """({motor: shaft torque}, {source tree: electrical W}) with every
        motor at ``k`` of its full-load (``drive``) or generator torque."""
        ctx = self.ctx
        torques, power = {}, {}
        for st, s, src, mc in self.motors:
            omega = src.m * self.unit[id(st)][s] * v
            rpm = abs(omega) * RPM
            bus = ctx.motor_bus.get(mc.el_id)
            v_bus = volts.get(bus.id, ctx.bus_voltage.get(bus.id, 0.0)) if bus else 0.0
            if v_bus <= 1.0 or rpm > mc.max_rpm * (1.0 + 1e-9):
                # no supply, or above its maximum speed: the inverter is off and
                # the motor's drag brakes it (as in motor_torque)
                if drive:
                    torques[mc.el_id] = -math.copysign(
                        interp1(mc.drag.pts, rpm, mc.drag.linear[0]), omega)
                continue
            t = k * full_torque(mc, v_bus, rpm, drive) * (1.0 if drive else -mc.q4_scale)
            torques[mc.el_id] = t
            root = self.root_of_motor.get(mc.el_id)
            power[root] = power.get(root, 0.0) + ctx.motor_power(mc, t, omega)
        return torques, power

    def envelope(self) -> None:
        """The powertrain's most force at the road, and the generator force,
        on a speed grid up to the motors' top speed, from the batteries'
        present state: every motor at full load at the bus voltage its draw
        leaves (two passes of the battery's internal-resistance drop), all
        cut back by one factor when the source trees cannot deliver it (the
        handshake's room for the motors: the battery's maximum-power point
        and Output Power Limit less the other loads); the generator force
        likewise cut back to what the source trees can take (the battery's
        max charge power and charge room, plus the other loads)."""
        ctx = self.ctx
        if ctx.dt <= 0:
            ctx.dt = MAX_SUBSTEP  # (the handshake's charge floor needs a step)
        ctx.update_source_limits(ctx.t)
        room = {root: ctx.motor_room.get(root, (0.0, 0.0))[1] for root in ctx.bus_tree}
        regen_room = {}  # what the motors may feed back, W
        for root, nodes in ctx.bus_tree.items():
            regen_room[root] = -ctx.motor_room.get(root, (0.0, 0.0))[0]
            battery = nodes[0][0].battery
            if battery:  # its charge power limit: one full at the lap's start takes charge
                b = ctx.batteries[battery]  # again once the lap's first metres draw on it
                served = regen_room[root] - ctx.source_window.get(root, (0.0, 0.0))[1]
                take = b.max_charge_w
                if b.i_ch_lim < math.inf:  # (its cells' charge current, as its BMS sets it)
                    take = min(take, b.i_ch_lim * (b.ocv() - b.v_rc + b.i_ch_lim * b.r0))
                regen_room[root] = max(regen_room[root], served + take)
        capped = {root: bool(nodes[0][0].battery and ctx.batteries[nodes[0][0].battery].capped)
                  for root, nodes in ctx.bus_tree.items()}
        # up to the fastest motor's top speed: _full drops each motor above its own
        v_top = max((mc.max_rpm / RPM / abs(src.m * self.unit[id(st)][s])
                     for st, s, src, mc in self.motors if src.m * self.unit[id(st)][s]),
                    default=0.0)
        v_top = min(v_top, 150.0)
        n = max(1, math.ceil(v_top / ENVELOPE_DV))
        self.env_v = [min(v_top, k * ENVELOPE_DV) for k in range(n + 1)]
        self.env_f, self.env_code, self.env_gen = [], [], []
        for v in self.env_v:
            volts: dict[int, float] = {}
            for _ in range(2):  # the terminal voltage at the draw
                _, power = self._full(v, volts, 1.0, True)
                volts = {}
                for root, nodes in ctx.bus_tree.items():
                    bus = nodes[0][0]
                    if bus.battery:
                        b = ctx.batteries[bus.battery]
                        a_volt = b.ocv() - b.v_rc
                        p = min(power.get(root, 0.0), room[root]) + ctx.fixed_served_w.get(bus.id, 0.0)
                        disc = max(0.0, a_volt * a_volt - 4.0 * b.r0 * p)
                        volts[bus.id] = a_volt - (a_volt - math.sqrt(disc)) / 2.0
            torques, power = self._full(v, volts, 1.0, True)
            code = MOTOR
            over = [r for r, p in power.items() if p > room.get(r, 0.0)]
            if over:
                lo, hi = 0.0, 1.0
                for _ in range(40):
                    mid = 0.5 * (lo + hi)
                    _, p_mid = self._full(v, volts, mid, True)
                    if all(p <= room.get(r, 0.0) for r, p in p_mid.items()):
                        lo = mid
                    else:
                        hi = mid
                torques, _ = self._full(v, volts, lo, True)
                code = CAP if any(capped.get(r) for r in over) else BATTERY
            self.env_f.append(max(0.0, self.force(torques)))
            self.env_code.append(code)
            gen, fed = self._full(v, volts, 1.0, False)
            if any(-p > regen_room.get(r, 0.0) for r, p in fed.items()):
                lo, hi = 0.0, 1.0  # (the share of full regeneration the batteries take)
                for _ in range(40):
                    mid = 0.5 * (lo + hi)
                    _, p_mid = self._full(v, volts, mid, False)
                    if all(-p <= regen_room.get(r, 0.0) for r, p in p_mid.items()):
                        lo = mid
                    else:
                        hi = mid
                gen, _ = self._full(v, volts, lo, False)
            self.env_gen.append(max(0.0, -self.force(gen)))

    def powertrain(self, v: float) -> tuple[float, int, float]:
        """(the most force at the road, what limits it, the generator force)
        at speed v: linear on the envelope's grid, none above its top."""
        vs = self.env_v
        if v >= vs[-1]:
            return 0.0, MOTOR, 0.0
        k = min(int(v / ENVELOPE_DV), len(vs) - 2)
        f = (v - vs[k]) / (vs[k + 1] - vs[k])
        code = self.env_code[k] if f < 0.5 else self.env_code[k + 1]
        return (self.env_f[k] + f * (self.env_f[k + 1] - self.env_f[k]), code,
                self.env_gen[k] + f * (self.env_gen[k + 1] - self.env_gen[k]))

    # ---- the lap ---------------------------------------------------------------

    def apex(self, kappa: float, sin_t: float) -> float:
        """The highest steady speed on curvature ``kappa``, m/s: the tyres'
        lateral grip holds m·v²·|κ| with what the road load's longitudinal
        need leaves of it (inf on a straight)."""
        if abs(kappa) < 1e-9:
            return math.inf
        key = (kappa, sin_t)
        if key in self.apex_at:
            return self.apex_at[key]
        cos_t = math.sqrt(1.0 - sin_t * sin_t)

        def holds(v: float) -> bool:
            ay = v * v * kappa
            drive, total, lateral, roll = self.grip(self.loads(v, 0.0, ay, sin_t, cos_t))
            need = self.resist(v, roll, sin_t, cos_t)
            if drive <= 0:  # (a load sensitivity can take all the grip at a high load)
                return False
            use = need / drive if need > 0 else -need / total if need < 0 else 0.0
            if use >= 1.0:
                return False
            return self.m * abs(ay) <= lateral * ellipse_left(use, self.n_ell)

        lo, hi = 0.0, 150.0
        if holds(hi):
            lo = math.inf
        else:
            for _ in range(48):
                mid = 0.5 * (lo + hi)
                lo, hi = (mid, hi) if holds(mid) else (lo, mid)
        self.apex_at[key] = lo
        return lo

    def _accel(self, kappa: float, sin_t: float, v: float, a: float) -> tuple[float, int]:
        """The most acceleration at speed v on curvature ``kappa``, and what
        limits it: the driven tyres' grip the corner leaves pushes the body
        and the undriven wheels; the powertrain also spins its own
        driveline up. The longitudinal load transfer is solved from ``a`` by
        a short fixed-point iteration (it contracts by about μ·h/L a pass)."""
        cos_t = math.sqrt(1.0 - sin_t * sin_t)
        ay = v * v * kappa
        f_pt, why, _ = self.powertrain(v)
        for _ in range(4 if self.h else 1):
            drive, _, lateral, roll = self.grip(self.loads(v, a, ay, sin_t, cos_t))
            resist = self.resist(v, roll, sin_t, cos_t)
            left = ellipse_left(self.m * abs(ay) / lateral, self.n_ell) if lateral > 0 else 0.0
            a_grip = (drive * left - resist) / self.m_und
            a_pt = (f_pt - resist) / self.m_eff
            a, code = (a_grip, TRACTION) if a_grip < a_pt else (a_pt, why)
        return a, code

    def _decel(self, kappa: float, sin_t: float, v: float, d: float) -> float:
        """The most deceleration at speed v on curvature ``kappa``, m/s²:
        all tyres' grip the corner leaves (ideal brake balance), or the
        brakes and the motors' regeneration with the driveline's inertia.
        The regeneration is held as the energy pass holds it: to the
        Driver's Recuperation Weight (fading out below 3 m/s), the driven
        tyres' grip the corner leaves and what the batteries can take (the
        envelope's generator force)."""
        cos_t = math.sqrt(1.0 - sin_t * sin_t)
        ay = v * v * kappa
        regen_cap = self.regen_w() * self.powertrain(v)[2] * max(0.0, min(1.0, v / 3.0))
        for _ in range(4 if self.h else 1):
            drive, total, lateral, roll = self.grip(self.loads(v, -d, ay, sin_t, cos_t))
            resist = self.resist(v, roll, sin_t, cos_t)
            left = ellipse_left(self.m * abs(ay) / lateral, self.n_ell) if lateral > 0 else 0.0
            regen = min(regen_cap, drive * left)
            d = min((total * left + resist) / self.m, (self.fr_cap + regen + resist) / self.m_eff)
        return d

    def regen_w(self) -> float:
        return max(0.0, min(1.0, float(self.dp.get("regen_weight_pct", 80)) / 100.0))

    def _forward(self, v_start: float, apex: list[float],
                 coast: Optional[list[bool]] = None) -> tuple[list[float], list[int]]:
        """Full acceleration from ``v_start``, Heun's method over each
        spacing, capped at each point's cornering speed; on the stretches
        ``coast`` marks, the driver lifts: no drive, the road load and the
        slope slow the car (lift-and-coast). Returns (speeds, what limited
        each stretch)."""
        tr = self.track
        n, ds = len(tr.kappa) - 1, tr.ds
        kappa, sin_t = tr.kappa, tr.sin_t
        vf = [0.0] * (n + 1)
        why = [MOTOR] * n
        vf[0] = min(v_start, apex[0])
        a_prev = 0.0
        for i in range(n):
            v = vf[i]
            if coast is not None and coast[i]:
                a1 = self._coast(kappa[i], sin_t[i], v)
                v2 = math.sqrt(max(0.0, v * v + 2.0 * a1 * ds))
                a2 = self._coast(kappa[i + 1], sin_t[i + 1], v2)
                why[i] = COAST
                a_prev = 0.0
                a = 0.5 * (a1 + a2)
            else:
                a1, why[i] = self._accel(kappa[i], sin_t[i], v, a_prev)
                v2 = math.sqrt(max(0.0, v * v + 2.0 * a1 * ds))
                a2, _ = self._accel(kappa[i + 1], sin_t[i + 1], v2, a1)
                a = a_prev = 0.5 * (a1 + a2)
            vf[i + 1] = min(math.sqrt(max(0.0, v * v + 2.0 * a * ds)), apex[i + 1])
        return vf, why

    def _coast(self, kappa: float, sin_t: float, v: float) -> float:
        """The acceleration with no drive and no brakes, m/s²: the road load
        and the slope on the car and its drivelines' inertia."""
        cos_t = math.sqrt(1.0 - sin_t * sin_t)
        _, _, _, roll = self.grip(self.loads(v, 0.0, v * v * kappa, sin_t, cos_t))
        return -self.resist(v, roll, sin_t, cos_t) / self.m_eff

    def _coast_mask(self, prof: Profile, share: float) -> list[bool]:
        """The stretches where the driver lifts: the last ``share`` of each
        stretch of acceleration (traction, motor, battery or power cap)
        that ends where braking begins, from no slower than half the speed
        at which braking begins (a car does not coast away from rest)."""
        code, v = prof.code, prof.v
        n = len(code)
        mask = [False] * n
        driving = (TRACTION, MOTOR, BATTERY, CAP)
        for i in range(1, n):
            if code[i] == BRAKING and code[i - 1] != BRAKING:
                j = i
                while j > 0 and code[j - 1] in driving:
                    j -= 1
                w = i - round(share * (i - j))
                while w < i and v[w] < 0.5 * v[i]:
                    w += 1
                for k in range(w, i):
                    mask[k] = True
        return mask

    def solve(self, v_start: float, coast: Optional[float] = None,
              prepared: bool = False) -> Profile:
        """One lap's speed profile from ``v_start``, with the car and the
        powertrain's force re-read (live edits, the batteries' present
        state) unless ``prepared``; ``coast`` the share of each stretch of
        acceleration before braking driven as lift-and-coast (None: the
        Race Track's own)."""
        if not prepared:
            self.read_car()
            self.envelope()
        share = self.coast if coast is None else max(0.0, min(1.0, coast))
        tr = self.track
        apex = [self.apex(k, s) for k, s in zip(tr.kappa, tr.sin_t)]
        vf, why = self._forward(v_start, apex)
        prof = self._braking(vf, why, apex)
        if share > 0:
            mask = self._coast_mask(prof, share)
            if any(mask):
                vf, why = self._forward(v_start, apex, mask)
                prof = self._braking(vf, why, apex)
        return prof

    def _braking(self, vf: list[float], why: list[int], apex: list[float]) -> Profile:
        """Full braking back from the lap's end onto the forward pass's
        speeds ``vf``; the profile and what limited each stretch."""
        tr = self.track
        n, ds = len(tr.kappa) - 1, tr.ds
        kappa, sin_t = tr.kappa, tr.sin_t
        # full braking back from the end; on a closed track, or an open one
        # with a lap to come (driven on from this one's end), over the next
        # lap's corners too (at their cornering speed: the next lap's own
        # start is not known yet)
        stop = self.k == self.stop_after
        wrap = (tr.closed or self.k + 1 < self.laps) and not stop
        vb = vf + apex[1:] if wrap else vf[:]
        if stop:
            vb[n] = 0.0  # into the driver change area
        if wrap:
            vb[n] = min(vb[n], apex[0])  # where the next lap starts
            kappa, sin_t = kappa + kappa[1:], sin_t + sin_t[1:]
        d_prev = 0.0
        for j in range(len(vb) - 1, 0, -1):
            v = vb[j]
            if math.isinf(v):
                continue
            d1 = self._decel(kappa[j], sin_t[j], v, d_prev)
            v1 = math.sqrt(max(0.0, v * v + 2.0 * d1 * ds))
            d2 = self._decel(kappa[j - 1], sin_t[j - 1], v1, d1)
            d = 0.5 * (d1 + d2)
            v1 = math.sqrt(max(0.0, v * v + 2.0 * d * ds))
            if v1 < vb[j - 1]:
                vb[j - 1] = v1
                d_prev = d
            else:
                d_prev = 0.0
        v = vb[:n + 1]
        t = [0.0] * (n + 1)
        code = [0] * n
        for i in range(n):
            if v[i] + v[i + 1] <= 0:
                raise LapError(f"The car cannot drive the lap: it stops {i * ds:.0f} m into "
                               f"lap {self.k + 1} of the Race Track's {tr.name} layout (its "
                               f"motors do not overcome the road load there).")
            t[i + 1] = t[i] + 2.0 * ds / (v[i] + v[i + 1])
            code[i] = (BRAKING if v[i] < vf[i] or v[i + 1] < vf[i + 1]
                       else COAST if why[i] == COAST
                       else CORNER if v[i + 1] >= apex[i + 1] * (1.0 - AT_APEX) else why[i])
        return Profile(v=v, t=t, code=code)

    # ---- the result --------------------------------------------------------------

    def finish_lap(self, prof: Profile) -> None:
        tr = self.track
        self.lap_times.append(prof.t[-1])
        s = [i * tr.ds for i in range(len(prof.t))]
        cuts = [0.0] + [interp1(list(zip(s, prof.t)), e) for e in tr.sector_ends]
        self.sector_times.append([b - a for a, b in zip(cuts, cuts[1:])])
        for i, c in enumerate(prof.code):
            self.limit_s[c - 1] += prof.t[i + 1] - prof.t[i]

    def rows(self) -> list[tuple[str, float, str, str]]:
        """The lap case's summary rows: (label, value, unit, key)."""
        ctx, tr = self.ctx, self.track
        done = len(self.lap_times)
        if not done:
            return []
        best = min(range(done), key=lambda k: self.lap_times[k])
        total = sum(self.lap_times)
        rows = [("Lap time", self.lap_times[best], "s", "lap_time_s")]
        if self.laps > 1:
            rows += [("Lap 1 time", self.lap_times[0], "s", "lap1_time_s"),
                     ("Total time", total, "s", "total_time_s")]
        if len(tr.sector_ends) > 1:
            rows += [(f"Sector {k + 1} time", st, "s", f"sector{k + 1}_time_s")
                     for k, st in enumerate(self.sector_times[best])]
        rows.append(("Average speed", done * tr.length / total * 3.6, "km/h",
                     "average_speed_kmh"))
        if not tr.closed:
            rows.append(("Speed at the finish", ctx.v * 3.6, "km/h", "finish_speed_kmh"))
        net = self.source_net_j()
        rows.append(("Energy per lap", net / 3.6e6 / done, "kWh", "energy_per_lap_kwh"))
        if ctx.batteries and total > 0:
            rows.append(("RMS battery power", math.sqrt(self.p_sq / total) / 1000.0, "kW",
                         "rms_battery_power_kw"))
        rows += [(f"Time limited by {name}", s, "s",
                  f"time_limited_by_{name.replace(' ', '_').replace('-', '_')}_s")
                 for name, s in zip(LIMITS, self.limit_s)
                 if name != "lift-and-coast" or self.coast_used]
        if self.coast_used and self.coast_laps:
            rows.append(("Lift-and-coast, mean share", 100.0 * sum(self.coast_laps)
                         / len(self.coast_laps), "%", "lift_and_coast_share_pct"))
        if self.energy_target_j > 0:
            rows += [("Energy target", self.energy_target_j / 3.6e6, "kWh", "energy_target_kwh"),
                     ("Energy used against the target", 100.0 * (net - self.energy_target_j)
                      / self.energy_target_j, "%", "energy_against_target_pct")]
        rows.append(("Lap energy balance error", self.balance_pct(), "%",
                     "lap_energy_balance_error_pct"))
        return rows

    def source_net_j(self) -> float:
        """The energy the sources gave the buses, net of what they took back, J."""
        ctx = self.ctx
        return 3600.0 * (sum(b.energy_out_wh - b.energy_in_wh for b in ctx.batteries.values())
                         + sum(fc.energy_wh for fc in ctx.fuelcells.values())
                         + sum(ctx.vsource_energy_wh.values()))

    def balance_pct(self) -> float:
        """How far the energy the lap took (kinetic, road load, slope,
        friction brakes, gear and motor losses, consumers) is from what the
        sources gave, % of the latter: the motors' shortfall, where they
        gave less than the speed trace asked for."""
        b = self.book
        parts = (b.kinetic + b.road + b.grade + b.friction + b.gears + b.motors + b.consumers)
        net = self.source_net_j()
        return 100.0 * (parts - net) / max(abs(net), 1e-9)


COAST_GRID = (0.0, 0.1, 0.2, 0.3, 0.45, 0.6, 0.8, 1.0)
REGEN_GUESS = 0.6  # the share of the braking work the estimate takes as won back


class EnergyStrategy:
    """Lift-and-coast to an energy target (MOD-44): before each lap, the
    share of lift-and-coast whose lap fits the energy left per lap left.

    The lap's energy is estimated from its speed profile: the work the
    wheels must do, less a share of the braking work regeneration may win
    back, times a factor learned from the laps driven so far (the battery
    energy each took over its estimate; 1/0.8 before the first). The
    estimate against the share is found once, on the first lap's profiles
    for COAST_GRID, and is linear between those shares. As each lap's
    budget is what the target leaves after the laps before it, an estimate
    off by a few per cent on one lap is made up on the next ones."""

    def __init__(self, lap: "LapRun"):
        self.lap = lap
        self.curve: list[float] = []
        self.factor = 1.0 / 0.8
        self.short = False  # the target could not be met with full lift-and-coast

    def estimate(self, prof: Profile) -> float:
        lap = self.lap
        tr = lap.track
        roll_mg = sum(w.c_rr * w.load_share for w in lap.wheels) * lap.m * GRAVITY
        drive = braking = 0.0
        regen_w = lap.regen_w()
        for i in range(len(prof.v) - 1):
            v0, v1 = prof.v[i], prof.v[i + 1]
            vm, a = 0.5 * (v0 + v1), (v1 * v1 - v0 * v0) / (2.0 * tr.ds)
            sin_t = 0.5 * (tr.sin_t[i] + tr.sin_t[i + 1])
            cos_t = math.sqrt(1.0 - sin_t * sin_t)
            f = lap.m_eff * a + lap.resist(vm, roll_mg * cos_t, sin_t, cos_t)
            if f > 0:
                drive += f * tr.ds
            else:
                braking += min(-f, regen_w * lap.powertrain(vm)[2]) * tr.ds
        return max(1.0, drive - REGEN_GUESS * braking)

    def choose(self, v_start: float, budget_j: float) -> float:
        """The least lift-and-coast whose lap is estimated to take no more
        than ``budget_j``."""
        lap = self.lap
        if not self.curve:
            for k, share in enumerate(COAST_GRID):
                self.curve.append(self.estimate(lap.solve(v_start, share, prepared=k > 0)))
            for k in range(1, len(self.curve)):  # (more coasting never takes more)
                self.curve[k] = min(self.curve[k], self.curve[k - 1])
        need = [self.factor * e for e in self.curve]
        if need[0] <= budget_j:
            return 0.0
        if need[-1] > budget_j:
            self.short = True
            return 1.0
        for k in range(1, len(need)):
            if need[k] <= budget_j:
                f = (need[k - 1] - budget_j) / max(1e-9, need[k - 1] - need[k])
                return COAST_GRID[k - 1] + f * (COAST_GRID[k] - COAST_GRID[k - 1])
        return 1.0

    def learn(self, prof: Profile, used_j: float) -> None:
        if used_j > 0:
            self.factor = used_j / self.estimate(prof)


class LapSlave(_CtxSlave):
    """Lap mode's driver and mechanics: drives the lap's speed trace one
    stretch of track per step. At the stretch's mean speed it asks the
    motors for the force the trace needs (driving: one share of their full
    load for all; braking: regeneration first, as the Driver blends it, up
    to the Recuperation Weight and the driven tyres' grip, the friction
    brakes the rest) through the source-limit handshake, so their maps, the
    gear losses and the supply's limits act as in a drive cycle, and books
    where the energy went."""

    slave_id = "lap"

    def __init__(self, ctx: RunContext, lap: LapRun):
        super().__init__(ctx)
        self.lap = lap
        self.driver = DriverSlave(ctx)  # its regeneration check (regen_share)

    def setup(self, t0: float) -> None:
        ctx = self.ctx
        ctx.rt.publish(ctx.veh_id, "sig_speed", ctx.v * 3.6)
        ctx.rt.publish(ctx.veh_id, "sig_distance", ctx.distance)

    def publish_point(self, i: int, a: float, code: int) -> None:
        """The state at track point i of the lap being driven: speeds,
        loads and the Race Track's channels (``a``: the acceleration over
        the stretch that got there, ``code`` what limited it)."""
        ctx, lap = self.ctx, self.lap
        tr, v = lap.track, lap.prof.v[i]
        ctx.v = v
        lap.set_speed(v)
        for st, s, src, mc in lap.motors:
            mc.rpm = abs(src.m * lap.unit[id(st)][s] * v) * RPM
        sin_t = tr.sin_t[i]
        cos_t = math.sqrt(1.0 - sin_t * sin_t)
        ctx.slope_sin, ctx.slope_cos, ctx.accel = sin_t, cos_t, a
        for w, f in zip(lap.wheels, lap.loads(v, a, v * v * tr.kappa[i], sin_t, cos_t)):
            w.n_load = f
        rt = ctx.rt
        rt.publish(ctx.veh_id, "sig_speed", v * 3.6)
        rt.publish(ctx.veh_id, "sig_distance", ctx.distance)
        values = (i * tr.ds, lap.k + 1, tr.kappa[i], a / GRAVITY, v * v * tr.kappa[i] / GRAVITY,
                  code, tr.x[i], tr.y[i], tr.z[i])
        for port, value in zip(TRACK_PORTS, values):
            rt.publish(ctx.model.track, port, value)

    def do_step(self, t: float, h: float) -> StepResult:
        ctx, lap = self.ctx, self.lap
        tr, prof, i = lap.track, lap.prof, lap.i
        rt, model, book = ctx.rt, ctx.model, lap.book
        v0, v1 = prof.v[i], prof.v[i + 1]
        vm = 0.5 * (v0 + v1)
        a = (v1 * v1 - v0 * v0) / (2.0 * tr.ds)
        sin_t = 0.5 * (tr.sin_t[i] + tr.sin_t[i + 1])
        cos_t = math.sqrt(1.0 - sin_t * sin_t)
        ay = vm * vm * 0.5 * (tr.kappa[i] + tr.kappa[i + 1])
        fz = lap.loads(vm, a, ay, sin_t, cos_t)
        drive, _, lateral, roll = lap.grip(fz)
        aero, f_roll = ctx.road_load(vm, roll, cos_t)
        f_road = aero + f_roll * max(0.0, min(1.0, vm / 0.3))
        f_grade = lap.m * GRAVITY * sin_t
        # the force the trace needs at the road (the midpoint speed makes
        # m_eff·a·vm·h the exact change in kinetic energy)
        f_req = lap.m_eff * a + f_road + f_grade

        lap.set_speed(vm)
        omega = {mc.el_id: src.m * lap.unit[id(st)][s] * vm for st, s, src, mc in lap.motors}
        motors = [(mc, omega[mc.el_id], ctx.motor_bus.get(mc.el_id)) for _, _, _, mc in lap.motors]
        brake = accel = 0.0
        if f_req >= 0:
            full = lap.force({mc.el_id: ctx.motor_command(mc, 1.0, w)[0] for mc, w, _ in motors})
            # where the powertrain limits the car, full throttle, held back by
            # the supply's limit as in a drive cycle; else the share of full
            # load the trace needs (a command of 0 would switch the inverters
            # off, and the motors' drag would brake)
            demand = (1.0 if prof.code[i] in (MOTOR, BATTERY, CAP) or full <= f_req
                      else max(1e-12, f_req / full))
            accel = demand
        else:
            gen = -lap.force({mc.el_id: ctx.motor_command(mc, -1.0, w)[0] for mc, w, _ in motors})
            # regeneration first, on the driven wheels only: no more than
            # their braking grip the corner leaves
            left = ellipse_left(lap.m * abs(ay) / lateral, lap.n_ell) if lateral > 0 else 0.0
            want = (min(-f_req, drive * left) / gen if gen > 0 else 0.0)
            want = min(1.0, want, lap.regen_w() * max(0.0, min(1.0, vm / 3.0)))
            demand = -self.driver.regen_share([m for m in motors if m[2] is not None], want)
            brake = min(1.0, -f_req / max(1e-9, lap.fr_cap + lap.regen_w() * gen))
        ctx.allocate_motor_power({mc.el_id: (demand, w) for mc, w, _ in motors}, t)
        torques = {mc.el_id: ctx.motor_torque(mc, demand, w) for mc, w, _ in motors}
        seg_out: dict = {}
        f_pt = lap.force(torques, seg_out)
        # the friction brakes the rest, up to their Max Torque; braking they
        # cannot give is booked (the lap is then faster than the car can drive)
        want_fric = max(0.0, f_pt - f_req) if f_req < 0 else 0.0
        fric = min(want_fric, lap.fr_cap)
        missing = want_fric - fric
        if missing > BRAKE_TOL * max(1.0, lap.fr_cap - min(0.0, f_pt)):
            book.brake_short += missing * vm * h
            book.brake_short_s += h
        short = f_req - f_pt + want_fric  # the force the motors did not give

        # the channels over the step
        drv = model.driver
        if drv:
            cmd = fric / lap.fr_cap if lap.fr_cap > 0 else 0.0
            for port, value in (("sig_traction_cmd", demand), ("sig_brake_cmd", min(1.0, cmd)),
                                ("sig_accel_pedal", accel), ("sig_brake_pedal", brake)):
                rt.publish(drv, port, value)
        for br, _ in lap.brakes:
            rt.publish(br.el_id, "sig_torque",
                       min(1.0, fric / lap.fr_cap) * br.max_torque if lap.fr_cap > 0 else 0.0)
        # the tyres' force: the motors' on the driven wheels, the brakes'
        # on all, each in proportion to its load
        f_drv = sum(f for f, d in zip(fz, lap.driven) if d)
        f_all = sum(fz)
        for w, f, driven in zip(lap.wheels, fz, lap.driven):
            ctx.last_forces[w.el_id] = ((f_pt * f / f_drv if driven and f_drv > 0 else 0.0)
                                        - (fric * f / f_all if f_all > 0 else 0.0))
        p_mech = 0.0
        for st in lap.dls:
            st.chain_power_w = sum(mc.p_mech_w for st2, _, _, mc in lap.motors if st2 is st)
            p_mech += st.chain_power_w
            for j in st.dl.joints:  # a split passes on its input's torque, as in the mechanics
                if j.kind == "split" and j.parent_seg >= 0:
                    st.joint_speed_in[j.el_id] = j.parent_m * lap.unit[id(st)][j.parent_seg] * v1
                    t_split = (seg_out.get((id(st), j.parent_seg), 0.0) / max(1e-9, j.parent_m)
                               * j.ratio * j.eff)
                    st.joint_torque_a[j.el_id] = (1.0 - j.f_b) * t_split
                    st.joint_torque_b[j.el_id] = j.f_b * t_split

        # where the energy went, J
        book.kinetic += lap.m_eff * a * vm * h
        book.road += f_road * vm * h
        book.grade += f_grade * vm * h
        book.friction += fric * vm * h
        book.gears += (p_mech - f_pt * vm) * h
        book.motors += sum(mc.p_loss_w for _, _, _, mc in lap.motors) * h
        book.shortfall += short * vm * h

        ctx.distance += tr.ds
        self.publish_point(i + 1, a, prof.code[i])
        return StepResult()


def slaves(ctx: RunContext, lap: LapRun) -> list:
    """Lap mode's slave set: the signal blocks, the handshake, the lap
    driver and mechanics, the electrical buses (no gear shifts)."""
    return [ControlSlave(ctx), SourceLimitSlave(ctx), LapSlave(ctx, lap), ElectricalSlave(ctx)]


def run_laps(ctx: RunContext, master: Master, lap: LapRun,
             record: Callable[[float, float], None], after_step: Callable[[], None],
             control: Optional[Callable[[], list]], apply_control_msg: Callable[[dict], None],
             output_every: int) -> tuple[float, bool]:
    """Drive the laps: each lap's profile from the batteries' state at its
    start, then its energy pass, one master step per stretch of track,
    recording every ``output_every``-th point and the last. Returns (the
    time solved, whether a stop cut it short); lets LapError, SlaveStepError
    and OutsideDataError through."""
    tr, rt = lap.track, ctx.rt
    lap_slave = next(s for s in master.slaves if isinstance(s, LapSlave))
    n = len(tr.kappa) - 1
    total = n * lap.laps
    t = 0.0
    v_start = ctx.v
    e_start = lap.source_net_j()
    for k in range(lap.laps):
        lap.k = k
        ctx.t = t  # (the time an error solving this lap stops the run at)
        share = None
        if lap.energy_target_j > 0:  # this lap's lift-and-coast, to the target
            lap.strategy = lap.strategy or EnergyStrategy(lap)
            left = lap.energy_target_j - (lap.source_net_j() - e_start)
            share = lap.strategy.choose(v_start, left / (lap.laps - k))
        lap.prof = prof = lap.solve(v_start, share)
        lap.coast_laps.append(lap.coast if share is None else share)
        e_lap = lap.source_net_j()
        if k == 0:
            if prof.v[0] < v_start - 1e-6:
                rt.message("info", f"Lap 1 starts at {prof.v[0] * 3.6:.1f} km/h, not at the "
                                   f"Vehicle's Initial Speed ({v_start * 3.6:.1f} km/h): the most "
                                   f"the Race Track's first corner allows.")
            lap_slave.publish_point(0, 0.0, prof.code[0])
            ctx.publish_sources(0.0)
            record(0.0, 0.0)
        for i in range(n):
            if control:
                for msg in control():
                    if msg.get("type") == "cancel":
                        rt.message("info", f"Simulation cancelled by user at t = {t:g} s.")
                        return t, True
                    if msg.get("type") == "set_param":
                        apply_control_msg(msg)
            lap.i = i
            dt = prof.t[i + 1] - prof.t[i]
            ctx.t, ctx.dt = t, dt
            master.step(t, dt)
            t += dt
            after_step()
            lap.p_sq += sum(b.power_w for b in ctx.batteries.values()) ** 2 * dt
            # the consumers, and the DC-DC converters' losses feeding them (their
            # flows are set by the electrical buses, the step's last slave)
            lap.book.consumers += (sum(ctx.consumer_w.values())
                                   + sum(d - o for d, o in ctx.dcdc_flows.values())) * dt
            done = k * n + i + 1
            if done % output_every == 0 or done == total:
                ctx.publish_sources(t)
                record(t, round(100.0 * done / total, 1))
        lap.finish_lap(prof)
        if lap.strategy is not None:
            lap.strategy.learn(prof, lap.source_net_j() - e_lap)
        v_start = prof.v[-1]
    if lap.energy_target_j > 0:
        used = lap.source_net_j() - e_start
        over = 100.0 * (used - lap.energy_target_j) / lap.energy_target_j
        if over > 2.0:
            full = sum(1 for c in lap.coast_laps if c >= 1.0)
            rt.message("warning", f"The laps took {used / 3.6e6:.3f} kWh, {over:.1f} % more than "
                                  f"the Race Track's Energy Target of "
                                  f"{lap.energy_target_j / 3.6e6:g} kWh"
                                  f"{f', with full lift-and-coast on {full} laps' if full else ''}"
                                  f": lift-and-coast alone cannot save that much. Lower the "
                                  f"battery's Output Power Limit as well, or raise the target.")
    return t, False

