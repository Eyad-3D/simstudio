"""Golden-fixture snapshots of the bundled demo projects.

The fixtures freeze solver behavior (channels, summary, messages) so the
Phase-1 refactor can be verified against byte-level-stable results at every
step. Regenerate deliberately — only when a behavior change is intended:

    cd backend && python tests/update_golden.py
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from app.solver import simulate  # noqa: E402
from app.storage import load_example  # noqa: E402

GOLDEN_DIR = Path(__file__).parent / "golden"
CASES = [("bev-car", "case-city"), ("hybrid-car", "case-mixed")]
STRIDE = 10  # keep every Nth recorded point (plus the last) in the fixture


def snapshot(project_id: str, case_id: str) -> dict:
    """Run a bundled case and reduce the result to a comparable fixture."""
    result = simulate(load_example(project_id), case_id)
    channels: dict[str, dict] = {}
    for c in result.channels:
        pts = c.timeSeries
        idx = list(range(0, len(pts), STRIDE))
        if idx and idx[-1] != len(pts) - 1:
            idx.append(len(pts) - 1)
        channels[f"{c.elementId}:{c.portId}"] = {
            "unit": c.unit,
            "points": [
                [
                    pts[i]["t"],
                    None if pts[i]["value"] is None else round(pts[i]["value"], 6),
                ]
                for i in idx
            ],
        }
    return {
        "status": result.status,
        "messages": [f"{m.level}: {m.text}" for m in result.messages],
        "summary": {s.label: [round(s.value, 6), s.unit] for s in result.summary},
        "channels": channels,
    }


def fixture_path(project_id: str, case_id: str) -> Path:
    return GOLDEN_DIR / f"{project_id}__{case_id}.json"


def main() -> None:
    GOLDEN_DIR.mkdir(exist_ok=True)
    for project_id, case_id in CASES:
        data = snapshot(project_id, case_id)
        path = fixture_path(project_id, case_id)
        path.write_text(json.dumps(data, indent=1), encoding="utf-8")
        print(f"wrote {path} ({len(data['channels'])} channels, status {data['status']})")


if __name__ == "__main__":
    main()
