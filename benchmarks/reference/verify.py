"""The exact answers checked against a very fine numerical integration of
each problem's stated equations, written here a second time and
independently of exact.py's derivations: the states and the energy
integrals are integrated together (each energy is an extra state, its
power the right-hand side), events are located on the dense output, and a
phase change (a stop, a lock-up, a limit) starts a new integration from
the state the event left.

With SciPy, DOP853 at rtol = atol = 1e-13 (relative to each state's scale);
without it, classical RK4 at 20 000 steps per phase with cubic Hermite
dense output. The gear change's jump rule is checked against a dry clutch
of a trillion times the motor torque engaging the new gear, not against
the momentum rule exact.py uses.

    python -m benchmarks.reference.verify      (from the repository root)
"""
from __future__ import annotations

import math
import sys
from typing import Callable

from . import Problem, load_all
from .exact import solve

try:  # SciPy is a test dependency of the backend; the fallback needs nothing
    from scipy.integrate import solve_ivp
except ImportError:  # pragma: no cover - exercised only without SciPy
    solve_ivp = None

RTOL = 1e-13
State = list[float]
Rhs = Callable[[float, State], State]


class Segment:
    """One phase's dense solution: state(t) for t in [t0, t1]."""

    def __init__(self, t0: float, t1: float, state: Callable[[float], State]):
        self.t0, self.t1, self.state = t0, t1, state


def _ivp_scipy(rhs: Rhs, t0: float, x0: State, t1: float, event=None, scale=None):
    sc = scale or [max(1.0, abs(v)) for v in x0]
    ev = None
    if event is not None:
        def ev(t, x):
            return event(t, list(x))
        ev.terminal, ev.direction = True, 0
    sol = solve_ivp(lambda t, x: rhs(t, list(x)), (t0, t1), list(x0), method="DOP853",
                    rtol=RTOL, atol=[RTOL * s for s in sc], dense_output=True,
                    events=ev)
    if sol.status < 0:
        raise RuntimeError(sol.message)
    t_end = float(sol.t[-1])
    t_ev = float(sol.t_events[0][0]) if event is not None and len(sol.t_events[0]) else None
    return Segment(t0, t_end, lambda t: list(sol.sol(t))), t_ev


def _rk4(rhs: Rhs, t: float, x: State, h: float) -> State:
    k1 = rhs(t, x)
    k2 = rhs(t + h / 2, [a + h / 2 * b for a, b in zip(x, k1)])
    k3 = rhs(t + h / 2, [a + h / 2 * b for a, b in zip(x, k2)])
    k4 = rhs(t + h, [a + h * b for a, b in zip(x, k3)])
    return [a + h / 6 * (b + 2 * c + 2 * d + e) for a, b, c, d, e in zip(x, k1, k2, k3, k4)]


def _ivp_rk4(rhs: Rhs, t0: float, x0: State, t1: float, event=None, scale=None,
             steps: int = 20000):  # pragma: no cover - exercised only without SciPy
    h = (t1 - t0) / steps
    ts, xs, fs = [t0], [list(x0)], [rhs(t0, list(x0))]
    t_ev = None
    for k in range(steps):
        t, x = ts[-1], xs[-1]
        x_new = _rk4(rhs, t, x, h)
        if event is not None and (event(t, x) > 0) != (event(t + h, x_new) > 0):
            lo, hi = 0.0, h
            for _ in range(80):
                mid = (lo + hi) / 2
                if (event(t + mid, _rk4(rhs, t, x, mid)) > 0) == (event(t, x) > 0):
                    lo = mid
                else:
                    hi = mid
            t_ev = t + hi
            x_new = _rk4(rhs, t, x, hi)
            ts.append(t_ev), xs.append(x_new), fs.append(rhs(t_ev, x_new))
            break
        ts.append(t0 + (k + 1) * h), xs.append(x_new), fs.append(rhs(ts[-1], x_new))

    def state(t: float) -> State:
        j = min(max(0, int((t - t0) / h)), len(ts) - 2)
        ta, tb = ts[j], ts[j + 1]
        hh = tb - ta
        s = (t - ta) / hh if hh > 0 else 0.0
        h00, h10 = 2 * s ** 3 - 3 * s ** 2 + 1, s ** 3 - 2 * s ** 2 + s
        h01, h11 = -2 * s ** 3 + 3 * s ** 2, s ** 3 - s ** 2
        return [h00 * a + h10 * hh * fa + h01 * b + h11 * hh * fb
                for a, b, fa, fb in zip(xs[j], xs[j + 1], fs[j], fs[j + 1])]

    return Segment(t0, ts[-1], state), t_ev


def ivp(rhs: Rhs, t0: float, x0: State, t1: float, event=None, scale=None):
    """Integrate x' = rhs(t, x) from (t0, x0) to t1, stopping at the first
    zero of event(t, x) when given. Returns (Segment, event time or None)."""
    run = _ivp_scipy if solve_ivp is not None else _ivp_rk4
    return run(rhs, t0, x0, t1, event, scale)


def crossing(seg: Segment, fn: Callable[[float, State], float]) -> float:
    """The first zero of fn(t, state) on a segment's dense output (grid,
    then bisection)."""
    n = 20000
    prev_t = seg.t0
    prev = fn(prev_t, seg.state(prev_t))
    for k in range(1, n + 1):
        t = seg.t0 + (seg.t1 - seg.t0) * k / n
        cur = fn(t, seg.state(t))
        if (cur > 0) != (prev > 0) or cur == 0:
            lo, hi = prev_t, t
            for _ in range(200):
                mid = (lo + hi) / 2
                if mid in (lo, hi):
                    break
                if (fn(mid, seg.state(mid)) > 0) == (prev > 0):
                    lo = mid
                else:
                    hi = mid
            return (lo + hi) / 2
        prev_t, prev = t, cur
    raise ValueError("no crossing")


class Numeric:
    """A numerical solution: phases of dense state, and how each compared
    quantity is read from (t, state)."""

    def __init__(self):
        self.segments: list[tuple[Segment, Callable[[float, State], dict]]] = []
        self.events: dict[str, float] = {}

    def add(self, seg: Segment, read: Callable[[float, State], dict]) -> None:
        self.segments.append((seg, read))

    def at(self, t: float) -> dict:
        chosen = self.segments[0]
        for seg, read in self.segments:
            if t >= seg.t0:
                chosen = (seg, read)
        seg, read = chosen
        return read(t, seg.state(min(t, seg.t1)))


# ---- the stated equations, model by model ---------------------------------------------------

def _rc_step(p, x0, t_end):
    V, R, C = p["V"], p["R"], p["C"]

    def rhs(t, x):
        i = (V - x[0]) / R
        return [i / C, V * i, R * i * i]

    seg, _ = ivp(rhs, 0.0, [x0["v_C"], 0.0, 0.0], t_end, scale=[V, C * V * V, C * V * V])
    n = Numeric()
    n.add(seg, lambda t, x: {"v_C": x[0], "i": (V - x[0]) / R, "E_source": x[1], "E_R": x[2],
                             "E_C": C / 2 * (x[0] ** 2 - x0["v_C"] ** 2)})
    n.events["t_event"] = crossing(seg, lambda t, x: x[0] - p["event_fraction"] * V)
    return n


def _rl_step(p, x0, t_end):
    V, R, L = p["V"], p["R"], p["L"]

    def rhs(t, x):
        return [(V - R * x[0]) / L, V * x[0], R * x[0] ** 2]

    i_inf = V / R
    seg, _ = ivp(rhs, 0.0, [x0["i"], 0.0, 0.0], t_end, scale=[i_inf, L * i_inf ** 2, L * i_inf ** 2])
    n = Numeric()
    n.add(seg, lambda t, x: {"i": x[0], "v_L": V - R * x[0], "E_source": x[1], "E_R": x[2],
                             "E_L": L / 2 * (x[0] ** 2 - x0["i"] ** 2)})
    n.events["t_event"] = crossing(seg, lambda t, x: x[0] - p["i_event"])
    return n


def _battery_rhs(p, current_of):
    """(SOC, v1, E_chem, E_terminal, E_R0, E_R1) with the current a function
    of (t, SOC, v1); OCV = ocv_a + ocv_b SOC (or flat E)."""
    r0, r1, tau = p["R0"], p["R1"], p["tau1"]
    k = 1.0 / (3600.0 * p["Q_Ah"])

    def ocv(soc):
        return p["E"] if "E" in p else p["ocv_a"] + p["ocv_b"] * soc

    def rhs(t, x):
        soc, v1 = x[0], x[1]
        i = current_of(t, soc, v1)
        v = ocv(soc) - r0 * i - v1
        return [-k * i, i * r1 / tau - v1 / tau, ocv(soc) * i, v * i, r0 * i * i, v1 * v1 / r1]

    def read(t, x):
        soc, v1 = x[0], x[1]
        i = current_of(t, soc, v1)
        c1 = tau / r1
        e_c1 = c1 / 2 * v1 * v1
        return {"SOC": soc, "v1": v1, "I": i, "V": ocv(soc) - r0 * i - v1, "E_chem": x[2],
                "E_terminal": x[3], "E_R0": x[4], "E_R1": x[5], "E_C1": e_c1,
                "E_internal": x[4] + x[5] + e_c1}

    return rhs, read, ocv


def _battery_cc(p, x0, t_end):
    rhs, read, _ = _battery_rhs(p, lambda t, soc, v1: p["I"])
    e = p["I"] * 400.0 * t_end
    seg, _ = ivp(rhs, 0.0, [x0["SOC"], x0["v1"], 0, 0, 0, 0], t_end,
                 scale=[1.0, p["I"] * p["R1"], e, e, e, e])
    n = Numeric()
    n.add(seg, read)
    return n


def _battery_cp(p, x0, t_end):
    P, E, r0 = p["P"], p["E"], p["R0"]

    def current(t, soc, v1):
        u = E - v1
        return (u - math.sqrt(u * u - 4.0 * r0 * P)) / (2.0 * r0)

    rhs, read, _ = _battery_rhs(p, current)
    e = P * t_end
    seg, _ = ivp(rhs, 0.0, [x0["SOC"], x0["v1"], 0, 0, 0, 0], t_end,
                 scale=[1.0, 100.0, e, e, e, e])
    n = Numeric()
    n.add(seg, read)
    return n


def _battery_voltage_limit(p, x0, t_end):
    r0, v_min = p["R0"], p["V_min"]
    rhs1, read1, ocv = _battery_rhs(p, lambda t, soc, v1: p["I"])
    e = p["I"] * 400.0 * t_end
    scale = [1.0, p["I"] * p["R1"], e, e, e, e]
    seg1, t_star = ivp(rhs1, 0.0, [x0["SOC"], x0["v1"], 0, 0, 0, 0], t_end,
                       event=lambda t, x: ocv(x[0]) - r0 * p["I"] - x[1] - v_min, scale=scale)
    n = Numeric()
    n.add(seg1, read1)
    n.events["t_vmin"] = t_star
    rhs2, read2, _ = _battery_rhs(p, lambda t, soc, v1: (ocv(soc) - v1 - v_min) / r0)
    seg2, _ = ivp(rhs2, t_star, seg1.state(t_star), t_end, scale=scale)
    n.add(seg2, read2)
    return n


def _dc_motor(p, x0, t_end):
    V, R, L, k, J, b, tl = p["V"], p["R"], p["L"], p["k"], p["J"], p["b"], p["T_load"]
    w_inf = (k * V - R * tl) / (k * k + R * b)
    e = V * V / R * t_end
    if L > 0:
        def rhs(t, x):
            i, w = x[0], x[1]
            return [(V - R * i - k * w) / L, (k * i - b * w - tl) / J,
                    V * i, R * i * i, b * w * w, tl * w]

        def cur(x):
            return x[0]
        x_init = [x0["i"], x0["omega"], 0, 0, 0, 0]
        scale = [V / R, w_inf, e, e, e, e]
        wi = 1
    else:
        def cur(x):
            return (V - k * x[0]) / R

        def rhs(t, x):
            i, w = cur(x), x[0]
            return [(k * i - b * w - tl) / J, V * i, R * i * i, b * w * w, tl * w]
        x_init = [x0["omega"], 0, 0, 0, 0]
        scale = [w_inf, e, e, e, e]
        wi = 0
    seg, _ = ivp(rhs, 0.0, x_init, t_end, scale=scale)
    i0 = x0["i"]

    def read(t, x):
        i, w = cur(x), x[wi]
        return {"i": i, "omega": w, "T_e": k * i, "E_in": x[wi + 1], "E_R": x[wi + 2],
                "E_friction": x[wi + 3], "E_load": x[wi + 4],
                "E_L": L / 2 * (i * i - i0 * i0), "E_kin": J / 2 * (w * w - x0["omega"] ** 2)}

    n = Numeric()
    n.add(seg, read)
    n.events["t_event"] = crossing(seg, lambda t, x: x[wi] - p["event_fraction"] * w_inf)
    return n


def _hold(x_end: State, t0: float) -> Segment:
    return Segment(t0, math.inf, lambda t: list(x_end))


def _inertia_coastdown(p, x0, t_end):
    J, c, tc = p["J"], p["c"], p["T_c"]
    w0 = x0["omega"]

    def rhs(t, x):
        return [(-c * x[0] - tc) / J, x[0], c * x[0] ** 2]

    ke0 = J / 2 * w0 * w0
    seg, t_stop = ivp(rhs, 0.0, [w0, x0["theta"], 0.0], t_end, event=lambda t, x: x[0],
                      scale=[w0, w0 * t_end, ke0])

    def read(t, x):
        return {"omega": x[0], "theta": x[1], "E_viscous": x[2], "E_coulomb": tc * x[1],
                "E_kin": J / 2 * x[0] ** 2 - ke0}

    n = Numeric()
    n.add(seg, read)
    end = seg.state(t_stop)
    end[0] = 0.0
    n.add(_hold(end, t_stop), read)
    n.events["t_stop"] = t_stop
    return n


def _clutch_lockup(p, x0, t_end):
    J1, J2, tc, t1, t2, eps = p["J1"], p["J2"], p["T_c"], p["T_drive"], p["T_load"], p["eps_lock"]

    def slip_rhs(t, x):  # (omega1, omega2, theta1, theta2, E_clutch)
        return [(t1 - tc) / J1, (tc - t2) / J2, x[0], x[1], tc * (x[0] - x[1])]

    w10, w20 = x0["omega1"], x0["omega2"]
    ke0 = J1 / 2 * w10 ** 2 + J2 / 2 * w20 ** 2
    scale = [w10, w10, w10, w10, ke0]
    seg1, t_meet = ivp(slip_rhs, 0.0, [w10, w20, 0, 0, 0], t_end,
                       event=lambda t, x: x[0] - x[1], scale=scale)

    def read(t, x, torque):
        return {"omega1": x[0], "omega2": x[1], "slip": x[0] - x[1], "T_clutch": torque,
                "E_drive": t1 * x[2], "E_load": t2 * x[3], "E_clutch": x[4],
                "E_kin": J1 / 2 * x[0] ** 2 + J2 / 2 * x[1] ** 2 - ke0}

    n = Numeric()
    n.add(seg1, lambda t, x: read(t, x, tc))
    n.events["t_lock"] = crossing(seg1, lambda t, x: x[0] - x[1] - eps)
    alpha = (t1 - t2) / (J1 + J2)
    passed = J2 * alpha + t2  # what J2 needs from the clutch to follow

    def locked_rhs(t, x):
        return [alpha, alpha, x[0], x[1], 0.0]

    start = seg1.state(t_meet)
    w = (start[0] + start[1]) / 2
    seg2, _ = ivp(locked_rhs, t_meet, [w, w, start[2], start[3], start[4]], t_end, scale=scale)
    n.add(seg2, lambda t, x: read(t, x, passed))
    return n


def _gear_change(p, x0, t_end):
    """Before and after the shift: one rigid coordinate (omega2) and the
    motor angle. The shift itself: the motor (J1) and the load reflected
    through i2 (J2 / i2^2) slip on a dry clutch of 1e12 x T until they
    meet, which is the engagement's impulse in the limit."""
    J1, J2, T, i1, i2, ts = p["J1"], p["J2"], p["T"], p["i1"], p["i2"], p["t_shift"]

    def rigid(i):
        def rhs(t, x):  # (omega2, theta1, E_drive)
            return [T * i / (J2 + i * i * J1), i * x[0], T * i * x[0]]
        return rhs

    ke0 = 0.5 * (J2 + J1 * i1 * i1) * x0["omega2"] ** 2
    seg1, _ = ivp(rigid(i1), 0.0, [x0["omega2"], 0.0, 0.0], ts, scale=[100.0, 1e4, 1e6])
    w2m, th1, e_dr = seg1.state(ts)
    w1 = i1 * w2m
    # the engagement: clutch torque Tc on the motor's shaft, i2 Tc on the load
    tc = 1e12 * T
    rate = (tc - T) / J1 + i2 * i2 * tc / J2  # d(omega1 − i2 omega2)/dt = −rate
    dt_slip = (w1 - i2 * w2m) / rate
    w2p = w2m + i2 * tc / J2 * dt_slip
    loss_slip = tc * (w1 - i2 * w2m) * dt_slip / 2  # the clutch's heat
    seg2, _ = ivp(rigid(i2), ts, [w2p, th1, e_dr], t_end, scale=[100.0, 1e4, 1e6])

    def read(i, loss):
        def r(t, x):
            w2 = x[0]
            ke = 0.5 * J2 * w2 * w2 + 0.5 * J1 * (i * w2) ** 2 - ke0
            return {"omega2": w2, "omega1": i * w2, "theta1": x[1], "E_drive": x[2],
                    "E_kin": ke, "E_shift": loss}
        return r

    n = Numeric()
    n.add(seg1, read(i1, 0.0))
    n.add(seg2, read(i2, loss_slip))
    return n


def _vehicle_coastdown(p, x0, t_end):
    m, A, C = p["m"], p["A"], p["C"]
    v0 = x0["v"]
    ke0 = 0.5 * m * v0 * v0

    def rhs(t, x):
        return [-(A + C * x[0] ** 2) / m, x[0], C * x[0] ** 3]

    seg, t_stop = ivp(rhs, 0.0, [v0, x0["x"], 0.0], t_end, event=lambda t, x: x[0],
                      scale=[v0, 3000.0, ke0])

    def read(t, x):
        return {"v": x[0], "x": x[1], "E_aero": x[2], "E_roll": A * x[1],
                "E_kin": 0.5 * m * x[0] ** 2 - ke0}

    n = Numeric()
    n.add(seg, read)
    end = seg.state(t_stop)
    end[0] = 0.0
    n.add(_hold(end, t_stop), read)
    n.events["t_stop"] = t_stop
    n.events["t_event"] = crossing(seg, lambda t, x: x[0] - p["v_event"])
    return n


def _vehicle_constant_power(p, x0, t_end):
    m, P = p["m"], p["P"]
    v0 = x0["v"]

    def rhs(t, x):
        return [P / (m * x[0]), x[0], P]

    seg, _ = ivp(rhs, 0.0, [v0, x0["x"], 0.0], t_end, scale=[30.0, 300.0, P * t_end])
    n = Numeric()
    n.add(seg, lambda t, x: {"v": x[0], "x": x[1], "E_supplied": x[2],
                             "E_kin": 0.5 * m * (x[0] ** 2 - v0 * v0)})
    n.events["t_event"] = crossing(seg, lambda t, x: x[0] - p["v_event"])
    return n


def _thermal_lumped(p, x0, t_end):
    C, G, P, t_off, ta = p["C"], p["G"], p["P"], p["t_off"], p["T_amb"]
    T0 = x0["T"]

    def rhs(heat):
        def f(t, x):  # (T, E_heat, E_ambient)
            return [(heat - G * (x[0] - ta)) / C, heat, G * (x[0] - ta)]
        return f

    e = P * t_off
    read = lambda t, x: {"T": x[0], "E_heat": x[1], "E_ambient": x[2],  # noqa: E731
                         "E_stored": C * (x[0] - T0)}
    seg1, _ = ivp(rhs(P), 0.0, [T0, 0.0, 0.0], t_off, scale=[100.0, e, e])
    seg2, _ = ivp(rhs(0.0), t_off, seg1.state(t_off), t_end, scale=[100.0, e, e])
    n = Numeric()
    n.add(seg1, read)
    n.add(seg2, read)
    n.events["t_hot"] = crossing(seg1, lambda t, x: x[0] - p["T_hot"])
    n.events["t_cool"] = crossing(seg2, lambda t, x: x[0] - p["T_cool"])
    return n


def _thermal_two_masses(p, x0, t_end):
    C1, C2, G12, G2a, P, ta = p["C1"], p["C2"], p["G12"], p["G2a"], p["P"], p["T_amb"]
    T10, T20 = x0["T1"], x0["T2"]

    def rhs(t, x):  # (T1, T2, E_ambient, E_12)
        q12, q2a = G12 * (x[0] - x[1]), G2a * (x[1] - ta)
        return [(P - q12) / C1, (q12 - q2a) / C2, q2a, q12]

    e = P * t_end
    seg, _ = ivp(rhs, 0.0, [T10, T20, 0.0, 0.0], t_end, scale=[100.0, 100.0, e, e])
    n = Numeric()
    n.add(seg, lambda t, x: {"T1": x[0], "T2": x[1], "E_heat": P * t, "E_ambient": x[2],
                             "E_12": x[3], "E_stored1": C1 * (x[0] - T10),
                             "E_stored2": C2 * (x[1] - T20)})
    n.events["t_event"] = crossing(seg, lambda t, x: x[0] - p["T1_event"])
    return n


NUMERIC = {
    "rc_step": _rc_step, "rl_step": _rl_step, "battery_cc": _battery_cc,
    "battery_cp": _battery_cp, "battery_voltage_limit": _battery_voltage_limit,
    "dc_motor": _dc_motor, "inertia_coastdown": _inertia_coastdown,
    "clutch_lockup": _clutch_lockup, "gear_change": _gear_change,
    "vehicle_coastdown": _vehicle_coastdown, "vehicle_constant_power": _vehicle_constant_power,
    "thermal_lumped": _thermal_lumped, "thermal_two_masses": _thermal_two_masses,
}


def numeric(problem: Problem) -> Numeric:
    return NUMERIC[problem.model](problem.parameters, problem.initial, problem.t_end)


def verify(problem: Problem) -> dict[str, float]:
    """The largest gap between the exact and the numerical answer for every
    compared quantity and energy term, as a share of its scale (a signal's
    largest |value|, the energy scale, or 1 s for an event time)."""
    ex, num = solve(problem), numeric(problem)
    times = [t for t in problem.output_times()
             if t > 0 and all(abs(t - b) > 1e-9 for b in ex.breaks)]
    names = {c.name for c in problem.compare if c.kind != "event"} | set(problem.energy_terms)
    e_scale = max(abs(ex.at(n, problem.t_end)) for n in problem.energy_terms)
    rows = [num.at(t) for t in times]
    gaps = {}
    for name in sorted(names):
        exact_vals = [ex.at(name, t) for t in times]
        scale = e_scale if name.startswith("E_") else max(abs(v) for v in exact_vals) or 1.0
        gaps[name] = max(abs(a - r[name]) for a, r in zip(exact_vals, rows)) / scale
    for c in problem.compared("event"):
        gaps[c.name] = abs(ex.events[c.name] - num.events[c.name]) / max(1.0, ex.events[c.name])
    return gaps


def main() -> int:
    worst_all = 0.0
    print(f"integrator: {'SciPy DOP853' if solve_ivp else 'RK4 (no SciPy)'}, rtol {RTOL:g}")
    for prob in load_all():
        gaps = verify(prob)
        name, worst = max(gaps.items(), key=lambda kv: kv[1])
        worst_all = max(worst_all, worst)
        print(f"{prob.id:26s} worst {worst:.2e} ({name})")
    print(f"largest gap {worst_all:.2e}")
    return 0 if worst_all < 1e-7 else 1


if __name__ == "__main__":
    sys.exit(main())
