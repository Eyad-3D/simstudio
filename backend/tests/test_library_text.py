"""The component descriptions and parameter texts are the app's in-place
help (library tooltip, Properties panel, the help card), so they must say
something about every parameter and must not send users to ports that do
not exist."""
from __future__ import annotations

import re

from app.library import load_library

# "the Vehicle's Road Grade input", "the Driver's Target Speed input", …
PORT_REFERENCE = re.compile(
    r"\b([A-Z][\w-]*(?: [A-Z][\w-]*)*)'s ([A-Z][\w-]*(?: [A-Z][\w-]*)*) (input|output)\b"
)


def _help_texts(comp):
    """The component's description and its parameters' help texts."""
    yield comp.description or ""
    for p in comp.parameters:
        yield from (getattr(p, f) or "" for f in HELP_FIELDS)


def test_descriptions_name_ports_that_exist():
    by_name = {c.name: c for c in load_library()}
    wrong = []
    found = 0
    for comp in load_library():
        for owner, port, direction in (m for text in _help_texts(comp)
                                       for m in PORT_REFERENCE.findall(text)):
            found += 1
            target = by_name.get(owner)
            if target is None or not any(
                p.name == port and p.direction == direction for p in target.ports
            ):
                wrong.append(f"{comp.name}: {owner}'s {port} {direction}")
    assert found, "pattern matched nothing — the regex no longer fits the catalog"
    assert not wrong, "descriptions refer to missing ports: " + "; ".join(wrong)


LIMITS = ("minimum", "exclusiveMinimum", "maximum")
HELP_FIELDS = ("description", "typical", "whereToFind")


def test_every_parameter_is_explained():
    """LRN-05: every parameter says what it is, what values are usual and
    where the real number comes from, each in a sentence or three: the help
    card is 290 px wide, and the help's component pages, AI tools and the
    reference read the same texts."""
    missing, unfinished, long_ = [], [], []
    for comp in load_library():
        for p in comp.parameters:
            for field in HELP_FIELDS:
                text = (getattr(p, field) or "").strip()
                where = f"{comp.name} · {p.label} · {field}"
                if not text or "TODO" in text:
                    missing.append(where)
                elif not text.endswith((".", ")")):
                    unfinished.append(where)
                elif len(text) > 320:
                    long_.append(f"{where} ({len(text)})")
    assert not missing, f"{len(missing)} parameter texts missing, e.g. " + "; ".join(missing[:5])
    assert not unfinished, "not a full sentence: " + "; ".join(unfinished)
    assert not long_, "over 320 characters: " + "; ".join(long_)


def test_limits_fit_the_defaults():
    """UX-10: a limit belongs to a number parameter, has one lower bound at
    most and keeps the library's own default; it prints as a plain number,
    so Data Checks (Python) and the form (TypeScript) word it the same."""
    bad = []
    for comp in load_library():
        for p in comp.parameters:
            has = [getattr(p, k) for k in LIMITS if getattr(p, k) is not None]
            where = f"{comp.name} · {p.label}"
            if has and p.type != "number":
                bad.append(f"{where}: limits on a {p.type}")
            elif p.minimum is not None and p.exclusiveMinimum is not None:
                bad.append(f"{where}: both minimum and exclusiveMinimum")
            elif has and (problem := p.range_problem(float(p.default))):
                bad.append(f"{where}: default {p.default} {problem}")
            bad += [f"{where}: {x!r} does not print as a plain number" for x in has
                    if "e" in f"{x:g}" or float(f"{x:g}") != x]
    assert not bad, "; ".join(bad)


# number parameters with no limit on purpose: a negative value has a meaning
# (a coast-down fit, a reverse-acting controller, a signal level) or the
# signal input that usually drives it takes any value
UNBOUNDED = {
    ("vehicle.body", "road_load_a_N"), ("vehicle.body", "road_load_b_N_per_kmh"),
    ("vehicle.body", "road_load_c_N_per_kmh2"), ("controller.dcdc", "power_setpoint_kW"),
    ("electric.constant_drive", "power_kW"), ("signal.constant", "value"),
    ("control.pid", "kp"), ("control.pid", "ki"), ("control.pid", "kd"),
    ("control.pid", "out_min"), ("control.pid", "out_max"),
}


def test_every_number_has_limits_or_is_listed_as_free():
    """UX-10: a negative inertia, drag or resistance ran without a word, or
    the solver quietly changed it. Every number parameter now has limits,
    unless it is listed above, so a new one needs a decision."""
    free = {(c.id, p.key) for c in load_library() for p in c.parameters
            if p.type == "number" and all(getattr(p, k) is None for k in LIMITS)}
    assert free == UNBOUNDED
