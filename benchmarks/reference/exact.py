"""The exact answers of the reference problems (problems/*.toml).

Each problem names a ``model``; :func:`solve` returns that model's exact
solution for the problem's parameters and initial state: every compared
quantity as a function of time (states, flows and the energy terms, which
start at 0 at t = 0) and the event times. Standard library only; the
derivations are in each problem's ``solution`` text.

Conventions: SI units throughout (SOC as a fraction 0-1); a signal's value
at a phase boundary is the right limit (the value just after); ``breaks``
lists the times at which some signal jumps, where comparisons skip the
sample.
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field
from functools import lru_cache
from typing import Callable

from .mathx import (
    ExpPoly,
    Piecewise,
    after,
    bisect,
    first_crossing,
    lambert_w0,
    linear1,
    linear2,
    rational_integral,
)

Signal = Callable[[float], float]


@dataclass
class Exact:
    signals: dict[str, Signal]
    events: dict[str, float] = field(default_factory=dict)
    breaks: list[float] = field(default_factory=list)

    def at(self, name: str, t: float) -> float:
        return self.signals[name](t)


MODELS: dict[str, Callable[[dict, dict], Exact]] = {}


def model(name: str):
    def register(fn):
        MODELS[name] = fn
        return fn
    return register


def solve(problem) -> Exact:
    """The exact solution of a :class:`benchmarks.reference.Problem`."""
    return MODELS[problem.model](problem.parameters, problem.initial)


def _const(c: float) -> Signal:
    return lambda t: c


# ---- electrical ---------------------------------------------------------------------

@model("rc_step")
def rc_step(p: dict, x0: dict) -> Exact:
    """Series R-C charged from a voltage step V at t = 0."""
    V, R, C = p["V"], p["R"], p["C"]
    tau, vc0 = R * C, x0["v_C"]
    v_c = linear1(-1.0 / tau, V / tau, vc0)
    i = (V - v_c) / R
    sig = {
        "v_C": v_c, "i": i,
        "E_source": (i * V).integral(),
        "E_R": (i * i * R).integral(),
        "E_C": v_c * v_c * (C / 2) - C / 2 * vc0 ** 2,
    }
    v_ev = p["event_fraction"] * V
    return Exact(sig, {"t_event": tau * math.log((V - vc0) / (V - v_ev))})


@model("rl_step")
def rl_step(p: dict, x0: dict) -> Exact:
    """Series R-L driven by a voltage step V at t = 0."""
    V, R, L = p["V"], p["R"], p["L"]
    i0, i_inf = x0["i"], V / R
    i = linear1(-R / L, V / L, i0)
    sig = {
        "i": i, "v_L": V - i * R,
        "E_source": (i * V).integral(),
        "E_R": (i * i * R).integral(),
        "E_L": i * i * (L / 2) - L / 2 * i0 ** 2,
    }
    return Exact(sig, {"t_event": L / R * math.log((i_inf - i0) / (i_inf - p["i_event"]))})


# ---- battery --------------------------------------------------------------------------

def _cc_phase(p: dict, soc0: float, v10: float) -> dict[str, ExpPoly]:
    """OCV(SOC) = ocv_a + ocv_b·SOC, R0, R1‖C1 (τ1 = R1·C1) at constant
    current I from SOC0 and v1(0) = v10: every quantity an ExpPoly."""
    current, a, b = p["I"], p["ocv_a"], p["ocv_b"]
    r0, r1, tau = p["R0"], p["R1"], p["tau1"]
    c1 = tau / r1
    k = 1.0 / (3600.0 * p["Q_Ah"])
    soc = ExpPoly.const(soc0) + ExpPoly.t(-current * k)
    v1 = linear1(-1.0 / tau, current * r1 / tau, v10)
    ocv = a + soc * b
    v = ocv - current * r0 - v1
    i = ExpPoly.const(current)
    q = {"SOC": soc, "v1": v1, "V": v, "I": i,
         "E_chem": (ocv * current).integral(),
         "E_terminal": (v * current).integral(),
         "E_R0": ExpPoly.t(current * current * r0),
         "E_R1": (v1 * v1 / r1).integral(),
         "E_C1": v1 * v1 * (c1 / 2) - c1 / 2 * v10 ** 2}
    q["E_internal"] = q["E_R0"] + q["E_R1"] + q["E_C1"]
    return q


@model("battery_cc")
def battery_cc(p: dict, x0: dict) -> Exact:
    q = _cc_phase(p, x0["SOC"], x0["v1"])
    return Exact(dict(q))


@model("battery_cp")
def battery_cp(p: dict, x0: dict) -> Exact:
    """Flat OCV E, R0, R1‖C1, constant terminal power P. Parametrised by the
    current x: t(x), the charge Q(x) and ∫x² dt are integrals of rational
    functions of x with simple poles at 0 and at the roots a < b of
    (R0+R1)x² − E x + P = 0; I(t) is t(x) inverted by bisection."""
    P, E, r0, r1, tau = p["P"], p["E"], p["R0"], p["R1"], p["tau1"]
    c1, rs = tau / r1, r0 + r1
    k = 1.0 / (3600.0 * p["Q_Ah"])
    soc0, v10 = x0["SOC"], x0["v1"]
    u0 = E - v10
    i0 = 2.0 * P / (u0 + math.sqrt(u0 * u0 - 4.0 * r0 * P))  # smaller root of R0 x² − u0 x + P
    disc = E * E - 4.0 * rs * P
    if disc <= 0:
        raise ValueError("battery_cp: P is above the pack's steady maximum power")
    a = 2.0 * P / (E + math.sqrt(disc))  # the steady current (smaller root)
    b = P / (rs * a)
    if not 0 < i0 < a:
        raise ValueError("battery_cp: the current must rise from I(0) towards its steady value")
    # dt/dx = −τ/(R0+R1) · (R0 x² − P) / (x (x − a)(x − b))
    base = [-P, 0.0, r0]  # R0 x² − P
    scale = -tau / rs
    poles = [0.0, a, b]
    t_of = rational_integral([scale * c for c in base], poles)
    q_of = rational_integral([0.0] + [scale * c for c in base], poles)
    i2_of = rational_integral([0.0, 0.0] + [scale * c for c in base], poles)
    top = a * (1.0 - 1e-15)

    @lru_cache(maxsize=None)
    def current(t: float) -> float:
        if t <= 0:
            return i0
        if t_of(i0, top) <= t:
            return top
        return bisect(lambda x: t_of(i0, x) - t, i0, top)

    def v1(t):
        x = current(t)
        return E - r0 * x - P / x

    def charge(t):
        return q_of(i0, current(t))

    def e_r0(t):
        return r0 * i2_of(i0, current(t))

    def e_c1(t):
        return c1 / 2 * (v1(t) ** 2 - v10 ** 2)

    sig = {
        "I": current, "V": lambda t: P / current(t), "v1": v1,
        "SOC": lambda t: soc0 - k * charge(t),
        "E_terminal": lambda t: P * t,
        "E_chem": lambda t: E * charge(t),
        "E_R0": e_r0, "E_C1": e_c1,
        # the RC pair takes I·v1: its resistor the part its capacitor does not store
        "E_R1": lambda t: E * charge(t) - P * t - e_r0(t) - e_c1(t),
        "E_internal": lambda t: E * charge(t) - P * t,
    }
    return Exact(sig)


@model("battery_voltage_limit")
def battery_voltage_limit(p: dict, x0: dict) -> Exact:
    """Constant current until the terminal voltage reaches V_min (t*, by
    Lambert's W), then held at V_min: a linear 2 × 2 system in (SOC, v1)."""
    current, a, b = p["I"], p["ocv_a"], p["ocv_b"]
    r0, r1, tau, v_min = p["R0"], p["R1"], p["tau1"], p["V_min"]
    c1, k = tau / r1, 1.0 / (3600.0 * p["Q_Ah"])
    soc0, v10 = x0["SOC"], x0["v1"]
    ph1 = _cc_phase(p, soc0, v10)
    # V(t) = A − B t + C e^(−t/τ) during phase 1
    A = a + b * soc0 - current * r0 - current * r1
    B = b * current * k
    C = current * r1 - v10
    D = A - v_min
    if ph1["V"](0.0) <= v_min or B <= 0 or C < 0:
        raise ValueError("battery_voltage_limit: needs V(0+) > V_min, a rising OCV and v1(0) ≤ I·R1")
    t_star = D / B + tau * lambert_w0(C / (B * tau) * math.exp(-D / (B * tau)))
    s_star, v1_star = ph1["SOC"](t_star), ph1["v1"](t_star)
    # phase 2: I = (OCV(SOC) − v1 − V_min)/R0, SOC' = −k I, v1' = I/C1 − v1/τ
    c0 = a - v_min
    m = [[-k * b / r0, k / r0], [b / (r0 * c1), -1.0 / (r0 * c1) - 1.0 / tau]]
    f = [-k * c0 / r0, c0 / (r0 * c1)]
    soc2, v12 = linear2(m, f, [s_star, v1_star])
    i2 = (c0 + soc2 * b - v12) / r0
    ocv2 = a + soc2 * b
    ph2 = {"SOC": soc2, "v1": v12, "I": i2, "V": ExpPoly.const(v_min),
           "E_chem": (ocv2 * i2).integral() + ph1["E_chem"](t_star),
           "E_terminal": (i2 * v_min).integral() + ph1["E_terminal"](t_star),
           "E_R0": (i2 * i2 * r0).integral() + ph1["E_R0"](t_star),
           "E_R1": (v12 * v12 / r1).integral() + ph1["E_R1"](t_star),
           "E_C1": v12 * v12 * (c1 / 2) - c1 / 2 * v10 ** 2}
    ph2["E_internal"] = ph2["E_R0"] + ph2["E_R1"] + ph2["E_C1"]
    sig = {name: Piecewise([(0.0, ph1[name]), (t_star, after(t_star, ph2[name]))])
           for name in ph2}
    return Exact(sig, {"t_vmin": t_star})


# ---- motors and rotating parts -------------------------------------------------------

@model("dc_motor")
def dc_motor(p: dict, x0: dict) -> Exact:
    """Permanent-magnet DC motor on a voltage step: L i' = V − R i − k ω,
    J ω' = k i − b ω − T_L. Two states with L > 0; with L = 0 the current
    follows the speed, i = (V − k ω)/R, and one state is left."""
    V, R, L, k, J, b, tl = p["V"], p["R"], p["L"], p["k"], p["J"], p["b"], p["T_load"]
    i0, w0 = x0["i"], x0["omega"]
    if L > 0:
        i, w = linear2([[-R / L, -k / L], [k / J, -b / J]], [V / L, -tl / J], [i0, w0])
    else:
        w = linear1(-(k * k / R + b) / J, (k * V / R - tl) / J, w0)
        i = (V - w * k) / R
    sig = {
        "i": i, "omega": w, "T_e": i * k,
        "E_in": (i * V).integral(),
        "E_R": (i * i * R).integral(),
        "E_L": i * i * (L / 2) - L / 2 * i0 ** 2,
        "E_kin": w * w * (J / 2) - J / 2 * w0 ** 2,
        "E_friction": (w * w * b).integral(),
        "E_load": (w * tl).integral(),
    }
    w_inf = (k * V - R * tl) / (k * k + R * b)
    t_ev = first_crossing(w, p["event_fraction"] * w_inf, 0.0, p["t_search"])
    return Exact(sig, {"t_event": t_ev})


@model("inertia_coastdown")
def inertia_coastdown(p: dict, x0: dict) -> Exact:
    """J ω' = −c ω − T_c (ω > 0) until ω = 0 at t_s = (J/c) ln(1 + c ω0 / T_c);
    then at rest (the friction holds it: nothing else acts)."""
    J, c, tc = p["J"], p["c"], p["T_c"]
    w0 = x0["omega"]
    w = linear1(-c / J, -tc / J, w0)
    theta = w.integral()
    t_s = J / c * math.log(1.0 + c * w0 / tc)
    e_visc = (w * w * c).integral()
    th_s, ev_s = theta(t_s), e_visc(t_s)
    ke0 = 0.5 * J * w0 * w0
    sig = {
        "omega": Piecewise([(0.0, w), (t_s, _const(0.0))]),
        "theta": Piecewise([(0.0, theta), (t_s, _const(th_s))]),
        "E_viscous": Piecewise([(0.0, e_visc), (t_s, _const(ev_s))]),
        "E_coulomb": Piecewise([(0.0, theta * tc), (t_s, _const(tc * th_s))]),
        "E_kin": Piecewise([(0.0, w * w * (J / 2) - ke0), (t_s, _const(-ke0))]),
    }
    return Exact(sig, {"t_stop": t_s})


@model("clutch_lockup")
def clutch_lockup(p: dict, x0: dict) -> Exact:
    """Two inertias joined by a dry clutch of constant friction torque T_c
    (kinetic = static) from t = 0: slip at constant decelerations until the
    speeds meet at t_lock = Δω0 / rate, then locked."""
    J1, J2, tc, t1, t2 = p["J1"], p["J2"], p["T_c"], p["T_drive"], p["T_load"]
    w10, w20 = x0["omega1"], x0["omega2"]
    dw0 = w10 - w20
    if dw0 <= 0:
        raise ValueError("clutch_lockup: the driving side must start faster")
    rate = (tc - t1) / J1 + (tc - t2) / J2
    t_lock = dw0 / rate
    w1 = ExpPoly.const(w10) + ExpPoly.t((t1 - tc) / J1)
    w2 = ExpPoly.const(w20) + ExpPoly.t((tc - t2) / J2)
    w_lock = w1(t_lock)
    alpha = (t1 - t2) / (J1 + J2)
    t_tr = (J2 * t1 + J1 * t2) / (J1 + J2)  # what the locked clutch passes
    if abs(t_tr) > tc:
        raise ValueError("clutch_lockup: the clutch would slip again after locking")
    wl = ExpPoly.const(w_lock) + ExpPoly.t(alpha)
    th1, th2, thl = w1.integral(), w2.integral(), wl.integral()
    th1_l, th2_l = th1(t_lock), th2(t_lock)
    slip = w1 - w2
    e_cl = (slip * tc).integral()
    e_cl_l = e_cl(t_lock)
    ke = lambda a, b: 0.5 * J1 * a * a + 0.5 * J2 * b * b  # noqa: E731
    ke0 = ke(w10, w20)

    def two(f_slip, f_locked):
        return Piecewise([(0.0, f_slip), (t_lock, f_locked)])

    sig = {
        "omega1": two(w1, after(t_lock, wl)),
        "omega2": two(w2, after(t_lock, wl)),
        "slip": two(slip, _const(0.0)),
        "T_clutch": two(_const(tc), _const(t_tr)),
        "E_drive": two(th1 * t1, lambda t: t1 * (th1_l + thl(t - t_lock))),
        "E_load": two(th2 * t2, lambda t: t2 * (th2_l + thl(t - t_lock))),
        "E_clutch": two(e_cl, _const(e_cl_l)),
        "E_kin": lambda t: ke(sig["omega1"](t), sig["omega2"](t)) - ke0,
    }
    eps = p["eps_lock"]
    return Exact(sig, {"t_lock": (dw0 - eps) / rate}, breaks=[t_lock])


@model("gear_change")
def gear_change(p: dict, x0: dict) -> Exact:
    """Motor (J1, constant torque T) → ideal gear (ratio i = ω1/ω2) → load
    (J2). At t_s the ratio changes from i1 to i2 as a rigid, instantaneous
    engagement: the gear's impulse keeps J2 ω2 + i2 J1 ω1, so
    ω2⁺ = (J2 + i1 i2 J1) ω2⁻ / (J2 + i2² J1), and kinetic energy is lost."""
    J1, J2, T, i1, i2, ts = p["J1"], p["J2"], p["T"], p["i1"], p["i2"], p["t_shift"]
    w20 = x0["omega2"]
    je1, je2 = J2 + J1 * i1 * i1, J2 + J1 * i2 * i2
    w2a = ExpPoly.const(w20) + ExpPoly.t(T * i1 / je1)
    w2_minus = w2a(ts)
    w2_plus = (J2 + i1 * i2 * J1) * w2_minus / je2
    w2b = ExpPoly.const(w2_plus) + ExpPoly.t(T * i2 / je2)  # in τ = t − t_s
    th1a, th1b = (w2a * i1).integral(), (w2b * i2).integral()
    th1_s = th1a(ts)
    ke = lambda w2, i: 0.5 * (J2 + J1 * i * i) * w2 * w2  # noqa: E731
    ke0 = ke(w20, i1)
    loss = ke(w2_minus, i1) - ke(w2_plus, i2)
    sig = {
        "omega2": Piecewise([(0.0, w2a), (ts, after(ts, w2b))]),
        "omega1": Piecewise([(0.0, w2a * i1), (ts, after(ts, w2b * i2))]),
        "theta1": Piecewise([(0.0, th1a), (ts, lambda t: th1_s + th1b(t - ts))]),
        "E_shift": Piecewise([(0.0, _const(0.0)), (ts, _const(loss))]),
    }
    sig["E_drive"] = lambda t: T * sig["theta1"](t)
    sig["E_kin"] = Piecewise([(0.0, lambda t: ke(w2a(t), i1) - ke0),
                              (ts, lambda t: ke(w2b(t - ts), i2) - ke0)])
    return Exact(sig, breaks=[ts])


# ---- vehicle ------------------------------------------------------------------------------

@model("vehicle_coastdown")
def vehicle_coastdown(p: dict, x0: dict) -> Exact:
    """m v' = −(A + C v²) for v > 0: v = s tan(φ0 − κ t), s = √(A/C),
    φ0 = atan(v0/s), κ = √(AC)/m, until it stops at t_s = φ0/κ;
    x = (m/C) ln(cos(φ0 − κ t)/cos φ0). Then at rest."""
    m, A, C = p["m"], p["A"], p["C"]
    v0 = x0["v"]
    s, kap = math.sqrt(A / C), math.sqrt(A * C) / m
    phi0 = math.atan(v0 / s)
    t_s = phi0 / kap
    x_s = m / (2 * C) * math.log1p(C * v0 * v0 / A)

    def v(t):
        return s * math.tan(phi0 - kap * t) if t < t_s else 0.0

    def x(t):
        return m / C * math.log(math.cos(phi0 - kap * t) / math.cos(phi0)) if t < t_s else x_s

    ke0 = 0.5 * m * v0 * v0
    sig = {
        "v": v, "x": x,
        "E_roll": lambda t: A * x(t),
        # m v v' = −A v − C v³: what the air took is the rest of the kinetic energy
        "E_aero": lambda t: ke0 - 0.5 * m * v(t) ** 2 - A * x(t),
        "E_kin": lambda t: 0.5 * m * v(t) ** 2 - ke0,
    }
    v_half = p["v_event"]
    return Exact(sig, {"t_stop": t_s, "t_event": (phi0 - math.atan(v_half / s)) / kap})


@model("vehicle_constant_power")
def vehicle_constant_power(p: dict, x0: dict) -> Exact:
    """m v v' = P: v = √(v0² + 2 P t / m), x = m/(3P) ((v0² + 2Pt/m)^(3/2) − v0³)."""
    m, P = p["m"], p["P"]
    v0 = x0["v"]

    def w(t):
        return v0 * v0 + 2.0 * P * t / m

    sig = {
        "v": lambda t: math.sqrt(w(t)),
        "x": lambda t: m / (3.0 * P) * (w(t) ** 1.5 - v0 ** 3),
        "E_supplied": lambda t: P * t,
        "E_kin": lambda t: 0.5 * m * (w(t) - v0 * v0),
    }
    v1 = p["v_event"]
    return Exact(sig, {"t_event": m * (v1 * v1 - v0 * v0) / (2.0 * P)})


# ---- thermal ---------------------------------------------------------------------------------

@model("thermal_lumped")
def thermal_lumped(p: dict, x0: dict) -> Exact:
    """C T' = P(t) − G (T − T_amb), P = P_heat until t_off, then 0."""
    C, G, P, t_off, ta = p["C"], p["G"], p["P"], p["t_off"], p["T_amb"]
    T0 = x0["T"]
    tau = C / G
    heat = linear1(-G / C, (P + G * ta) / C, T0)
    T_off = heat(t_off)
    cool = linear1(-G / C, G * ta / C, T_off)
    e_amb1 = ((heat - ta) * G).integral()
    e_amb2 = ((cool - ta) * G).integral()
    e_amb_off = e_amb1(t_off)
    T = Piecewise([(0.0, heat), (t_off, after(t_off, cool))])
    sig = {
        "T": T,
        "E_heat": lambda t: P * min(t, t_off),
        "E_ambient": Piecewise([(0.0, e_amb1), (t_off, lambda t: e_amb_off + e_amb2(t - t_off))]),
        "E_stored": lambda t: C * (T(t) - T0),
    }
    t_inf = ta + P / G
    t_hot, t_cool = p["T_hot"], p["T_cool"]
    if not T0 < t_hot < T_off or not ta < t_cool < T_off:
        raise ValueError("thermal_lumped: the event temperatures must be crossed")
    events = {"t_hot": tau * math.log((t_inf - T0) / (t_inf - t_hot)),
              "t_cool": t_off + tau * math.log((T_off - ta) / (t_cool - ta))}
    return Exact(sig, events)


@model("thermal_two_masses")
def thermal_two_masses(p: dict, x0: dict) -> Exact:
    """C1 T1' = P − G12 (T1 − T2); C2 T2' = G12 (T1 − T2) − G2a (T2 − T_amb)."""
    C1, C2, G12, G2a, P, ta = p["C1"], p["C2"], p["G12"], p["G2a"], p["P"], p["T_amb"]
    T10, T20 = x0["T1"], x0["T2"]
    m = [[-G12 / C1, G12 / C1], [G12 / C2, -(G12 + G2a) / C2]]
    T1, T2 = linear2(m, [P / C1, G2a * ta / C2], [T10, T20])
    sig = {
        "T1": T1, "T2": T2,
        "E_heat": ExpPoly.t(P),
        "E_ambient": ((T2 - ta) * G2a).integral(),
        "E_stored1": (T1 - T10) * C1,
        "E_stored2": (T2 - T20) * C2,
        "E_12": ((T1 - T2) * G12).integral(),
    }
    return Exact(sig, {"t_event": first_crossing(T1, p["T1_event"], 0.0, p["t_search"])})
