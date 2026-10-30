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
(*The holds suite*) has the shapes, their truth and the named limits:
the round-4 review shapes (extra, coin, coinr, before, rdmode, noisy,
noisy2, noisy3, noisy3r, noisyown, mixedc, bcoin, bcoinw, bfreeze,
bfreezew, slow, slowsib, slowr) and re-check 4's (sibeq, slowbp, slowobs,
slowmode, asym, slowfrz, slowstop) beside the probes before them; and the
limits bounded, postlimit, located, moderead, shallowpeak and restartloss
(the hold spec, 4.9 and 4.10) beside L1, noise, onestep and storm.

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


# ---------------------------------------------------------------------------
# The hold rule's review shapes (round 4: the hold spec's reviews 1 to 3)
# ---------------------------------------------------------------------------

# the configurations where x is an iteration variable
IMPL = ["ida", "ida-implicit", "nostate-ida", "diffsol"]
SHAPE_RATES = [0.5, 0.7, 1.0, 1.3, 2.3, 3.1, 4.4, 6.0, 7.0]
# the coin shapes' located instants: where a first run (own's reset, or its
# reinit, at ex = 1 rtol) located the crossing, per configuration, shape, a
# and rtol; and slowstop's (slow, the stop never), per K too. The integration up to the first event is the same at every
# commit of round 4 (4d24528 to the fix); where the integrator changes, the
# shapes stay right by their truth but no longer land on the coincidence
TF = HERE / "holds_tf.json"


def band(rtol: float) -> float:
    """The hold's band in x: what x's tolerance allows there, rtol |x| + atol
    at x = 0.5 (atol = 1e-2 rtol)."""
    return 0.5 * rtol + 1e-2 * rtol


def extra(tier: str) -> list:
    """Round 4's extra shapes: moves smaller than a location error can be, at
    the crossing itself (the crossing's own `when`, forth, counts n and sets
    k; back counts m), E = 1.75 fx (or ex):
      fwd:   R = 0.625 + s (dd + k E): x from 0.5 to 0.5 + s fx: forth once,
             back never;
      small: own's reset at ex = 0.05 .. 0.3 rtol: forth [t1, tb], back at
             forth's first instant;
      frzf:  a freeze and a forward move, rs := r at the event, R = 0.625 + s
             ((1 - k) dd + k (rs - 0.625 + E)): x = 0.5 + s fx frozen: forth
             once, back never;
      frzb:  the same backward (- E), lifted at tb (k2): forth [t1, tb], back
             at forth's first instant.
    fx, ex: 5.7e-13 and 0.05 .. 1 rtol."""
    quick = tier == "quick"
    out = []
    for kind in ("fwd", "small", "frzf", "frzb"):
        for up in ((True,) if quick else (True, False)):
            for config in CONFIGS:
                out.append(extra_build(kind, up, config, [1.0, 4.4] if quick else [0.7, 1.0, 2.3, 4.4],
                                       ["noise", "0.2", "1"] if quick else
                                       ["noise", "0.05", "0.1", "0.2", "0.3", "0.5", "1"],
                                       QUICK_RTOLS if quick else RTOLS))
    return out


def extra_build(kind, up, config, rates, exks, rtols):
    implicit, z_state, _, _ = CONFIGS[config]
    sg = "+" if up else "-"
    r = "z" if z_state else "a * time"
    dd = f"({r} - 0.625)"
    E = "1.75 * ex"
    phi = {"fwd": f"+ k * {E}",
           "small": f"- k * ({dd} + {E}) + k2 * (0.1 + {E})",
           "frzf": f"- k * {dd} + k * (rs - 0.625 + {E})",
           "frzb": f"- k * {dd} + k * (rs - 0.625 - {E}) + k2 * (0.1 + {E})"}[kind]
    rr = f"0.625 {sg} ({dd} {phi})"
    params = {"a": (1.0, "1/s"), "tb": (1.0, "s"), "ex": (1e-6, "")}
    decls = [f"Real x(start = {0.0 if up else 0.78})"] + [DISC.format(v) for v in ("n", "m", "k", "k2", "rs")]
    eqs = []
    if z_state:
        decls.append("Real z(start = 0, fixed = true)")
        eqs.append("der(z) = a")
    eqs.append(x_eq(implicit, rr))
    fop, bop = ops(up, True)
    forth = [count("n"), "k = 1"] + ([f"rs = {r}"] if kind.startswith("frz") else [])
    whens = [(f"x {fop} 0.5", forth, "forth"), (f"x {bop} 0.5", [count("m")], "back"),
             ("time >= tb", ["k2 = 1"], "k2 set")]
    text = mo(params, decls, eqs, whens)
    back_kind = kind in ("small", "frzb")
    runs = []
    for a in rates:
        for exk in exks:
            for rtol in rtols:
                ex = 1e-12 / 1.75 if exk == "noise" else float(exk) * rtol
                t1, tb = 0.625 / a, 0.625 / a + 0.5
                L = loc(rtol, a / 1.75)
                tag = {"a": a, "ex": exk}
                limits = []
                if back_kind:
                    req = {"forth": [[t1, L], [tb, EXACT]], "back": [[t1, L]]}
                    fin = {"n": [2, 0], "m": [1, 0]}
                    same = [["back", 0, "forth", 0]]
                    free = [t1 - L, tb + EXACT]
                else:
                    req = {"forth": [[t1, L]], "back": []}
                    fin = {"n": [1, 0], "m": [0, 0]}
                    same = []
                    free = [t1 - L, t1 + L]
                if ex <= resolution(rtol):
                    keep = {"forth": [[t1, L]], "back": [], "_counters": {"n": "forth", "m": "back"}}
                    limits.append(noise_alt(keep, free, dict(tag, ex="1", rtol=rtol)))
                tr = truth(req, fin, same=same, limits=limits,
                           **({"after": slowed(0.0)} if kind.startswith("frz") else {}))
                runs.append(run({"a": a, "tb": tb, "ex": ex}, rtol, tr, tag))
    shape = {"kind": kind, "dir": "up" if up else "down"}
    return build("extra", shape, config, text, runs, ["forth", "back"], ["n", "m"])


def coin(tier: str) -> list:
    """Review 1's coin (B2): own's reset (forth sets k, R back to 0.625 - s
    E) with ex = a (t1 - tf) / 1.75, tf the instant a first run (ex = 1
    rtol) located the crossing at ([`TF`]): the reset lands x on the value
    the consistent x had there, so at the event's point it changes nothing.
    Where the first run located it late, ex = 1 rtol (no coincidence). And
    coinr (revision 1): own's reinit (forth reinits z to 0.625 - E) with ex
    = q a (t1 - tf) / 1.75, q = 0.5 and 1, early-located runs only. Truth:
    own's (forth [t1, tb], back at forth's first instant). Each run has its
    twin at ex = 1 rtol (the noise limit's)."""
    tf = json.loads(TF.read_text())
    quick = tier == "quick"
    rates, rtols = ([0.7, 2.3, 4.4], QUICK_RTOLS) if quick else (SHAPE_RATES, RTOLS)
    out = []
    for fam_, kind, configs in (("coin", "own", IMPL), ("coinr", "reinit", ["ida", "ida-implicit", "diffsol"])):
        for up in ((True,) if quick else (True, False)):
            for strict in ((True,) if quick else (True, False)):
                for config in configs:
                    base = own_build(kind, up, strict, config, [1.0], ["1"], [1e-6])
                    d = f"{'up' if up else 'down'}-{'s' if strict else 'n'}"
                    runs, twins = [], {}
                    for a in rates:
                        for rtol in rtols:
                            t1 = 0.625 / a
                            f = tf.get(f"{config}|{d}|{kind}|{a!r}|{rtol!r}")
                            for q in ((1.0,) if fam_ == "coin" else (0.5, 1.0)):
                                if f is None or f >= t1:
                                    if fam_ == "coinr":
                                        continue
                                    ex, tag = rtol, {"a": a, "q": "late"}
                                else:
                                    ex, tag = q * a * (t1 - f) / 1.75, {"a": a, "q": q}
                                r = own_build(kind, up, strict, config, [a], [repr(ex / rtol)], [rtol])["runs"][0]
                                r["tag"] = dict(tag, rtol=rtol)
                                r["key"] = key(r["tag"])
                                tw = own_build(kind, up, strict, config, [a], ["1"], [rtol])["runs"][0]
                                tw["tag"] = {"a": a, "twin": True, "rtol": rtol}
                                tw["key"] = key(tw["tag"])
                                for lim in r["truth"]["limits"]:
                                    if "twin" in lim:
                                        lim["twin"] = tw["key"]
                                runs.append(r)
                                twins[tw["key"]] = tw
                    if runs:
                        out.append(dict(base, runs=runs + list(twins.values()), family=fam_,
                                        id=base["id"].replace("own/", fam_ + "/")))
    return out


def before(tier: str) -> list:
    """Review 1's before (S2): a time event at t2 = t1 - d, before the
    crossing, resets R to 0.625 - s e (x stays on the back side), lifted at
    tb (k2). Truth: forth [tb], back never. Where the crossing was located
    before t2, the run fires forth there and back at t2: located (the
    mirror of L1)."""
    quick = tier == "quick"
    out = []
    for config in CONFIGS:
        implicit, z_state, _, _ = CONFIGS[config]
        for up in ((True,) if quick else (True, False)):
            sg = "+" if up else "-"
            r = "z" if z_state else "a * time"
            dd = f"({r} - 0.625)"
            rr = f"0.625 {sg} ({dd} - k * ({dd} + e) + k2 * (0.1 + e))"
            params = {"a": (1.0, "1/s"), "t2": (1.0, "s"), "tb": (1.0, "s"), "e": (1e-12, "")}
            decls = [f"Real x(start = {0.0 if up else 0.78})"] + [DISC.format(v) for v in ("n", "m", "k", "k2")]
            eqs = []
            if z_state:
                decls.append("Real z(start = 0, fixed = true)")
                eqs.append("der(z) = a")
            eqs.append(x_eq(implicit, rr))
            fop, bop = ops(up, True)
            whens = [("time >= t2", ["k = 1"], "k set"), ("time >= tb", ["k2 = 1"], "k2 set"),
                     (f"x {fop} 0.5", [count("n")], "forth"), (f"x {bop} 0.5", [count("m")], "back")]
            runs = []
            for a in ([1.0, 4.4] if quick else [0.7, 1.0, 2.3, 4.4]):
                for d in ([1e-7, 1e-5] if quick else [1e-8, 1e-7, 1e-6, 1e-5]):
                    for ek in ("e05", "erel"):
                        for rtol in (QUICK_RTOLS if quick else RTOLS):
                            e = (0.875 if ek == "e05" else 1.75) * rtol
                            t1 = 0.625 / a
                            t2, tb = t1 - d, t1 + 0.5
                            tr = truth({"forth": [[tb, EXACT]], "back": []}, {"n": [1, 0], "m": [0, 0]})
                            runs.append(run({"a": a, "t2": t2, "tb": tb, "e": e}, rtol, tr, {"a": a, "d": d, "e": ek}))
            out.append(build("before", {"dir": "up" if up else "down"}, config, mo(params, decls, eqs, whens),
                             runs, ["forth", "back"], ["n", "m"]))
    return out


def rdmode(tier: str) -> list:
    """Review 1's rdmode (S1): y = if x > 0.5 then 1 else 0; forth sets k,
    and `when k > 0.5 then m = y` (the next round of event iteration reads
    the mode), or forth sets m = y itself (own). Truth: m = 1 (the relation
    holds just after its event instant), n = 1. A crossing located early
    sets the mode from its relation literally (0) while the `when`'s hold
    holds: mode read, a named limit (the hold spec's open question 2)."""
    quick = tier == "quick"
    out = []
    for config in CONFIGS:
        implicit, z_state, _, _ = CONFIGS[config]
        for strict in ((True,) if quick else (True, False)):
            for own_ in (False, True):
                r = "z" if z_state else "a * time"
                params = {"a": (1.0, "1/s")}
                decls = ["Real x(start = 0)", "Real y"] + [DISC.format(v) for v in ("n", "m", "k")]
                eqs = []
                if z_state:
                    decls.append("Real z(start = 0, fixed = true)")
                    eqs.append("der(z) = a")
                eqs.append(x_eq(implicit, r))
                fop, _ = ops(True, strict)
                eqs.append(f"y = if x {fop} 0.5 then 1 else 0")
                if own_:
                    whens = [(f"x {fop} 0.5", [count("n"), "m = y"], "forth")]
                else:
                    whens = [(f"x {fop} 0.5", [count("n"), "k = 1"], "forth"), ("k > 0.5", ["m = y"], "read")]
                runs = []
                for a in ([1.0, 4.4] if quick else SHAPE_RATES):
                    for rtol in (QUICK_RTOLS if quick else RTOLS):
                        t1 = 0.625 / a
                        tr = truth({"forth": [[t1, loc(rtol, a / 1.75)]]}, {"n": [1, 0], "m": [1, 0]})
                        runs.append(run({"a": a}, rtol, tr, {"a": a}))
                out.append(build("rdmode", {"s": "s" if strict else "n", "own": "own" if own_ else "next"}, config,
                                 mo(params, decls, eqs, whens), runs, ["forth"], ["n", "m"]))
    return out


def noisy(tier: str) -> list:
    """Rounding the event changes (review 1's B1, review 2's B2 and review
    3's S1): the event changes x only by the solve's rounding. Truth: forth
    once at t1, back never.
      noisy:    x^3 + x + B w = R + B c, w^3 + w = c^3 + c (w = c), q^3 + q =
                1 + k; forth sets k (k moves q only);
      noisy2:   x^3 + x + B (u - v) = R, u^3 + u = 1 + k, v = (1 + k) / (u^2
                + 1): u = v exactly; forth sets k;
      noisy3:   x^3 + x + (B t + k c) - B t - k c = R (exactly x^3 + x = R),
                c = 0.3; forth sets k;
      noisy3r:  the same with k from 1, forth clearing it, C 1e3 .. 1e6: the
                large term switched off.
    (Where the integrator or a restart cannot meet the tolerance through the
    rounding, the run stops: wrong, as it stops without holds too.)"""
    quick = tier == "quick"
    out = []
    rtols = QUICK_RTOLS if quick else RTOLS
    for config in IMPL:
        _, z_state, _, _ = CONFIGS[config]
        r = "z" if z_state else "a * time"
        zdecl = ["Real z(start = 0, fixed = true)"] if z_state else []
        zeq = ["der(z) = a"] if z_state else []
        shapes = []
        decls = ["Real x(start = 0)", "Real w(start = 0.5)", "Real q(start = 0.68)"] + \
            [DISC.format(v) for v in ("n", "m", "k")] + zdecl
        eqs = zeq + [f"x ^ 3 + x + B * w = {r} + B * c", "w ^ 3 + w = c ^ 3 + c", "q ^ 3 + q = 1 + k"]
        shapes.append(("noisy", {"a": (1.0, "1/s"), "B": (1.0, ""), "c": (0.5, "")}, decls, eqs, "k = 1",
                       [{"B": B, "c": 0.5} for B in ([0.0, 1e4] if quick else [0.0, 1e2, 1e3, 1e4, 1e5])], [0.7, 1.0, 2.3, 4.4]))
        decls = ["Real x(start = 0)", "Real u(start = 0.68)", "Real v"] + [DISC.format(w) for w in ("n", "m", "k")] + zdecl
        eqs = zeq + [f"x ^ 3 + x + B * (u - v) = {r}", "u ^ 3 + u = 1 + k", "v = (1 + k) / (u ^ 2 + 1)"]
        shapes.append(("noisy2", {"a": (1.0, "1/s"), "B": (1.0, "")}, decls, eqs, "k = 1",
                       [{"B": B} for B in ([0.0, 1e5] if quick else [0.0, 1e2, 1e3, 1e4, 1e5, 1e6])], [0.7, 1.0, 2.3, 4.4]))
        for up in ((True,) if quick else (True, False)):
            sg = "+" if up else "-"
            rr = f"0.625 {sg} ({r} - 0.625)"
            x0 = 0.0 if up else 0.78
            decls = [f"Real x(start = {x0})"] + [DISC.format(v) for v in ("n", "m", "k")] + zdecl
            eqs = zeq + [f"x ^ 3 + x + (B * time + k * c) - B * time - k * c = {rr}"]
            shapes.append((f"noisy3/{'up' if up else 'down'}", {"a": (1.0, "1/s"), "B": (1.0, "1/s"), "c": (0.3, "")},
                           decls, eqs, "k = 1",
                           [{"B": B} for B in ([1e4] if quick else [1e3, 1e4, 1e5, 1e6])], SHAPE_RATES))
            decls = [f"Real x(start = {x0})"] + [DISC.format(v) for v in ("n", "m")] + \
                ["discrete Real k(start = 1, fixed = true)"] + zdecl
            eqs = zeq + [f"x ^ 3 + x + (B * time + k * C) - B * time - k * C = {rr}"]
            shapes.append((f"noisy3r/{'up' if up else 'down'}", {"a": (1.0, "1/s"), "B": (1.0, "1/s"), "C": (1.0, "")},
                           decls, eqs, "k = 0",
                           [{"B": B, "C": C} for B in (0.0, 1e3) for C in ([1e5] if quick else [1e3, 1e4, 1e5, 1e6])],
                           SHAPE_RATES))
        for name, params, decls, eqs, act, sets, rates in shapes:
            fam_, *d = name.split("/")
            up = not d or d[0] == "up"
            fop, bop = ops(up, True)
            whens = [(f"x {fop} 0.5", [count("n"), act], "forth"), (f"x {bop} 0.5", [count("m")], "back")]
            runs = []
            for ps in sets:
                for a in ([1.0, 4.4] if quick else rates):
                    for rtol in rtols:
                        t1 = 0.625 / a
                        tr = truth({"forth": [[t1, loc(rtol, a / 1.75)]], "back": []}, {"n": [1, 0], "m": [0, 0]})
                        runs.append(run(dict(ps, a=a), rtol, tr, dict(ps, a=a)))
            shape = {"dir": "up" if up else "down"}
            out.append(build(fam_, shape, config, mo(params, decls, eqs, whens), runs, ["forth", "back"], ["n", "m"]))
    return out


def noisyown(tier: str) -> list:
    """Revision 2's noisyown (review 2's S2): noisy2's cancelling terms (x^3
    + x + B (u - v) = R, u = v exactly) beside own's reset (forth puts x
    back by ex = q band through k, q = 0.5 and 1; lifted at tb). Truth:
    own's (forth [t1, tb], back at forth's first instant). Where the
    rounding bound exceeds ex (B 1e5 and 1e6 at rtol 1e-9 and 1e-10), the
    rule cannot resolve the move: held, read at the hold's expiry."""
    quick = tier == "quick"
    out = []
    for config in IMPL:
        implicit, z_state, _, _ = CONFIGS[config]
        for up in ((True,) if quick else (True, False)):
            sg = "+" if up else "-"
            r = "z" if z_state else "a * time"
            dd = f"({r} - 0.625)"
            E = "1.75 * ex"
            rr = f"0.625 {sg} ({dd} - k * ({dd} + {E}) + k2 * (0.1 + {E}))"
            params = {"a": (1.0, "1/s"), "B": (1.0, ""), "tb": (1.0, "s"), "ex": (1e-6, "")}
            decls = [f"Real x(start = {0.0 if up else 0.78})", "Real u(start = 0.68)", "Real v"] + \
                [DISC.format(w) for w in ("n", "m", "k", "k2")]
            eqs = []
            if z_state:
                decls.append("Real z(start = 0, fixed = true)")
                eqs.append("der(z) = a")
            eqs += [f"x ^ 3 + x + B * (u - v) = {rr}", "u ^ 3 + u = 1 + k", "v = (1 + k) / (u ^ 2 + 1)"]
            fop, bop = ops(up, True)
            whens = [(f"x {fop} 0.5", [count("n"), "k = 1"], "forth"), (f"x {bop} 0.5", [count("m")], "back"),
                     ("time >= tb", ["k2 = 1"], "k2 set")]
            runs = []
            for B in ((0.0, 1e5) if quick else (0.0, 1e4, 1e5, 1e6)):
                for a in ((1.0,) if quick else (0.7, 1.0, 2.3, 4.4)):
                    for q in (0.5, 1.0):
                        for rtol in (QUICK_RTOLS if quick else RTOLS):
                            ex = q * band(rtol)
                            t1, tb = 0.625 / a, 0.625 / a + 0.5
                            L = loc(rtol, a / 1.75)
                            req = {"forth": [[t1, L], [tb, EXACT]], "back": [[t1, L]]}
                            tr = truth(req, {"n": [2, 0], "m": [1, 0]}, same=[["back", 0, "forth", 0]])
                            runs.append(run({"a": a, "B": B, "tb": tb, "ex": ex}, rtol, tr, {"a": a, "B": B, "q": q}))
            out.append(build("noisyown", {"dir": "up" if up else "down"}, config, mo(params, decls, eqs, whens),
                             runs, ["forth", "back"], ["n", "m"]))
    return out


def rest(tier: str) -> list:
    """Review 2's shapes (R2-B1): x stopped short of 0.5 by an event between
    an early-located crossing and the true one, at t2 = t1 - d, d = q band /
    |x'| (q 0.35 .. 2), by a time event (bcoin, bfreeze) or where a state w
    = t passes t2 (bcoinw, bfreezew):
      bcoin:   R reset to its own value there (0.625 - s a d): x = 0.5 - s q
               band, until k2 at tb lifts it;
      bfreeze: z stopped (z' = a (1 - k); with no state R = a t - k a (t -
               t2)): the same trajectory, by a rate change.
    Truth: forth [tb], back never. Where the crossing was located before t2,
    the run fires forth there and back at t2 (the event read as a change):
    located, the mirror of L1."""
    quick = tier == "quick"
    out = []
    for kind, trig in (("bcoin", "time"), ("bfreeze", "time"), ("bcoin", "w"), ("bfreeze", "w")):
        for config in IMPL:
            implicit, z_state, _, _ = CONFIGS[config]
            for up in ((True,) if quick else (True, False)):
                for strict in ((True,) if quick else (True, False)):
                    sg = "+" if up else "-"
                    r = "z" if z_state else "a * time"
                    dd = f"({r} - 0.625)"
                    if kind == "bcoin":
                        rr = f"0.625 {sg} ({dd} - k * ({dd} + e) + k2 * (0.1 + e))"
                    elif z_state:
                        rr = f"0.625 {sg} ({dd} + k2 * 0.1)"
                    else:
                        rr = f"0.625 {sg} ({dd} - k * a * (time - t2) + k2 * 0.1)"
                    params = {"a": (1.0, "1/s"), "t2": (1.0, "s"), "tb": (1.0, "s"), "e": (1e-12, "")}
                    decls = [f"Real x(start = {0.0 if up else 0.78})"] + [DISC.format(v) for v in ("n", "m", "k", "k2")]
                    eqs = []
                    if z_state:
                        decls.append("Real z(start = 0, fixed = true)")
                        eqs.append("der(z) = a * (1 - k)" if kind == "bfreeze" else "der(z) = a")
                    eqs.append(x_eq(implicit, rr))
                    fop, bop = ops(up, strict)
                    if trig == "w":
                        decls.append('Real w(unit = "s", start = 0, fixed = true)')
                        eqs.append("der(w) = 1")
                    t2c = "w > t2" if trig == "w" else "time >= t2"
                    whens = [(t2c, ["k = 1"], "k set"), ("time >= tb", ["k2 = 1"], "k2 set"),
                             (f"x {fop} 0.5", [count("n")], "forth"), (f"x {bop} 0.5", [count("m")], "back")]
                    runs = []
                    for a in ([1.0, 4.4] if quick else SHAPE_RATES):
                        for q in ([0.4, 1.0] if quick else [0.35, 0.4, 0.5, 0.7, 1.0, 1.5, 2.0]):
                            for rtol in (QUICK_RTOLS if quick else RTOLS):
                                t1 = 0.625 / a
                                d = q * band(rtol) * 1.75 / a
                                t2, tb = t1 - d, t1 + 0.5
                                e = a * d
                                L = loc(rtol, a / 1.75)
                                tr = truth({"forth": [[tb, EXACT]], "back": []}, {"n": [1, 0], "m": [0, 0]},
                                           stop=stopped(t2, q * band(rtol), a, 0.0, tb),
                                           **({"after": slowed(0.0)} if kind == "bfreeze" else {}))
                                ev = ev_tol("state" if trig == "w" else "time", rtol)
                                tr["limits"] = [{"cls": "located", "req": {"forth": [[t1, L], [tb, EXACT]],
                                                                           "back": [[t2, ev]]},
                                                 "final": {"n": [2, 0], "m": [1, 0]}}]
                                runs.append(run({"a": a, "t2": t2, "tb": tb, "e": e}, rtol, tr, {"a": a, "q": q}))
                    fam_ = kind + ("w" if trig == "w" else "")
                    out.append(build(fam_, {"dir": "up" if up else "down", "s": "s" if strict else "n"}, config,
                                     mo(params, decls, eqs, whens), runs, ["forth", "back"], ["n", "m"]))
    return out


def mixedc(tier: str) -> list:
    """Review 1's mixedc (B3): a condition that reads time and an iteration
    variable (a mixed one, checked along the dense output every step), `x >
    0.5 + c (t - t1p)`, t1p = t1: it crosses at t1, x' - c = a / 1.75 - c
    there. Truth: forth once at t1, back never."""
    quick = tier == "quick"
    out = []
    for config in CONFIGS:
        implicit, z_state, _, _ = CONFIGS[config]
        for strict in ((True,) if quick else (True, False)):
            r = "z" if z_state else "a * time"
            params = {"a": (1.0, "1/s"), "c": (0.05, "1/s"), "t1p": (1.0, "s")}
            decls = ["Real x(start = 0)"] + [DISC.format(w) for w in ("n", "m")]
            eqs = []
            if z_state:
                decls.append("Real z(start = 0, fixed = true)")
                eqs.append("der(z) = a")
            eqs.append(x_eq(implicit, r))
            fop, bop = ops(True, strict)
            th = "0.5 + c * (time - t1p)"
            whens = [(f"x {fop} {th}", [count("n")], "forth"), (f"x {bop} {th}", [count("m")], "back")]
            runs = []
            for a in ([1.0, 4.4] if quick else SHAPE_RATES):
                for rtol in (QUICK_RTOLS if quick else RTOLS):
                    t1 = 0.625 / a
                    tr = truth({"forth": [[t1, loc(rtol, a / 1.75 - 0.05)]], "back": []}, {"n": [1, 0], "m": [0, 0]})
                    runs.append(run({"a": a, "c": 0.05, "t1p": t1}, rtol, tr, {"a": a}))
            out.append(build("mixedc", {"s": "s" if strict else "n"}, config, mo(params, decls, eqs, whens), runs,
                             ["forth", "back"], ["n", "m"]))
    return out


def slow(tier: str) -> list:
    """Review 3's R3-B1: the crossing's own event slows x by K (z' = a (1 -
    c k), c = 1 - 1/K; with no state R = a t - k c a (t - tk), tk := time at
    the event), K = 10 .. 1e4:
      slow:    forth (counts n) sets k;
      slowsib: a sibling `when` on the same relation sets it (forth only
               counts);
      slowr:   forth sets k, back (counts m) clears it (fast below, slow
               above).
    In truth x reaches 0.5 at t1 and goes on past it, slower: forth [t1]
    once, back never."""
    quick = tier == "quick"
    out = []
    for kind in ("slow", "slowsib", "slowr"):
        for config in IMPL + ["cvode"]:
            implicit, z_state, _, _ = CONFIGS[config]
            for up in ((True,) if quick else (True, False)):
                for strict in ((True,) if quick else (True, False)):
                    sg = "+" if up else "-"
                    dd = "(z - 0.625)" if z_state else "(a * time - 0.625 - k * c * a * (time - tk))"
                    rr = f"0.625 {sg} {dd}"
                    params = {"a": (1.0, "1/s"), "c": (0.0, "")}
                    decls = [f"Real x(start = {0.0 if up else 0.78})"] + [DISC.format(v) for v in ("n", "m", "k")] + \
                        ['discrete Real tk(unit = "s", start = 0, fixed = true)']
                    eqs = []
                    if z_state:
                        decls.append("Real z(start = 0, fixed = true)")
                        eqs.append("der(z) = a * (1 - c * k)")
                    eqs.append(x_eq(implicit, rr))
                    fop, bop = ops(up, strict)
                    if kind == "slowsib":
                        whens = [(f"x {fop} 0.5", [count("n")], "forth"), (f"x {fop} 0.5", ["k = 1", "tk = time"], "slows"),
                                 (f"x {bop} 0.5", [count("m")], "back")]
                    else:
                        whens = [(f"x {fop} 0.5", [count("n"), "k = 1", "tk = time"], "forth"),
                                 (f"x {bop} 0.5", [count("m")] + (["k = 0"] if kind == "slowr" else []), "back")]
                    runs = []
                    for a in ([0.5, 2.3] if quick else SHAPE_RATES):
                        for K in ([100.0, 1e4] if quick else [10.0, 100.0, 1000.0, 1e4]):
                            for rtol in (QUICK_RTOLS if quick else RTOLS):
                                t1 = 0.625 / a
                                aft = slowed(a / K, rb=a if kind == "slowr" else None)
                                tr = truth({"forth": [[t1, loc(rtol, a / 1.75)]], "back": []}, {"n": [1, 0], "m": [0, 0]},
                                           after=aft)
                                runs.append(run({"a": a, "c": 1.0 - 1.0 / K}, rtol, tr, {"a": a, "K": K}))
                    out.append(build(kind, {"dir": "up" if up else "down", "s": "s" if strict else "n"}, config,
                                     mo(params, decls, eqs, whens), runs, ["forth", "back"], ["n", "m"]))
    return out


def slowed(r, rb=None, D=None, t2=None) -> dict:
    """How the events of a shape change x's rate, for the scorer (the
    truth's `after`; also where restart loss is not applied): R's rate
    right after the crossing's own event `r` (x's is r / 1.75; 0 where an
    event stops x); `rb` R's rate after a back at that instant (slowr, a
    mode on the relation: the fast one); `D` the headroom R tends to above
    its value at the event, at rate r / D (asym); `t2` an event that stops
    R (slowstop, slowfrz)."""
    out = {"r": r}
    for k, v in (("rb", rb), ("D", D), ("t2", t2)):
        if v is not None:
            out[k] = v
    return out


def stopped(t, gap, a, creep, lift) -> dict:
    """An event stops x short of its threshold, for the scorer (the truth's
    `stop`; re-check 5's RC5-B1): at `t` x is `gap` short of 0.5 (in x),
    having moved at a / 1.75 before it; after it x moves at `creep` (x's
    rate, toward the crossing's side where positive) and stays short until
    the event at `lift` moves it on."""
    return {"t": t, "gap": gap, "speed": a / 1.75, "creep": creep, "lift": lift}


SIBS = {"two": "2 * x {op} 1", "par": "x {op} p", "sub": "x - 0.5 {op} 0", "flip": "0.5 {rop} x"}
FLIP = {">": "<", "<": ">", ">=": "<=", "<=": ">="}


def recheck4(tier: str) -> list:
    """Re-check 4's shapes (RC4-B1, RC4-B2), each built from review 3's slow
    (z' = a (1 - c k), c = 1 - 1/K; with no state R = a t - k c a (t - tk))
    with the hand-worked truth forth [t1] once, back never:
      sibeq:    slowsib with the slowing `when` written another way on an
                equal relation (2 x > 1, x > p with p = 0.5, x - 0.5 > 0,
                0.5 < x);
      slowbp:   slow with back written x < p or 2 x < 1;
      slowobs:  slow beside a passive `when` on an equal relation that only
                counts (o, once at t1);
      slowmode: a mode on the relation slows x (z' = if x > 0.5 then a / K
                else a);
      asym:     the crossing's own `when` sets k and freezes zs := z; z then
                tends to zs + D at rate c (z' = c (zs + D - z)), D / 1.75 = q
                rtol, q 0.25 .. 0.6: x tends to 0.5 + q rtol;
      slowfrz:  slow, and a time event at t2 = t1 + q rtol 1.75 K / a that
                stops z: x frozen at 0.5 + q rtol;
      slowstop: slow, and a time event that stops z half-way to the run's own
                slowed arrival, t2 = tf + K (t1 - tf) / 2 (tf where a first
                run, t2 never, located the crossing: `holds_tf.json`; early
                runs only): x frozen above 0.5 in truth, short of it in the
                run's located truth."""
    quick = tier == "quick"
    rates, rtols = ([1.0, 4.4], QUICK_RTOLS) if quick else (SHAPE_RATES, RTOLS)
    Ks = [100.0, 1e4] if quick else [10.0, 100.0, 1000.0, 1e4]
    dirs = [(True, True)] if quick else [(u, s) for u in (True, False) for s in (True, False)]
    tfs = json.loads(TF.read_text())
    out = []

    def model(config, up, strict, slow_by, acts, whens_more, extra_params=(), extra_decls=(), dd=None, rate=None):
        implicit, z_state, _, _ = CONFIGS[config]
        sg = "+" if up else "-"
        if dd is None:
            dd = "(z - 0.625)" if z_state else "(a * time - 0.625 - k * c * a * (time - tk))"
        params = {"a": (1.0, "1/s"), "c": (0.0, ""), **dict(extra_params)}
        decls = [f"Real x(start = {0.0 if up else 0.78})"] + [DISC.format(v) for v in ("n", "m", "k")] + \
            ['discrete Real tk(unit = "s", start = 0, fixed = true)'] + list(extra_decls)
        eqs = []
        if z_state:
            decls.append("Real z(start = 0, fixed = true)")
            eqs.append(f"der(z) = {rate or 'a * (1 - c * k)'}")
        eqs.append(x_eq(implicit, f"0.625 {sg} {dd}"))
        fop, bop = ops(up, strict)
        whens = [(f"x {fop} 0.5", [count("n")] + (acts if slow_by == "forth" else []), "forth")]
        whens += [(f"x {bop} 0.5", [count("m")], "back")] + whens_more(fop, bop)
        return params, decls, eqs, whens, z_state

    def tr1(a, rtol, after, **fin):
        return truth({"forth": [[0.625 / a, loc(rtol, a / 1.75)]], "back": []}, {"n": [1, 0], "m": [0, 0], **fin},
                     after=after)

    slowing = ["k = 1", "tk = time"]
    # sibeq, slowbp, slowobs: RC4-B1's equal relations written otherwise
    for form, pat in SIBS.items():
        for config in IMPL + ["cvode"]:
            for up in ((True,) if quick else (True, False)):
                fop0 = ops(up, True)[0]
                sib = pat.format(op=fop0, rop=FLIP[fop0])
                p, dcl, eqs, whens, _ = model(config, up, True, "sib", [],
                                              lambda f, b: [(sib, slowing, "slows")], {"p": (0.5, "")}.items())
                runs = [run({"a": a, "c": 1.0 - 1.0 / K}, rtol, tr1(a, rtol, slowed(a / K)), {"a": a, "K": K})
                        for a in rates for K in Ks for rtol in rtols]
                out.append(build("sibeq", {"form": form, "dir": "up" if up else "down"}, config,
                                 mo(p, dcl, eqs, whens), runs, ["forth", "back"], ["n", "m"]))
    for form, pat in (("par", "x {op} p"), ("two", "2 * x {op} 1")):
        for config in IMPL + ["cvode"]:
            for up in ((True,) if quick else (True, False)):
                bop0 = ops(up, True)[1]
                p, dcl, eqs, whens, _ = model(config, up, True, "forth", slowing, lambda f, b: [],
                                              {"p": (0.5, "")}.items())
                whens[1] = (pat.format(op=bop0), [count("m")], "back")
                runs = [run({"a": a, "c": 1.0 - 1.0 / K}, rtol, tr1(a, rtol, slowed(a / K)), {"a": a, "K": K})
                        for a in rates for K in Ks for rtol in rtols]
                out.append(build("slowbp", {"form": form, "dir": "up" if up else "down"}, config,
                                 mo(p, dcl, eqs, whens), runs, ["forth", "back"], ["n", "m"]))
    for form, pat in (("two", "2 * x > 1"), ("par", "x > p")):
        for config in IMPL + ["cvode"]:
            p, dcl, eqs, whens, _ = model(config, True, True, "forth", slowing,
                                          lambda f, b: [(pat, [count("o")], "observer")], {"p": (0.5, "")}.items(),
                                          [DISC.format("o")])
            runs = [run({"a": a, "c": 1.0 - 1.0 / K}, rtol, tr1(a, rtol, slowed(a / K), o=[1, 0]), {"a": a, "K": K})
                    for a in rates for K in Ks for rtol in rtols]
            out.append(build("slowobs", {"form": form}, config, mo(p, dcl, eqs, whens), runs, ["forth", "back"],
                             ["n", "m", "o"]))
    # slowmode: a mode on the relation slows x (its state z only)
    for config in ["ida", "ida-implicit", "diffsol", "cvode"]:
        for up, strict in dirs:
            fop = ops(up, strict)[0]
            p, dcl, eqs, whens, _ = model(config, up, strict, "none", [], lambda f, b: [], {"K": (1.0, "")}.items(),
                                          rate=f"if x {fop} 0.5 then a / K else a")
            # (a mode on the condition's relation: the storm beside it is a named limit, as in ramp and fam)
            runs = [run({"a": a, "K": K}, rtol, dict(tr1(a, rtol, slowed(a / K, rb=a)),
                                                     storm=[[0.625 / a, loc(rtol, a / 1.75)]]), {"a": a, "K": K})
                    for a in rates for K in Ks for rtol in rtols]
            out.append(build("slowmode", {"dir": "up" if up else "down", "s": "s" if strict else "n"}, config,
                             mo(p, dcl, eqs, whens), runs, ["forth", "back"], ["n", "m"]))
    # asym: the crossing's own event makes x tend to 0.5 + q rtol
    for config in IMPL + ["cvode"]:
        z_state = CONFIGS[config][1]
        dd = "(z - 0.625)" if z_state else \
            "(a * time * (1 - k) + k * (a * tk + D * (1 - exp(-c * (time - tk)))) - 0.625)"
        for up, strict in dirs:
            p, dcl, eqs, whens, _ = model(config, up, strict, "forth", slowing + (["zs = z"] if z_state else []),
                                          lambda f, b: [], {"c": (1.0, "1/s"), "D": (0.0, "")}.items(),
                                          [DISC.format("zs")], dd=dd,
                                          rate="a * (1 - k) + k * c * (zs + D - z)")
            runs = []
            for a in rates:
                for q in ([0.4] if quick else [0.25, 0.4, 0.6]):
                    for cc in ([500.0] if quick else [50.0, 500.0]):
                        for rtol in rtols:
                            D = 1.75 * q * rtol
                            runs.append(run({"a": a, "c": cc, "D": D}, rtol, tr1(a, rtol, slowed(cc * D, D=D)),
                                            {"a": a, "q": q, "c": cc}))
            out.append(build("asym", {"dir": "up" if up else "down", "s": "s" if strict else "n"}, config,
                             mo(p, dcl, eqs, whens), runs, ["forth", "back"], ["n", "m"], t_end=3.0))
    # slowfrz and slowstop: slow, then a time event stops z
    for fam_ in ("slowfrz", "slowstop"):
        for config in IMPL + (["cvode"] if fam_ == "slowfrz" else []):
            z_state = CONFIGS[config][1]
            dd = "(z - 0.625)" if z_state else "((1 - k2) * (a * time - k * c * a * (time - tk)) + k2 * rf - 0.625)"
            stop = ["k2 = 1"] if z_state else ["k2 = 1", "rf = a * time - k * c * a * (time - tk)"]
            for up, strict in dirs:
                d_ = f"{'up' if up else 'down'}-{'s' if strict else 'n'}"
                p, dcl, eqs, whens, _ = model(config, up, strict, "forth", slowing,
                                              lambda f, b: [("time >= t2", stop, "stop")], {"t2": (1.0, "s")}.items(),
                                              [DISC.format("k2"), DISC.format("rf")], dd=dd,
                                              rate="a * (1 - c * k) * (1 - k2)")
                runs = []
                for a in rates:
                    for K in ([1e3, 1e4] if not quick else [1e4]):
                        for rtol in rtols:
                            t1 = 0.625 / a
                            if fam_ == "slowfrz":
                                for q in ([0.4] if quick else [0.25, 0.4, 0.6]):
                                    t2 = t1 + q * rtol * 1.75 * K / a
                                    runs.append(run({"a": a, "c": 1.0 - 1.0 / K, "t2": t2}, rtol,
                                                    tr1(a, rtol, slowed(a / K, t2=t2)), {"a": a, "K": K, "q": q}))
                                continue
                            tf = tfs.get(f"slowstop|{config}|{d_}|{a!r}|{K!r}|{rtol!r}")
                            if tf is None or tf >= t1:
                                continue
                            t2 = tf + 0.5 * K * (t1 - tf)
                            runs.append(run({"a": a, "c": 1.0 - 1.0 / K, "t2": t2}, rtol,
                                            tr1(a, rtol, slowed(a / K, t2=t2)), {"a": a, "K": K}))
                if runs:
                    out.append(build(fam_, {"dir": "up" if up else "down", "s": "s" if strict else "n"}, config,
                                     mo(p, dcl, eqs, whens), runs, ["forth", "back"], ["n", "m"], t_end=3.0))
    return out


def recheck5(tier: str) -> list:
    """Re-check 5's shapes (RC5-B1, RC5-B2; tmp/rc5/probe.py), each with its
    hand-worked truth:
      f2:  review 2's bfreeze / bfreezew (x stopped short of 0.5 at t2 = t1 -
           d, d = q band / |x'|, by a time event or where a state w = t
           passes t2; lifted at tb = t1 + 0.5 by R + 0.1), and a second event
           at t3 = t2 + f L that changes nothing x reads (j := 1: a time
           event, or a state w3 = t passing t3), f 1e-3, 0.03, 0.1. Truth: x
           frozen short until tb: forth [tb] once, back never. Located (the
           crossing located before t2, the mirror): forth tf, back t2, forth
           tb;
      f3:  f2 with a creep after the stop, R' = eps = +-1e-9 /s (x' = eps /
           1.75), the second event also a tick at t3, q 0.35 .. 2; rtol 1e-4 ..
           1e-8 only: x creeps by at most 1e-9 0.5 / 1.75 = 2.9e-10 before tb,
           at most 0.16 of its shortfall (q 0.35 at rtol 1e-8; at rtol 1e-9, q
           0.35 or 0.5, it would cross before tb). f2's truth;
      tev: slow (forth slows x by K, K 100 and 1e4) beside an unrelated time
           event at te = t1 + f L (j := 1), f -0.3 .. 0.01. Truth: forth [t1]
           once, back never;
      s2:  a later event slows x, the mirror: z' = a (1 - c k), c = 1 - 1/K,
           k := 1 at time t2 or where w = t passes t2, t2 = t1 - d. Truth: x
           short at t2 by q band, then slowed: forth once at t1' = t2 + K d
           (at the slowed rate's tolerance), back never; where t1' is past the
           run's end (3 s; a = 0.5, K 1e4, rtol 1e-4, q 1 and 2), no firing,
           or forth once within that tolerance of t1' where it reaches back
           before the end. Located: forth tf, back t2, forth t1' (where it
           comes)."""
    quick = tier == "quick"
    out = []
    # f2 and f3: x stopped short, then an event that changes nothing x reads
    for fam_ in ("f2", "f3"):
        creep = fam_ == "f3"
        for frz in ("time", "w"):
            for sec in (("time", "state", "tick") if creep else ("time", "state")):
                for config in IMPL:
                    implicit, z_state, _, _ = CONFIGS[config]
                    for up in ((True,) if quick else (True, False)):
                        sg = "+" if up else "-"
                        rr = f"0.625 {sg} ((z - 0.625) + k2 * 0.1)" if z_state else \
                            f"0.625 {sg} ((a * time - 0.625) - k * (a - eps) * (time - t2) + k2 * 0.1)"
                        params = {"a": (1.0, "1/s"), "t2": (1.0, "s"), "t3": (1.0, "s"), "tb": (1.0, "s"),
                                  "eps": (1e-30, "1/s"), "never": (1e3, "s")}
                        decls = [f"Real x(start = {0.0 if up else 0.78})"] + \
                            [DISC.format(v) for v in ("n", "m", "k", "k2", "j")]
                        eqs = []
                        if z_state:
                            decls.append("Real z(start = 0, fixed = true)")
                            eqs.append("der(z) = a * (1 - k) + k * eps")
                        eqs.append(x_eq(implicit, rr))
                        if frz == "w":
                            decls.append('Real w(unit = "s", start = 0, fixed = true)')
                            eqs.append("der(w) = 1")
                        if sec == "state":
                            decls.append('Real w3(unit = "s", start = 0, fixed = true)')
                            eqs.append("der(w3) = 1")
                        fop, bop = ops(up, True)
                        whens = [("w > t2" if frz == "w" else "time >= t2", ["k = 1"], "k set"),
                                 ("time >= tb", ["k2 = 1"], "k2 set"),
                                 ({"state": "w3 > t3", "time": "time >= t3", "tick": "time >= never"}[sec], ["j = 1"],
                                  "j set"),
                                 (f"x {fop} 0.5", [count("n")], "forth"), (f"x {bop} 0.5", [count("m")], "back")]
                        if quick:
                            grid = ([0.5, 2.3], [0.35, 1.0], [0.03], [1e-5, 1e-8]) if creep else \
                                ([2.3], [0.5], [1e-3, 0.1], QUICK_RTOLS)
                        else:
                            grid = (RATES5, [0.35, 0.5, 1.0, 2.0], [1e-3, 0.03, 0.1], RTOLS[:5]) if creep else \
                                (RATES5, [0.5, 1.0, 2.0], [1e-3, 0.03, 0.1], RTOLS)
                        runs = []
                        for a in grid[0]:
                            for q in grid[1]:
                                for f in grid[2]:
                                    for rtol in grid[3]:
                                        t1 = 0.625 / a
                                        L = loc(rtol, a / 1.75)
                                        d = q * band(rtol) * 1.75 / a
                                        t2 = t1 - d
                                        t3 = t2 + f * L
                                        tb = t1 + 0.5
                                        ev = ev_tol("state" if frz == "w" else "time", rtol)
                                        for eps in ([1e-9, -1e-9] if creep else [0.0]):
                                            tr = truth({"forth": [[tb, EXACT]], "back": []}, {"n": [1, 0], "m": [0, 0]},
                                                       after=slowed(0.0),
                                                       stop=stopped(t2, q * band(rtol), a, eps / 1.75, tb))
                                            tr["limits"] = [{"cls": "located",
                                                             "req": {"forth": [[t1, L], [tb, EXACT]], "back": [[t2, ev]]},
                                                             "final": {"n": [2, 0], "m": [1, 0]}}]
                                            runs.append(run({"a": a, "t2": t2, "t3": t3, "tb": tb, "eps": eps}, rtol, tr,
                                                            {"a": a, "q": q, "f": f, "eps": eps},
                                                            ticks=[t3] if sec == "tick" else []))
                        out.append(build(fam_, {"frz": frz, "sec": sec, "dir": "up" if up else "down"}, config,
                                         mo(params, decls, eqs, whens), runs, ["forth", "back"], ["n", "m"],
                                         tick_vars=["j"] if sec == "tick" else []))
    # tev: slow beside an unrelated time event
    for config in IMPL + ["cvode"]:
        implicit, z_state, _, _ = CONFIGS[config]
        for up in ((True,) if quick else (True, False)):
            sg = "+" if up else "-"
            dd = "(z - 0.625)" if z_state else "(a * time - 0.625 - k * c * a * (time - tk))"
            params = {"a": (1.0, "1/s"), "c": (0.0, ""), "te": (1.0, "s")}
            decls = [f"Real x(start = {0.0 if up else 0.78})"] + [DISC.format(v) for v in ("n", "m", "k", "j")] + \
                ['discrete Real tk(unit = "s", start = 0, fixed = true)']
            eqs = []
            if z_state:
                decls.append("Real z(start = 0, fixed = true)")
                eqs.append("der(z) = a * (1 - c * k)")
            eqs.append(x_eq(implicit, f"0.625 {sg} {dd}"))
            fop, bop = ops(up, True)
            whens = [("time >= te", ["j = 1"], "j set"),
                     (f"x {fop} 0.5", [count("n"), "k = 1", "tk = time"], "forth"),
                     (f"x {bop} 0.5", [count("m")], "back")]
            runs = []
            for a in ([1.0, 4.4] if quick else RATES5):
                for K in (100.0, 1e4):
                    for f in ([-0.03, -0.01, 0.0] if quick else [-0.3, -0.1, -0.03, -0.01, 0.0, 0.01]):
                        for rtol in (QUICK_RTOLS if quick else RTOLS):
                            t1 = 0.625 / a
                            tr = truth({"forth": [[t1, loc(rtol, a / 1.75)]], "back": []}, {"n": [1, 0], "m": [0, 0]},
                                       after=slowed(a / K))
                            runs.append(run({"a": a, "c": 1.0 - 1.0 / K, "te": t1 + f * loc(rtol, a / 1.75)}, rtol,
                                            tr, {"a": a, "K": K, "f": f}))
            out.append(build("tev", {"dir": "up" if up else "down"}, config, mo(params, decls, eqs, whens),
                             runs, ["forth", "back"], ["n", "m"], t_end=3.0))
    # s2: a later event slows x (the mirror)
    for trig in ("w", "time"):
        for config in IMPL + ["cvode"]:
            implicit, z_state, _, _ = CONFIGS[config]
            for up in ((True,) if quick else (True, False)):
                sg = "+" if up else "-"
                dd = "(z - 0.625)" if z_state else "(a * time - 0.625 - k * c * a * (time - t2))"
                params = {"a": (1.0, "1/s"), "c": (0.0, ""), "t2": (1.0, "s")}
                decls = [f"Real x(start = {0.0 if up else 0.78})"] + [DISC.format(v) for v in ("n", "m", "k")]
                eqs = []
                if z_state:
                    decls.append("Real z(start = 0, fixed = true)")
                    eqs.append("der(z) = a * (1 - c * k)")
                eqs.append(x_eq(implicit, f"0.625 {sg} {dd}"))
                if trig == "w":
                    decls.append('Real w(unit = "s", start = 0, fixed = true)')
                    eqs.append("der(w) = 1")
                fop, bop = ops(up, True)
                whens = [("w > t2" if trig == "w" else "time >= t2", ["k = 1"], "k set"),
                         (f"x {fop} 0.5", [count("n")], "forth"), (f"x {bop} 0.5", [count("m")], "back")]
                runs = []
                for a in ([4.4] if quick else RATES5):
                    for q in ([0.5, 2.0] if quick else [0.5, 1.0, 2.0]):
                        for K in (100.0, 1e4):
                            for rtol in (QUICK_RTOLS if quick else RTOLS):
                                t1 = 0.625 / a
                                L = loc(rtol, a / 1.75)
                                d = q * band(rtol) * 1.75 / a
                                t2 = t1 - d
                                t1p = t2 + K * d
                                Ls = loc(rtol, a / K / 1.75)
                                ev = ev_tol("state" if trig == "w" else "time", rtol)
                                late = [{"req": {"forth": [[t1p, Ls]], "back": []}, "final": {"n": [1, 0], "m": [0, 0]}}]
                                if t1p <= 3.0:
                                    tr = truth({"forth": [[t1p, Ls]], "back": []}, {"n": [1, 0], "m": [0, 0]},
                                               after=slowed(0.0))
                                    lim = [([[t1, L], [t1p, Ls]], 2)]
                                else:
                                    # past the run's end: no firing, or one located within Ls before the end
                                    tr = truth({"forth": [], "back": []}, {"n": [0, 0], "m": [0, 0]}, after=slowed(0.0),
                                               either=late if t1p - Ls <= 3.0 else [])
                                    lim = [([[t1, L]], 1)] + ([([[t1, L], [t1p, Ls]], 2)] if t1p - Ls <= 3.0 else [])
                                tr["limits"] = [{"cls": "located", "req": {"forth": fl, "back": [[t2, ev]]},
                                                 "final": {"n": [nf, 0], "m": [1, 0]}} for fl, nf in lim]
                                runs.append(run({"a": a, "c": 1.0 - 1.0 / K, "t2": t2}, rtol, tr, {"a": a, "q": q, "K": K}))
                out.append(build("s2", {"trig": trig, "dir": "up" if up else "down"}, config,
                                 mo(params, decls, eqs, whens), runs, ["forth", "back"], ["n", "m"], t_end=3.0))
    return out


RATES5 = [0.5, 1.0, 2.3, 4.4, 7.0]   # re-check 5's rates

FAMILIES = {"fam": fam, "own": own, "ramp": ramp, "peak": peak, "moved": moved, "stale": stale,
            "count": count_cases, "extra": extra, "coin": coin, "before": before, "rdmode": rdmode,
            "noisy": noisy, "noisyown": noisyown, "rest": rest, "mixedc": mixedc, "slow": slow,
            "recheck4": recheck4, "recheck5": recheck5}


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


# ---------------------------------------------------------------------------
# The named limits the hold rule's own reading needs (the hold spec, 4.9 and
# 4.10): a run `classify` finds wrong is tried against these, in order
# ---------------------------------------------------------------------------

def windows(r) -> list:
    """The truth's crossing windows: [tc - L, tc + L] around the crossing
    t1 = 0.625 / a (where the shape has a) and around every required firing
    with a tolerance of at least 1e-9 s (a located one; a scheduled instant
    gets none)."""
    w = []
    p, rtol = r["params"], r["rtol"]
    if "a" in p:
        a = p["a"]
        L = loc(rtol, a / 1.75)
        w.append((0.625 / a - L, 0.625 / a + L))
    for req in r["truth"]["req"].values():
        for t, tol in req:
            if tol >= 1e-9:
                w.append((t - tol, t + tol))
    return w


def located_truth(b, r, res):
    """The one located definition (the hold spec, 4.9), computed where a
    shape's truth depends on it: the truth redone with the crossing at the
    instant tf the run fired the crossing's own `when` (|tf - t1| <= L),
    each event's new values from the exact values just before it. `None`
    where it is the truth or does not apply.
      extra fwd, frzf (x0(tf) + fx short of 0.5: back at tf; fwd forth again
        at t1 - E / a), frzb (late: x0(tf) - ex still past 0.5, frozen: forth
        once);
      before (the mirror: the crossing located before t2, forth there, back
        at t2, forth at tb)."""
    fam_ = b["family"]
    if not res or not res.get("ok"):
        return None
    p, rtol = r["params"], r["rtol"]
    f = fired_by(res, ["forth"])["forth"]
    if not f or "a" not in p:
        return None
    a = p["a"]
    t1 = 0.625 / a
    L = loc(rtol, a / 1.75)
    tf = f[0]
    if abs(tf - t1) > L:
        return None
    if fam_ == "extra":
        kind = b["shape"]["kind"]
        E = 1.75 * p["ex"]
        if kind in ("fwd", "frzf"):
            if a * (tf - t1) + E > 0:
                return None
            if kind == "fwd":
                return truth({"forth": [[tf, EXACT], [t1 - E / a, L]], "back": [[tf, EXACT]]},
                             {"n": [2, 0], "m": [1, 0]})
            return truth({"forth": [[tf, EXACT]], "back": [[tf, EXACT]]}, {"n": [1, 0], "m": [1, 0]})
        if kind == "frzb" and a * (tf - t1) - E >= 0:
            return truth({"forth": [[tf, EXACT]], "back": []}, {"n": [1, 0], "m": [0, 0]})
        return None
    if fam_ == "before" and tf <= p["t2"] + EXACT:
        return truth({"forth": [[tf, EXACT], [p["tb"], EXACT]], "back": [[p["t2"], EXACT]]},
                     {"n": [2, 0], "m": [1, 0]})
    aft = r["truth"].get("after")
    if aft and aft["r"] > 0 and tf < t1:
        # the crossing's own event at tf slows R (`slowed`): x0(tf) is short,
        # so back fires there; x arrives where R, from its value at tf at the
        # rate after (after a back, `rb`), reaches 0.625, unless it tends to
        # a level short of it (asym) or an event stops it first
        short, ra = a * (tf - t1) * -1.0, aft.get("rb", aft["r"])
        if "D" in aft:
            ta = tf - math.log(1.0 - short / aft["D"]) * aft["D"] / ra if short < aft["D"] else None
        else:
            ta = tf + short / ra
            if "t2" in aft and ta > aft["t2"]:
                ta = None
        forth = [[tf, EXACT]] + ([[ta, loc(rtol, ra / 1.75)]] if ta is not None else [])
        fin = dict(r["truth"]["final"], n=[len(forth), 0], m=[1, 0])
        if "o" in fin:
            fin["o"] = [len(forth), 0]
        return truth({"forth": forth, "back": [[tf, EXACT]]}, fin)
    return None


def in_windows(spec, r) -> bool:
    """Every firing `spec` requires is one the truth requires (at its
    instant, within its tolerance) or lies inside one of the truth's
    windows (review 3's S3: a located truth counts only where it moves
    firings within the location's own scale)."""
    ws = windows(r)
    req = r["truth"]["req"]
    return all(any(lo <= t <= hi for lo, hi in ws) or any(abs(t - q[0]) <= q[1] for q in req.get(lab, []))
               for lab, want in spec["req"].items() for t, _ in want)


def bounded(b, r, res) -> bool:
    """The time limit's bound (the hold spec, 4.9, as review 3's S2
    tightened it): the run did not stop; outside the truth's windows each
    counted `when` fires exactly as the truth requires; inside each window
    an extra or a lost pair at most (where the shape counts back, forth's
    and back's differences from the truth are equal, so the relation ends
    the window as the truth's does; with forth alone, one extra firing at
    most); the counters equal the firings. A lone firing lost for good is
    not bounded."""
    if not res or not res.get("ok"):
        return False
    labels = b["labels"]
    fired = fired_by(res, labels)
    ws = windows(r)
    req = r["truth"]["req"]

    def inw(t):
        return [i for i, (lo, hi) in enumerate(ws) if lo <= t <= hi]

    for lab in labels:
        want = req.get(lab, [])
        out_req = [q for q in want if not inw(q[0])]
        out_got = [t for t in fired[lab] if not inw(t)]
        if len(out_req) != len(out_got) or any(abs(t - q[0]) > q[1] for t, q in zip(out_got, out_req)):
            return False
    pair = len(labels) >= 2 and labels[1] in ("back", "it counts down")
    for i in range(len(ws)):
        diff = [sum(1 for t in fired[lab] if i in inw(t)) - sum(1 for q in req.get(lab, []) if i in inw(q[0]))
                for lab in labels[:2]]
        if pair:
            if diff[0] != diff[1] or abs(diff[0]) > 1:
                return False
        elif diff and diff[0] not in (0, 1):
            return False
    fin = res.get("final", {})
    cnt = dict(zip(["n", "m"], labels))
    for v, (want, tol) in r["truth"].get("final", {}).items():
        g = fin.get(v)
        if v in cnt:
            if g != len(fired[cnt[v]]):
                return False
        elif want != "count" and (g is None or abs(g - want) > tol):
            return False
    return True


def moved_back(b, r, res, lt) -> bool:
    """A lone back moved by the time limit (review 3's S2: frzf): the run is
    the located truth (inside the truth's windows) with its back firings up
    to W / |x'| later (W = 8 bands, the hold's width)."""
    if lt is None or "a" not in r["params"] or not in_windows(lt, r):
        return False
    w = 8 * band(r["rtol"]) * 1.75 / r["params"]["a"]
    spec = dict(lt, req={k: ([[t + w / 2, w / 2 + EXACT] for t, _ in v] if k == "back" else v)
                         for k, v in lt["req"].items()})
    return matches(res, spec, b["labels"])


def postlimit(b, r, res) -> bool:
    """The time limit of a hold the crossing made, at x's speed after the
    latest event (the hold spec's revision 4, variant A): in a shape whose
    events slow x (the truth's `after`), B = W / |x'|, W = 8 bands and x' the
    truth's rate after the crossing's own event (where a later event stops
    x, the rate before that event: the limit there is revision 2's, from it).
    The run is the located truth (4.9: the crossing at the instant tf the
    run fired it, x0(tf) read from there on, x at the rate after the event)
    with each firing up to B later than the latest event from its located
    instant on (that instant, or one of the run's other events in between:
    a stop at t2); and every firing of that located truth lies no more than
    B after the crossing's window or the latest other event before it.
    Anything beyond it is wrong."""
    aft = r["truth"].get("after")
    if not aft or aft["r"] <= 0 or not res or not res.get("ok") or "a" not in r["params"]:
        return False
    lt = located_truth(b, r, res)
    if lt is None:
        return False
    a, rtol = r["params"]["a"], r["rtol"]
    t1, L = 0.625 / a, loc(rtol, a / 1.75)
    B = 8 * band(rtol) * 1.75 / aft["r"]
    labels = b["labels"]
    evs = sorted(t for t, lab in res.get("fired", []) if not any(lab.endswith(x) for x in labels))
    if any(q > max([t1 + L] + [e for e in evs if e <= q]) + B for v in lt["req"].values() for q, _ in v):
        return False
    fired = fired_by(res, labels)
    for lab in labels:
        want, got = lt["req"].get(lab, []), fired[lab]
        if len(got) != len(want):
            return False
        for t, (q, tol) in zip(got, want):
            anchor = max([q] + [e for e in evs if q <= e < t])
            if not q - tol <= t <= max(q + tol, anchor + B):
                return False
    cnt = dict(zip(["n", "m"], labels))
    for v, (want, tol) in lt["final"].items():
        g = res.get("final", {}).get(v)
        if g is None or (g != len(fired[cnt[v]]) if v in cnt else abs(g - want) > tol):
            return False
    return True


def shallow_peak(b, r, res) -> bool:
    """A missed shallow peak (the hold spec, 4.10; no hold involved): a peak
    whose height above the threshold, d / 1.75 in x, is within 100 rtol, and
    the run fires none of its crossings (every counter 0): the error test
    lets the integrators step over a peak so shallow (the twin with a short
    largest step need not catch it either)."""
    if b["family"] != "peak" or not res or not res.get("ok"):
        return False
    if b["shape"]["d"] / 1.75 > 100 * r["rtol"]:
        return False
    if any(fired_by(res, b["labels"]).values()):
        return False
    return all(res.get("final", {}).get(v, 0) == 0 for v in ("n", "m", "on"))


def restart_loss(b, r, res, cls_of_key) -> bool:
    """Diffsol right after an event's restart (the hold spec, 4.10; no hold
    involved, out of scope by the brief): the run is right, or inside L1,
    noise, located or bounded, but for one firing of a counted `when` lost
    right after an event (an instant where the run fired, or the shape's
    t2): with that firing put back just after the event, it is. Not a
    firing the truth ties to another's instant (event iteration's, at the
    event itself: `same`), nor one at a scheduled instant: those are not
    crossings the integrator finds after a restart. Only in shapes whose
    events leave x's rate as it was (a crossing x makes after the event
    then comes within the location's scale of it): not where an event
    slows or stops x (its truth says how: `after`, [`slowed`]), where a
    lost firing may be a late one."""
    if b["config"] != "diffsol" or not res or not res.get("ok"):
        return False
    if "after" in r["truth"]:
        return False
    tied = {x for la, _, lb, _ in r["truth"].get("same", []) for x in (la, lb)}
    labels = b["labels"]
    insts = {t for t, _ in res.get("fired", [])}
    if "t2" in r["params"]:
        insts.add(r["params"]["t2"])
    counters = dict(zip(labels, ["n", "m"]))
    for te in sorted(insts):
        for lab in labels:
            if lab in tied:
                continue
            fin = dict(res.get("final", {}))
            v = counters.get(lab)
            if v in fin and fin[v] is not None:
                fin[v] = fin[v] + 1
            fired = sorted(res.get("fired", []) + [[te + 1e-13, lab]], key=lambda x: x[0])
            put = dict(res, fired=fired, final=fin)
            # (the firing put back answers a located requirement, not a
            # scheduled one: its tolerance is at least 1e-9 s)
            if not any(abs(te - q[0]) <= q[1] and q[1] >= 1e-9 for q in r["truth"]["req"].get(lab, [])) and \
                    not any(lo <= te <= hi for lo, hi in windows(r)):
                continue
            c = classify(put, r["truth"], labels, cls_of_key)
            if c in ("right", "L1", "noise", "located") or bounded(b, r, put):
                return True
    return False


def classify_run(b, r, res, cls_of_key) -> str:
    """A run's class: `classify`'s (right, or a limit its truth names: L1,
    noise, onestep, located, storm), else, in order, located (the one
    located definition, its firings inside the truth's windows), moderead,
    shallowpeak, bounded (with a lone back moved by the time limit),
    postlimit, restartloss, else wrong."""
    c = classify(res, r["truth"], b["labels"], cls_of_key)
    if c != "wrong":
        return c
    lt = located_truth(b, r, res)
    if lt is not None and in_windows(lt, r) and matches(res, lt, b["labels"]):
        return "located"
    if mode_read(b, r, res):
        return "moderead"
    if shallow_peak(b, r, res):
        return "shallowpeak"
    if bounded(b, r, res) or moved_back(b, r, res, lt):
        return "bounded"
    if postlimit(b, r, res):
        return "postlimit"
    if restart_loss(b, r, res, cls_of_key):
        return "restartloss"
    return "wrong"


def mode_read(b, r, res) -> bool:
    """Mode read (the hold spec, S1): the crossing was located early (its
    `when` fired before t1), and the run is the truth in every firing and
    value but m, the value a later round of that event read from the mode on
    the same relation."""
    if not res or not res.get("ok") or "m" not in r["truth"]["final"] or "a" not in r["params"]:
        return False
    f = fired_by(res, ["forth"])["forth"]
    if not f or f[0] >= 0.625 / r["params"]["a"]:
        return False
    want = r["truth"]["final"]["m"]
    got = res.get("final", {}).get("m")
    rest = dict(r["truth"], final={k: v for k, v in r["truth"]["final"].items() if k != "m"})
    return got is not None and abs(got - want[0]) > want[1] and matches(res, rest, b["labels"])


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
        c = classify_run(b, r, res.get((bid, i)), lambda tk: of(bid, tk))
        cls[(bid, k)] = c
        return c

    for (bid, k) in keyed:
        of(bid, k)
    return cls, keyed


CLASSES = ["right", "L1", "located", "moderead", "noise", "onestep", "storm", "shallowpeak", "restartloss",
           "bounded", "postlimit", "wrong", "missing"]
SHOWN = ["wrong", "bounded", "postlimit", "L1", "located", "moderead", "noise", "onestep", "storm", "shallowpeak",
         "restartloss", "right"]


def sub(b) -> str:
    """A build's shape family: its family and main shape parameter."""
    if b["family"] == "count":
        return "count:" + re.sub(r"-(implicit|nostate|other|nudge|land|rising)?-?[\d.e-]*$", "",
                                 b["shape"]["case"][6:]).split("-implicit")[0]
    k = {"fam": "effect", "own": "kind", "ramp": "cond", "peak": "cond", "moved": "shape", "stale": "trig"}
    sh = b["shape"]
    v = sh[k[b["family"]]] if b["family"] in k else sh.get("kind", sh.get("own", sh.get("s", sh.get("dir", ""))))
    return f"{b['family']}:{v}"


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
        lines.append(f"\n## {title}: {' / '.join(SHOWN)}\n")
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
    abbr = {"bounded": "B", "postlimit": "A", "L1": "L1", "located": "Lo", "moderead": "M", "noise": "N", "onestep": "S1",
            "storm": "St", "shallowpeak": "P", "restartloss": "R"}
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
        lines.append("Each cell: wrong runs, then the named limits (B bounded, A postlimit, L1, Lo located, M mode read, "
                     "N noise, S1 onestep, St storm, P shallow peak, R restart loss); "
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
    # bounded (review 3's S2): an extra pair within the crossing's window, not a lone lost firing
    bb = {"family": "own", "config": "ida", "labels": lab, "shape": {}}
    rr = {"params": {"a": 1.0}, "rtol": 1e-6, "truth": truth({"forth": [[0.625, loc(1e-6, 1 / 1.75)]], "back": []},
                                                            {"n": [1, 0], "m": [0, 0]})}
    pair = {"ok": True, "fired": [[0.62499, "forth"], [0.625001, "back"], [0.625002, "forth"]], "final": {"n": 2, "m": 1}}
    assert classify_run(bb, rr, pair, lambda k: "wrong") == "bounded"
    lost = {"ok": True, "fired": [], "final": {"n": 0, "m": 0}}
    assert classify_run(bb, rr, lost, lambda k: "wrong") == "wrong"
    lone = {"ok": True, "fired": [[0.62499, "forth"], [0.625001, "back"]], "final": {"n": 1, "m": 1}}
    assert classify_run(bb, rr, lone, lambda k: "wrong") == "wrong"
    far = {"ok": True, "fired": [[0.62499, "forth"], [0.625001, "back"], [0.7, "forth"]], "final": {"n": 2, "m": 1}}
    assert classify_run(bb, rr, far, lambda k: "wrong") == "wrong"
    # postlimit (revision 4): a slowed shape whose run never arrives, back moved by the time limit at the
    # rate after the event (B = 8 bands / (r / 1.75) = 0.0714 s here), and no further
    sl = {"family": "asym", "config": "ida", "labels": lab, "shape": {}}
    rs = dict(rr, truth=dict(rr["truth"], after=slowed(1e-4, D=1e-6)))
    moved = {"ok": True, "fired": [[0.62499, "forth"], [0.69, "back"]], "final": {"n": 1, "m": 1}}
    assert classify_run(sl, rs, moved, lambda k: "wrong") == "postlimit"
    beyond = dict(moved, fired=[[0.62499, "forth"], [0.7, "back"]])
    assert classify_run(sl, rs, beyond, lambda k: "wrong") == "wrong"
    assert classify_run(bb, rr, moved, lambda k: "wrong") == "wrong"
    # moderead: only m differs from the truth; a double firing is not one
    md = {"family": "rdmode", "config": "ida", "labels": ["forth"], "shape": {}}
    rm = dict(rr, truth=truth({"forth": [[0.625, loc(1e-6, 1 / 1.75)]]}, {"n": [1, 0], "m": [1, 0]}))
    read0 = {"ok": True, "fired": [[0.62499, "forth"]], "final": {"n": 1, "m": 0}}
    assert classify_run(md, rm, read0, lambda k: "wrong") == "moderead"
    twice0 = {"ok": True, "fired": [[0.62499, "forth"], [0.625001, "forth"]], "final": {"n": 2, "m": 0}}
    assert classify_run(md, rm, twice0, lambda k: "wrong") == "wrong"
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
