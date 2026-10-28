"""The holds suite: every probe of a condition on an iteration variable that
the solver work's reviews used (a `when x > 0.5` with x solved from
x^3 + x = R, and the events that move R), in one set, scored against the
counts and times a hand calculation gives, never against a commit's output.

    judge.py holds cases  OUT [--tier quick|full]     the models and runs (OUT/cases.json)
    judge.py holds build  CHECKOUT RUNNER             the runner, built in that checkout
    judge.py holds run    RUNNER CASES RESULTS.jsonl [--threads N]
    judge.py holds score  CASES LABEL=RESULTS.jsonl ... [--out FILE.md]
    judge.py holds selftest

`build` copies `holds_runner.rs` into the checkout's lsim-project examples,
builds it (`cargo build --release --locked`, CARGO_TARGET_DIR as set) and
takes it out again: the same runner at every commit measured. README.md
(*The holds suite*) has the shapes, their truth and the named limits.

Every x crosses 0.5 where R crosses 0.625 (x^3 + x is increasing and
0.5^3 + 0.5 = 0.625), with x' = R' / (3 x^2 + 1) = R' / 1.75 there. On
CVODE and the stateless backend x is written explicitly, x = 0.5 +
(R - 0.625) / 1.75: the same crossings and slopes, no iteration variable
(the controls: nothing is held there).
"""
from __future__ import annotations

import collections
import json
import math
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
RTOLS = [1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10]
QUICK_RTOLS = [1e-5, 1e-8, 1e-10]
T_END = 2.0
EXACT = 1e-12      # a time event's or a tick's time (scheduled exactly)
LOC = 10.0         # a located crossing: within LOC rtol in x, over |x'| in time
STATE_EV = 10.0    # a state's own crossing (s = t passing t2): LOC rtol in s
# the solve's resolution in x: a third of the tolerance on x (rtol |x| +
# atol, x = 0.5, atol = 1e-2 rtol): the integrators' Newton iterations stop
# at a third of it (IDA's and CVODE's convergence constant 0.33), and
# diffsol's x lags its equation by a tenth to a quarter of it
RHO = 1.0 / 3.0

# name: (x implicit, r a state z, prep force_implicit, backend)
CONFIGS = {
    "cvode": (False, True, False, "sundials"),
    "ida": (True, True, False, "sundials"),
    "ida-implicit": (True, True, True, "sundials"),
    "diffsol": (True, True, False, "diffsol"),
    "nostate-ida": (True, False, False, "sundials"),
    "nostate-explicit": (False, False, False, "sundials"),
}
STATE_CONFIGS = ["cvode", "ida", "ida-implicit", "diffsol"]


def loc(rtol: float, slope: float) -> float:
    return LOC * rtol / abs(slope) + 1e-12


def resolution(rtol: float) -> float:
    return RHO * (0.5 * rtol + 1e-2 * rtol)


def ev_tol(kind: str, rtol: float) -> float:
    return STATE_EV * rtol + 1e-12 if kind == "state" else EXACT


# ---------------------------------------------------------------------------
# The text of a model
# ---------------------------------------------------------------------------

def mo(params: dict, decls: list[str], eqs: list[str], whens: list) -> str:
    """`model Holds.M`: params {name: (value, unit)}, declarations, equations
    and whens [(condition, [action], label)]."""
    out = ['model Holds.M "a holds suite shape"']
    for p, (v, unit) in params.items():
        u = f'(unit = "{unit}")' if unit else ""
        out.append(f"  parameter Real {p}{u} = {v!r};")
    out += [f"  {d};" for d in decls]
    out.append("equation")
    out += [f"  {e};" for e in eqs]
    for cond, acts, label in whens:
        out.append(f"  when {cond} then")
        out += [f"    {a};" for a in acts]
        out.append(f'  end when "{label}";')
    out.append("end Holds.M;")
    return "\n".join(out) + "\n"


def x_eq(implicit: bool, rr: str, var: str = "x", at: float = 0.5, d: float = 1.75) -> str:
    return f"{var} ^ 3 + {var} = {rr}" if implicit else f"{var} = {at} + (({rr}) - {at ** 3 + at!r}) / {d}"


def count(v: str) -> str:
    return f"{v} = pre({v}) + 1"


DISC = "discrete Real {}(start = 0, fixed = true)"


def ops(up: bool, strict: bool) -> tuple[str, str]:
    """The crossing's own direction's relation and the opposite one."""
    return {(True, True): (">", "<"), (True, False): (">=", "<="),
            (False, True): ("<", ">"), (False, False): ("<=", ">=")}[(up, strict)]


def build(fam, shape, config, text, runs, labels, record, grid=(), tick_vars=(), t_end=T_END):
    implicit, z_state, force, backend = CONFIGS.get(config, (True, True, False, "sundials"))
    for r in runs:
        r.setdefault("backend", backend)
    sid = "-".join(f"{v}" for v in shape.values())
    return {"id": f"{fam}/{sid}/{config}", "family": fam, "shape": shape, "config": config,
            "text": text, "model": "Holds.M", "force_implicit": force, "t_end": t_end,
            "record": list(record), "grid": list(grid), "tick_vars": list(tick_vars),
            "labels": list(labels), "runs": runs}


def key(tag: dict) -> str:
    return "|".join(f"{k}={tag[k]}" for k in sorted(tag))


def run(params, rtol, truth, tag, **kw):
    tag = dict(tag, rtol=rtol)
    return dict(kw, params=params, rtol=rtol, truth=truth, tag=tag, key=key(tag))


def truth(req, final, **kw):
    return dict(kw, req=req, final=final)


def noise_alt(keep: dict, free, twin_tag):
    """The condition left within the solve's resolution of its threshold:
    the firings `keep` still required, any other firing of the counted
    `when`s inside `free`, the counters as fired, and the twin (the same run
    with the move made resolvable: e = 1.75 rtol, x put back by rtol) right
    or L1."""
    fin = {v: ["count", lab] for v, lab in keep.pop("_counters").items()}
    return {"cls": "noise", "req": keep, "free": [free], "final": fin,
            "twin": key(twin_tag), "twin_ok": ["right", "L1"]}


# ---------------------------------------------------------------------------
# The families
# ---------------------------------------------------------------------------

def fam(tier: str) -> list:
    """Fix round 3's family probe: x rises (or falls) through 0.5 at
    t1 = 0.625 / a, r = a t or a state z (z' = a); at t2 = t1 + d an event
    sets k (kind: a time event, a state s = t passing t2, a sampled block's
    tick inside a step), which
      back:    R = 0.625 + s (dd - k (dd + e) + k2 (0.1 + e)), dd = r - 0.625:
               x back across by e/1.75 (R = 0.625 - s e), until k2 at
               tb = t1 + 0.5 lifts it 0.1/1.75 onto the far side again;
               forth at [t1, tb], back at [t2];
      rerise:  R = 0.625 + s (dd - k j), j = a d + e: R(t2) = 0.625 - s e,
               rising again at a: across at t3 = t2 + e / a; forth at
               [t1, t3], back at [t2];
      further: R = 0.625 + s (dd + 0.1 k): forth at [t1] only;
      nothing: R = 0.625 + s dd, k read by another iteration variable q
               (q^3 + q = 1 + k): forth at [t1] only;
      backfwd: as back, but k3 (0.1 + e) set at t2b = t2 + 1e-6 by the same
               kind: forth at [t1, t2b], back at [t2].
    Conditions: forth alone (strict, or not), with back (strict, or not),
    or strict both with a mode y = if forth then 1 else 0 (and w' = y where
    the model has a state: w(2) is the time on the forth side), or the mode
    and w alone. e = 1e-12 (x back by 5.7e-13) or 1.75 rtol (by rtol)."""
    out = []
    quick = tier == "quick"
    effects = ["back", "rerise", "backfwd"] if quick else ["back", "rerise", "further", "nothing", "backfwd"]
    conds = ["pairs", "mode"] if quick else ["lones", "lonen", "pairs", "pairn", "mode", "modelone"]
    rates = [2.3] if quick else [0.7, 1.0, 2.3, 4.4]
    deltas = [1e-6, 1e-4] if quick else [1e-6, 1e-4, 1e-2]
    rtols = QUICK_RTOLS if quick else RTOLS
    for up in (True, False):
        for kind in ("time", "state", "tick"):
            for effect in effects:
                for cond in conds:
                    for config in CONFIGS:
                        out.append(fam_build(up, kind, effect, cond, config, rates, deltas, rtols))
    return out


def fam_build(up, kind, effect, cond, config, rates, deltas, rtols):
    implicit, z_state, _, _ = CONFIGS[config]
    has_state = z_state or kind == "state"
    sg = "+" if up else "-"
    r = "z" if z_state else "a * time"
    dd = f"({r} - 0.625)"
    phi = {"back": f"- k * ({dd} + e) + k2 * (0.1 + e)", "rerise": "- k * j", "further": "0.1 * k",
           "nothing": "0", "backfwd": f"- k * ({dd} + e) + k3 * (0.1 + e)"}[effect]
    rr = f"0.625 {sg} ({dd} {phi if phi.startswith('-') else '+ ' + phi})"
    params = {"a": (1.0, "1/s"), "t2": (1.0, "s"), "t2b": (1.0, "s"), "tb": (1.0, "s"),
              "e": (1e-12, ""), "j": (1e-6, ""), "never": (1e3, "s")}
    decls = [f"Real x(start = {0.0 if up else 0.78})"] + [DISC.format(v) for v in ("n", "m", "k", "k2", "k3")]
    eqs = []
    if z_state:
        decls.append("Real z(start = 0, fixed = true)")
        eqs.append("der(z) = a")
    if kind == "state":
        decls.append('Real s(unit = "s", start = 0, fixed = true)')
        eqs.append("der(s) = 1")
    eqs.append(x_eq(implicit, rr))
    if effect == "nothing":
        decls.append("Real q(start = 0.68)" if implicit else "Real q")
        eqs.append("q ^ 3 + q = 1 + k" if implicit else "q = 1 + k")
    at = {"time": "time >= {}", "state": "s > {}", "tick": "time >= never"}[kind]
    whens = [(at.format("t2"), ["k = 1"], "k set")]
    if effect == "backfwd":
        whens.append((at.format("t2b"), ["k3 = 1"], "k3 set"))
    if effect == "back":
        whens.append(("time >= tb", ["k2 = 1"], "k2 set"))
    strict = cond not in ("lonen", "pairn")
    fop, bop = ops(up, strict)
    pair = cond in ("pairs", "pairn", "mode")
    mode = cond in ("mode", "modelone")
    if cond != "modelone":
        whens.append((f"x {fop} 0.5", [count("n")], "forth"))
        labels = ["forth"]
    else:
        whens.append(("time >= never", [count("n")], "never"))
        labels = ["never"]
    if pair:
        whens.append((f"x {bop} 0.5", [count("m")], "back"))
        labels.append("back")
    with_w = mode and has_state
    if mode:
        decls.append("Real y")
        eqs.append(f"y = if x {fop} 0.5 then 1 else 0")
        if with_w:
            decls.append('Real w(unit = "s", start = 0, fixed = true)')
            eqs.append("der(w) = y")
    text = mo(params, decls, eqs, whens)
    tick_vars = ["k"] + (["k3"] if effect == "backfwd" else []) if kind == "tick" else []
    runs = []
    for a in rates:
        for d in deltas:
            for ek in ("e12", "erel"):
                for rtol in rtols:
                    e = 1e-12 if ek == "e12" else 1.75 * rtol
                    t1 = 0.625 / a
                    t2, tb = t1 + d, t1 + 0.5
                    t2b = t2 + 1e-6
                    p = {"a": a, "t2": t2, "t2b": t2b, "tb": tb, "e": e, "j": a * d + e}
                    ticks = ([t2] + ([t2b] if effect == "backfwd" else [])) if kind == "tick" else []
                    tag = {"a": a, "d": d, "e": ek}
                    tr = fam_truth(effect, kind, cond, pair, mode, with_w, a, d, e, ek, rtol, t1, t2, t2b, tb, tag)
                    runs.append(run(p, rtol, tr, tag, ticks=ticks))
    record = ["n", "m"] + (["w"] if with_w else [])
    shape = {"dir": "up" if up else "down", "kind": kind, "effect": effect, "cond": cond}
    return build("fam", shape, config, text, runs, labels, record, tick_vars=tick_vars)


def fam_truth(effect, kind, cond, pair, mode, with_w, a, d, e, ek, rtol, t1, t2, t2b, tb, tag):
    L, ev = loc(rtol, a / 1.75), ev_tol(kind, rtol)
    if effect == "back":
        f, b = [[t1, L], [tb, EXACT]], [[t2, ev]]
    elif effect == "rerise":
        f, b = [[t1, L], [t2 + e / a, L + ev]], [[t2, ev]]
    elif effect == "backfwd":
        f, b = [[t1, L], [t2b, ev]], [[t2, ev]]
    else:
        f, b = [[t1, L]], []
    if cond == "modelone":
        req, fin = {"never": []}, {"n": [0, 0]}
    else:
        req, fin = {"forth": f}, {"n": [len(f), 0]}
    if pair:
        req["back"] = b
        fin["m"] = [len(b), 0]
    wt = 4 * L + 20 * rtol + 1e-9

    def forth_time(l1):
        if not b:
            return T_END - f[0][0]
        return (0.0 if l1 else b[0][0] - f[0][0]) + T_END - f[1][0]

    if with_w:
        fin["w"] = [forth_time(False), wt]
    limits = []
    counters = {"n": "forth"} | ({"m": "back"} if pair else {})
    if b and d < L:
        # L1: the crossing located after the event that undoes it
        lreq = {lab: v[1:] for lab, v in req.items()}
        lfin = {k: [v[0] - 1, 0] for k, v in fin.items() if k in ("n", "m")}
        if cond == "modelone":
            lreq, lfin = req, {"n": [0, 0]}
        if with_w:
            lfin["w"] = [forth_time(True), wt]
        limits.append({"cls": "L1", "req": lreq, "final": lfin})
    if b and ek == "e12" and e / 1.75 <= resolution(rtol) and cond != "modelone":
        leave = f[1]
        free = [t2 - ev, leave[0] + leave[1]]
        twin = dict(tag, e="erel", rtol=rtol)
        keep = {"forth": [f[0]], "_counters": dict(counters)}
        if pair:
            keep["back"] = []
        limits.append(noise_alt(keep, free, twin))
        if d < L:
            keep = {"forth": [], "_counters": dict(counters)}
            if pair:
                keep["back"] = []
            limits.append(noise_alt(keep, [t1 - L, free[1]], twin))
    storm = [[t, L + ev] for t in (t1, t2, t2b, tb, t2 + e / a)] if mode else []
    return truth(req, fin, limits=limits, storm=storm)


def own(tier: str) -> list:
    """Recheck 3's own-move probe: an event at the very crossing that makes
    the hold. x crosses 0.5 at t1 = 0.625 / a; E = 1.75 ex;
      own:     the crossing's own `when` (forth, counts n) sets k:
               R = 0.625 + s (dd - k (dd + E) + k2 (0.1 + E)): x from 0.5
               to 0.5 - s ex (to first order) at that instant;
      sep:     the same, k set by another `when` on the same condition;
      thr:     the threshold moves instead: 0.5 + s k ex, R = 0.625 + s (dd -
               k dd + k2 0.1) (x stays at 0.5: ex on the back side of it);
      reinit:  forth reinits z to 0.625 - E and stops it (z' = a (1 - k)),
               R = 0.625 + s (dd + k2 (0.1 + E)) (states only);
      ownmode: own, beside a mode y = if back then 1 else 0.
    Each: back (counts m) fires at that same instant, t1 (event iteration:
    x is back across after k), and forth again at tb = t1 + 0.5 where k2
    lifts R by 0.1 + E: n = 2 at [t1, tb], m = 1 at forth's first instant.
    Not strict (>=, <=): x is exactly 0.5 at t1, so the back relation is
    true there already and may never turn: m = 1 at t1, or m = 0 (both
    right). ex: 5.7e-13 (noise), 0.5, 1, 1.5 and 100 rtol (beyond every
    hold). The big moves, kick (R = r - 0.2 k, the own `when` sets k: forth
    at [0.625, 0.825], back at forth's first instant), steps (R = r - 0.6 n:
    forth at 0.625, 1.225, 1.825) and saw (forth reinits z to 0: at 0.625,
    1.25, 1.875), with r = t."""
    out = []
    quick = tier == "quick"
    rates = [1.0, 4.4] if quick else [0.7, 1.0, 2.3, 4.4]
    exks = ["0.5", "1.5", "100"] if quick else ["noise", "0.5", "1", "1.5", "100"]
    rtols = QUICK_RTOLS if quick else RTOLS
    kinds = ["own", "sep", "thr", "reinit"] + ([] if quick else ["ownmode"])
    for kind in kinds:
        for up in ((True,) if quick else (True, False)):
            for strict in ((True,) if quick else (True, False)):
                for config in CONFIGS:
                    if kind == "reinit" and config not in STATE_CONFIGS:
                        continue
                    out.append(own_build(kind, up, strict, config, rates, exks, rtols))
    for kind in ("kick", "steps", "saw"):
        for config in (STATE_CONFIGS if kind == "saw" else CONFIGS):
            out.append(own_big(kind, config, rtols))
    return out


def own_build(kind, up, strict, config, rates, exks, rtols):
    implicit, z_state, _, _ = CONFIGS[config]
    sg = "+" if up else "-"
    r = "z" if z_state else "a * time"
    dd = f"({r} - 0.625)"
    E = "1.75 * ex"
    phi = {"thr": "- k * {dd} + k2 * 0.1".format(dd=dd), "reinit": f"+ k2 * (0.1 + {E})"}.get(
        kind, f"- k * ({dd} + {E}) + k2 * (0.1 + {E})")
    rr = f"0.625 {sg} ({dd} {phi})"
    params = {"a": (1.0, "1/s"), "tb": (1.0, "s"), "ex": (1e-6, "")}
    decls = [f"Real x(start = {0.0 if up else 0.78})"] + [DISC.format(v) for v in ("n", "m", "k", "k2")]
    eqs = []
    if z_state:
        decls.append("Real z(start = 0, fixed = true)")
        eqs.append("der(z) = a * (1 - k)" if kind == "reinit" else "der(z) = a")
    eqs.append(x_eq(implicit, rr))
    th = f"0.5 {sg} k * ex" if kind == "thr" else "0.5"
    fop, bop = ops(up, strict)
    forth = [count("n")]
    if kind != "sep":
        forth.append("k = 1")
    if kind == "reinit":
        forth.append(f"reinit(z, 0.625 - {E})")
    whens = [(f"x {fop} {th}", forth, "forth")]
    if kind == "sep":
        whens.append((f"x {fop} {th}", ["k = 1"], "k set"))
    whens.append((f"x {bop} {th}", [count("m")], "back"))
    if kind == "ownmode":
        decls.append("Real y")
        eqs.append(f"y = if x {bop} {th} then 1 else 0")
    whens.append(("time >= tb", ["k2 = 1"], "k2 set"))
    text = mo(params, decls, eqs, whens)
    runs = []
    for a in rates:
        for exk in exks:
            for rtol in rtols:
                ex = 1e-12 / 1.75 if exk == "noise" else float(exk) * rtol
                t1, tb = 0.625 / a, 0.625 / a + 0.5
                L = loc(rtol, a / 1.75)
                req = {"forth": [[t1, L], [tb, EXACT]], "back": [[t1, L]]}
                fin = {"n": [2, 0], "m": [1, 0]}
                same = [["back", 0, "forth", 0]]
                either = [] if strict else [{"req": {"forth": req["forth"], "back": []},
                                             "final": {"n": [2, 0], "m": [0, 0]}, "same": []}]
                tag = {"a": a, "ex": exk}
                limits = []
                if ex <= resolution(rtol):
                    keep = {"forth": [[t1, L]], "back": [], "_counters": {"n": "forth", "m": "back"}}
                    limits.append(noise_alt(keep, [t1 - L, tb + EXACT], dict(tag, ex="1", rtol=rtol)))
                tr = truth(req, fin, same=same, either=either, limits=limits,
                           storm=[[t1, L], [tb, L]] if kind == "ownmode" else [])
                runs.append(run({"a": a, "tb": tb, "ex": ex}, rtol, tr, tag))
    shape = {"kind": kind, "dir": "up" if up else "down", "cond": "strict" if strict else "nonstrict"}
    return build("own", shape, config, text, runs, ["forth", "back"], ["n", "m"])


def own_big(kind, config, rtols):
    implicit, z_state, _, _ = CONFIGS[config]
    r = "z" if z_state else "a * time"
    rr = {"kick": f"{r} - 0.2 * k", "steps": f"{r} - 0.6 * n", "saw": r}[kind]
    decls = ["Real x(start = 0)"] + [DISC.format(v) for v in ("n", "m", "k")]
    eqs = []
    if z_state:
        decls.append("Real z(start = 0, fixed = true)")
        eqs.append("der(z) = a")
    eqs.append(x_eq(implicit, rr))
    forth = [count("n")] + (["k = 1"] if kind == "kick" else []) + (["reinit(z, 0)"] if kind == "saw" else [])
    whens = [("x > 0.5", forth, "forth")]
    labels = ["forth"]
    if kind == "kick":
        whens.append(("x < 0.5", [count("m")], "back"))
        labels.append("back")
    text = mo({"a": (1.0, "1/s")}, decls, eqs, whens)
    times = {"kick": [0.625, 0.825], "steps": [0.625, 1.225, 1.825], "saw": [0.625, 1.25, 1.875]}[kind]
    runs = []
    for rtol in rtols:
        L = loc(rtol, 1 / 1.75)
        req = {"forth": [[t, L] for t in times]}
        fin = {"n": [len(times), 0]}
        same = []
        if kind == "kick":
            req["back"] = [[0.625, L]]
            fin["m"] = [1, 0]
            same = [["back", 0, "forth", 0]]
        runs.append(run({}, rtol, truth(req, fin, same=same), {}))
    return build("own", {"kind": kind, "dir": "up", "cond": "strict"}, config, text, runs, labels, ["n", "m"])


RAMP_RATES = [0.3, 0.37, 0.5, 0.7, 0.81, 1.0, 1.29, 1.3, 1.7, 2.3, 3.1, 4.4, 6.0, 7.0]


def ramp(tier: str) -> list:
    """Review A's and recheck 1's ramps, pairs and modes, the judge's
    count-implicit and count-pair, implicit_conditions.rs' first tests: x
    crosses 0.5 once, at t1 = 0.625 / a (R = a t, or 1.25 - a t falling;
    over 2 / min(a, 1) + 1 s); forth fires once there, back never; the
    elsewhen switch (when forth then on = 1 elsewhen back then on = 0) is 0
    before t1 and 1 after; a mode on forth (y) is 1 at the end; w' = (if
    forth then 1 else 0) gives w(T) = T - t1. `two`: a second iteration
    variable v^3 + v = 2 r crosses 0.5 at t1 / 2, upwards, under `when v <
    0.5`: never fires. `fall2`: R = 2 - r, falling through 0.625 at 1.375 /
    a; `osc`: z = 0.5 sin(3 t) (z' = v, v' = -9 z), x^3 + x = z, `when x >
    0.3`: once, at asin(0.327 / 0.5) / 3, x' = z' / 1.27 there."""
    out = []
    quick = tier == "quick"
    rates = [0.7, 2.3, 4.4] if quick else RAMP_RATES
    conds = ["lones", "switch", "switchmode"] if quick else [
        "lones", "lonen", "pairs", "pairn", "switch", "switchmode", "lonemode", "modeonly", "two"]
    for up in ((True,) if quick else (True, False)):
        for cond in conds:
            if cond == "two" and not up:
                continue
            for config in CONFIGS:
                out.append(ramp_build(up, cond, config, rates, RTOLS))
    if not quick:
        for config in CONFIGS:
            out.append(ramp_build(False, "lones", config, [1.0], RTOLS, r0=2.0))
        for config in STATE_CONFIGS:
            out.append(osc_build(config))
    return out


def ramp_build(up, cond, config, rates, rtols, r0=None):
    implicit, z_state, _, _ = CONFIGS[config]
    r = "z" if z_state else "a * time"
    rr = r if up else (f"{r0!r} - {r}" if r0 else f"1.25 - {r}")
    decls = [f"Real x(start = {0.0 if up else 0.78})"] + [DISC.format(v) for v in ("n", "m", "on", "q")]
    eqs = []
    if z_state:
        decls.append("Real z(start = 0, fixed = true)")
        eqs.append("der(z) = a")
    eqs.append(x_eq(implicit, rr))
    strict = cond not in ("lonen", "pairn")
    fop, bop = ops(up, strict)
    whens, labels, record, grid = [], [], ["n", "m"], []
    if cond != "modeonly":
        whens.append((f"x {fop} 0.5", [count("n")], "forth"))
        labels.append("forth")
    if cond in ("pairs", "pairn", "switch", "switchmode", "two"):
        whens.append((f"x {bop} 0.5", [count("m")], "back"))
        labels.append("back")
    if cond in ("switch", "switchmode"):
        whens.append((f"x {fop} 0.5 then\n    on = 1;\n  elsewhen x {bop} 0.5", ["on = 0"], "the switch"))
        record.append("on")
        grid.append("on")
    if cond in ("switchmode", "lonemode"):
        decls.append("Real y")
        eqs.append(f"y = if x {fop} 0.5 then 1 else 0")
        record.append("y")
    if cond == "modeonly":
        decls.append('Real w(unit = "s", start = 0, fixed = true)')
        eqs.append(f"der(w) = if x {fop} 0.5 then 1 else 0")
        record.append("w")
    if cond == "two":
        decls.append("Real v(start = 0)")
        eqs.append(x_eq(implicit, f"2 * {r}", var="v"))
        whens.append(("v < 0.5", [count("q")], "vdown"))
        labels.append("vdown")
        record.append("q")
    text = mo({"a": (1.0, "1/s")}, decls, eqs, whens)
    runs = []
    for a in rates:
        t_end = 2.0 / min(a, 1.0) + 1.0
        t1 = (abs(r0 - 0.625) if r0 else 0.625) / a
        for rtol in rtols:
            L = loc(rtol, a / 1.75)
            req = {lab: [] for lab in labels}
            fin = {"n": [0, 0], "m": [0, 0]}
            if "forth" in labels:
                req["forth"] = [[t1, L]]
                fin["n"] = [1, 0]
            if "on" in record:
                fin["on"] = [1, 0]
            if "y" in record:
                fin["y"] = [1, 0]
            if "w" in record:
                fin["w"] = [t_end - t1, 4 * L + 20 * rtol + 1e-9]
            if "q" in record:
                fin["q"] = [0, 0]
            gridv = {"on": [[0, t1, 0], [t1, t_end, 1]]} if "on" in record else {}
            mode = cond in ("switchmode", "lonemode", "modeonly")
            tr = truth(req, fin, gridv=gridv, storm=[[t1, L]] if mode else [])
            runs.append(run({"a": a}, rtol, tr, {"a": a}, t_end=t_end))
    shape = {"dir": "up" if up else "down", "cond": cond if r0 is None else "fall2"}
    return build("ramp", shape, config, text, runs, labels, record, grid=grid)


def osc_build(config):
    implicit = CONFIGS[config][0]
    decls = ["Real z(start = 0, fixed = true)", 'Real v(unit = "1/s", start = 1.5, fixed = true)',
             "Real x(start = 0)", DISC.format("n")]
    eqs = ["der(z) = v", "der(v) = -w * w * z",
           "x ^ 3 + x = z" if implicit else "x = 0.3 + (z - 0.327) / 1.27"]
    text = mo({"w": (3.0, "rad/s")}, decls, eqs, [("x > 0.3", [count("n")], "forth")])
    ts = math.asin(0.327 / 0.5) / 3.0
    slope = 1.5 * math.cos(3.0 * ts) / 1.27
    runs = [run({}, rtol, truth({"forth": [[ts, loc(rtol, slope)]]}, {"n": [1, 0]}), {}) for rtol in RTOLS]
    return build("ramp", {"dir": "up", "cond": "osc"}, config, text, runs, ["forth"], ["n"])


def peak(tier: str) -> list:
    """Review A's peaks (recheck 1's peaks, the judge's count-peak,
    implicit_conditions.rs' peaked pair): R = 0.625 + d - c (t - 1)^2 (or
    z with z'' = -2 c from z(0) = 0.625 + d - 1, z'(0) = 2), c = 1: x over
    0.5 from 1 - sqrt(d) to 1 + sqrt(d), x' = +-2 sqrt(d) / 1.75 there.
    forth (rise) fires at 1 - sqrt(d), back (fall) at 1 + sqrt(d), the
    switch is 1 between them and 0 after. `rise`: forth alone. Each run has
    a twin with max_step = 2 sqrt(d) / 4 (the one-step limit's check)."""
    out = []
    quick = tier == "quick"
    for d in ([1e-3, 1e-4] if quick else [1e-2, 1e-3, 1e-4]):
        for cond in (["switch"] if quick else ["rise", "risefall", "switch"]):
            for config in CONFIGS:
                out.append(peak_build(d, cond, config))
    return out


def peak_build(d, cond, config):
    implicit, z_state, _, _ = CONFIGS[config]
    decls = ["Real x(start = 0)"] + [DISC.format(v) for v in ("n", "m", "on")]
    eqs = []
    if z_state:
        decls += [f"Real z(start = {0.625 + d - 1.0!r}, fixed = true)",
                  'Real u(unit = "1/s", start = 2, fixed = true)']
        eqs += ["der(z) = u", "der(u) = -2 * c"]
        rr = "z"
    else:
        rr = f"{0.625 + d!r} - c * (time - 1) ^ 2"
    eqs.append(x_eq(implicit, rr))
    whens = [("x > 0.5", [count("n")], "forth")]
    labels, record, grid = ["forth"], ["n", "m"], []
    if cond != "rise":
        whens.append(("x < 0.5", [count("m")], "back"))
        labels.append("back")
    if cond == "switch":
        whens.append(("x > 0.5 then\n    on = 1;\n  elsewhen x < 0.5", ["on = 0"], "the switch"))
        record.append("on")
        grid.append("on")
    text = mo({"c": (1.0, "1/s2")}, decls, eqs, whens)
    tr_, tf_ = 1 - math.sqrt(d), 1 + math.sqrt(d)
    h = 2 * math.sqrt(d) / 4
    runs = []
    for rtol in RTOLS:
        L = loc(rtol, 2 * math.sqrt(d) / 1.75)
        req = {"forth": [[tr_, L]]}
        fin = {"n": [1, 0], "m": [0, 0]}
        none = {"forth": []}
        if "back" in labels:
            req["back"] = [[tf_, L]]
            fin["m"] = [1, 0]
            none["back"] = []
        gridv = {}
        if cond == "switch":
            fin["on"] = [0, 0]
            gridv = {"on": [[0, tr_, 0], [tr_, tf_, 1], [tf_, T_END, 0]]}
        for twin in (False, True):
            tag = {"twin": twin}
            limits = [] if twin else [{
                "cls": "onestep", "req": none, "final": {"n": [0, 0], "m": [0, 0]} | ({"on": [0, 0]} if gridv else {}),
                "gridv": {"on": [[0, T_END, 0]]} if gridv else {},
                "twin": key(dict(tag, twin=True, rtol=rtol)), "twin_ok": ["right"]}]
            kw = {"max_step": h} if twin else {}
            runs.append(run({}, rtol, truth(req, fin, gridv=gridv, limits=limits), tag, **kw))
    return build("peak", {"d": d, "cond": cond}, config, text, runs, labels, record, grid=grid)


def moved(tier: str) -> list:
    """Recheck 1's moved conditions (the judge's count-moved,
    implicit_conditions.rs' a_condition_its_event_moves_across_fires_there)
    and the held nudge (a_held_condition_does_not_fire_again_at_the_next_
    event). k set at 1 s (by a time event, or where a state s = t passes 1):
      nudge:    R = 0.625 - e + 2 e k (+ z, z' = 0), `when x > 0.5`: x
                across 0.5 by e/1.75 at 1 s, then constant: fires once, at 1;
      land:     R = 0.5 + (0.125 + e) k (+ z, z' = 0), `when x >= 0.5`: x
                from 0.41 to 0.5 + e/1.75 (onto 0.5 itself, e = 0): once,
                at 1;
      landrise: land, rising on after (+ 0.1 a k (t - 1), or + 0.1 z with
                z' = a k): once, at 1;
      heldnext: R = r + e k, k set by a time event at t1 = 0.625 / a, the
                crossing itself: x further on by e/1.75: once, at t1.
    e: as named, and 1.75 rtol (the twin of the small ones)."""
    out = []
    quick = tier == "quick"
    rtols = QUICK_RTOLS if quick else RTOLS
    for trig in ("time", "state"):
        for config in CONFIGS:
            out.append(moved_build("nudge", trig, config, ["1e-12", "1e-7"] if quick else
                                   ["1e-12", "1e-9", "1e-7", "1e-5"], rtols))
    for shape in ("land", "landrise"):
        for config in CONFIGS:
            out.append(moved_build(shape, "time", config, ["0", "1e-9", "1e-6"], rtols))
    if not quick:
        for config in CONFIGS:
            out.append(moved_build("heldnext", "time", config, ["1e-12"], rtols))
    return out


def moved_build(shape, trig, config, eks, rtols):
    implicit, z_state, _, _ = CONFIGS[config]
    decls = [f"Real x(start = {0.4 if shape.startswith('land') else 0.5 if shape == 'nudge' else 0.0})",
             DISC.format("n"), DISC.format("k")]
    eqs = []
    if z_state:
        decls.append("Real z(start = 0, fixed = true)")
    zrate = "0"
    if shape == "nudge":
        rr = "0.625 - e + 2 * e * k" + (" + z" if z_state else "")
        cond = "x > 0.5"
    elif shape == "land":
        rr = "0.5 + (0.125 + e) * k" + (" + z" if z_state else "")
        cond = "x >= 0.5"
    elif shape == "landrise":
        rr = "0.5 + (0.125 + e) * k + " + ("0.1 * z" if z_state else "0.1 * a * k * (time - 1)")
        zrate = "a * k"
        cond = "x >= 0.5"
    else:
        rr = ("z" if z_state else "a * time") + " + e * k"
        zrate = "a"
        cond = "x > 0.5"
    if z_state:
        eqs.append(f"der(z) = {zrate}")
    if trig == "state":
        decls.append('Real s(unit = "s", start = 0, fixed = true)')
        eqs.append("der(s) = 1")
    eqs.append(x_eq(implicit, rr))
    at = "s > tk" if trig == "state" else "time >= tk"
    whens = [(at, ["k = 1"], "k set"), (cond, [count("n")], "forth")]
    text = mo({"a": (1.0, "1/s"), "e": (1e-12, ""), "tk": (1.0, "s")}, decls, eqs, whens)
    rates = [0.7, 1.0, 2.3, 4.4] if shape == "heldnext" else [1.0]
    runs = []
    for a in rates:
        for ek in eks + ["erel"]:
            for rtol in rtols:
                e = 1.75 * rtol if ek == "erel" else float(ek)
                tk = 0.625 / a if shape == "heldnext" else 1.0
                t = tk
                tol = loc(rtol, a / 1.75) if shape == "heldnext" else ev_tol(trig, rtol)
                tag = {"a": a, "e": ek}
                limits = []
                if shape != "heldnext" and ek != "erel" and e / 1.75 <= resolution(rtol):
                    keep = {"forth": [], "_counters": {"n": "forth"}}
                    limits.append(noise_alt(keep, [t - tol, T_END], dict(tag, e="erel", rtol=rtol)))
                tr = truth({"forth": [[t, tol]]}, {"n": [1, 0]}, limits=limits)
                runs.append(run({"a": a, "e": e, "tk": tk}, rtol, tr, tag))
    return build("moved", {"shape": shape, "trig": trig}, config, text, runs, ["forth"], ["n"])


def stale(tier: str) -> list:
    """Recheck 2's stale probe: R = r (1 - k) + (0.625 - e) k + 0.2 k2: x
    rises through 0.5 at t1 = 0.625 / a; k set at t2 = t1 + d (a time event,
    or where a state s = t passes t2) drops it to 0.5 - e/1.75; k2 at 1.5 s
    lifts it to about 0.62. forth at [t1, 1.5], back at [t2]. e = 1e-12 (the
    probe's) and 1.75 rtol (its twin)."""
    out = []
    quick = tier == "quick"
    for trig in (("time",) if quick else ("time", "state")):
        for config in CONFIGS:
            out.append(stale_build(trig, config, [0.7, 2.3] if quick else [0.7, 1.0, 1.3, 2.3, 4.4],
                                   [1e-6, 1e-4] if quick else [1e-6, 1e-4, 1e-2],
                                   QUICK_RTOLS if quick else RTOLS))
    return out


def stale_build(trig, config, rates, deltas, rtols):
    implicit, z_state, _, _ = CONFIGS[config]
    r = "z" if z_state else "a * time"
    decls = ["Real x(start = 0)"] + [DISC.format(v) for v in ("n", "m", "k", "k2")]
    eqs = []
    if z_state:
        decls.append("Real z(start = 0, fixed = true)")
        eqs.append("der(z) = a")
    if trig == "state":
        decls.append('Real s(unit = "s", start = 0, fixed = true)')
        eqs.append("der(s) = 1")
    eqs.append(x_eq(implicit, f"{r} * (1 - k) + (0.625 - e) * k + 0.2 * k2"))
    at = "s > t2" if trig == "state" else "time >= t2"
    whens = [(at, ["k = 1"], "k set"), ("time >= 1.5", ["k2 = 1"], "k2 set"),
             ("x > 0.5", [count("n")], "forth"), ("x < 0.5", [count("m")], "back")]
    text = mo({"a": (1.0, "1/s"), "t2": (1.0, "s"), "e": (1e-12, "")}, decls, eqs, whens)
    runs = []
    for a in rates:
        for d in deltas:
            for ek in ("e12", "erel"):
                for rtol in rtols:
                    e = 1e-12 if ek == "e12" else 1.75 * rtol
                    t1 = 0.625 / a
                    t2 = t1 + d
                    L, ev = loc(rtol, a / 1.75), ev_tol(trig, rtol)
                    req = {"forth": [[t1, L], [1.5, EXACT]], "back": [[t2, ev]]}
                    fin = {"n": [2, 0], "m": [1, 0]}
                    tag = {"a": a, "d": d, "e": ek}
                    limits = []
                    if d < L:
                        limits.append({"cls": "L1", "req": {"forth": [[1.5, EXACT]], "back": []},
                                       "final": {"n": [1, 0], "m": [0, 0]}})
                    if ek == "e12" and e / 1.75 <= resolution(rtol):
                        twin = dict(tag, e="erel", rtol=rtol)
                        cn = {"n": "forth", "m": "back"}
                        limits.append(noise_alt({"forth": [[t1, L]], "back": [], "_counters": dict(cn)},
                                                [t2 - ev, 1.5 + EXACT], twin))
                        if d < L:
                            limits.append(noise_alt({"forth": [], "back": [], "_counters": dict(cn)},
                                                    [t1 - L, 1.5 + EXACT], twin))
                    runs.append(run({"a": a, "t2": t2, "e": e}, rtol, truth(req, fin, limits=limits), tag))
    return build("stale", {"trig": trig}, config, text, runs, ["forth", "back"], ["n", "m"])


def count_cases(tier: str) -> list:
    """The judge's own count cases on conditions on iteration variables
    (count-implicit, count-pair, count-peak, count-moved, count-held-back,
    judge.py crossings()), their texts as the judge writes them, run as the
    judge runs them (the engine's default backend): the exact times the
    judge has, with this suite's tolerances (crossings located to LOC rtol
    in x, time events exact, a state's crossing to STATE_EV rtol)."""
    sys.path.insert(0, str(HERE))
    import judge  # noqa: E402  (the judge beside this file)
    out = []
    for name, (text, case) in judge.crossings().items():
        if not name.startswith(("count-implicit", "count-pair", "count-peak", "count-moved", "count-held-back")):
            continue
        if tier == "quick" and name.startswith("count-peak"):
            continue
        j = case["judge"]
        nums = [float(x) for x in re.findall(r"\d+(?:\.\d+)?(?:e-?\d+)?", name)]
        has_m = name.startswith(("count-pair", "count-peak", "count-held-back"))
        runs = []
        for rtol in RTOLS:
            if name.startswith(("count-implicit", "count-pair")):
                a = nums[0]
                req = {"it counts": [[j["exact"][0], loc(rtol, a / 1.75)]], "it counts down": []}
            elif name.startswith("count-peak"):
                sl = 2 * math.sqrt(nums[0]) / 1.75
                req = {"it counts": [[j["exact"][0], loc(rtol, sl)]],
                       "it counts down": [[j["also"]["m"]["exact"][0], loc(rtol, sl)]]}
            elif name.startswith("count-moved"):
                req = {"it counts": [[1.0, ev_tol("state" if "other" in name else "time", rtol)]]}
            else:
                a, d = nums[0], nums[1]
                L = loc(rtol, a / 1.75)
                ev = ev_tol("state" if "other" in name else "time", rtol)
                req = {"it counts": [[j["exact"][0], L], [j["exact"][1], EXACT]],
                       "it counts down": [[j["also"]["m"]["exact"][0], ev]]}
            fin = {"n": [len(req["it counts"]), 0]}
            if has_m:
                fin["m"] = [len(req["it counts down"]), 0]
            for v, val in j.get("final", {}).items():
                fin[v] = [val, 0]
            limits = []
            if name.startswith("count-held-back") and d < L:
                limits.append({"cls": "L1", "req": {"it counts": req["it counts"][1:], "it counts down": []},
                               "final": {"n": [1, 0], "m": [0, 0]}})
            for twin in ((False, True) if name.startswith("count-peak") else (False,)):
                tag = {"twin": twin}
                lim = list(limits)
                kw = {}
                if name.startswith("count-peak"):
                    if twin:
                        kw = {"max_step": math.sqrt(nums[0]) / 2}
                    else:
                        lim.append({"cls": "onestep", "req": {"it counts": [], "it counts down": []},
                                    "final": {"n": [0, 0], "m": [0, 0], "on": [0, 0]},
                                    "twin": key(dict(tag, twin=True, rtol=rtol)), "twin_ok": ["right"]})
                runs.append(run({}, rtol, truth(req, fin, limits=lim), tag, **kw))
        labels = ["it counts"] + (["it counts down"] if has_m else [])
        record = ["n"] + (["m"] if has_m else []) + list(j.get("final", {}))
        b = build("count", {"case": name}, "default", text, runs, labels, record, t_end=case["t_end"])
        b["model"] = case["model"]
        out.append(b)
    return out


FAMILIES = {"fam": fam, "own": own, "ramp": ramp, "peak": peak, "moved": moved, "stale": stale,
            "count": count_cases}


def all_builds(tier: str) -> list:
    out = [b for f in FAMILIES.values() for b in f(tier)]
    assert len({b["id"] for b in out}) == len(out), "build ids repeat"
    return out


# ---------------------------------------------------------------------------
# Scoring
# ---------------------------------------------------------------------------

def fired_by(res: dict, labels: list) -> dict:
    out = {lab: [] for lab in labels}
    for t, lab in res.get("fired", []):
        for want in labels:
            if lab.endswith(want):
                out[want].append(t)
    return out


def matches(res: dict, spec: dict, labels: list, grid_dt: float = 0.1) -> bool:
    """Whether a run is `spec`'s outcome: each counted `when` fired at its
    required times (each within its tolerance, in order), anything else
    only inside `free`, the instants `same` names equal, the final values
    and the grid values as given."""
    if not res.get("ok"):
        return False
    fired = fired_by(res, labels)
    free = spec.get("free", [])
    got = {}
    for lab in labels:
        req = spec["req"].get(lab, [])
        qi, m = 0, []
        for t in fired[lab]:
            if qi < len(req) and abs(t - req[qi][0]) <= req[qi][1]:
                m.append(t)
                qi += 1
            elif not any(a <= t <= b for a, b in free):
                return False
        if qi != len(req):
            return False
        got[lab] = m
    for la, i, lb, j in spec.get("same", []):
        if i >= len(got[la]) or j >= len(got[lb]) or got[la][i] != got[lb][j]:
            return False
    fin = res.get("final", {})
    for v, (want, tol) in spec.get("final", {}).items():
        g = fin.get(v)
        if want == "count":
            if g != len(fired[tol]):
                return False
        elif g is None or abs(g - want) > tol:
            return False
    for v, spans in spec.get("gridv", {}).items():
        vals = res.get("grid", {}).get(v, [])
        for k, val in enumerate(vals):
            t = k * grid_dt
            for a, b, want in spans:
                if a + 1e-6 < t < b - 1e-6 and val != want:
                    return False
    return True


def storm_at(res: dict, spec: dict):
    err = res.get("error") or ""
    if "event storm" not in err:
        return False
    try:
        t = float(err.split("event storm at t = ")[1].split()[0])
    except (IndexError, ValueError):
        return False
    return any(abs(t - s) <= tol for s, tol in spec.get("storm", []))


def classify(res, tr, labels, cls_of_key) -> str:
    """right; else a named limit whose criterion holds (L1, noise, onestep,
    storm); else wrong."""
    if res is None:
        return "missing"
    if matches(res, tr, labels) or any(matches(res, dict(tr, **alt), labels) for alt in tr.get("either", [])):
        return "right"
    for lim in tr.get("limits", []):
        if not matches(res, lim, labels):
            continue
        if "twin" in lim:
            tc = cls_of_key(lim["twin"])
            if tc not in lim["twin_ok"]:
                continue
        return lim["cls"]
    if storm_at(res, tr):
        return "storm"
    return "wrong"


def describe(res, tr, labels) -> str:
    """How a wrong run differs from the truth, in a few words."""
    if not res.get("ok"):
        return "stopped: " + (res.get("error") or "")[:60].split(" at t =")[0]
    fired = fired_by(res, labels)
    out = []
    for lab in labels:
        req = tr["req"].get(lab, [])
        got = fired[lab]
        if len(got) != len(req):
            out.append(f"{lab} {len(got)}/{len(req)}")
        else:
            off = [abs(g - q[0]) / q[1] for g, q in zip(got, req) if abs(g - q[0]) > q[1]]
            if off:
                out.append(f"{lab} off {max(off):.3g} tol")
    for v, (want, tol) in tr.get("final", {}).items():
        g = res.get("final", {}).get(v)
        if want != "count" and (g is None or abs(g - want) > tol) and v not in ("n", "m", "q"):
            out.append(f"{v}={g if g is None else round(g, 6)} not {round(want, 6)}")
    if not out and not matches(res, tr, labels):
        out.append("same instant or grid")
    return ", ".join(out)


def load_results(path: Path) -> dict:
    out = {}
    with open(path) as f:
        for line in f:
            r = json.loads(line)
            out[(r["b"], r["r"])] = r
    return out


def score_one(builds, res: dict):
    """Each run's class, and its build."""
    cls, keyed = {}, {}
    for b in builds:
        for i, r in enumerate(b["runs"]):
            keyed[(b["id"], r["key"])] = (b, i, r)

    def of(bid, k):
        if (bid, k) in cls:
            return cls[(bid, k)]
        if (bid, k) not in keyed:
            return "missing"
        b, i, r = keyed[(bid, k)]
        c = classify(res.get((bid, i)), r["truth"], b["labels"], lambda tk: of(bid, tk))
        cls[(bid, k)] = c
        return c

    for (bid, k) in keyed:
        of(bid, k)
    return cls, keyed


CLASSES = ["right", "L1", "noise", "onestep", "storm", "wrong", "missing"]
SHOWN = ["wrong", "L1", "noise", "onestep", "storm", "right"]


def sub(b) -> str:
    """A build's shape family: its family and main shape parameter."""
    if b["family"] == "count":
        return "count:" + re.sub(r"-(implicit|nostate|other|nudge|land|rising)?-?[\d.e-]*$", "",
                                 b["shape"]["case"][6:]).split("-implicit")[0]
    k = {"fam": "effect", "own": "kind", "ramp": "cond", "peak": "cond", "moved": "shape", "stale": "trig"}
    return f"{b['family']}:{b['shape'][k[b['family']]]}"


def tally(builds, labelled: list, out_path: Path | None) -> str:
    """The classes per build, by family, shape family and configuration,
    and the wrong runs by how they are wrong. The peaks' max_step twins
    only decide the one-step limit: they are not counted."""
    scored = []
    for label, path in labelled:
        res = load_results(path)
        cls, keyed = score_one(builds, res)
        counted = {kk: v for kk, v in cls.items() if not keyed[kk][2]["tag"].get("twin")}
        scored.append((label, res, counted, keyed))
    lines = ["## All runs, by build\n", "| build | runs | " + " | ".join(SHOWN) + " |",
             "|---|---:|" + "---:|" * len(SHOWN)]
    for label, res, cls, keyed in scored:
        c = collections.Counter(cls.values())
        lines.append(f"| {label} | {sum(c.values())} | " + " | ".join(str(c[k]) for k in SHOWN) + " |")

    def table(title, keyf, keys):
        lines.append(f"\n## {title}: wrong / L1 / noise / onestep / storm / right\n")
        lines.append("| | " + " | ".join(lab for lab, *_ in scored) + " |")
        lines.append("|---|" + "---|" * len(scored))
        for k in keys:
            row = []
            for label, res, cls, keyed in scored:
                c = collections.Counter(v for kk, v in cls.items() if keyf(keyed[kk][0]) == k)
                row.append(" / ".join(str(c[x]) for x in SHOWN))
            lines.append(f"| {k} | " + " | ".join(row) + " |")

    subs = sorted({sub(b) for b in builds})
    table("By family", lambda b: b["family"], sorted({b["family"] for b in builds}))
    table("By configuration", lambda b: b["config"], list(CONFIGS) + ["default"])
    table("By shape family", sub, subs)
    cfgs = list(CONFIGS) + ["default"]
    abbr = {"L1": "L1", "noise": "N", "onestep": "S1", "storm": "St"}
    label0, res0, cls0, keyed0 = scored[0]
    runs = collections.Counter((sub(keyed0[kk][0]), keyed0[kk][0]["config"]) for kk in cls0)
    lines.append("\n## Runs, by shape family and configuration\n")
    lines.append("| shape family | " + " | ".join(cfgs) + " |")
    lines.append("|---|" + "---:|" * len(cfgs))
    for f in subs:
        lines.append(f"| {f} | " + " | ".join(str(runs[(f, c)] or "") for c in cfgs) + " |")
    for label, res, cls, keyed in scored:
        per = collections.defaultdict(collections.Counter)
        for kk, v in cls.items():
            b = keyed[kk][0]
            per[(sub(b), b["config"])][v] += 1
        lines.append(f"\n### {label}: wrong, and named limits, by shape family and configuration\n")
        lines.append("Each cell: wrong runs, then the named limits (L1, N noise, S1 onestep, St storm); "
                     "· none; the rest of the cell's runs (table above) right.\n")
        lines.append("| shape family | " + " | ".join(cfgs) + " |")
        lines.append("|---|" + "---|" * len(cfgs))
        for f in subs:
            row = []
            for c in cfgs:
                p = per[(f, c)]
                if not runs[(f, c)]:
                    row.append("")
                    continue
                lim = " ".join(f"{abbr[k]} {p[k]}" for k in abbr if p[k])
                row.append(("·" if not p["wrong"] and not lim else f"**{p['wrong']}**" if p["wrong"] else "")
                           + (" " + lim if lim else ""))
            lines.append(f"| {f} | " + " | ".join(x.strip() for x in row) + " |")
        sig, ex = collections.Counter(), {}
        for kk, v in cls.items():
            if v != "wrong":
                continue
            b, i, r = keyed[kk]
            sh = {k2: v2 for k2, v2 in b["shape"].items() if k2 != "dir"}
            what = describe(res.get((b["id"], i)) or {"ok": False, "error": "missing"}, r["truth"], b["labels"])
            s_ = (b["family"], "-".join(map(str, sh.values())), b["config"], what)
            sig[s_] += 1
            ex.setdefault(s_, f"{b['id']} {r['key']}")
        lines.append(f"\nWrong runs of {label}, by shape and how (the 40 largest of {len(sig)} kinds):\n")
        lines.append("| runs | family | shape | config | how | e.g. |")
        lines.append("|---:|---|---|---|---|---|")
        for s_, n in sig.most_common(40):
            lines.append(f"| {n} | " + " | ".join(s_) + f" | {ex[s_]} |")
    text = "\n".join(lines) + "\n"
    if out_path:
        out_path.write_text(text)
    return text


# ---------------------------------------------------------------------------
# The commands
# ---------------------------------------------------------------------------

def write_cases(out: Path, tier: str) -> None:
    out.mkdir(parents=True, exist_ok=True)
    builds = all_builds(tier)
    slim = [{k: v for k, v in b.items() if k not in ("family", "shape", "labels")}
            | {"runs": [{k: v for k, v in r.items() if k not in ("truth", "tag", "key")} for r in b["runs"]]}
            for b in builds]
    (out / "cases.json").write_text(json.dumps({"tier": tier, "builds": slim}))
    n = sum(len(b["runs"]) for b in builds)
    print(f"{len(builds)} models, {n} runs ({tier}) in {out / 'cases.json'}")


def tier_of(cases: Path) -> str:
    with open(cases / "cases.json" if cases.is_dir() else cases) as f:
        head = f.read(64)
    return "quick" if '"tier": "quick"' in head else "full"


def build_runner(checkout: Path, dest: Path) -> None:
    ex = checkout / "engine" / "crates" / "lsim-project" / "examples" / "holds_runner.rs"
    if ex.exists():
        sys.exit(f"{ex} exists already")
    shutil.copyfile(HERE / "holds_runner.rs", ex)
    try:
        subprocess.run(["cargo", "build", "--release", "--locked", "-q", "-p", "lsim-project",
                        "--example", "holds_runner"], cwd=checkout / "engine", check=True)
        meta = json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps"],
                                         cwd=checkout / "engine", check=True, capture_output=True,
                                         text=True).stdout)
        exe = Path(meta["target_directory"]) / "release" / "examples" / "holds_runner"
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(exe, dest)
        dest.chmod(0o755)
    finally:
        ex.unlink()
    print(f"runner of {checkout} in {dest}")


def run_suite(runner: Path, cases: Path, results: Path, threads: int) -> None:
    c = cases / "cases.json" if cases.is_dir() else cases
    subprocess.run([str(runner), str(c), str(results), str(threads)], check=True)


def selftest() -> None:
    """The scorer's own logic on made-up runs whose classes are known."""
    tr = truth({"forth": [[1.0, 0.1], [2.0, 1e-12]], "back": [[1.5, 1e-12]]}, {"n": [2, 0], "m": [1, 0]},
               same=[], limits=[{"cls": "L1", "req": {"forth": [[2.0, 1e-12]], "back": []},
                                 "final": {"n": [1, 0], "m": [0, 0]}}])
    lab = ["forth", "back"]
    ok = {"ok": True, "fired": [[1.05, "forth"], [1.5, "M.back"], [2.0, "forth"]], "final": {"n": 2, "m": 1}}
    assert classify(ok, tr, lab, None) == "right"
    late = dict(ok, fired=[[1.2, "forth"], [1.5, "back"], [2.0, "forth"]])
    assert classify(late, tr, lab, None) == "wrong"
    l1 = {"ok": True, "fired": [[2.0, "forth"]], "final": {"n": 1, "m": 0}}
    assert classify(l1, tr, lab, None) == "L1"
    twice = dict(ok, fired=ok["fired"] + [[1.06, "forth"]], final={"n": 3, "m": 1})
    assert classify(twice, tr, lab, None) == "wrong"
    nz = dict(tr, limits=[noise_alt({"forth": [[1.0, 0.1]], "back": [], "_counters": {"n": "forth",
                                                                                                  "m": "back"}},
                                    [1.5, 2.0], {"e": "erel"})])
    hidden = {"ok": True, "fired": [[1.05, "forth"]], "final": {"n": 1, "m": 0}}
    assert classify(hidden, nz, lab, lambda k: "right") == "noise"
    assert classify(hidden, nz, lab, lambda k: "wrong") == "wrong"
    assert classify({"ok": False, "error": "event storm at t = 1.01 s: ..."},
                    dict(tr, storm=[[1.0, 0.1]]), lab, None) == "storm"
    # the truth's own numbers: x^3 + x = 0.625 at 0.5, and the fam probe's back effect
    assert 0.5 ** 3 + 0.5 == 0.625
    b = fam_build(True, "time", "back", "pairs", "ida", [1.0], [1e-4], [1e-6])
    r = b["runs"][0]
    assert r["truth"]["req"]["forth"][1][0] == 0.625 + 0.5 and r["truth"]["req"]["back"][0][0] == 0.625 + 1e-4
    print("holds selftest OK")


def main(argv: list[str]) -> None:
    import argparse
    ap = argparse.ArgumentParser(prog="judge.py holds", description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("cases")
    p.add_argument("out", type=Path)
    p.add_argument("--tier", choices=["quick", "full"], default="full")
    p = sub.add_parser("build")
    p.add_argument("checkout", type=Path)
    p.add_argument("runner", type=Path)
    p = sub.add_parser("run")
    p.add_argument("runner", type=Path)
    p.add_argument("cases", type=Path)
    p.add_argument("results", type=Path)
    p.add_argument("--threads", type=int, default=12)
    p = sub.add_parser("score")
    p.add_argument("cases", type=Path)
    p.add_argument("results", nargs="+", help="LABEL=RESULTS.jsonl")
    p.add_argument("--out", type=Path)
    sub.add_parser("selftest")
    a = ap.parse_args(argv)
    if a.cmd == "cases":
        write_cases(a.out, a.tier)
    elif a.cmd == "build":
        build_runner(a.checkout.resolve(), a.runner.resolve())
    elif a.cmd == "run":
        run_suite(a.runner, a.cases, a.results, a.threads)
    elif a.cmd == "score":
        builds = all_builds(tier_of(a.cases))
        labelled = [x.split("=", 1) if "=" in x else (Path(x).stem, x) for x in a.results]
        print(tally(builds, [(lab, Path(p)) for lab, p in labelled], a.out))
    else:
        selftest()


if __name__ == "__main__":
    main(sys.argv[1:])
