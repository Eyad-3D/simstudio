"""Golden fixtures of the bundled examples: report or regenerate.

    cd backend
    python tests/update_golden.py                   # diff report, writes nothing
    python tests/update_golden.py --reason "Title"  # regenerate every fixture

Regenerate only for an intended behaviour change. The reason is required:
it becomes the heading of a new entry in tests/golden/CHANGES.md, with the
headline numbers that moved (old -> new) filled in, and every fixture
records it (test_golden checks that the entry exists). Add the why in prose
under the heading. See golden_compare.py for what the tests compare and
with which tolerances; LIGHTSIM_GOLDEN_EXACT=1 demands identical results.
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from golden_compare import (  # noqa: E402
    CASES,
    CHANGES,
    GOLDEN_DIR,
    compare,
    dump,
    fixture_path,
    load,
    report,
    run,
    snapshot,
)

CASE_NAMES = {"case-city": "City Cycle", "case-mixed": "Mixed Cycle"}


def changelog_rows(project_id: str, case_id: str, variant: str, cmp) -> list[str]:
    where = f"{project_id} {CASE_NAMES.get(case_id, case_id)} ({variant} step)"
    rows = []
    if cmp.status[0] != cmp.status[1]:
        rows.append(f"| {where} | status | {cmp.status[0]} | {cmp.status[1]} | |")
    for h in cmp.headlines:
        if not h.changed:
            continue
        if h.old is None or h.new is None:
            change = "new row" if h.old is None else "row gone"
        else:
            change = f"{h.new - h.old:+.4g}" + (f" ({100 * (h.new - h.old) / h.old:+.2f} %)"
                                                if h.old else "")
        old = "—" if h.old is None else f"{h.old:g} {h.unit}"
        new = "—" if h.new is None else f"{h.new:g} {h.unit}"
        rows.append(f"| {where} | {h.label} | {old} | {new} | {change} |")
    if cmp.moved:
        rows.append(f"| {where} | channels that moved | | {len(cmp.moved)} | "
                    f"{len(cmp.misses)} outside their tube |")
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--reason", help="why the fixtures change: the CHANGES.md heading")
    parser.add_argument("--verbose", action="store_true", help="list every headline number")
    args = parser.parse_args()

    results, comparisons, rows = {}, [], []
    for project_id, case_id, variant in CASES:
        path = fixture_path(project_id, case_id, variant)
        result = run(project_id, case_id, variant)
        results[(project_id, case_id, variant)] = result
        if path.exists() and load(path).get("format") == 2:
            cmp = compare(load(path), result)
            comparisons.append(cmp)
            print(report(cmp, verbose=args.verbose), "\n", flush=True)
            rows += changelog_rows(project_id, case_id, variant, cmp)
        else:
            print(f"### {path.name}: no v2 fixture yet\n", flush=True)

    if not args.reason:
        print("Nothing written. To regenerate: python tests/update_golden.py --reason '<why>'")
        return 0 if all(c.ok for c in comparisons) else 1

    GOLDEN_DIR.mkdir(exist_ok=True)
    for (project_id, case_id, variant), result in results.items():
        path = fixture_path(project_id, case_id, variant)
        path.write_text(dump(snapshot(project_id, case_id, variant, result=result,
                                      change=args.reason)), encoding="utf-8")
        print(f"wrote {path.name}")

    text = CHANGES.read_text(encoding="utf-8")
    if f"## {args.reason}\n" in text:
        print(f"CHANGES.md already has '## {args.reason}': add the rows below yourself.")
        print("\n".join(rows))
    else:
        entry = [f"## {args.reason}", "", "(Why the behaviour changed: write it here.)", ""]
        if rows:
            entry += ["| Fixture | Number | Old | New | Change |", "|---|---|---|---|---|", *rows, ""]
        else:
            entry += ["No headline number or channel moved.", ""]
        CHANGES.write_text(text.rstrip("\n") + "\n\n" + "\n".join(entry), encoding="utf-8")
        print(f"added '## {args.reason}' to {CHANGES.name}: describe the change there")
    return 0


if __name__ == "__main__":
    sys.exit(main())
