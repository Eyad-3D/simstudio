"""Stored reference results of the shipped examples (CON-15).

Each example has ``projects/reference/<example>.json``: for every case that
is not a live (paced) copy, its status, every summary value and a few
comparison signals recorded every second (vehicle speed and target, battery
SOC and power, motor speed and torque, engine fuel rate, as the model has
them), plus how it was made (app version, git commit, solver step, date,
machine and the tests/golden/CHANGES.md entry that explains it).

The app opens an example with these runs already in Results, marked as
stored; Run recomputes them. tests/update_golden.py rewrites the files with
the golden fixtures, and test_example_cards.py checks that fresh runs still
give the stored summary.
"""
from __future__ import annotations

import datetime as dt
import json
import platform
import re
import subprocess
from pathlib import Path
from typing import Optional

from .schemas import Channel, SimMessage, SimResult, SummaryValue
from .version import VERSION

DIR = Path(__file__).resolve().parents[1] / "projects" / "reference"
# (component type, port) of the comparison signals, as the model has them
SIGNALS = [
    ("vehicle.body", "sig_speed"), ("signal.driving_task", "sig_demand"),
    ("battery.generic", "sig_soc"), ("battery.generic", "sig_power"),
    ("motor.emotor", "sig_speed"), ("motor.emotor", "sig_torque"),
    ("engine.combustion", "sig_fuel_rate"),
]
STEP_S = 1.0  # recorded every second


def path_of(example_id: str) -> Path:
    return DIR / f"{example_id}.json"


def _git_commit() -> Optional[str]:
    try:
        return subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True,
                              text=True, check=True, cwd=DIR.parent).stdout.strip() or None
    except (OSError, subprocess.CalledProcessError):
        return None


def record(project, result: SimResult) -> dict:
    """A case's stored result: status, summary and the comparison signals at
    1 s (the run's own points where it records more often)."""
    types = {el.id: el.componentDefId for s in project.systems for el in s.elements}
    wanted = [(t, p) for t, p in SIGNALS]
    signals = []
    times: list[float] = []
    for ch in result.channels:
        if (types.get(ch.elementId), ch.portId) not in wanted:
            continue
        keep = [i for i, p in enumerate(ch.timeSeries)  # every second, and the end
                if i == 0 or i == len(ch.timeSeries) - 1
                or int(p["t"] / STEP_S + 1e-9) > int(ch.timeSeries[i - 1]["t"] / STEP_S + 1e-9)]
        times = times or [round(ch.timeSeries[i]["t"], 3) for i in keep]
        signals.append({"elementId": ch.elementId, "portId": ch.portId, "label": ch.label,
                        "unit": ch.unit, "values": [ch.timeSeries[i]["value"] for i in keep]})
    signals.sort(key=lambda s: [(t, p) for t, p in wanted].index(
        (types[s["elementId"]], s["portId"])))
    return {"status": result.status,
            "messages": [m.model_dump() for m in result.messages if m.level != "info"],
            "summary": [s.model_dump(exclude_none=True) for s in result.summary],
            "references": [r.model_dump(exclude_none=True) for r in result.references],
            "t": times, "signals": signals}


def write(example_id: str, project, results: dict[str, SimResult], note: str) -> Path:
    data = {
        "_about": "Stored reference results of this example (CON-15): written by "
                  "backend/tests/update_golden.py, explained in tests/golden/CHANGES.md.",
        "creation": {"appVersion": VERSION, "gitCommit": _git_commit(),
                     "solver": "fixed step, at most 10 ms; recorded every 1 s",
                     "date": dt.date.today().isoformat(),
                     "machine": f"{platform.system()} {platform.machine()}, Python "
                                f"{platform.python_version()}",
                     "note": note},
        "cases": {cid: record(project, r) for cid, r in results.items()},
    }
    DIR.mkdir(parents=True, exist_ok=True)
    path = path_of(example_id)
    text = json.dumps(data, indent=1, ensure_ascii=False)
    # a list of numbers on one line: a signal is one line, not 1,800
    text = re.sub(r"\[\s*((?:-?[\d.eE+-]+|null)(?:,\s*(?:-?[\d.eE+-]+|null))*)\s*\]",
                  lambda m: "[" + re.sub(r",\s+", ",", m.group(1)) + "]", text)
    path.write_text(text + "\n", encoding="utf-8")
    return path


def load(example_id: str) -> Optional[dict]:
    path = path_of(example_id)
    if not path.is_file():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def stored_results(example_id: str) -> list[dict]:
    """The example's stored runs as the app shows them: case id, its result
    (the comparison signals only) and how it was made."""
    data = load(example_id)
    if data is None:
        return []
    out = []
    for case_id, rec in data["cases"].items():
        result = SimResult(
            caseId=case_id, status=rec["status"],
            messages=[SimMessage(level="info", text=(
                f"Stored result, made with LightSim {data['creation']['appVersion']} on "
                f"{data['creation']['date']}: only the comparison signals are kept. Press "
                f"Run to recompute every channel."))]
            + [SimMessage(**m) for m in rec["messages"]],
            channels=[Channel(elementId=s["elementId"], portId=s["portId"], label=s["label"],
                              unit=s["unit"],
                              timeSeries=[{"t": t, "value": v}
                                          for t, v in zip(rec["t"], s["values"])])
                      for s in rec["signals"]],
            summary=[SummaryValue(**s) for s in rec["summary"]],
            references=rec.get("references", []))
        out.append({"caseId": case_id, "result": result.model_dump(),
                    "creation": data["creation"]})
    return out
