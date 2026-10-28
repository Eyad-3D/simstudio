"""The both-engines diff: every field of two SimResults compared, each
difference measured against its band, and classed by classes.toml.

Bands as DESIGN.md 12.3 and lsim-project/golden/README.md set them, and as
golden.rs computes them: a figure's band is the larger of twice the
difference between today's 10 ms and 1 ms runs and a floor (0.1 % of the
value for kWh and J, 0.5 % for anything else, at least 1e-9); a channel's
is twice the RMS of today's two runs' difference, at least 0.5 % of its RMS
(and 1e-9), over every output point but the first. A difference's size is
"x", its distance from today's 1 ms value in bands; a number is outside at
x > 1. Text, identities and presence compare equal or differ (x None).
"""
from __future__ import annotations

import fnmatch
import importlib.util
import math
import re
import tomllib
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
RULES = tomllib.loads((HERE / "classes.toml").read_text(encoding="utf-8"))["rule"]
NUMBER = re.compile(r"[-+]?\d[\d,]*(\.\d+)?(e[-+]?\d+)?", re.I)
GEAR_TYPES = ("mech.gearbox", "mech.final_drive", "mech.differential")


def rms(xs) -> float:
    xs = list(xs)
    return math.sqrt(sum(x * x for x in xs) / len(xs)) if xs else 0.0


def figure_band(normal: float | None, fine: float, unit: str) -> float:
    floor = 0.001 if unit in ("kWh", "J") else 0.005
    spread = 2.0 * abs(normal - fine) if normal is not None else 0.0
    return max(spread, floor * abs(fine), 1e-9)


def masked(text: str) -> str:
    """A message with its numbers taken out: the same message at another value."""
    return NUMBER.sub("#", text)


class Diff(dict):
    """One difference: item, field, key, what ("missing", "extra", "value",
    ...), x (its size in bands, or None), today's and the new value."""


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


def _series_x(n: list, f: list, v: list) -> tuple[float | None, float]:
    """(x, band) of a channel's values ``v`` against today's ``f`` (1 ms) and
    ``n`` (10 ms), over the points but the first where all three have data."""
    m = min(len(n), len(f), len(v))
    idx = [k for k in range(1, m) if None not in (n[k], f[k], v[k])
           and math.isfinite(n[k]) and math.isfinite(f[k])]
    if not idx:
        return None, 0.0
    band = max(2.0 * rms(n[k] - f[k] for k in idx), 0.005 * rms(f[k] for k in idx), 1e-9)
    if any(not math.isfinite(v[k]) for k in idx):
        return math.inf, band
    return rms(v[k] - f[k] for k in idx) / band, band


def where(n: list, f: list, v: list, band: float, times: list) -> dict:
    """Where a channel's difference sits: how many of its worst points must
    go for the rest to be inside the band (and their share), the times of
    the five worst, and on which side of today's 1 ms value the new one lies
    (above, below: the shares of the points)."""
    m = min(len(n), len(f), len(v), len(times))
    idx = [k for k in range(1, m) if None not in (n[k], f[k], v[k]) and math.isfinite(v[k])]
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


def _rows(rows: list[dict], key) -> dict:
    out: dict = {}
    for r in rows or []:
        out.setdefault(key(r), r)
    return out


def compare(item: str, normal: dict, fine: dict, new: dict) -> list[Diff]:
    """Every difference between today's 1 ms result ``fine`` (with its 10 ms
    run ``normal`` for the bands) and the new path's result ``new``."""
    out: list[Diff] = []
    if fine["caseId"] != new["caseId"]:
        out.append(_d(item, "caseId", "", "text", today=fine["caseId"], new=new["caseId"]))
    if fine["status"] != new["status"]:
        out.append(_d(item, "status", "", "text", today=fine["status"], new=new["status"]))

    # messages, with their numbers taken out
    a = Counter((m["level"], masked(m["text"])) for m in fine["messages"])
    b = Counter((m["level"], masked(m["text"])) for m in new["messages"])
    for key in sorted(set(a) | set(b)):
        if a[key] != b[key]:
            out.append(_d(item, "messages", f"{key[0]}: {key[1]}",
                          "missing" if b[key] < a[key] else "extra", today=a[key], new=b[key]))

    # channels
    chan = {c: _rows(r["channels"], lambda x: f"{x['elementId']}:{x['portId']}")
            for c, r in (("n", normal), ("f", fine), ("v", new))}
    grid: dict = {}
    for name in sorted(set(chan["f"]) | set(chan["v"])):
        f, v, n = chan["f"].get(name), chan["v"].get(name), chan["n"].get(name)
        if f is None or v is None:
            out.append(_d(item, "channels", name, "missing" if v is None else "extra"))
            continue
        for attr in ("label", "unit"):
            if f[attr] != v[attr]:
                out.append(_d(item, f"channels.{attr}", name, "text", today=f[attr], new=v[attr]))
        series = {c: [p["value"] for p in (r or {}).get("timeSeries", [])]
                  for c, r in (("n", n), ("f", f), ("v", v))}
        tf, tv = [p["t"] for p in f["timeSeries"]], [p["t"] for p in v["timeSeries"]]
        if tf != tv:  # the output times: once for the run, and for any channel off its grid
            key = "" if (tf, tv) == grid.setdefault("run", (tf, tv)) else name
            if key or not grid.get("told"):
                grid["told"] = True
                out.append(_d(item, "times", key, "grid", today=len(tf), new=len(tv),
                              last=[tf[-1] if tf else None, tv[-1] if tv else None]))
        x, band = _series_x(series["n"], series["f"], series["v"])
        if x is not None and x > 1.0:
            out.append(_d(item, "channels.values", name, "value", x=x, band=band,
                          where=where(series["n"], series["f"], series["v"], band, tf)))
        for stat in ("min", "max", "mean"):
            fs, vs, ns = f.get(stat), v.get(stat), (n or {}).get(stat)
            if (fs is None) != (vs is None):
                out.append(_d(item, f"channels.{stat}", name, "missing" if vs is None else "extra"))
            elif fs is not None:
                x, band = _series_x(ns or fs, fs, vs)
                if x is not None and x > 1.0:
                    out.append(_d(item, f"channels.{stat}", name, "value", x=x, band=band,
                                  where=where(ns or fs, fs, vs, band, tf)))

    # summary rows, by key (by label where a row has none)
    rows = {c: _rows(r["summary"], lambda x: x["key"] or x["label"])
            for c, r in (("n", normal), ("f", fine), ("v", new))}
    for key in sorted(set(rows["f"]) | set(rows["v"])):
        f, v, n = rows["f"].get(key), rows["v"].get(key), rows["n"].get(key)
        if f is None or v is None:
            out.append(_d(item, "summary", key, "missing" if v is None else "extra"))
            continue
        for attr in ("label", "unit", "notValid", "passed"):
            if f.get(attr) != v.get(attr):
                out.append(_d(item, f"summary.{attr}", key, "text", today=f.get(attr), new=v.get(attr)))
        if (f.get("limit") is None) != (v.get("limit") is None) or (
                f.get("limit") is not None and abs(f["limit"] - v["limit"]) > 1e-9 * abs(f["limit"])):
            out.append(_d(item, "summary.limit", key, "text", today=f.get("limit"), new=v.get("limit")))
        out += _num(item, "summary.value", key, (n or {}).get("value"), f["value"], v["value"], f["unit"])

    # the parts' energy books
    pe = {c: _rows(r.get("partEnergy"), lambda x: f"{x.get('elementId') or x['label']}|{x['part']}")
          for c, r in (("n", normal), ("f", fine), ("v", new))}
    for key in sorted(set(pe["f"]) | set(pe["v"])):
        f, v, n = pe["f"].get(key), pe["v"].get(key), pe["n"].get(key) or {}
        if f is None or v is None:
            out.append(_d(item, "partEnergy", key, "missing" if v is None else "extra"))
            continue
        for attr in ("energyIn", "energyOut", "losses", "stored", "energyInReverse"):
            out += _num(item, f"partEnergy.{attr}", key, n.get(attr), f.get(attr), v.get(attr), "kWh")
        for attr in ("peakPower", "meanPower", "rmsPower"):
            out += _num(item, f"partEnergy.{attr}", key, n.get(attr), f.get(attr), v.get(attr), "kW")
        for term in sorted(set(f.get("terms", {})) | set(v.get("terms", {}))):
            out += _num(item, "partEnergy.terms", f"{key}|{term}", n.get("terms", {}).get(term),
                        f.get("terms", {}).get(term), v.get("terms", {}).get(term), "kWh")

    # the energy report
    fe, ve, ne = fine.get("energy"), new.get("energy"), normal.get("energy") or {}
    if (fe is None) != (ve is None):
        out.append(_d(item, "energy", "", "missing" if ve is None else "extra"))
    elif fe is not None:
        for attr in ("sourceKWh", "remainderKWh"):
            out += _num(item, f"energy.{attr}", "", ne.get(attr), fe[attr], ve[attr], "kWh")
        for group in ("parts", "sources", "sinks"):
            gf = _rows(fe[group], lambda x: f"{x.get('elementId') or ''}|{x['label']}")
            gv = _rows(ve[group], lambda x: f"{x.get('elementId') or ''}|{x['label']}")
            gn = _rows(ne.get(group, []), lambda x: f"{x.get('elementId') or ''}|{x['label']}")
            for key in sorted(set(gf) | set(gv)):
                f, v = gf.get(key), gv.get(key)
                if f is None or v is None:
                    out.append(_d(item, f"energy.{group}", key, "missing" if v is None else "extra"))
                    continue
                for attr in ("inKWh", "outKWh", "lostKWh", "storedKWh", "kWh"):
                    if attr in f:
                        out += _num(item, f"energy.{group}.{attr}", key, gn.get(key, {}).get(attr),
                                    f[attr], v.get(attr), "kWh")

    # duty
    du = {c: {f"{p['elementId']}|{row['quantity']}": row for p in r.get("duty", []) for row in p["rows"]}
          for c, r in (("n", normal), ("f", fine), ("v", new))}
    for key in sorted(set(du["f"]) | set(du["v"])):
        f, v, n = du["f"].get(key), du["v"].get(key), du["n"].get(key) or {}
        if f is None or v is None:
            out.append(_d(item, "duty", key, "missing" if v is None else "extra"))
            continue
        for attr in ("max", "min", "mean", "rms"):
            out += _num(item, f"duty.{attr}", key, n.get(attr), f[attr], v[attr], f["unit"])

    # limits
    fl, vl, nl = fine.get("limits"), new.get("limits"), normal.get("limits") or {}
    if (fl is None) != (vl is None):
        out.append(_d(item, "limits", "", "missing" if vl is None else "extra"))
    elif fl is not None:
        if fl["states"] != vl["states"]:
            out.append(_d(item, "limits.states", "", "text", today=fl["states"], new=vl["states"]))
        lf, lv = _rows(fl["lanes"], lambda x: x["label"]), _rows(vl["lanes"], lambda x: x["label"])
        ln = _rows(nl.get("lanes", []), lambda x: x["label"])
        for key in sorted(set(lf) | set(lv)):
            f, v = lf.get(key), lv.get(key)
            if f is None or v is None:
                out.append(_d(item, "limits.lanes", key, "missing" if v is None else "extra"))
                continue
            for state in sorted(set(f["seconds"]) | set(v["seconds"])):
                out += _num(item, "limits.seconds", f"{key}|{state}",
                            ln.get(key, {}).get("seconds", {}).get(state),
                            f["seconds"].get(state, 0.0), v["seconds"].get(state, 0.0), "s")

    # references
    rf, rv = _rows(fine.get("references"), lambda x: x["label"]), _rows(new.get("references"), lambda x: x["label"])
    rn = _rows(normal.get("references"), lambda x: x["label"])
    for key in sorted(set(rf) | set(rv)):
        f, v = rf.get(key), rv.get(key)
        if f is None or v is None:
            out.append(_d(item, "references", key, "missing" if v is None else "extra"))
            continue
        if f["grade"] != v["grade"]:
            out.append(_d(item, "references.grade", key, "text", today=f["grade"], new=v["grade"]))
        if f.get("value") is not None or v.get("value") is not None:
            out += _num(item, "references.value", key, rn.get(key, {}).get("value"), f.get("value"),
                        v.get("value"), f.get("unit", ""))
    return out


# ---- classes ------------------------------------------------------------------

def _definitions():
    spec = importlib.util.spec_from_file_location(
        "golden_definitions", HERE.parents[1] / "engine" / "crates" / "lsim-project" / "golden" / "definitions.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def gear_rebuild(project: dict, normal: dict, fine: dict, new: dict) -> dict[str, tuple[float, float]]:
    """golden/definitions.py's rebuild of today's gear rule: for each gear
    channel it rebuilds, (x of the rule applied to the new run's own
    channels, x of the rule applied to today's 1 ms run), both against the
    channel's band. The first says whether today's rule explains the
    difference, the second that the rule is today's. Keyed by the channel
    for its values, and by "channel|min" (max, mean) for its statistics
    over each output interval, the rule applied to the sources' statistic
    there: right where the gear's quantity is linear in the sources' over
    the interval, which the second number checks on today's own run."""
    d = _definitions()
    try:
        chain, motors, clutches, node = d.driveline(project)
    except (StopIteration, KeyError, IndexError):
        return {}
    out = {}
    for stat in ("values", "min", "max", "mean"):
        def series(r):
            got = {}
            for x in r["channels"]:
                vs = [p["value"] for p in x["timeSeries"]] if stat == "values" else x.get(stat)
                if vs is not None:
                    got[f"{x['elementId']}:{x['portId']}"] = [math.nan if v is None else v for v in vs]
            return got
        ch = {c: series(r) for c, r in (("n", normal), ("f", fine), ("v", new))}
        try:
            theirs = d.rebuild(ch["v"], chain, motors, clutches, node)
            mine = d.rebuild(ch["f"], chain, motors, clutches, node)
        except (KeyError, IndexError, ZeroDivisionError):
            continue
        for name, values in theirs.items():
            if name not in ch["f"] or name not in ch["n"]:
                continue
            x_new, _ = _series_x(ch["n"][name], ch["f"][name], values)
            x_today, _ = _series_x(ch["n"][name], ch["f"][name], mine[name])
            if x_new is not None and x_today is not None:
                out[name if stat == "values" else f"{name}|{stat}"] = (x_new, x_today)
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
    if check == "gear_rebuild":  # today's rule on the new run lands inside, and is today's
        stat = df["field"].split(".")[-1]
        got = ctx.get("gears", {}).get(df["key"] if stat == "values" else f"{df['key']}|{stat}")
        return got is not None and got[0] <= 1.0 and got[1] <= 1.0
    if check == "today_has_new_none":  # today's run has it, the new path leaves it out
        return df["today"] is not None and df["new"] is None
    if check == "preparation_note":  # the message is one of the engine's build notes
        return df["key"].split(": ", 1)[-1] in ctx.get("notes", ())
    if check == "ends_at_the_line":  # an acceleration test's end: today past the line, the new at it
        end = ctx.get("line_end")
        return bool(end) and abs(end["new"] - end["line"]) <= 1e-3 and \
            end["today"] - end["line"] > 10 * abs(end["new"] - end["line"])
    if check == "status_from_notes":  # without its build notes the new run says today's status
        return ctx.get("status_without_notes") == df["today"]
    return True
