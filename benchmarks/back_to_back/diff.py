"""The both-engines diff: every field of two SimResults compared, each
difference measured against its band, and classed by classes.toml.

compare() walks the union of what today's 10 ms run (n), today's 1 ms run
(f) and the new run (v) give: every record, field, channel, statistic,
list and point, and every message. Its Ledger accounts for every leaf of
that union (leaves()): one f and v both have is compared (equal or a
difference), one either of them lacks is reported (a difference), one only
n has is a gap. The judge fails an item whose ledger does not balance (a
leaf left out, or taken twice). A channel only the new run records is
compared with the port the project's data bus wires to it, where today's
run records that port (_lend: its leaves "by source"); a text or flag n says
otherwise than f at a leaf f and v share is counted ("10 ms only"), and
what n flags (a warning or failed status, a notValid, a passed=false, a
warning or error it writes) that neither f nor v does is a difference.

Bands as DESIGN.md 12.3 and lsim-project/golden/README.md set them, and as
golden.rs computes them: a figure's band is the larger of twice the
difference between today's 10 ms and 1 ms runs and a floor (0.1 % of the
value for kWh and J, 0.5 % for anything else, at least 1e-9); a channel's
is twice the RMS of today's two runs' difference over the points both
have, at least 0.5 % of the RMS of today's 1 ms run (and 1e-9), and its
size the RMS of the new run's difference to today's 1 ms run over every
point of today's 1 ms run but the first. A difference's size is "x", its
distance from today's 1 ms value in bands; a number is outside at x > 1.
The first point, which today records before its first step, is a figure of
its own ("first point"). Text, identities and presence compare equal or
differ (x None). A point where today's 1 ms run has a value and the new run
has none (the path writes None for not a number, or its list ends sooner)
or a non-finite one is a difference ("not finite", at the first such
time); one where the new run has a value and today's 1 ms run has none is
one too ("extra"); a statistic's list of another length than today's is a
"grid" difference, as the output times are. FIELDS says how every field of
every model of a SimResult is compared (a field it does not name, as
text); a test holds it to the models. Rows are matched by their key
(ROWS), in order where a key repeats, and a key repeated a different number
of times is a difference ("duplicate"). A message pairs with today's
message of the same level and text without its numbers (a quoted name
keeps its numbers: it is text), and each of its numbers is a figure against
the same number of today's message.
"""
from __future__ import annotations

import fnmatch
import heapq
import importlib.util
import math
import re
import tomllib
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
RULES = tomllib.loads((HERE / "classes.toml").read_text(encoding="utf-8"))["rule"]
NUMBER = re.compile(r"[-+]?\d+(,\d{3})*(\.\d+)?(e[-+]?\d+)?", re.I)  # (1,450: a thousands comma, not "10, 70")
QUOTED = re.compile(r"(?<!\w)'[^']*'(?!\w)")  # a part's or a case's name in a message ('12 V Loads')
GEAR_TYPES = ("mech.gearbox", "mech.final_drive", "mech.differential")


def rms(xs) -> float:
    xs = list(xs)
    return math.sqrt(sum(x * x for x in xs) / len(xs)) if xs else 0.0


def figure_band(normal: float | None, fine: float, unit: str) -> float:
    floor = 0.001 if unit in ("kWh", "J") else 0.005
    # (a 10 ms value that is not a number gives no spread: a band of not a number would pass anything)
    spread = 2.0 * abs(normal - fine) if _finite(normal) else 0.0
    return max(spread, floor * abs(fine), 1e-9)


def numbers(text: str) -> tuple[str, list[float]]:
    """A message with its numbers taken out (the same message at another
    value), but those inside a quoted name, and the numbers taken out."""
    found: list[float] = []

    def take(m):
        found.append(float(m.group().replace(",", "")))
        return "#"
    parts, pos = [], 0
    for q in QUOTED.finditer(text):
        parts += [NUMBER.sub(take, text[pos:q.start()]), q.group()]
        pos = q.end()
    parts.append(NUMBER.sub(take, text[pos:]))
    return "".join(parts), found


def masked(text: str) -> str:
    """The text a message pairs by (numbers())."""
    return numbers(text)[0]


def _energy_key(x) -> str:
    return f"{x.get('elementId') or ''}|{x.get('label')}"


# The keyed lists of a SimResult (by their path with the keys left out) and each one's row key
ROWS = {
    ("messages",): lambda m: f"{m.get('level')}: {masked(m.get('text') or '')}",
    ("channels",): lambda x: f"{x.get('elementId')}:{x.get('portId')}",
    ("summary",): lambda x: x.get("key") or x.get("label"),
    ("partEnergy",): lambda x: f"{x.get('elementId') or x.get('label')}|{x.get('part')}",
    ("energy", "parts"): _energy_key, ("energy", "sources"): _energy_key, ("energy", "sinks"): _energy_key,
    ("duty",): lambda x: x.get("elementId"),
    ("duty", "rows"): lambda x: x.get("quantity"),
    ("limits", "lanes"): lambda x: x.get("label"),
    ("references",): lambda x: x.get("label"),
}


def leaves(x, path: tuple = (), shape: tuple = (), into: dict | None = None) -> dict:
    """Every leaf of ``x`` (a number, a text, a flag or None) by its path,
    with its value: a row of a keyed list (ROWS, by ``shape``: the path with
    its keys left out) by its key and the occurrence of that key, an item of
    any other list by its index, a dict's by its key. An empty list or dict
    has none."""
    into = {} if into is None else into
    if isinstance(x, dict):
        for k, v in x.items():
            leaves(v, path + (k,), shape + (k,), into)
    elif isinstance(x, list):
        key, seen = ROWS.get(shape), Counter()
        for i, v in enumerate(x):
            if key:
                k = key(v)
                leaves(v, path + (k, seen[k]), shape, into)
                seen[k] += 1
            else:
                leaves(v, path + (i,), shape, into)
    else:
        into[path] = x
    return into


def _says_other(normal, fine) -> bool:
    """Whether today's 10 ms run says another text or flag (a status, a
    notValid, a passed, a unit; or no number: None, the path's not a
    number, or a non-finite one) than its 1 ms run at a leaf both have;
    another number there is its band."""
    def text(x):
        return x is None or isinstance(x, (str, bool)) or not math.isfinite(x)

    def nan(x):
        return isinstance(x, float) and math.isnan(x)
    return (text(normal) or text(fine)) and normal != fine and not (nan(normal) and nan(fine))


class Ledger:
    """compare()'s account of the union of what today's 10 ms run (n), its
    1 ms run (f) and the new run (v) give, leaf by leaf (leaves()). A leaf f
    and v both have is compared once (same()), equal or a difference: by
    source where it is a leaf of today's channel lent to one only the new
    run records (``sourced``, _lend), and "10 ms only" where today's 10 ms
    run says another text or flag there than its 1 ms run (_says_other: what
    the app's own step says today, which the comparison with today's 1 ms
    run does not read); a leaf one of them lacks is reported once (told(), on it or a path
    above it): a difference (or, for a limit lane's state, compared with the
    0 s its absence means); a leaf only n has is a gap (nothing to judge the
    new run by, and the new run agrees with today's 1 ms run in lacking it).
    It balances when each leaf of the union is exactly one of these."""

    def __init__(self, normal: dict, fine: dict, new: dict, sourced: set = frozenset()):
        self.n, self.f, self.v = leaves(normal), leaves(fine), leaves(new)
        self.sourced = sourced
        self.compared, self.reported = Counter(), Counter()

    def same(self, path: tuple) -> None:
        self.compared[path] += 1

    def told(self, path: tuple) -> None:
        self.reported[path] += 1

    def totals(self) -> dict:
        lengths = sorted({len(p) for p in self.reported})
        union, got, wrong, gaps, ten = self.n.keys() | self.f.keys() | self.v.keys(), Counter(), [], [], []
        for p in union:
            c, r = self.compared[p], sum(self.reported[p[:i]] for i in lengths if i <= len(p))
            if p in self.f and p in self.v:
                kind = None
                if (c, r) == (1, 0):
                    kind = "sourced" if p in self.sourced else "compared"
                    if kind == "compared" and p in self.n and _says_other(self.n[p], self.f[p]):
                        kind = "ten_ms_only"
                        ten.append(p)
            elif p in self.f or p in self.v:
                kind = "reported" if (c, r) == (0, 1) else None
            else:
                kind = "gaps" if c == 0 and r <= 1 else None
                gaps.append(p)
            if kind:
                got[kind] += 1
            else:
                wrong.append((p, c, r))
        kinds = ("compared", "sourced", "ten_ms_only", "reported", "gaps")
        return {"union": len(union), **{k: got[k] for k in kinds},
                "balanced": sum(got[k] for k in kinds) == len(union),
                "unbalanced": len(wrong),
                "first_unbalanced": [f"{'/'.join(map(str, p))}: compared {c}, reported {r}"
                                     for p, c, r in heapq.nsmallest(5, wrong, key=str)],
                "first_gaps": ["/".join(map(str, p)) for p in heapq.nsmallest(5, gaps, key=str)],
                "first_ten_ms_only": [f"{'/'.join(map(str, p))}: 10 ms {self.n[p]!r}, 1 ms {self.f[p]!r}, "
                                      f"new {self.v[p]!r}" for p in heapq.nsmallest(5, ten, key=str)]}


class Diff(dict):
    """One difference: item, field, key, what ("missing", "extra", "value",
    ...), x (its size in bands, or None), today's and the new value."""


class Diffs(list):
    """compare()'s differences, with its ledger (``ledger``)."""
    ledger: Ledger


def _d(item, field, key, what, x=None, today=None, new=None, **more) -> Diff:
    return Diff(item=item, field=field, key=key, what=what, x=x, today=today, new=new, **more)


def _num(item, field, key, normal, fine, new, unit) -> list[Diff]:
    if fine is None and new is None:
        return []
    if fine is None or new is None:
        return [_d(item, field, key, "missing" if new is None else "extra", today=fine, new=new)]
    if not (math.isfinite(fine) and math.isfinite(new)):
        return [] if fine == new or (math.isnan(fine) and math.isnan(new)) else \
            [_d(item, field, key, "not finite", today=fine, new=new)]
    band = figure_band(normal, fine, unit)
    x = abs(new - fine) / band
    return [_d(item, field, key, "value", x=x, today=fine, new=new, band=band)] if x > 1.0 else []


def _finite(x) -> bool:
    return x is not None and math.isfinite(x)


def _at(xs: list, k: int):
    return xs[k] if k < len(xs) else None


def _gaps(f: list, v: list) -> list[int]:
    """Where today's 1 ms run has a value and ``v`` has none (None, the
    path's not a number, or no point at all: its list ends sooner) or a
    non-finite one: every point, the first among them."""
    return [k for k in range(len(f)) if _finite(f[k]) and (k >= len(v) or not _finite(v[k]))]


def _series_x(n: list, f: list, v: list, ref: list | None = None) -> tuple[float | None, float]:
    """(x, band) of a channel's values ``v`` against ``ref`` (today's ``f``,
    1 ms, unless given): the RMS of their difference over f's points but the
    first where f has a value, in the band of today's two runs (twice the
    RMS of f's difference to ``n``, 10 ms, where both have a value, at least
    0.5 % of f's RMS); x is infinite where ``v`` or ``ref`` lacks a value f
    has, None where f has none."""
    ref = f if ref is None else ref
    idx = [k for k in range(1, len(f)) if _finite(f[k])]
    both = [k for k in idx if k < len(n) and _finite(n[k])]
    band = max(2.0 * rms(n[k] - f[k] for k in both), 0.005 * rms(f[k] for k in idx), 1e-9)
    if _gaps(f, v) or _gaps(f, ref):
        return math.inf, band
    if not idx:
        return None, 0.0
    return rms(v[k] - ref[k] for k in idx) / band, band


def where(f: list, v: list, band: float, times: list) -> dict:
    """Where a channel's difference sits: how many of its worst points must
    go for the rest to be inside the band (and their share), the times of
    the five worst, and on which side of today's 1 ms value the new one lies
    (above, below: the shares of the points)."""
    m = min(len(f), len(v), len(times))
    idx = [k for k in range(1, m) if _finite(f[k]) and _finite(v[k])]
    err = sorted(((v[k] - f[k]) ** 2, k) for k in idx)
    total, drop = sum(e for e, _ in err), 0
    while err[:len(err) - drop] and math.sqrt(max(0.0, total) / max(1, len(err) - drop)) > band:
        total -= err[len(err) - 1 - drop][0]
        drop += 1
    worst = [times[k] for _, k in reversed(err[-5:])]
    above = sum(v[k] > f[k] for k in idx) / max(1, len(idx))
    below = sum(v[k] < f[k] for k in idx) / max(1, len(idx))
    return {"points": drop, "share": drop / max(1, len(idx)), "worst_t": worst,
            "above": above, "below": below}


def _rows(rows: list[dict] | None, key) -> dict[str, list[dict]]:
    out: dict = {}
    for r in rows or []:
        out.setdefault(key(r), []).append(r)
    return out


# How each field of each model of a SimResult (app/schemas.py) is compared:
# "key" (part of its row's key, so matched, and compared as text), "text"
# (equal or not), a unit (a number against its band, with that unit's floor),
# "unit" (a number in its row's own unit) or "own" (by compare's own code).
# A field a row has that its model does not name is compared as text.
FIELDS = {
    "SimResult": {"caseId": "text", "status": "text", "messages": "own", "channels": "own",
                  "summary": "own", "partEnergy": "own", "energy": "own", "duty": "own",
                  "limits": "own", "references": "own", "engine": "text", "engineVersion": "text",
                  "solverOptions": "text", "liveEdits": "text"},
    # (each live edit within its list's text)
    "LiveEdit": {"t": "text", "elementId": "text", "key": "text", "value": "text"},
    "SimMessage": {"level": "own", "text": "own"},
    "Channel": {"elementId": "key", "portId": "key", "label": "text", "unit": "text",
                "timeSeries": "own", "min": "own", "max": "own", "mean": "own"},
    "SummaryValue": {"key": "key", "label": "text", "value": "unit", "unit": "text",
                     "notValid": "text", "limit": "own", "passed": "text"},
    "PartEnergyFlow": {"elementId": "key", "label": "text", "part": "key", "energyIn": "kWh",
                       "energyOut": "kWh", "losses": "kWh", "stored": "kWh",
                       "energyInReverse": "kWh", "peakPower": "kW", "meanPower": "kW",
                       "rmsPower": "kW", "terms": "own"},
    "EnergyReport": {"parts": "own", "sources": "own", "sinks": "own", "sourceKWh": "kWh",
                     "remainderKWh": "kWh", "remainderPct": "%", "balanceErrorPct": "%"},
    "EnergyPart": {"elementId": "key", "label": "key", "kind": "text", "inKWh": "kWh",
                   "outKWh": "kWh", "lostKWh": "kWh", "storedKWh": "kWh", "lostPct": "%"},
    "EnergyFlow": {"label": "key", "kWh": "kWh", "group": "text", "elementId": "key"},
    "DutyPart": {"elementId": "key", "label": "text", "kind": "text", "rows": "own"},
    "DutyRow": {"quantity": "key", "unit": "text", "max": "unit", "min": "unit", "mean": "unit",
                "rms": "unit"},
    "LimitReport": {"states": "text", "lanes": "own", "tEnd": "s"},
    "LimitLane": {"label": "key", "elementIds": "text", "changes": "own", "seconds": "own"},
    "ReferenceCheck": {"label": "key", "value": "unit", "reference": "unit", "unit": "text",
                       "difference": "unit", "differencePct": "%", "tolerance": "unit",
                       "grade": "text", "source": "text", "automatic": "text", "bound": "text",
                       "note": "text"},
}


class _Walk:
    """One item's walk (compare): its differences and its ledger."""

    def __init__(self, item: str, led: Ledger):
        self.item, self.led, self.out = item, led, Diffs()
        self.out.ledger = led

    def diff(self, field, key, what, **more) -> None:
        self.out.append(_d(self.item, field, key, what, **more))

    def mark(self, path: tuple, f, v) -> None:
        """Every leaf under ``path`` of today's ``f`` and the new ``v``:
        compared where both have it, reported where one does."""
        fl, vl = leaves(f, path), leaves(v, path)
        for p in fl | vl:
            (self.led.same if p in fl and p in vl else self.led.told)(p)

    def both(self, field, key, path, f, v) -> bool:
        """Whether today's 1 ms run and the new run both have this list or
        record (not None): where one of them lacks it, a difference."""
        if f is not None and v is not None:
            return True
        if f is None and v is None:
            self.led.same(path)
        else:
            self.led.told(path)
            self.diff(field, key, "missing" if v is None else "extra")
        return False

    def value(self, field, key, path, how, n, f, v, fh=True, vh=True) -> None:
        """One field, which today's 1 ms run has (``fh``) and the new run has
        (``vh``): as text (equal or not), or a number in unit ``how``."""
        if not (fh and vh):
            if fh or vh:
                self.led.told(path)
                self.diff(field, key, "missing" if fh else "extra", today=f, new=v)
            return
        if how == "text":
            self.mark(path, f, v)
            if f != v:
                self.diff(field, key, "text", today=f, new=v)
        else:
            self.led.same(path)
            self.out += _num(self.item, field, key, n, f, v, how)

    def fields(self, field, key, path, model, f, v, n=None) -> None:
        """A row's fields (FIELDS[model], and any it does not name, as text)
        but those compare's own code compares ("own")."""
        spec, n = FIELDS[model], n or {}
        for attr in [*spec, *sorted((set(f) | set(v)) - set(spec))]:
            how = spec.get(attr, "text")
            if how == "own":
                continue
            how = "text" if how == "key" else (f.get("unit") or "") if how == "unit" else how
            self.value(f"{field}.{attr}", key, path + (attr,), how, n.get(attr), f.get(attr), v.get(attr),
                       attr in f, attr in v)

    def match(self, field, path, shape, fine, new, normal, label=None) -> list[tuple]:
        """Rows of today's 1 ms run and the new run matched by their key
        (ROWS[shape]), in order where a key repeats, each with today's 10 ms
        row of the same key and place: [(key, i, today's, new, 10 ms or
        None)]. A key on one side only is "missing" or "extra", one the two
        runs repeat a different number of times a "duplicate" (today's and
        the new count), each row with no partner reported. ``label`` gives a
        difference's key (the row's key unless given)."""
        rows = {c: _rows(r, ROWS[shape]) for c, r in (("f", fine), ("v", new), ("n", normal))}
        pairs = []
        for k in sorted(set(rows["f"]) | set(rows["v"]), key=str):
            fs, vs, ns = (rows[c].get(k, []) for c in "fvn")
            name = label(k) if label else k
            if not fs or not vs:
                self.diff(field, name, "missing" if not vs else "extra")
            elif len(fs) != len(vs):
                self.diff(field, name, "duplicate", today=len(fs), new=len(vs))
            for i in range(min(len(fs), len(vs)), max(len(fs), len(vs))):
                self.led.told(path + (k, i))
            pairs += [(k, i, f, v, ns[i] if i < len(ns) else None) for i, (f, v) in enumerate(zip(fs, vs))]
        return pairs

    def times(self, name, path, fts, vts, unit, grid) -> None:
        """A channel's output times, point by point (and a point's numbers
        other than its value, should it have any), and the grid's
        difference: once for the run, again for a channel off its grid."""
        for k in range(max(len(fts), len(vts))):
            fp, vp = (fts[k] if k < len(fts) else {}), (vts[k] if k < len(vts) else {})
            for attr in sorted((set(fp) | set(vp)) - {"value"}):
                p = path + ("timeSeries", k, attr)
                if attr == "t":
                    (self.led.same if attr in fp and attr in vp else self.led.told)(p)
                else:
                    self.value("channels.points", f"{name}|{k}|{attr}", p, unit, None, fp.get(attr), vp.get(attr),
                               attr in fp, attr in vp)
        tf, tv = [p.get("t") for p in fts], [p.get("t") for p in vts]
        if tf != tv:
            key = "" if (tf, tv) == grid.setdefault("run", (tf, tv)) else name
            if key or not grid.get("told"):
                grid["told"] = True
                # where the times part (the first point that differs) and today's last output step
                first = next((k for k, (a, b) in enumerate(zip(tf, tv)) if a != b), min(len(tf), len(tv)))
                self.diff("times", key, "grid", today=len(tf), new=len(tv), first=first,
                          last=[tf[-1] if tf else None, tv[-1] if tv else None],
                          step=tf[-1] - tf[-2] if len(tf) > 1 else None)

    def series(self, field, name, n, f, v, tf, tv, at, unit) -> None:
        """A channel's values or one of its statistics, point by point over
        the points of today's 1 ms run and of the new run (``at``: a point's
        leaf): a point one of them lacks is reported, any other compared. A
        value one lacks where the other has one is a difference ("not
        finite": the new run lacks it; "extra": today's does); the first
        point is a figure of its own, the others are compared in the
        channel's band (_series_x)."""
        for k in range(max(len(f), len(v))):
            (self.led.same if k < len(f) and k < len(v) else self.led.told)(at(k))
        lacks = [k for k in range(len(f)) if k >= len(v) or (_finite(f[k]) and not _finite(v[k]))]
        extra = [k for k in range(len(v)) if k >= len(f) or (_finite(v[k]) and not _finite(f[k]))]
        x, band = _series_x(n, f, v)
        if lacks:
            self.diff(field, name, "not finite", x=math.inf, band=band, points=len(lacks), first_t=_at(tf, lacks[0]))
        if extra:
            self.diff(field, name, "extra", band=band, points=len(extra), first_t=_at(tv, extra[0]))
        if f and v and _finite(f[0]) and _finite(v[0]):
            b0 = figure_band(n[0] if n else None, f[0], unit)
            if abs(v[0] - f[0]) > b0:
                self.diff(field, name, "first point", x=abs(v[0] - f[0]) / b0, today=f[0], new=v[0], band=b0,
                          next=f[1] if len(f) > 1 else None, series_band=band)
        if not lacks and x is not None and x > 1.0:
            self.diff(field, name, "value", x=x, band=band, where=where(f, v, band, tf))

    def changes(self, key, path, fstates, vstates, fc, vc, step) -> None:
        """A limit lane's changes of state as intervals: the same states in
        the same order, each change within one output step of today's."""
        if not self.both("limits.changes", key, path + ("changes",), fc, vc):
            return
        self.mark(path + ("changes",), fc, vc)

        def named(lane, states):
            return [states[int(s)] if _finite(s) and 0 <= int(s) < len(states) else s for _, s in lane]
        if named(fc, fstates) != named(vc, vstates):
            self.diff("limits.changes", key, "text", today=named(fc, fstates), new=named(vc, vstates))
        elif not all(_finite(c[0]) for c in [*fc, *vc]):
            self.diff("limits.changes", key, "not finite", today=[c[0] for c in fc], new=[c[0] for c in vc])
        else:
            worst = max((abs(a[0] - b[0]) for a, b in zip(fc, vc)), default=0.0)
            if worst > step + 1e-9:
                self.diff("limits.changes", key, "value", x=worst / step if step else math.inf,
                          today=[c[0] for c in fc], new=[c[0] for c in vc], step=step)


def _lend(normal: dict, fine: dict, new: dict, wires: dict) -> tuple[dict, dict, dict, list]:
    """Today's runs with, for each channel only the new run records that the
    project's data bus wires to a port today's 1 ms run records (``wires``:
    port → (the ports wired to it, the label today's engine gives a channel
    of it)), that port's channel of each of today's runs lent under the new
    channel's key and today's label for it, as a port reads what is wired to
    it: (10 ms run, 1 ms run, {channel: the port lent}, the lent rows)."""
    key = ROWS[("channels",)]
    today = [{key(c): c for c in r.get("channels") or []} for r in (normal, fine)]
    lent: dict = {}
    for c in new.get("channels") or []:
        k = key(c)
        to, label = wires.get(k, ((), None))
        src = next((s for s in sorted(to) if s in today[1]), None)
        if k not in today[1] and src:
            lent[k] = (src, {"elementId": c.get("elementId"), "portId": c.get("portId"), "label": label})
    if not lent:
        return normal, fine, {}, []
    rows = [[{**chans[s], **ids} for s, ids in lent.values() if s in chans] for chans in today]
    return ({**normal, "channels": [*(normal.get("channels") or []), *rows[0]]},
            {**fine, "channels": [*(fine.get("channels") or []), *rows[1]]}, {k: s for k, (s, _) in lent.items()}, rows[1])


def flags(res: dict) -> dict:
    """What a run flags: its status where it warns or fails, each summary
    row's notValid and a check's passed=false, each warning or error it
    writes ({(field, key): what it says})."""
    got = {("status", ""): res.get("status")} if res.get("status") in ("warning", "failed") else {}
    for r in res.get("summary") or []:
        if r.get("notValid"):
            got[("summary.notValid", ROWS[("summary",)](r))] = r["notValid"]
        if r.get("passed") is False:
            got[("summary.passed", ROWS[("summary",)](r))] = False
    for m in res.get("messages") or []:
        if m.get("level") in ("warning", "error"):
            got[("messages", ROWS[("messages",)](m))] = m.get("text")
    return got


def compare(item: str, normal: dict, fine: dict, new: dict, wires: dict | None = None) -> Diffs:
    """Every difference between today's 1 ms result ``fine`` (with its 10 ms
    run ``normal`` for the bands) and the new path's result ``new``: every
    field of every model, as FIELDS says, over the union of what the three
    give, with its ledger (Diffs.ledger). A channel only the new run records
    is compared with the port the data bus wires to it (_lend), where today
    records that one. What today's 10 ms run flags (flags()) and neither its
    1 ms run nor the new run does is a difference ("10 ms only")."""
    flagged = [(k, x) for k, x in flags(normal).items() if k not in flags(fine) and k not in flags(new)]
    normal, fine, lent, rows = _lend(normal, fine, new, wires or {})
    w = _Walk(item, Ledger(normal, fine, new, {p for c in rows for p in leaves(
        c, ("channels", ROWS[("channels",)](c), 0), ("channels",))}))
    w.fields("", "", (), "SimResult", fine, new, normal)
    for d in w.out:
        d["field"] = d["field"].lstrip(".")

    # messages: paired by level and text without their numbers (in order where one repeats);
    # each number of a pair a figure against today's, its band from today's 10 ms run's same message
    if w.both("messages", "", ("messages",), fine.get("messages"), new.get("messages")):
        texts = {c: _rows(r.get("messages"), ROWS[("messages",)]) for c, r in (("n", normal), ("f", fine), ("v", new))}
        for key in sorted(set(texts["f"]) | set(texts["v"])):
            fs, vs, ns = (texts[c].get(key, []) for c in ("f", "v", "n"))
            if len(fs) != len(vs):
                w.diff("messages", key, "missing" if len(vs) < len(fs) else "extra", today=len(fs), new=len(vs))
                for i in range(min(len(fs), len(vs)), max(len(fs), len(vs))):
                    w.led.told(("messages", key, i))
            for i, (f, v) in enumerate(zip(fs, vs)):
                path = ("messages", key, i)
                w.led.same(path + ("level",))  # (the key's: equal)
                w.led.same(path + ("text",))  # (its words the key's, its numbers below)
                w.fields("messages", key, path, "SimMessage", f, v)
                n = numbers(ns[i]["text"])[1] if i < len(ns) else None
                for j, (a, b) in enumerate(zip(numbers(f["text"])[1], numbers(v["text"])[1])):
                    for d in _num(item, "messages", key, n[j] if n else None, a, b, ""):
                        w.out.append(Diff(d, number=j + 1, today_text=f["text"], new_text=v["text"]))

    # channels: each one's fields, output times, values and statistics, point by point
    step, grid = 0.0, {}  # today's output step: the limit lanes' changes are judged to one
    if w.both("channels", "", ("channels",), fine.get("channels"), new.get("channels")):
        for name, i, f, v, n in w.match("channels", ("channels",), ("channels",), fine["channels"], new["channels"],
                                        normal.get("channels")):
            path, n, unit = ("channels", name, i), n or {}, f.get("unit") or ""
            w.fields("channels", name, path, "Channel", f, v, n)
            fts, vts, nts = f.get("timeSeries"), v.get("timeSeries"), n.get("timeSeries") or []
            tf, tv = [p.get("t") for p in fts or []], [p.get("t") for p in vts or []]
            if w.both("channels.values", name, path + ("timeSeries",), fts, vts):
                step = max([step] + [b - a for a, b in zip(tf, tf[1:]) if _finite(a) and _finite(b)])
                w.times(name, path, fts, vts, unit, grid)
                w.series("channels.values", name, *([p.get("value") for p in ts] for ts in (nts, fts, vts)), tf, tv,
                         lambda k: path + ("timeSeries", k, "value"), unit)
            for stat in ("min", "max", "mean"):
                fs, vs, ns = f.get(stat), v.get(stat), n.get(stat)
                if w.both(f"channels.{stat}", name, path + (stat,), fs, vs):
                    if len(vs) != len(fs):  # a statistic at fewer or more points than today's
                        w.diff(f"channels.{stat}", name, "grid", today=len(fs), new=len(vs))
                    w.series(f"channels.{stat}", name, ns or [], fs, vs, tf, tv, lambda k, s=stat: path + (s, k), unit)

    # summary rows, by key (by label where a row has none)
    if w.both("summary", "", ("summary",), fine.get("summary"), new.get("summary")):
        for key, i, f, v, n in w.match("summary", ("summary",), ("summary",), fine["summary"], new["summary"],
                                       normal.get("summary")):
            path = ("summary", key, i)
            w.fields("summary", key, path, "SummaryValue", f, v, n)
            fl, vl = f.get("limit"), v.get("limit")
            if ("limit" in f) != ("limit" in v):
                w.value("summary.limit", key, path + ("limit",), "text", None, fl, vl, "limit" in f, "limit" in v)
            elif "limit" in f:
                w.led.same(path + ("limit",))
                if (fl is None) != (vl is None) or (fl is not None and not abs(fl - vl) <= 1e-9 * abs(fl)):
                    w.diff("summary.limit", key, "text", today=fl, new=vl)

    # the parts' energy books
    if w.both("partEnergy", "", ("partEnergy",), fine.get("partEnergy"), new.get("partEnergy")):
        for key, i, f, v, n in w.match("partEnergy", ("partEnergy",), ("partEnergy",), fine["partEnergy"],
                                       new["partEnergy"], normal.get("partEnergy")):
            path, n = ("partEnergy", key, i), n or {}
            w.fields("partEnergy", key, path, "PartEnergyFlow", f, v, n)
            ft, vt, nt = f.get("terms"), v.get("terms"), n.get("terms") or {}
            if w.both("partEnergy.terms", key, path + ("terms",), ft, vt):
                for term in sorted(set(ft) | set(vt)):
                    w.value("partEnergy.terms", f"{key}|{term}", path + ("terms", term), "kWh", nt.get(term),
                            ft.get(term), vt.get(term), term in ft, term in vt)

    # the energy report
    fe, ve, ne = fine.get("energy"), new.get("energy"), normal.get("energy") or {}
    if w.both("energy", "", ("energy",), fe, ve):
        w.fields("energy", "", ("energy",), "EnergyReport", fe, ve, ne)
        for group, model in (("parts", "EnergyPart"), ("sources", "EnergyFlow"), ("sinks", "EnergyFlow")):
            if w.both(f"energy.{group}", "", ("energy", group), fe.get(group), ve.get(group)):
                for key, i, f, v, n in w.match(f"energy.{group}", ("energy", group), ("energy", group), fe[group],
                                               ve[group], ne.get(group)):
                    w.fields(f"energy.{group}", key, ("energy", group, key, i), model, f, v, n)

    # duty: each part (one with no rows too), and each of its rows
    if w.both("duty", "", ("duty",), fine.get("duty"), new.get("duty")):
        for part, i, f, v, n in w.match("duty", ("duty",), ("duty",), fine["duty"], new["duty"], normal.get("duty")):
            path = ("duty", part, i)
            w.fields("duty.part", part, path, "DutyPart", f, v)
            if w.both("duty", part, path + ("rows",), f.get("rows"), v.get("rows")):
                for key, j, fr, vr, nr in w.match("duty", path + ("rows",), ("duty", "rows"), f["rows"], v["rows"],
                                                  (n or {}).get("rows"), label=lambda q, part=part: f"{part}|{q}"):
                    w.fields("duty", f"{part}|{key}", path + ("rows", key, j), "DutyRow", fr, vr, nr)

    # limits
    fl, vl, nl = fine.get("limits"), new.get("limits"), normal.get("limits") or {}
    if w.both("limits", "", ("limits",), fl, vl):
        w.fields("limits", "", ("limits",), "LimitReport", fl, vl, nl)
        if w.both("limits.lanes", "", ("limits", "lanes"), fl.get("lanes"), vl.get("lanes")):
            for key, i, f, v, n in w.match("limits.lanes", ("limits", "lanes"), ("limits", "lanes"), fl["lanes"],
                                           vl["lanes"], nl.get("lanes")):
                path = ("limits", "lanes", key, i)
                w.fields("limits.lanes", key, path, "LimitLane", f, v)
                w.changes(key, path, fl.get("states") or [], vl.get("states") or [], f.get("changes"),
                          v.get("changes"), step)
                fs, vs, ns = f.get("seconds"), v.get("seconds"), (n or {}).get("seconds") or {}
                if w.both("limits.seconds", key, path + ("seconds",), fs, vs):
                    # (a lane lists the states it spent time in: one it does not list, 0 s)
                    for state in sorted(set(fs) | set(vs)):
                        (w.led.same if state in fs and state in vs else w.led.told)(path + ("seconds", state))
                        w.out += _num(item, "limits.seconds", f"{key}|{state}", ns.get(state),
                                      fs.get(state, 0.0), vs.get(state, 0.0), "s")

    # references
    if w.both("references", "", ("references",), fine.get("references"), new.get("references")):
        for key, i, f, v, n in w.match("references", ("references",), ("references",), fine["references"],
                                       new["references"], normal.get("references")):
            w.fields("references", key, ("references", key, i), "ReferenceCheck", f, v, n)

    # today's engine at the app's own step flags it (warns, fails, a row not valid or a check not passed),
    # and neither today's 1 ms run nor the new run does: the case FAULTS counts, shown here
    for (field, key), says in flagged:
        w.diff(field, key, "10 ms only", today=says, new=new.get("status") if field == "status" else None)
    for d in w.out:  # (a lent channel's differences: against the port wired to it)
        if d["field"].startswith(("channels", "times")) and (d["key"] or "").split("|")[0] in lent:
            d["source"] = lent[d["key"].split("|")[0]]
    return w.out


# ---- classes ------------------------------------------------------------------

def _definitions():
    spec = importlib.util.spec_from_file_location(
        "golden_definitions", HERE.parents[1] / "engine" / "crates" / "lsim-project" / "golden" / "definitions.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def teeth(d, ch: dict, chain, motors, clutches, node, j_src: float, carrier: list, times: list,
          shift: int = 0) -> dict:
    """The gear channels the new engine's law gives (lsim-lib
    blocks/driveline.rs), rebuilt from a run's own channels ``ch`` (name →
    values) as golden/definitions.py rebuilds today's: each gear takes the
    torque at its mesh, what the sources send less the inertias before it
    (the motors' rotors, ``j_src``, and the gears' own input and output
    sides) times their acceleration, loses its efficiency on that (the way
    the power flows) and reports it; a differential's output torques are
    its share of it. The acceleration is the differential carrier's
    (``carrier``, its speed at the output ``times``, which a gear change does
    not jump) over the interval before each point, times the ratios above
    it: a drive cycle's speed is linear between its points (the 1 s output
    grid's), an output time on a change records the value before it, and
    over an interval that is the mean's exact term. With ``shift`` the
    acceleration is that of the interval so many after each point's own (the
    first point's, before any interval, 0; one past the last, the last's): the
    law at the accelerations around an interval (_interval)."""
    out: dict = {}
    left = [0.0] + [(carrier[k] - carrier[k - 1]) / (times[k] - times[k - 1]) / d.RPM for k in range(1, len(carrier))]
    for k in range(len(carrier)):
        ratios = [d.nearest_ratio(prm["ratios"], ch[f"{el}:sig_gear"][k]) if kind == "mech.gearbox"
                  else float(prm.get("ratio", 1.0) or 1.0) for el, kind, prm, _ in chain]
        j = min(k + shift, len(left) - 1)
        a = (left[j] if j >= 0 else 0.0) * math.prod(ratios[:-1])
        w = (ch[f"{motors[0]}:sig_speed"] if motors else ch[f"{node}:sig_speed"])[k] / d.RPM
        tq = sum(ch[f"{m}:sig_torque"][k] for m in motors) + sum(ch[f"{c}:sig_torque"][k] for c in clutches)
        tq -= j_src * a
        for (el, kind, prm, eta), ratio in zip(chain, ratios):
            tx = tq - float(prm.get("inertia_in_kgm2") or prm.get("inertia_kgm2") or 0.0) * a
            p = tx * w / 1000.0  # kW
            eta_d = eta if p >= 0.0 else 1.0 / eta
            out.setdefault(f"{el}:sig_power", []).append(p)
            out.setdefault(f"{el}:sig_losses", []).append(p * (1.0 - eta_d))
            if kind == "mech.differential":
                out.setdefault(f"{el}:sig_torque_a", []).append(0.5 * ratio * eta_d * tx)
                out.setdefault(f"{el}:sig_torque_b", []).append(0.5 * ratio * eta_d * tx)
            a, w = a / ratio, w / ratio
            tq = ratio * eta_d * tx - float(prm.get("inertia_out_kgm2") or 0.0) * a
    return out


def _holds(stat: str, s: list, values: list) -> bool:
    """Whether a minimum (maximum) over each output interval is at most (at
    least) the values at both ends of the interval, as the new engine's are."""
    if len(s) != len(values) or not all(_finite(x) for x in [*s, *values]):
        return False
    pick = min if stat == "min" else max
    return all(abs(s[k] - pick(s[k], values[k - 1], values[k])) <= 1e-9 * max(1.0, abs(s[k]))
               for k in range(1, len(s)))


def _interval(s: list, laws: list[list], values: list, law_values: list, band: float) -> float:
    """How far, in ``band``s and at the worst output interval, a minimum or
    maximum ``s`` over each interval lies outside the range the new engine's
    law gives it there (``laws``: the law on the sources' statistic at the
    accelerations of the interval and of the ones before and after it),
    widened by that law's own error on the run's values at the interval's
    two ends (``values`` against ``law_values``, the law on the values): 0
    inside; infinite where any of them is missing or not a number."""
    got = [s, values, law_values, *laws]
    if len({len(x) for x in got}) != 1 or not all(_finite(x) for xs in got for x in xs) or not _finite(band) \
            or band <= 0.0:
        return math.inf
    worst = 0.0
    for k in range(1, len(s)):
        lo, hi = min(x[k] for x in laws), max(x[k] for x in laws)
        err = max(abs(values[j] - law_values[j]) for j in (k - 1, k))
        worst = max(worst, lo - err - s[k], s[k] - hi - err)
    return worst / band


def _scale(pct) -> float:
    """A percentage as lsim-lib blocks/motor.rs scale_pct takes it (100 where
    not given; 1 where not above 0, at least 0.01)."""
    v = float(100.0 if pct is None else pct)
    return max(v / 100.0, 0.01) if v > 0.0 else 1.0


GEAR_X = ("rule_today", "rule_new", "teeth", "inertia", "new_rule")


def gear_rebuild(project: dict, normal: dict, fine: dict, new: dict) -> dict[str, dict]:
    """golden/definitions.py's rebuild of today's gear rule, and ``teeth``'s
    of the new engine's: for each gear channel they rebuild, in the
    channel's band, how far (rule_today) today's rule on today's 1 ms run is
    from today's channel (the rule is today's), (rule_new) today's rule on
    the new run's own sources is from today's channel (the sources agree),
    (teeth) the new run's own channel is from the new engine's law on those
    sources (the channel is that law's), (inertia) that law is from today's
    rule on them (the inertias' term) and (new_rule) the new run's channel
    is from today's rule on them; for a minimum or maximum also (holds)
    whether it holds the new run's values at its interval's ends (_holds)
    and (interval) how far it lies outside the law's range interval by
    interval (_interval).
    Keyed by the channel for its values, and by "channel|min" (max, mean)
    for its statistics over each output interval, each rule applied to the
    sources' statistic there: right where the gear's quantity is linear in
    the sources' over the interval, which rule_today checks on today's run."""
    d = _definitions()
    try:
        chain, motors, clutches, node = d.driveline(project)
    except (StopIteration, KeyError, IndexError):
        return {}
    dfl = d.defaults()
    prm = {e["id"]: {**dfl.get(e["componentDefId"], {}), **e.get("parameterOverrides", {})}
           for s in project["systems"] for e in s["elements"]}
    # a rotor's inertia grows with the motor's Torque Scale (lsim-lib blocks/motor.rs: J)
    j_src = sum(float(prm[m].get("inertia_kgm2") or 0.0) * _scale(prm[m].get("torque_scale_pct")) for m in motors)

    def series(r, stat):
        got = {}
        for x in r["channels"]:
            vs = [p["value"] for p in x["timeSeries"]] if stat == "values" else x.get(stat)
            if vs is not None:
                got[f"{x['elementId']}:{x['portId']}"] = [math.nan if v is None else v for v in vs]
        return got
    values = series(new, "values")
    carrier = f"{chain[-1][0]}:sig_speed_in"
    times = next(([p["t"] for p in x["timeSeries"]] for x in new["channels"]
                  if f"{x['elementId']}:{x['portId']}" == carrier), [])
    out = {}
    try:
        law_values = teeth(d, values, chain, motors, clutches, node, j_src, values[carrier], times)
    except (KeyError, IndexError, ZeroDivisionError):
        return {}
    for stat in ("values", "min", "max", "mean"):
        ch = {c: series(r, stat) for c, r in (("n", normal), ("f", fine), ("v", new))}
        try:
            theirs = d.rebuild(ch["v"], chain, motors, clutches, node)
            mine = d.rebuild(ch["f"], chain, motors, clutches, node)
            laws = [teeth(d, ch["v"], chain, motors, clutches, node, j_src, values[carrier], times, shift=s)
                    for s in ((0, -1, 1) if stat in ("min", "max") else (0,))]
        except (KeyError, IndexError, ZeroDivisionError):
            continue
        law = laws[0]
        for name, rule_new in theirs.items():
            if any(name not in x for x in (ch["f"], ch["n"], ch["v"], law)):
                continue
            n, f, v = ch["n"][name], ch["f"][name], ch["v"][name]
            if _gaps(f, v):
                continue  # the rules explain the new run's channel, not one it leaves out
            got = {"rule_today": _series_x(n, f, mine[name])[0], "rule_new": _series_x(n, f, rule_new)[0],
                   "teeth": _series_x(n, f, v, law[name])[0], "inertia": _series_x(n, f, law[name], rule_new)[0],
                   "new_rule": _series_x(n, f, v, rule_new)[0]}
            if None in got.values():
                continue
            if stat in ("min", "max"):
                got["holds"] = _holds(stat, v, values.get(name, []))
                got["interval"] = _interval(v, [x[name] for x in laws], values.get(name, []),
                                            law_values.get(name, []), _series_x(n, f, v)[1])
            out[name if stat == "values" else f"{name}|{stat}"] = got
    return out


def classify(diffs: list[Diff], context: dict) -> None:
    """Give each difference its class and rule (``cls``, ``rule``) from
    classes.toml, the first rule that matches; "unclassified" when none
    does. ``context[item]`` holds what the rules' checks read."""
    for df in diffs:
        df["cls"], df["rule"] = "unclassified", None
        for rule in RULES:
            if _matches(rule, df, context.get(df["item"], {})):
                df["cls"], df["rule"] = rule["class"], rule["id"]
                break


def _matches(rule: dict, df: Diff, ctx: dict) -> bool:
    if df["what"] in ("not finite", "10 ms only") and df["what"] not in rule.get("what", ()):
        return False  # a value the new run lacks, or a flag only today's 10 ms run raises: only a rule that names it
    if "field" in rule and not any(fnmatch.fnmatchcase(df["field"], f) for f in rule["field"]):
        return False
    if "what" in rule and df["what"] not in rule["what"]:
        return False
    if "item" in rule and not any(fnmatch.fnmatchcase(df["item"], i) for i in rule["item"]):
        return False
    if "key" in rule and not re.search(rule["key"], df["key"] or ""):
        return False
    if "max_x" in rule and not (df["x"] is not None and df["x"] <= rule["max_x"]):
        return False
    check = rule.get("check")
    if check == "gear_rebuild":  # today's rule is today's and the sources agree; the new channel is its law's
        stat = df["field"].split(".")[-1]
        g = ctx.get("gears", {}).get(df["key"] if stat == "values" else f"{df['key']}|{stat}")
        if not g or not all(_finite(g.get(k)) for k in GEAR_X):
            return False  # (a not a number passes nothing)
        if g["rule_today"] > 1.0 or g["rule_new"] > 1.0:
            return False
        if stat in ("values", "mean"):  # the law rebuilt exactly: inside the band
            return g["teeth"] <= 1.0
        # a minimum or maximum: the acceleration at it is not on the output grid, so the law on the
        # sources' statistic is not exact; the new statistic holds its values' ends, lies within the
        # inertias' term (and the band) of both the law and today's rule, and at every interval within
        # one band of the law's range there (_interval)
        return (g.get("holds") is True and max(g["teeth"], g["new_rule"]) <= g["inertia"] + 1.0
                and _finite(g.get("interval")) and g["interval"] <= 1.0)
    if check == "today_has_new_none":  # today's run has it, the new path leaves it out
        return df["today"] is not None and df["new"] is None
    if check == "preparation_note":  # the message is one of the engine's build notes
        return df["key"].split(": ", 1)[-1] in ctx.get("notes", ())
    if check == "ends_at_the_line":  # an acceleration test's end: today past the line, the new at it
        end = ctx.get("line_end")
        if not end or not all(_finite(end.get(k)) for k in ("today", "new", "line")):
            return False
        # and only the last output time differs: the same count, the same times before it, the new
        # run's last inside today's last output step
        last_today, last_new = df.get("last") or (None, None)
        if not (df["today"] == df["new"] and df.get("first") == df["today"] - 1
                and all(_finite(x) for x in (last_today, last_new, df.get("step")))
                and 0.0 < last_today - last_new < df["step"]):
            return False
        # and this grid ends where the distance channel does, at the line (a channel told on its own, off
        # the run's grid, that ends elsewhere is not today's discretisation)
        if (last_today, last_new) != (end.get("t_today"), end.get("t_new")):
            return False
        return abs(end["new"] - end["line"]) <= 1e-3 and \
            end["today"] - end["line"] > 10 * abs(end["new"] - end["line"])
    if check == "before_first_step":  # today's first point is recorded before its first step
        got = [df.get(k) for k in ("today", "next", "new", "series_band")]
        if not all(_finite(x) for x in got):
            return False
        lo, hi = sorted(got[:2])  # the new run's first value where today's run is after its first step
        return lo - got[3] <= got[2] <= hi + got[3]
    if check == "status_from_notes":  # without its build notes the new run says today's status
        return ctx.get("status_without_notes") == df["today"]
    if check == "ran_as_its_engine":  # each run says what ran it: its own engine's, not the other's
        return _ran_as(df["field"], df["today"], "today") and _ran_as(df["field"], df["new"], "new")
    return True


# what each engine's result says ran it (app/solver: core.run_case, new_engine.run_new)
RAN = {"today": {"engine": "today", "solverOptions": {"timeStep", "solverStep", "outputEvery"}},
       "new": {"engine": "new", "solverOptions": {"rtol", "atol", "outputStep", "traceStep"}}}


def _ran_as(field: str, value, engine: str) -> bool:
    """Whether ``value`` of the result's ``field`` is what a run of
    ``engine`` says ran it: its name, a version, its solver options (every
    one a finite number)."""
    if field == "engine":
        return value == RAN[engine]["engine"]
    if field == "engineVersion":
        return isinstance(value, str) and bool(value)
    if field == "solverOptions":
        return (isinstance(value, dict) and set(value) == RAN[engine]["solverOptions"]
                and all(_finite(x) for x in value.values()))
    return False
