"""The examples' stored results (CON-15): report or regenerate.

    cd backend
    python tests/update_stored.py                   # what would change, writes nothing
    python tests/update_stored.py --reason "Title"  # rewrite projects/reference/

An example opens with these runs in Results, so they are what the app's
engine computes: every case that is not a live (paced) copy runs as a user's
Run does (``simulate``, the app's engine, never LIGHTSIM_ENGINE=today). A case
the app's engine does not run stops the regeneration with its message; no
other engine's figure is kept in its place. The reason becomes a heading in
tests/golden/CHANGES.md with each case's headline figures before and after
(add the why in prose under it); test_example_cards checks the entry exists
and that a fresh run gives the stored figures.

The golden fixtures (tests/golden/*.json, update_golden.py) are a different
thing: the engines' own reference, not touched here.
"""
from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

os.environ.pop("LIGHTSIM_ENGINE", None)  # the app's engine, as a user's Run
sys.path.insert(0, str(Path(__file__).parent.parent))

from app import reference_results  # noqa: E402
from app.solver import simulate  # noqa: E402
from app.storage import load_example  # noqa: E402

EXAMPLES = ("aero-bev", "bev-car", "fs-electric", "hybrid-car")
CHANGES = Path(__file__).parent / "golden" / "CHANGES.md"
# the rows a case's line in CHANGES.md gives (with the case's own expected values)
HEADLINES = ("Consumption", "Fuel consumption", "Fuel-economy equivalent (MPGe, AC)", "Lap time",
             "Total time", "Energy per lap", "Time to 75 m")


def headline(label: str, case) -> bool:
    return (label in HEADLINES or label.endswith(("final SOC", "energy delivered"))
            or any(r.kpi == label for r in case.references))


def rows_of(example_id: str, case, old: dict | None, result) -> list[str]:
    where = f"{example_id} {case.name}"
    before = {s["label"]: s for s in (old or {}).get("summary", [])}
    out = []
    if old and old["status"] != result.status:
        out.append(f"| {where} | status | {old['status']} | {result.status} | |")
    for s in result.summary:
        if not headline(s.label, case) or not isinstance(s.value, (int, float)):
            continue
        o = before.get(s.label, {}).get("value")
        change = "" if not isinstance(o, (int, float)) else f"{s.value - o:+.4g}" + (
            f" ({100 * (s.value - o) / o:+.2f} %)" if o else "")
        old_text = "—" if o is None else f"{o:g} {s.unit}"
        out.append(f"| {where} | {s.label} | {old_text} | {s.value:g} {s.unit} | {change} |")
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--reason", help="why the stored results change: the CHANGES.md heading")
    args = parser.parse_args()

    runs, rows, refused = {}, [], []
    for example_id in EXAMPLES:
        project = load_example(example_id)
        old = (reference_results.load(example_id) or {}).get("cases", {})
        runs[example_id] = (project, {})
        for case in project.cases:
            if case.realtimeFactor:
                continue
            result = simulate(project, case.id)
            if result.engine != "new" or result.status != "success":
                why = "; ".join(m.text for m in result.messages if m.level == "error")[:300]
                refused.append(f"{example_id} {case.id}: {result.engine} {result.status} {why}")
            runs[example_id][1][case.id] = result
            rows += rows_of(example_id, case, old.get(case.id), result)
            print(f"{example_id} {case.id}: {result.status} ({result.engine})", flush=True)
    print("\n".join(["| Case | Figure | Before | After | Change |", "|---|---|---|---|---|", *rows]))
    if refused:
        print("Not written: the app's engine did not run these cases:\n" + "\n".join(refused))
        return 1
    if not args.reason:
        print("Nothing written. To regenerate: python tests/update_stored.py --reason '<why>'")
        return 0

    for example_id, (project, results) in runs.items():
        print(f"wrote {reference_results.write(example_id, project, results, args.reason).name}")
    text = CHANGES.read_text(encoding="utf-8")
    if f"## {args.reason}\n" in text:
        print(f"CHANGES.md already has '## {args.reason}': add the rows above yourself.")
    else:
        entry = [f"## {args.reason}", "", "(Why the stored results changed: write it here.)", "",
                 "| Case | Figure | Before | After | Change |", "|---|---|---|---|---|", *rows, ""]
        CHANGES.write_text(text.rstrip("\n") + "\n\n" + "\n".join(entry), encoding="utf-8")
        print(f"added '## {args.reason}' to {CHANGES.name}: describe the change there")
    return 0


if __name__ == "__main__":
    sys.exit(main())
