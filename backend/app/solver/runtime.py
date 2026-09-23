"""Shared solver runtime: constants, per-element state caches, the message/
signal Runtime, the driveline-plan resolver and the tiny linear solver.

Everything here is domain-agnostic plumbing used by the wrapped domain
slaves (domains.py) and the simulate() orchestration (core.py). Moved
verbatim out of core.py in Phase 1.4 — behavior is unchanged.
"""
from __future__ import annotations

import math
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Callable, Optional

from ..schemas import SimMessage
from .maps import interp1
from .network import Driveline, Model

GRAVITY = 9.81
AIR_DENSITY = 1.2
MAX_SUBSTEP = 0.01  # s
V_EPS = 0.5  # m/s — slip regularization
W_EPS = 0.5  # rad/s — static/dynamic brake threshold
CLUTCH_BAND = 0.5  # rad/s — smooth Coulomb band (residual slip under load)
RPM = 60.0 / (2.0 * math.pi)

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
    capacity_wh: float
    min_soc: float
    r0: float
    r1: float
    tau: float
    max_charge_w: float
    ocv_pts: list
    v_rc: float = 0.0
    v_term: float = 0.0
    depleted_flagged: bool = False
    energy_out_wh: float = 0.0
    energy_in_wh: float = 0.0
    loss_wh: float = 0.0
    current: float = 0.0
    power_w: float = 0.0

    def ocv(self) -> float:
        return interp1(self.ocv_pts, max(0.0, min(1.0, self.soc)) * 100.0)


@dataclass
class MotorCache:
    el_id: str
    full_load: list
    loss: list
    drag: list
    q4_scale: float
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
    full_load: list
    drag: list
    fuel_map: list
    idle_rpm: float
    reentry_rpm: float  # zero throttle above this speed cuts the fuel
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
    pol: list  # V(I)
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

    # torque-weighted split-efficiency chain below each segment
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
        return j.eff * ((1.0 - j.f_b) * eff_chain(j.child_a, depth + 1)
                        + j.f_b * eff_chain(j.child_b, depth + 1))

    plan.eff_chain = [eff_chain(s) for s in range(n_seg)]
    return plan
