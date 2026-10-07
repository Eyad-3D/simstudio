"""Shared solver runtime: constants, per-element state caches, the message/
signal Runtime, the driveline-plan resolver and the tiny linear solver.

Everything here is domain-agnostic plumbing used by the wrapped domain
slaves (domains.py) and the simulate() orchestration (core.py). Moved
verbatim out of core.py in Phase 1.4 — behavior is unchanged.
"""
from __future__ import annotations

import math
from collections import defaultdict, deque
from dataclasses import dataclass, field
from typing import Callable, Optional

from ..schemas import SimMessage
from .maps import Map, MapUse, Sheets2D, inner_range, interp1
from .network import Driveline, Model

GRAVITY = 9.81
R_AIR = 287.05  # J/(kg·K), specific gas constant of dry air


def air_density(temperature_c: float = 20.0, pressure_kpa: float = 101.325) -> float:
    """Dry air, ideal gas: rho = p / (R · T), in kg/m³."""
    return pressure_kpa * 1000.0 / (R_AIR * (temperature_c + 273.15))


def axle_load_shift(m: float, h: float, wheelbase: float, a_x: float, sin_t: float,
                    cza: float, aero_front: float, rho: float, v: float,
                    w_front: float, w_rear: float) -> tuple[float, float, str | None]:
    """(load added to the front axle, load added to the rear axle, the axle
    that lifted: "Front", "Rear", "Both" or None), in N, on top of the
    static axle loads w_front and w_rear: the longitudinal transfer
    m·(a_x + g·sin θ)·h/L to the rear (drag is taken to act at ground
    height, so it adds no pitch) and the downforce ½·ρ·CzA·v² split by the
    aero balance (the front's share, 0-1; a negative CzA lifts). An axle
    cannot pull on the road: one that would carry less than nothing carries
    nothing, and the other axle takes the rest, so the two still carry the
    weight and the downforce together; a lift greater than the weight lifts
    both, and neither carries anything."""
    dz = m * (a_x + GRAVITY * sin_t) * h / wheelbase if wheelbase > 0 else 0.0
    down = 0.5 * rho * cza * v * v
    d_front, d_rear = down * aero_front - dz, down * (1.0 - aero_front) + dz
    if w_front + w_rear + down < 0:
        return -w_front, -w_rear, "Both"
    if w_front + d_front < 0:
        return -w_front, d_rear + w_front + d_front, "Front"
    if w_rear + d_rear < 0:
        return d_front + w_rear + d_rear, -w_rear, "Rear"
    return d_front, d_rear, None


def lateral_load_shift(m: float, h: float, a_y: float, track_front: float,
                       track_rear: float, front_share: float) -> tuple[float, float]:
    """(load moved from the inner to the outer wheels of the front axle, of
    the rear axle), in N: the cornering moment m·|a_y|·h, taken by the front
    axle in ``front_share`` (0-1) and by the rear axle in the rest, over each
    axle's track width. An axle with a track width of 0 or less takes none."""
    moment = m * abs(a_y) * h
    return (front_share * moment / track_front if track_front > 0 else 0.0,
            (1.0 - front_share) * moment / track_rear if track_rear > 0 else 0.0)


def tyre_mu(w, fz: float, lateral: bool = False) -> float:
    """A tyre's friction coefficient at the normal load ``fz`` (N): its μ
    (its lateral μ_y when ``lateral`` and one is set), changed by its load
    sensitivity dμ/dFz per N away from its nominal load (its static load when
    none is set), never below 0. The one tyre-friction entry point: the drive
    cycles' tyre force and lap mode read grip through it."""
    mu = w.mu_y if lateral and w.mu_y > 0 else w.mu
    return max(0.0, mu + w.dmu_per_n * (fz - (w.fz0 or w.fz_static)))


# The air vehicles drive in (sea level to about 5,500 m): beyond it a value is
# more likely typed in the wrong unit (bar, Pa, °F, K) than meant.
AMBIENT_C = (-60.0, 60.0)
AMBIENT_KPA = (50.0, 110.0)
# the air density without an Ambient block (20 °C, 101.325 kPa: 1.2041 kg/m³),
# and the density a Vehicle's road-load coefficient C is taken at
AIR_DENSITY = air_density()
MAX_SUBSTEP = 0.01  # s
V_EPS = 0.5  # m/s — slip regularization
W_EPS = 0.5  # rad/s — static/dynamic brake threshold
CLUTCH_BAND = 0.5  # rad/s — smooth Coulomb band (residual slip under load)
RPM = 60.0 / (2.0 * math.pi)
# an E-Motor's drive torque falls to zero over this share of its maximum
# speed below it (ponytail: a constant; make it a parameter when users
# bring their inverter's speed-limit ramp)
SPEED_LIMIT_BAND = 0.02

EmitFn = Callable[[dict], None]
ControlFn = Callable[[], list[dict]]


class SingularMatrixError(RuntimeError):
    """The driveline mass matrix could not be solved (degenerate configuration)."""


def solve_linear(m: list[list[float]], q: list[float]) -> list[float]:
    """Gaussian elimination with partial pivoting (systems are tiny). One-
    and two-coordinate systems, the common drivelines, are solved inline
    with exactly the operations of the general loop below."""
    n = len(q)
    if n == 1:
        a00 = m[0][0]
        if abs(a00) < 1e-12:
            raise SingularMatrixError(f"pivot {a00:.3e} in column 1 of 1")
        return [q[0] / a00]
    if n == 2:
        (a00, a01), (a10, a11) = m
        q0, q1 = q
        if abs(a10) > abs(a00):  # pivot; a tie keeps the first row, as max() does
            a00, a01, q0, a10, a11, q1 = a10, a11, q1, a00, a01, q0
        if abs(a00) < 1e-12:
            raise SingularMatrixError(f"pivot {a00:.3e} in column 1 of 2")
        fac = a10 / a00
        if fac:
            a11 -= fac * a01
            q1 -= fac * q0
        if abs(a11) < 1e-12:
            raise SingularMatrixError(f"pivot {a11:.3e} in column 2 of 2")
        x1 = q1 / a11
        return [(q0 - (0.0 + a01 * x1)) / a00, x1]
    a = [row[:] + [q[i]] for i, row in enumerate(m)]
    for col in range(n):
        piv = max(range(col, n), key=lambda r: abs(a[r][col]))
        if abs(a[piv][col]) < 1e-12:
            raise SingularMatrixError(
                f"pivot {a[piv][col]:.3e} in column {col + 1} of {n}")
        a[col], a[piv] = a[piv], a[col]
        for r in range(col + 1, n):
            fac = a[r][col] / a[col][col]
            if fac:
                for c in range(col, n + 1):
                    a[r][c] -= fac * a[col][c]
    x = [0.0] * n
    for r in range(n - 1, -1, -1):
        s = a[r][n] - sum(a[r][c] * x[c] for c in range(r + 1, n))
        x[r] = s / a[r][r]
    return x


@dataclass
class BatteryState:
    el_id: str
    soc: float
    q_ah: float  # charge capacity, A·h: the SOC counts charge
    min_soc: float
    r0: float
    r1: float
    tau: float
    max_charge_w: float
    ocv_map: Map
    eta_charge: float = 1.0  # coulombic efficiency: share of charging current stored
    v_rc: float = 0.0
    v_term: float = 0.0
    depleted_flagged: bool = False
    energy_out_wh: float = 0.0
    energy_in_wh: float = 0.0
    loss_wh: float = 0.0
    current: float = 0.0
    power_w: float = 0.0
    p_peak_w: float = 0.0  # the highest terminal power over a solver step, W
    i_peak_a: float = 0.0  # the highest discharge current over a solver step, A
    v_peak: float = 0.0  # the highest terminal voltage at a solver step's end, V
    v_low: float = math.inf  # the lowest terminal voltage at a solver step's end, V
    p_sq_ws: float = 0.0  # ∫ (terminal power)² dt, W²·s (for the RMS power)
    t_on: float = 0.0  # the time those cover, s
    # the Output Power Limit the terminals are held to, W (output_power_cap_w),
    # whether it and not the cells set this step's deliverable power, and the
    # run checks when a limit or a Voltage Class is set
    p_cap_w: float = math.inf
    capped: bool = False
    check: Optional[TerminalCheck] = None

    def ocv(self) -> float:
        return self.ocv_map.at(self.soc_pct())

    def soc_pct(self) -> float:
        """The SOC the OCV table is read at, %."""
        return max(0.0, min(1.0, self.soc)) * 100.0


@dataclass
class TerminalCheck:
    """A battery's terminal power and voltage over a run, for the checks
    against its Output Power Limit and Voltage Class (verdict.terminal_checks).
    Fed once per solver step with the step's terminal power (constant over
    the step) and its end voltage. The power check is on the moving average
    over ``window_s`` (0: the step's own power), with no power before t = 0,
    as an energy meter's log starts."""

    limit_w: float  # 0 = no power limit
    window_s: float
    v_class: float  # 0 = no Voltage Class
    v_full: float  # open-circuit voltage at 100 % SOC
    enforced: bool = True  # the limit holds the power (else it is only checked)
    peak_w: float = 0.0
    avg_peak_w: float = 0.0
    t_avg_peak: float = 0.0
    v_peak: float = 0.0  # the highest and lowest terminal voltage, from t = 0
    t_v_peak: float = 0.0
    p_at_v_peak: float = 0.0
    v_min: float = math.inf
    t_v_min: float = 0.0
    limit_s: float = 0.0  # time held at (enforced) or over (checked) the limit
    _q: deque = field(default_factory=deque)  # (t0, t1, W) in the window
    _e: float = 0.0  # their energy, J

    def add(self, t0: float, t1: float, p_w: float, v: float, held: bool) -> None:
        self.peak_w = max(self.peak_w, p_w)
        if v > self.v_peak:
            self.v_peak, self.t_v_peak, self.p_at_v_peak = v, t1, p_w
        if v < self.v_min:
            self.v_min, self.t_v_min = v, t1
        if held:
            self.limit_s += t1 - t0
        avg = p_w
        if self.window_s > 0:
            self._q.append((t0, t1, p_w))
            self._e += p_w * (t1 - t0)
            lo = t1 - self.window_s
            while self._q[0][1] <= lo:
                a, b, p = self._q.popleft()
                self._e -= p * (b - a)
            a, _, p = self._q[0]  # the oldest step counts from the window's start
            avg = (self._e - max(0.0, lo - a) * p) / self.window_s
        if avg > self.avg_peak_w:
            self.avg_peak_w, self.t_avg_peak = avg, t1


def output_power_cap_w(p: dict) -> float:
    """The terminal power a battery is held to, W: its Output Power Limit less
    its margin, or inf with no limit (0) or when the limit is only checked."""
    limit_kw = float(p.get("output_power_limit_kW", 0) or 0)
    if limit_kw <= 0 or not p.get("power_limit_enforced", True):
        return math.inf
    return limit_kw * 1000.0 * (1.0 - float(p.get("power_limit_margin_pct", 0) or 0) / 100.0)


def ocv_mean(points: list, linear: bool = False, lo: float = 0.0, hi: float = 100.0) -> float:
    """The OCV table's mean over ``lo`` to ``hi`` % SOC (lo < hi; 0-100 %),
    weighted by SOC: a discharge at open circuit over that span gives out
    this voltage times the charge it moves. Exact for the piecewise-linear
    table, read as ocv() reads it: beyond its ends flat, or on the end slope
    when its SOC axis is set to Linear (``linear``)."""
    xs = sorted({lo, hi, *(x for x, _ in points if lo < x < hi)})
    return sum((b - a) * (interp1(points, a, linear) + interp1(points, b, linear))
               for a, b in zip(xs, xs[1:])) / (2.0 * (hi - lo))


def usable_energy_left_wh(b: BatteryState) -> float:
    """The open-circuit energy a battery can still give before its minimum SOC, W·h."""
    soc, floor = max(0.0, min(1.0, b.soc)), max(0.0, min(1.0, b.min_soc))
    if soc <= floor:
        return 0.0
    return b.q_ah * (soc - floor) * ocv_mean(b.ocv_map.pts, b.ocv_map.linear[0],
                                             floor * 100.0, soc * 100.0)


def motor_max_rpm(full_load: Sheets2D, value: object) -> float:
    """An E-Motor's maximum speed, 1/min: its Maximum Speed, or when that is
    0 (every project before 0.3) the last speed point of its full-load
    curve, the lowest over its voltage sheets so none is read past its data."""
    try:
        n = float(value or 0)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        n = 0.0
    r = inner_range(full_load)
    return n if n > 0 else r[1] if r else math.inf


@dataclass
class MotorCache:
    el_id: str
    full_load: Map
    loss: Map
    drag: Map
    q4_scale: float
    max_rpm: float  # maximum speed (motor_max_rpm)
    speed_use: MapUse  # time above the maximum speed and the highest speed
    overshoot_rpm: float = 0.0  # see RunContext.over_speed
    rpm: float = 0.0
    torque: float = 0.0
    p_mech_w: float = 0.0
    p_loss_w: float = 0.0
    p_elec_w: float = 0.0
    # source-limit handshake: this step's electrical power window (W) and the
    # time the supply held the torque below the command
    p_lo_w: float = -math.inf
    p_hi_w: float = math.inf
    limited_s: float = 0.0
    # regeneration the command asked for that the supply could not take
    # (electrical energy the motor was not allowed to feed back)
    regen_lost_wh: float = 0.0
    # (command, speed, torque, inverter on, electrical W) evaluated by the
    # handshake this step, reused when the mechanics apply the same command
    request: Optional[tuple] = None


@dataclass
class EngineCache:
    el_id: str
    full_load: Map
    drag: Map
    fuel_map: Map
    idle_rpm: float
    reentry_rpm: float  # zero throttle above this speed cuts the fuel
    speed_use: MapUse  # time above the full-load curve's last speed
    overshoot_rpm: float = 0.0  # see RunContext.over_speed
    rpm: float = 0.0
    torque: float = 0.0
    fuel_kgh: float = 0.0
    p_mech_w: float = 0.0
    fuel_used_kg: float = 0.0
    stalled_flagged: bool = False


@dataclass
class TankState:
    el_id: str
    capacity_kg: float
    mass_kg: float
    empty_flagged: bool = False


@dataclass
class FuelCellCache:
    el_id: str
    pol: Map  # V(I)
    i_max: float
    h2_g_per_kwh: float
    voltage: float = 0.0
    current: float = 0.0
    power_w: float = 0.0
    h2_kgh: float = 0.0
    energy_wh: float = 0.0


@dataclass
class DrivePlan:
    """Per-driveline linear structure for the current joint configuration."""
    n: int = 0
    gvec: list[list[float]] = field(default_factory=list)  # per segment
    x: list[float] = field(default_factory=list)
    coord_root: list[int] = field(default_factory=list)  # segment idx per coordinate
    root_of_seg: list[int] = field(default_factory=list)
    scale_of_seg: list[float] = field(default_factory=list)
    eff_chain: list[float] = field(default_factory=list)  # per segment
    gear_key: tuple = ()
    lock_key: tuple = ()
    over_constrained: bool = False


@dataclass
class DrivelineState:
    dl: Driveline
    plan: DrivePlan = field(default_factory=DrivePlan)
    # channel bookkeeping
    joint_torque_a: dict[str, float] = field(default_factory=dict)
    joint_torque_b: dict[str, float] = field(default_factory=dict)
    joint_speed_in: dict[str, float] = field(default_factory=dict)
    clutch_torque: dict[str, float] = field(default_factory=dict)
    clutch_slip: dict[str, float] = field(default_factory=dict)
    chain_power_w: float = 0.0
    # solver-step view of the plan (domains.DrivelineLayout), built on first
    # use and dropped whenever the plan is rebuilt
    layout: Optional[object] = None
    # segment speeds at the end of the last solver step
    omega_end: list[float] = field(default_factory=list)


class Runtime:
    def __init__(self, model: Model, emit: Optional[EmitFn]):
        self.model = model
        self.emit = emit
        self.messages: list[SimMessage] = []
        self.warned: set[str] = set()
        self.signal_values: dict[tuple[str, str], float] = {}
        # recorded values; None marks "no data yet" gaps from decimated backfill
        self.series: dict[tuple[str, str], list[float | None]] = defaultdict(list)

    def message(self, level: str, text: str) -> None:
        self.messages.append(SimMessage(level=level, text=text))  # type: ignore[arg-type]
        if self.emit:
            self.emit({"type": "message", "level": level, "text": text})

    def warn_once(self, key: str, text: str, level: str = "warning") -> None:
        if key not in self.warned:
            self.warned.add(key)
            self.message(level, text)

    def read_signal(self, el_id: str, port_id: str) -> float | None:
        src = self.model.signal_route.get((el_id, port_id))
        if src is None:
            return None
        return self.signal_values.get(src)

    def publish(self, el_id: str, port_id: str, value: float) -> None:
        self.signal_values[(el_id, port_id)] = value


def _sign(x: float) -> float:
    return 1.0 if x > 0 else (-1.0 if x < 0 else 0.0)


def make_plan(dl: Driveline, params_of: dict, gear_of: dict[str, float]) -> DrivePlan:
    """Resolve the segment tree into coordinates + g-vectors for the
    current lock states and gear ratios. Rigid merges (locked splits) go
    through a weighted union-find; the remaining open-split constraints
    form a homogeneous linear system whose null space provides the
    independent coordinates — this handles shared parents (e.g. a locked
    transfer case above two open axle differentials) uniformly."""
    n_seg = len(dl.segments)
    plan = DrivePlan()
    plan.gear_key = tuple(sorted(
        (g.el_id, gear_of.get(g.el_id, float(params_of[g.el_id].get("default_gear", 1) or 1)))
        for seg in dl.segments for g in seg.gearboxes))
    lock_states = {}
    for j in dl.joints:
        if j.kind == "split":
            lock_states[j.el_id] = bool(params_of[j.el_id].get("locked", j.locked))
    plan.lock_key = tuple(sorted(lock_states.items()))

    # weighted union-find: ω_i = weight[i] · ω_parent[i]
    parent = list(range(n_seg))
    weight = [1.0] * n_seg

    def find(i: int) -> tuple[int, float]:
        w = 1.0
        while parent[i] != i:
            w *= weight[i]
            i = parent[i]
        return i, w

    def union(i: int, j: int, r: float) -> None:
        """Declare ω_i = r · ω_j."""
        ri, wi = find(i)
        rj, wj = find(j)
        if ri == rj:
            return
        # ω_ri = ω_i / wi = r·ω_j / wi = (r·wj/wi)·ω_rj
        parent[ri] = rj
        weight[ri] = r * wj / wi

    for j in dl.joints:
        if j.kind == "split" and lock_states.get(j.el_id, j.locked):
            # locked: both output axes and (via ratio) the input axis are rigid
            union(j.child_a, j.child_b, j.child_b_m / j.child_a_m)
            if j.parent_seg >= 0:
                union(j.parent_seg, j.child_a, j.ratio * j.child_a_m / j.parent_m)

    roots: list[int] = []
    root_of, scale_of = [0] * n_seg, [1.0] * n_seg
    for s in range(n_seg):
        r, w = find(s)
        root_of[s], scale_of[s] = r, w
        if r not in roots:
            roots.append(r)
    plan.root_of_seg, plan.scale_of_seg = root_of, scale_of
    m = len(roots)
    col_of_root = {r: c for c, r in enumerate(roots)}

    # open-split constraints over the group variables:
    #   ω_parent_axis − ratio·((1−f)·ω_a_axis + f·ω_b_axis) = 0
    rows: list[list[float]] = []
    for j in dl.joints:
        if j.kind != "split" or lock_states.get(j.el_id, j.locked) or j.parent_seg < 0:
            continue
        row = [0.0] * m
        row[col_of_root[root_of[j.parent_seg]]] += j.parent_m * scale_of[j.parent_seg]
        row[col_of_root[root_of[j.child_a]]] -= (
            j.ratio * (1.0 - j.f_b) * j.child_a_m * scale_of[j.child_a])
        row[col_of_root[root_of[j.child_b]]] -= (
            j.ratio * j.f_b * j.child_b_m * scale_of[j.child_b])
        if any(abs(v) > 1e-12 for v in row):
            rows.append(row)

    # column order: pivot anchor-less groups first, so groups that carry a
    # wheel/source/prop end up as the free (state) variables
    def group_has_anchor(root: int) -> bool:
        return any(
            (seg.wheels or seg.sources or seg.props)
            for s, seg in enumerate(dl.segments) if root_of[s] == root
        )

    col_order = sorted(range(m), key=lambda c: (group_has_anchor(roots[c]), c))

    # RREF over the permuted columns → pivot/free split + expressions
    a = [row[:] for row in rows]
    pivots: list[tuple[int, int]] = []  # (row, col)
    r_idx = 0
    for col in col_order:
        if r_idx >= len(a):
            break
        piv = max(range(r_idx, len(a)), key=lambda rr: abs(a[rr][col]))
        if abs(a[piv][col]) < 1e-10:
            continue
        a[r_idx], a[piv] = a[piv], a[r_idx]
        pv = a[r_idx][col]
        a[r_idx] = [v / pv for v in a[r_idx]]
        for rr in range(len(a)):
            if rr != r_idx and abs(a[rr][col]) > 1e-12:
                fac = a[rr][col]
                a[rr] = [a[rr][c2] - fac * a[r_idx][c2] for c2 in range(m)]
        pivots.append((r_idx, col))
        r_idx += 1

    pivot_cols = {c for _, c in pivots}
    free_cols = [c for c in col_order if c not in pivot_cols]
    plan.n = len(free_cols)
    if plan.n == 0:
        plan.over_constrained = True
        return plan
    plan.coord_root = [roots[c] for c in free_cols]

    g_col: dict[int, list[float]] = {}
    for k, c in enumerate(free_cols):
        g_col[c] = [1.0 if i == k else 0.0 for i in range(plan.n)]
    for r, c in pivots:  # ω_pivot = −Σ_free R[r][free]·ω_free
        g_col[c] = [-a[r][fc] * 1.0 for fc in free_cols]

    plan.gvec = [
        [scale_of[s] * v for v in g_col[col_of_root[root_of[s]]]]
        for s in range(n_seg)
    ]
    plan.x = [0.0] * plan.n

    # torque-weighted split-efficiency chain below each segment: the split,
    # then each child's gears from the split to its own output, and so on
    split_below: dict[int, object] = {}
    for j in dl.joints:
        if j.kind == "split" and j.parent_seg >= 0:
            split_below[j.parent_seg] = j

    def eff_chain(seg_idx: int, depth: int = 0) -> float:
        if depth > 8:
            return 1.0
        j = split_below.get(seg_idx)
        if j is None:
            return 1.0
        seg_a, seg_b = dl.segments[j.child_a], dl.segments[j.child_b]
        return j.eff * ((1.0 - j.f_b) * seg_a.path_eff(j.child_a_region)
                        * eff_chain(j.child_a, depth + 1)
                        + j.f_b * seg_b.path_eff(j.child_b_region)
                        * eff_chain(j.child_b, depth + 1))

    plan.eff_chain = [eff_chain(s) for s in range(n_seg)]
    return plan
