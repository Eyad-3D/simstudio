"""Lap cases on the new engine (lsim-lap): the case's car as the engine
models it, solved one instant at a time for each lap's speed profile by
today's quasi-steady method (lapsim's cornering speeds, forward and
backward passes), then driven along that profile in fast mode for its
energy, channels and books.

``run_lap`` runs a lap case: each lap's profile from the car's state where
the last lap ended (its battery's charge), then its energy pass, the laps
joined into one run; the endurance's driver change (a lap that ends at
rest), lift-and-coast and the energy target; live edits taken at the next
lap's start; today's Race Track channels, lap rows and their validity; the
Formula Student event rows. Calibrate lap fits the profile alone (``solve``).
"""
from __future__ import annotations

import bisect
import math
from types import SimpleNamespace
from typing import Optional

from .. import cycles
from ..schemas import LiveEdit, Project, SimMessage, SimResult, SummaryValue
from . import lapsim
from .domains import ModelInitError
from .network import build_model
from .runtime import GRAVITY, ControlFn, EmitFn, MovingAverage

# what held the car back on a stretch, by lsim-lap's code (its position + 1):
# today's (lapsim.LIMITS) and the motors' top speed in their gear
LIMITS = lapsim.LIMITS + ("top speed",)
CAP_CODE = 5
LAP_NOTE = ("Lap mode results are quasi-steady-state estimates: an ideal driver at the tyres' "
            "limit on the given line, with no transients, suspension or yaw. The "
            "car is the engine's own: its brakes share the braking by their Max Torque, and "
            "its differential is open, so the inner driven wheel's grip can hold the car back "
            "in a corner. They are usually optimistic: calibrate the tyres' μ and μ_y and the "
            "downforce against a lap your car has driven.")


def lap_model(project: Project, case_id: str):
    """The case's lap model (lightsim_engine.LapModel)."""
    import lightsim_engine

    from .new_engine import read_texts  # (new_engine imports this package)
    return lightsim_engine.lap_model(project.model_dump_json(), case_id,
                                     cycles_dir=str(cycles.DIR), texts=read_texts(project))


def track_of(project: Project, overrides: dict, spacing: float = 1.0) -> lapsim.Track:
    """The Race Track's points with ``overrides`` (a case's) applied."""
    model = build_model(project, {}, overrides)
    if model.track is None:
        raise ValueError("A lap needs a Race Track.")
    return lapsim.load_track(model.params_of[model.track], spacing)


def solve(lm, track: lapsim.Track, v_start: float, more: bool = False, coast: float = 0.0,
          stop: bool = False) -> dict:
    """One lap's profile (LapModel.solve) on ``track``; a car that cannot
    drive it is a lapsim.LapError."""
    try:
        return lm.solve(track.kappa, track.sin_t, track.ds, track.closed, v_start, coast=coast,
                        more=more, stop=stop)
    except ValueError as e:
        raise lapsim.LapError(f"The car cannot drive the lap: {e}.") from e


class Strategy:
    """Lift-and-coast to an energy target (today's lapsim.EnergyStrategy):
    before each lap, the least share of lift-and-coast whose lap fits the
    energy left per lap left. The energy a lap takes against its share is
    measured once, on the first lap, by the energy pass of each share of
    lapsim.COAST_GRID (linear between them), times a factor learned from
    each lap driven (the energy it took over the curve's), so that a lap
    off by a few per cent is made up on the next ones."""

    def __init__(self):
        self.curve: list[float] = []
        self.factor = 1.0
        self.short = False  # the target could not be met with full lift-and-coast

    def at(self, share: float) -> float:
        """The curve's energy at ``share``."""
        g = lapsim.COAST_GRID
        k = min(max(1, bisect.bisect_left(g, share)), len(g) - 1)
        f = (share - g[k - 1]) / (g[k] - g[k - 1])
        return self.curve[k - 1] + f * (self.curve[k] - self.curve[k - 1])

    def choose(self, measure, budget_j: float) -> float:
        """The share for a lap that may take ``budget_j``; ``measure(share)``
        the energy a lap with that share takes (asked on the first call)."""
        g = lapsim.COAST_GRID
        if not self.curve:
            self.curve = [measure(s) for s in g]
            for k in range(1, len(self.curve)):  # (more coasting never takes more)
                self.curve[k] = min(self.curve[k], self.curve[k - 1])
        need = [self.factor * e for e in self.curve]
        if need[0] <= budget_j:
            return 0.0
        if need[-1] > budget_j:
            self.short = True
            return 1.0
        k = next(k for k in range(1, len(need)) if need[k] <= budget_j)
        f = (need[k - 1] - budget_j) / max(1e-9, need[k - 1] - need[k])
        return g[k - 1] + f * (g[k] - g[k - 1])

    def learn(self, share: float, used_j: float) -> None:
        if used_j > 0 and self.at(share) > 0:
            self.factor = used_j / self.at(share)


class _LapEdits:
    """Live edits as the lap model's parameter changes: a lap model of the
    project with the edits so far built again, its parameters against the
    run's. A structural one, or one the engine builds another model for,
    takes effect on the next run."""

    def __init__(self, project: Project, case_id: str, lm):
        self.project = project.model_copy(deep=True)  # with the edits kept so far
        self.case_id = case_id
        self.params = dict(lm.params())

    def changes(self, model, el_id: str, key: str, value) -> dict:
        """The engine's parameter changes the edit makes (SI, by engine
        name), the edit kept for the next ones; raises _Later."""
        from .new_engine import _Later

        pdef = next((p for p in model.cdef_of[el_id].parameters if p.key == key), None)
        if pdef is not None and pdef.variability == "fixed":
            raise _Later("structural parameters take effect on the next run")
        if not isinstance(value, (bool, int, float, str)):
            raise _Later("a table takes effect on the next run on the new engine")
        trial = self.project.model_copy(deep=True)
        next(c for c in trial.cases if c.id == self.case_id).parameterOverrides.setdefault(
            el_id, {})[key] = value
        try:
            new = dict(lap_model(trial, self.case_id).params())
        except Exception as e:  # noqa: BLE001 — the engine's refusal of the value
            raise _Later(f"the new engine takes it on the next run ({e})") from e
        if new.keys() != self.params.keys():
            raise _Later("the new engine builds another model for it: it takes effect on the "
                         "next run")
        out = {n: v for n, v in new.items()
               if v != self.params[n] and not (math.isnan(v) and math.isnan(self.params[n]))}
        self.project = trial
        self.params.update(out)
        return out


def thin(times: list[float], raw: dict, vmin: dict, vmax: dict, vmean: dict, every: int):
    """Channels recorded at every track point kept at every ``every``-th
    point and the last (Store every N): the values there, the extremes over
    the points between and the mean over them by time."""
    keep = [j for j in range(len(times)) if j % every == 0 or j == len(times) - 1]
    lo_, hi_, mu_ = {}, {}, {}
    for name in vmin:
        lo, hi, mu = vmin[name], vmax[name], vmean[name]
        a_lo, a_hi, a_mu = [lo[0]], [hi[0]], [mu[0]]
        for a, b in zip(keep, keep[1:]):
            a_lo.append(min(lo[a + 1:b + 1]))
            a_hi.append(max(hi[a + 1:b + 1]))
            span = times[b] - times[a]
            a_mu.append(sum(mu[k] * (times[k] - times[k - 1]) for k in range(a + 1, b + 1)) / span
                        if span > 0 else mu[b])
        lo_[name], hi_[name], mu_[name] = a_lo, a_hi, a_mu
    return ([times[j] for j in keep], {n: [x[j] for j in keep] for n, x in raw.items()},
            lo_, hi_, mu_)


def long_flags(flags: list[dict], times: list[float]) -> list[dict]:
    """The energy pass's flags that cover a whole stretch of track or more
    (two track points or more inside them, ``times`` the points' times):
    the car could not follow the profile there. A shorter one is the
    profile's constant acceleration over a stretch meeting a limit inside
    it, its mean over the stretch within the limit."""
    out = []
    for f in flags:
        a = bisect.bisect_right(times, float(f["t_start"]))
        b = bisect.bisect_left(times, float(f["t_end"]))
        if b - a >= 2:
            out.append(f)
    return out


def track_columns(track: lapsim.Track, profiles: list[dict]) -> dict[str, list[float]]:
    """The Race Track's channels at each point of the laps driven (the
    first lap's start, then each lap's points after its first), in their
    display units, as today's LapSlave.publish_point gives them: the
    acceleration over the stretch that got there, the limit of that
    stretch (the first stretch's at the start)."""
    cols: dict[str, list[float]] = {p: [] for p in lapsim.TRACK_PORTS}
    for k, prof in enumerate(profiles):
        v, code = prof["v"], prof["code"]
        for i in range(0 if k == 0 else 1, len(v)):
            a = (v[i] ** 2 - v[i - 1] ** 2) / (2.0 * track.ds) if i else 0.0
            values = (i * track.ds, k + 1, track.kappa[i], a / GRAVITY,
                      v[i] * v[i] * track.kappa[i] / GRAVITY, code[i - 1] if i else code[0],
                      track.x[i], track.y[i], track.z[i])
            for p, x in zip(lapsim.TRACK_PORTS, values):
                cols[p].append(x)
    return cols


def run_lap(project: Project, case_id: str, emit: Optional[EmitFn] = None,
            control: Optional[ControlFn] = None, edits: list[LiveEdit] = ()) -> SimResult:
    """A lap case on the new engine, as today's SimResult (see the module's
    docstring). ``edits``: a run's live edits made again at the lap start
    they were taken at (a replay)."""
    import lightsim_engine

    from . import core, fs_events
    from .new_engine import NEXT_RUN, _channels, _context, _Later

    case = next(c for c in project.cases if c.id == case_id)
    try:
        ctx = _context(project, case_id)
    except ModelInitError as e:  # as today's engine says it
        return SimResult(caseId=case_id, status="failed", channels=[],
                         messages=[SimMessage(level="error", text=t) for t in e.messages])
    model = ctx.model
    output_every = max(1, int(getattr(case, "outputEvery", 1) or 1))
    found = lapsim.problems(model, output_every)
    if any(level == "error" for level, _, _ in found):
        return SimResult(caseId=case_id, status="failed", channels=[], messages=[
            SimMessage(level="error", text=t) for level, t, _ in found if level == "error"])
    messages = [SimMessage(level="warning", text=w) for w in core.model_warnings(model)]
    messages += [SimMessage(level=level, text=text) for level, text, _ in found]
    messages.append(SimMessage(level="info", text=LAP_NOTE))
    tp = model.params_of[model.track]
    track = lapsim.load_track(tp)
    laps = int(float(tp.get("laps", 1)))
    # the endurance's driver change (FS Rules 2026 v1.1 (FSG) D 7.2.3, D 7.5):
    # this lap (from 0) ends at rest, the next starts from it
    stop_after = laps // 2 - 1 if case.fsEvent == "endurance" and laps >= 2 else None
    coast = min(1.0, max(0.0, float(tp.get("coast_pct", 0) or 0) / 100.0))
    target_j = max(0.0, float(tp.get("energy_target_kWh", 0) or 0)) * 3.6e6
    messages += [SimMessage(level=level, text=text)
                 for level, text in fs_events.lap_check(case, laps, track.length)]

    lm = lap_model(project, case_id)
    inst = lm.instances
    bats = [b for b in ctx.batteries if b in inst]
    names = [f"{inst[b]}.{q}" for b in bats for q in ("e_delivered", "e_recuperated", "e_losses")]

    def battery_net(end: dict) -> float:  # the batteries' net energy out so far, J
        return sum(end[f"{inst[b]}.e_delivered"] - end[f"{inst[b]}.e_recuperated"] for b in bats)

    drive = lm.laps()
    lap_edits = _LapEdits(project, case_id, lm) if control or edits else None
    replay = sorted(edits, key=lambda e: e.t)
    taken: list[LiveEdit] = []
    queued: list[tuple[str, str, object]] = []

    def take(el_id: str, key: str, value, t: float) -> None:
        if el_id not in model.params_of:
            return  # (no such part: as today's, nothing)
        try:
            changes = lap_edits.changes(model, el_id, key, value)
            if changes:
                drive.set_params(changes)
            taken.append(LiveEdit(t=t, elementId=el_id, key=key, value=value))
        except _Later as e:
            note = f"'{model.elements[el_id].label}.{key}' changed — {e} [{NEXT_RUN}]."
            taken.append(LiveEdit(t=t, elementId=el_id, key=key, value=value, note=note))
            if emit:
                emit({"type": "message", "level": "info", "text": note})

    v_start = ctx.v
    t_now = 0.0
    lap = SimpleNamespace(laps=laps, lap_times=[], sector_times=[], stop_after=stop_after,
                          coast_used=coast > 0 or target_j > 0, coast_laps=[], target_j=target_j,
                          limit_s=[0.0] * len(LIMITS), net_j=0.0, track=track, profiles=[],
                          end={})
    strategy: Optional[Strategy] = None
    stopped, failed_at = False, None
    s_pts = [i * track.ds for i in range(len(track.kappa))]
    for k in range(laps):
        for msg in control() if control else ():
            if msg.get("type") == "cancel":
                stopped = True
            elif msg.get("type") == "set_param":
                queued.append((str(msg.get("elementId")), str(msg.get("key")), msg.get("value")))
        while replay and replay[0].t <= t_now + 1e-9:  # (a replay's, where it took them)
            e = replay.pop(0)
            if e.note:
                taken.append(e)
            else:
                take(e.elementId, e.key, e.value, t_now)
        for el_id, key, value in queued:  # this lap's profile and energy pass take them
            take(el_id, key, value, t_now)
        queued.clear()
        if stopped:
            messages.append(SimMessage(level="info", text=f"Simulation cancelled by user at "
                                                          f"t = {t_now:g} s."))
            break
        more, stop = k + 1 < laps, k == stop_after
        try:
            share = coast
            if target_j > 0:  # this lap's lift-and-coast, to the target
                strategy = strategy or Strategy()
                net0 = lap.net_j

                def measure(s: float) -> float:
                    drive.solve(track.kappa, track.sin_t, track.ds, track.closed, v_start,
                                coast=s, more=more, stop=stop)
                    return battery_net(drive.drive(keep=False, names=names)["end"]) - net0

                share = strategy.choose(measure, (target_j - lap.net_j) / (laps - k))
            prof = drive.solve(track.kappa, track.sin_t, track.ds, track.closed, v_start,
                               coast=share, more=more, stop=stop)
            got = drive.drive(names=names)
        except ValueError as e:  # the car cannot drive it, or its energy pass stopped
            messages.append(SimMessage(level="error", text=f"The car cannot drive the lap: {e}."))
            failed_at = t_now
            break
        lap.end = got["end"]
        used = battery_net(lap.end) - lap.net_j
        lap.net_j += used
        lap.coast_laps.append(share)
        if strategy is not None:
            strategy.learn(share, used)
        v, t, code = prof["v"], prof["t"], prof["code"]
        if k == 0 and v[0] < v_start - 1e-6:
            messages.append(SimMessage(level="info", text=(
                f"Lap 1 starts at {v[0] * 3.6:.1f} km/h, not at the Vehicle's Initial Speed "
                f"({v_start * 3.6:.1f} km/h): the most the Race Track's first corner allows.")))
        lap.lap_times.append(t[-1])
        cuts = [0.0] + [lapsim.interp1(list(zip(s_pts, t)), e) for e in track.sector_ends]
        lap.sector_times.append([b - a for a, b in zip(cuts, cuts[1:])])
        for i, c in enumerate(code):
            lap.limit_s[c - 1] += t[i + 1] - t[i]
        lap.profiles.append(prof)
        t_now += t[-1]
        v_start = v[-1]
        if emit:
            emit({"type": "step", "t": t_now, "pct": round(100.0 * (k + 1) / laps, 1),
                  "values": {}})
    lap.v_end = v_start
    if target_j > 0 and lap.lap_times:
        over = 100.0 * (lap.net_j - target_j) / target_j
        if over > 2.0:
            full = sum(1 for c in lap.coast_laps if c >= 1.0)
            messages.append(SimMessage(level="warning", text=(
                f"The laps took {lap.net_j / 3.6e6:.3f} kWh, {over:.1f} % more than the Race "
                f"Track's Energy Target of {target_j / 3.6e6:g} kWh"
                f"{f', with full lift-and-coast on {full} laps' if full else ''}: lift-and-coast "
                f"alone cannot save that much. Lower the battery's Output Power Limit as well, "
                f"or raise the target.")))
    run = drive.finish() if lap.lap_times else None
    result = lap_result(project, case, ctx, lm, run, drive.books() or {}, lap, messages, stopped,
                        failed_at, output_every, _channels)
    result.liveEdits = taken
    result.messages += [SimMessage(level="info", text=n)
                        for n in dict.fromkeys(e.note for e in taken if e.note)]
    result.engine, result.engineVersion = "new", lightsim_engine.version()
    result.status = core.status_of(result.messages, stopped)
    return result


def _col(d: dict, name: str) -> list[float]:
    """A channel of the run as a list (empty when it has none)."""
    got = d.get(name)
    return [] if got is None else [float(x) for x in got]


def lap_result(project, case, ctx, lm, run, books: dict, lap, messages: list, stopped: bool,
               failed_at: Optional[float], output_every: int, channels_of) -> SimResult:
    """The lap case's SimResult from its joined run: its channels with the
    Race Track's, today's lap rows first, the engine's figures (a battery's
    peaks and time at its limit per stretch of track), the rows a failed
    check makes not valid, the Formula Student event rows."""
    from . import core, fs_events
    from .keys import fill_keys
    from .labfig import lab_base
    from .new_engine import _tables
    from .references import check_references
    from .runtime import usable_energy_left_wh
    from .verdict import beyond_data, terminal_checks

    model = ctx.model
    case_id = case.id
    if run is None:  # (not one lap driven)
        return SimResult(caseId=case_id, status=core.status_of(messages, stopped), channels=[],
                         messages=messages)
    times = [float(x) for x in run.times]
    raw, vmin, vmax, vmean = (dict(run.channels), dict(run.min), dict(run.max), dict(run.mean))
    for p, col in track_columns(lap.track, lap.profiles).items():
        name = f"{model.track}:{p}"
        raw[name] = vmin[name] = vmax[name] = vmean[name] = col

    # ---- the batteries, per stretch (the lap is solved per stretch; inside one
    # its constant acceleration runs the power a few per cent over the cap at the
    # stretch's end and under it at its start, the mean on the cap) -----------
    inst = lm.instances
    cap_s = lap.limit_s[CAP_CODE - 1]
    per_stretch: dict[str, float] = {}
    for b in ctx.batteries.values():
        label = model.elements[b.el_id].label
        p_mean = _col(vmean, f"{b.el_id}:sig_power")
        if len(p_mean) > 1:
            if case.fsEvent:  # the power rule check's 500 ms average (fs_events._checks)
                avg = MovingAverage(fs_events.RULE_WINDOW_S)
                for j in range(1, len(times)):
                    avg.add(times[j - 1], times[j], p_mean[j] * 1000.0)
                b.rule_avg = SimpleNamespace(peak_w=avg.peak_w)
            b.p_peak_w = max(p_mean[1:]) * 1000.0
            b.p_sq_ws = sum((p_mean[j] * 1000.0) ** 2 * (times[j] - times[j - 1])
                            for j in range(1, len(times)))
            b.t_on = times[-1] - times[0]
            per_stretch[f"{b.el_id}.peak_terminal_power_kw"] = b.p_peak_w / 1000.0
        v_mean = _col(vmean, f"{b.el_id}:sig_voltage")
        if len(v_mean) > 1:
            b.v_peak, b.v_low = max(v_mean[1:]), min(v_mean[1:])
            per_stretch[f"{b.el_id}.max_pack_voltage_v"] = b.v_peak
            per_stretch[f"{b.el_id}.min_pack_voltage_v"] = b.v_low
        i_mean = _col(vmean, f"{b.el_id}:sig_current")
        if len(i_mean) > 1:
            b.i_peak_a = max(i_mean[1:])
        if b.check is not None and b.check.limit_w > 0:
            per_stretch[f"{b.el_id}.time_at_power_limit_s"] = cap_s
        if b.check is not None and len(p_mean) > 1:  # its terminal checks, per stretch
            avg = MovingAverage(b.check.window_s)
            for j in range(1, len(times)):
                avg.add(times[j - 1], times[j], p_mean[j] * 1000.0)
            b.check.peak_w, b.check.avg_peak_w = b.p_peak_w, avg.peak_w
            b.check.t_avg_peak = avg.t_peak
            if len(v_mean := _col(vmean, f"{b.el_id}:sig_voltage")) > 1:
                b.check.v_peak, b.check.v_min = max(v_mean[1:]), min(v_mean[1:])
            b.check.limit_s = cap_s if b.check.enforced else b.check.limit_s
        e = {q: float(lap.end.get(f"{inst[b.el_id]}.{q}", 0.0)) for q in
             ("e_delivered", "e_recuperated", "e_losses")} if b.el_id in inst else {}
        b.energy_out_wh = e.get("e_delivered", 0.0) / 3600.0
        b.energy_in_wh = e.get("e_recuperated", 0.0) / 3600.0
        b.loss_wh = e.get("e_losses", 0.0) / 3600.0
        soc = _col(raw, f"{b.el_id}:sig_soc")
        if soc:
            b.soc = float(soc[-1]) / 100.0
        low = _col(vmin, f"{b.el_id}:sig_soc")
        if low and min(low) <= b.min_soc * 100.0 + 1e-6:
            b.depleted_flagged = True
            k = next(k for k, x in enumerate(low) if x <= b.min_soc * 100.0 + 1e-6)
            messages.append(SimMessage(level="warning", text=(
                f"Battery '{label}' reached minimum SOC ({b.min_soc * 100:.0f} %) at "
                f"t = {times[k]:.0f} s — no further discharge; the motors get only what is "
                f"left.")))
        if b.check is not None and b.check.enforced and b.check.limit_w > 0 and cap_s > 0:
            k = next((i for i, c in enumerate(lap.profiles[0]["code"]) if c == CAP_CODE), None)
            at = lap.profiles[0]["t"][k] if k is not None else None
            margin = b.check.margin_pct
            cap = (f"its Output Power Limit less its {margin:g} % margin "
                   f"({b.p_cap_w / 1000.0:g} of {b.check.limit_w / 1000.0:g} kW at the "
                   f"terminals)" if margin > 0 else
                   f"its Output Power Limit ({b.p_cap_w / 1000.0:g} kW at the terminals)")
            if at is not None:
                messages.append(SimMessage(level="info", text=(
                    f"Battery '{label}' held at {cap} from t = {at:.2f} s — the motors get "
                    f"what is left after the other loads. The run summary says for how long.")))

    for mc in ctx.motors.values():  # held by its supply: the battery or its cap held the car
        per_stretch[f"{mc.el_id}.time_limited_by_supply_s"] = (
            lap.limit_s[LIMITS.index("battery")] + cap_s)

    # ---- the engine's records: tables read past their data, flags -----------
    told: list[SimMessage] = []
    _tables(ctx, run, told)
    messages += told
    beyond = beyond_data(core.judged_uses(ctx), times[-1],
                         lambda el: f"{model.cdef_of[el].name} '{model.elements[el].label}'")
    messages += [SimMessage(level="warning", text=text) for text, _ in beyond]
    flags = long_flags(run.report["fast"]["flags"], times)
    for f in flags[:5]:
        messages.append(SimMessage(level="warning", text=(
            f"Lap mode: {f['label']} from t = {float(f['t_start']):.2f} s to "
            f"{float(f['t_end']):.2f} s: the car could not have followed the lap's speed there "
            f"(its energy pass met a limit the lap's profile did not), so the lap times are "
            f"optimistic and not valid.")))
    if len(flags) > 5:
        messages.append(SimMessage(level="warning", text=f"Lap mode: {len(flags) - 5} more "
                                                         f"stretches like these."))

    # ---- the rows ------------------------------------------------------------
    rows = lap_rows(lap, books, ctx)
    summary = [SummaryValue(key=k, label=label, value=v, unit=u) for label, v, u, k in rows]
    figures = []
    for f in run.figures:
        s = SummaryValue(key=f["key"], label=f["label"], value=f["value"], unit=f["unit"])
        if s.key in per_stretch:
            s.value = per_stretch[s.key]
        figures.append(s)
    summary += figures
    by_key = {s.key: s for s in summary}
    for b in ctx.batteries.values():  # its terminal checks, as a full run's
        if b.check is None:
            continue
        found, problems = terminal_checks(b.el_id, model.elements[b.el_id].label, b.check,
                                          usable_energy_left_wh(b) / 1000.0, b.depleted_flagged)
        for row_label, v, u, lim, ok, k in found:
            s = by_key.get(k)
            if s is None:
                s = by_key[k] = SummaryValue(key=k, label=row_label, value=v, unit=u)
                summary.append(s)
            s.label, s.value, s.limit, s.passed = row_label, v, lim, ok
        messages += [SimMessage(level="warning", text=t) for t in problems]
    labels = [label for label, *_ in rows if label != "Lap energy balance error"]
    not_valid: dict[str, str] = {}
    times_done = bool(times)
    if (stopped or failed_at is not None) and times_done:
        why = (f"run cancelled at t = {times[-1]:g} s" if stopped
               else f"run stopped by an error at t = {failed_at:.2f} s")
        for label in labels + ["Consumption"]:
            not_valid.setdefault(label, why)
    if any(b.depleted_flagged for b in ctx.batteries.values()):
        for label in labels + ["Consumption"]:  # the laps were solved with power it no longer had
            not_valid.setdefault(label, "the battery reached its minimum SOC")
    if beyond:
        why = "; ".join(r for _, r in beyond)
        for label in labels + ["Consumption"]:
            not_valid.setdefault(label, why)
    if flags:
        for label in labels:
            not_valid.setdefault(label, "the car could not follow the lap's speed")
    error = next(v for label, v, _, _ in rows if label == "Lap energy balance error")
    if abs(error) > lapsim.BALANCE_PCT:
        not_valid.setdefault("Energy per lap", "the lap energy balance does not close")
        messages.append(SimMessage(level="warning", text=(
            f"Lap energy balance: the energy the laps took (kinetic, road load, slope, brakes, "
            f"tyre, gear and motor losses, consumers) differs from what the sources gave by "
            f"{error:+.2f} %, more than {lapsim.BALANCE_PCT:g} %. The Energy per lap is not "
            f"valid.")))
    electric = lap.net_j > 0 and not ctx.fuelcells
    base = lab_base(summary, electric)
    for s in summary:
        s.notValid = not_valid.get(s.label)
        if s.notValid is None and s.label in base:  # shares its base row's validity
            s.notValid = not_valid.get(base[s.label])
    finished = not stopped and failed_at is None
    messages += [SimMessage(level=level, text=text) for level, text in fs_events.add_event_rows(
        case, ctx, lap, summary, finished=finished, points=times_done)]
    fill_keys(summary, project)

    if output_every > 1:
        times, raw, vmin, vmax, vmean = thin(times, raw, vmin, vmax, vmean, output_every)
    channels = channels_of(model, SimpleNamespace(channels=raw, min=vmin, max=vmax, mean=vmean),
                           times)
    tr = lap.track
    rec_note = f", stored every {output_every}" if output_every > 1 else ""
    messages.insert(0, SimMessage(level="info", text=(
        f"Case '{case.name}' solved in lap mode: {lap.laps} lap{'s' if lap.laps > 1 else ''} of "
        f"the Race Track's {tr.name} layout ({tr.length:,.0f} m, a point every {tr.ds:.2f} m), "
        f"{len(times)} points recorded{rec_note}, {len(channels)} result channels.")))
    return SimResult(caseId=case_id, status=core.status_of(messages, stopped), channels=channels,
                     summary=summary, messages=messages,
                     references=check_references(case.references, summary))


def lap_rows(lap, books: dict, ctx) -> list[tuple[str, float, str, str]]:
    """The lap case's summary rows (today's LapRun.rows): (label, value,
    unit, key). The energy is the batteries' net and what the sources that
    store nothing gave; the balance error is the energy books' closure
    over the laps, % of it."""
    tr = lap.track
    done = len(lap.lap_times)
    if not done:
        return []
    best = min(range(done), key=lambda k: lap.lap_times[k])
    total = sum(lap.lap_times)
    rows = [("Lap time", lap.lap_times[best], "s", "lap_time_s")]
    if lap.laps > 1:
        rows += [("Lap 1 time", lap.lap_times[0], "s", "lap1_time_s"),
                 ("Total time", total, "s", "total_time_s")]
    if len(tr.sector_ends) > 1:
        rows += [(f"Sector {k + 1} time", st, "s", f"sector{k + 1}_time_s")
                 for k, st in enumerate(lap.sector_times[best])]
    rows.append(("Average speed", done * tr.length / total * 3.6, "km/h", "average_speed_kmh"))
    if not tr.closed:
        rows.append(("Speed at the finish", lap.v_end * 3.6, "km/h", "finish_speed_kmh"))
    net = lap.net_j + float(books.get("supplied", 0.0))
    rows.append(("Energy per lap", net / 3.6e6 / done, "kWh", "energy_per_lap_kwh"))
    bats = list(ctx.batteries.values())
    t_on = max((b.t_on for b in bats), default=0.0)
    if bats and t_on > 0:
        rows.append(("RMS battery power", math.sqrt(sum(b.p_sq_ws for b in bats) / t_on) / 1000.0,
                     "kW", "rms_battery_power_kw"))
    for name, s in zip(LIMITS, lap.limit_s):
        if (name == "lift-and-coast" and not lap.coast_used) or (name == "top speed" and s <= 0):
            continue
        rows.append((f"Time limited by {name}", s, "s",
                     f"time_limited_by_{name.replace(' ', '_').replace('-', '_')}_s"))
    if lap.coast_used and lap.coast_laps:
        rows.append(("Lift-and-coast, mean share",
                     100.0 * sum(lap.coast_laps) / len(lap.coast_laps), "%",
                     "lift_and_coast_share_pct"))
    if lap.target_j > 0:
        rows += [("Energy target", lap.target_j / 3.6e6, "kWh", "energy_target_kwh"),
                 ("Energy used against the target", 100.0 * (net - lap.target_j) / lap.target_j,
                  "%", "energy_against_target_pct")]
    closure = float(books.get("closure", 0.0))
    rows.append(("Lap energy balance error", 100.0 * closure / max(abs(net), 1e-9), "%",
                 "lap_energy_balance_error_pct"))
    return rows
