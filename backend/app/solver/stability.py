"""Pre-run stability check of the solver step (ENG-14).

Parts of the solver step their forces explicitly: the vehicle takes the
tyres' force from the start of the step, a slipping clutch at its limit and
a propeller-type load pass their whole torque for the step. Each has a rate
r (1/s) at which that force pulls its speed back; a step h is stable only
while the step's gain h·r stays small (forward Euler on such a decay fails
past h·r = 2, and the coupled driveline earlier). Too stiff a tyre, too
light an inertia or too strong a clutch for the step, and the result is
numerical garbage without any message.

Before a run, solver_step() picks the largest step, at most MAX_SUBSTEP,
that keeps every gain within its limit, and says which part asked for it.
Data Checks report the same, so the user knows before running.

The tyre's gain is r = g·Σ(load share·slip stiffness)/V_EPS, the vehicle's
speed response to its tyres' slip at low speed (|v| ≤ V_EPS). The stability
grid of tests/test_numerics.py (a launch to 100 km/h and a car held braked,
at steps of 2.5 to 20 ms and slip stiffnesses of 10 to 300) is stable up to
a gain of 2.94 (stiffness 30 at 5 ms) and unstable from 3.92 (stiffness 10
at 20 ms) on; TYRE_GAIN_MAX sits between them. The default stiffness of 10
at 10 ms is 1.96.
"""
from __future__ import annotations

import math
from dataclasses import dataclass

from .network import Model
from .runtime import CLUTCH_BAND, GRAVITY, MAX_SUBSTEP, RPM, V_EPS

TYRE_GAIN_MAX = 3.0
# a clutch that passes its whole torque for a step changes its slip by
# gain × CLUTCH_BAND: above 1 it overshoots lock-up (the ring KNOWN-LIMITS
# describes); that is warned about, not fixed by a smaller step, as the
# clutch locks within a few steps and its energy is conserved
CLUTCH_GAIN_WARN = 20.0
PROP_GAIN_MAX = 0.5  # an explicit quadratic load: well inside the limit of 2
MIN_SUBSTEP = 0.0005  # s: below this the run would take too long; warn instead


@dataclass
class Gain:
    rate: float  # 1/s: the step's gain is h · rate
    limit: float  # the largest gain the step may have
    el_ids: tuple[str, ...]
    what: str  # e.g. "the tyres' Slip Stiffness (Wheel 'FL' 300)"
    fix: bool = True  # False: only warned about, the step is not reduced for it


def gains(model: Model) -> list[Gain]:
    """The explicit gains of a built model (its params_of hold any case
    values), per unit of step."""
    out: list[Gain] = []
    p_of, label = model.params_of, (lambda el: model.elements[el].label)
    wheels = [w for dl in model.drivelines for seg in dl.segments for w in seg.wheels]
    total_share = sum(w.load_share for w in wheels)
    if model.vehicle and wheels and total_share > 0:
        rate = GRAVITY * sum(w.load_share * w.c_slip for w in wheels) / total_share / V_EPS
        stiff = max(wheels, key=lambda w: w.c_slip)
        out.append(Gain(rate, TYRE_GAIN_MAX, tuple(w.el_id for w in wheels),
                        f"the tyres' Slip Stiffness (Wheel '{label(stiff.el_id)}': "
                        f"{stiff.c_slip:g})"))
    for dl in model.drivelines:
        for j in dl.joints:
            if j.kind != "clutch" or j.child_a < 0 or j.child_b < 0:
                continue
            cap = max(0.0, float(p_of[j.el_id].get("max_torque_Nm", 0) or 0))
            j_a = dl.segments[j.child_a].inertia / max(1e-12, j.child_a_m ** 2)
            j_b = dl.segments[j.child_b].inertia / max(1e-12, j.child_b_m ** 2)
            if cap <= 0 or min(j_a, j_b) <= 0:
                continue
            rate = cap * (1.0 / j_a + 1.0 / j_b) / CLUTCH_BAND
            out.append(Gain(rate, CLUTCH_GAIN_WARN, (j.el_id,),
                            f"Clutch '{label(j.el_id)}' ({cap:g} N·m on "
                            f"{min(j_a, j_b):.3g} kg·m²)", fix=False))
        for seg in dl.segments:
            for pr in seg.props:
                omega_ref = pr.n_ref / RPM
                j_p = seg.inertia / max(1e-12, pr.m ** 2)
                if pr.t_ref <= 0 or omega_ref <= 0 or j_p <= 0:
                    continue
                # dT/dω of T = t_ref·(ω/ω_ref)², at the reference speed
                rate = 2.0 * pr.t_ref / omega_ref / j_p
                out.append(Gain(rate, PROP_GAIN_MAX, (pr.el_id,),
                                f"'{label(pr.el_id)}' ({pr.t_ref:g} N·m at "
                                f"{pr.n_ref:g} 1/min on {j_p:.3g} kg·m²)"))
    return out


@dataclass
class StepChoice:
    step: float  # the solver step to use, s (≤ MAX_SUBSTEP)
    reason: str  # what asked for a smaller step ("" when none did)
    el_ids: tuple[str, ...]
    warnings: tuple[tuple[str, tuple[str, ...]], ...]  # (text, parts)


def solver_step(model: Model, cap: float = MAX_SUBSTEP) -> StepChoice:
    """The largest solver step, at most ``cap`` (MAX_SUBSTEP) and at least
    MIN_SUBSTEP, that keeps every gain to fix within its limit, with what
    asked for it and the warnings about the gains it cannot fix."""
    step, reason, ids = cap, "", ()
    warnings: list[tuple[str, tuple[str, ...]]] = []
    found = gains(model)
    for g in found:
        if not g.fix:
            continue
        need = g.limit / g.rate
        if need < step:
            step, reason, ids = need, g.what, g.el_ids
    if step < MIN_SUBSTEP:
        warnings.append((f"{reason} is too stiff for the solver even at its smallest step of "
                         f"{MIN_SUBSTEP * 1000:g} ms (it would need {step * 1000:.2g} ms): "
                         f"results may oscillate or be wrong.", ids))
        step = MIN_SUBSTEP
    elif step < cap:
        # a whole fraction of the cap, so the times stay readable (5, 3.33,
        # 2.5 … ms of 10 ms)
        step = cap / math.ceil(cap / step - 1e-9)
    for g in found:
        if not g.fix and g.rate * step > g.limit:
            warnings.append((f"{g.what} can ring as it closes at the {step * 1000:.3g} ms solver "
                             f"step: in one step it changes its slip by "
                             f"{g.rate * step * CLUTCH_BAND:.3g} rad/s, more than its "
                             f"{CLUTCH_BAND:g} rad/s band, so the shafts on either side can "
                             f"swing for a few steps before it locks.", g.el_ids))
    return StepChoice(step, reason, ids, tuple(warnings))
