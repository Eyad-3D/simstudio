"""CON-26: the examples say only true things about themselves. Their monitor
and script ports carry the unit of the signal they are wired to, their
descriptions name only parts the model has, and a case that drives a typed
profile instead of a standard cycle says so in the run's messages."""
from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

from app.solver import simulate
from app.storage import load_example

ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = sorted(p.stem for p in (ROOT / "projects").glob("*.json"))
LIBRARY = {c["id"]: c for c in json.loads(
    (ROOT / "app" / "library" / "components.json").read_text(encoding="utf-8"))["components"]}

# words a description may use only when the model has one of these parts
PARTS_NAMED = {
    # no inverter part: an E-Motor's loss map holds its losses, which may be said
    r"\binverter\b(?!'s losses| losses)": set(),
    r"\bDC-?DC\b|\bconverter\b": {"controller.dcdc"},
    r"\bfuel[- ]cell\b": {"fuelcell.stack"},
    r"\bclutch\b": {"mech.clutch"},
    r"\bgearbox\b|\b\d-speed\b": {"mech.gearbox"},
    r"\bdifferential\b": {"mech.differential"},
    r"\bengine\b": {"engine.combustion"},
    r"\bbrakes?\b": {"mech.brake"},
    r"\bscript\b": {"signal.script"},
}


def _raw(example: str) -> dict:
    return json.loads((ROOT / "projects" / f"{example}.json").read_text(encoding="utf-8"))


def _elements(raw: dict) -> dict[str, dict]:
    return {e["id"]: e for s in raw["systems"] for e in s["elements"]}


@pytest.mark.parametrize("example", EXAMPLES)
def test_monitor_and_script_ports_carry_the_unit_they_are_wired_to(example):
    raw = _raw(example)
    els = _elements(raw)

    def port(el: dict, pid: str) -> dict:
        ports = LIBRARY[el["componentDefId"]].get("ports", []) + el.get("dynamicPorts", [])
        return next(p for p in ports if p["id"] == pid)

    for link in raw.get("dataBusConnections", []):
        for a, b in ((1, 2), (2, 1)):
            el = els[link[f"element{a}Id"]]
            if not el.get("dynamicPorts"):
                continue
            mine = port(el, link[f"port{a}Id"]).get("unitGroup") or "No Unit"
            other = els[link[f"element{b}Id"]]
            theirs = port(other, link[f"port{b}Id"]).get("unitGroup") or "No Unit"
            assert mine == theirs, (f"{example}: {el['label']}.{link[f'port{a}Id']} is "
                                    f"'{mine}', wired to {other['label']}'s '{theirs}'")


@pytest.mark.parametrize("example", EXAMPLES)
def test_descriptions_name_only_parts_the_model_has(example):
    raw = _raw(example)
    kinds = {e["componentDefId"] for e in _elements(raw).values()}
    text = raw.get("description", "")
    for pattern, needs in PARTS_NAMED.items():
        for m in re.finditer(pattern, text, re.IGNORECASE):
            # "no inverter part", "there is no ..." say what is missing, truthfully
            before = text[max(0, m.start() - 30):m.start()].lower()
            if re.search(r"\b(no|not|without)\b[^.]*$", before):
                continue
            assert kinds & needs, f"{example}: its description names '{m.group(0)}', which it has not"


@pytest.mark.parametrize(("example", "case", "typed"), [
    ("bev-car", "case-city", True), ("bev-car", "case-wltc", False),
    ("hybrid-car", "case-mixed", True),
])
def test_a_typed_profile_run_says_it_is_not_a_standard_cycle(example, case, typed):
    project = load_example(example)
    for c in project.cases:
        c.duration = min(c.duration, 2.0)  # the message comes before the run
    texts = [m.text for m in simulate(project, case).messages if m.level == "info"]
    said = any("not a standard drive cycle" in t for t in texts)
    assert said == typed
