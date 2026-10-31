"""``python -m benchmarks.agent validate [ids]``: run every task's
reference, blank, wrong and other right solutions through its checker.
``python -m benchmarks.agent start <id> <folder>``: write a task's starting
files, to look at them."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

from . import (
    SMOKE_DIR,
    TASKS_DIR,
    heldout,
    heldout_problems,
    isolate,
    load_tasks,
    start_files,
    validate,
    write_files,
)


def main(argv=None) -> int:
    isolate()
    p = argparse.ArgumentParser(prog="python -m benchmarks.agent")
    sub = p.add_subparsers(dest="command", required=True)
    v = sub.add_parser("validate", help="check every task's solutions and keys")
    v.add_argument("ids", nargs="*")
    s = sub.add_parser("start", help="write a task's starting files")
    s.add_argument("id")
    s.add_argument("folder")
    args = p.parse_args(argv)
    tasks = {t.id: t for t in load_tasks(TASKS_DIR) + load_tasks(SMOKE_DIR)}
    if args.command == "start":
        write_files(start_files(tasks[args.id]), Path(args.folder))
        return 0
    problems = heldout_problems(load_tasks(TASKS_DIR), heldout())
    for task in [tasks[i] for i in args.ids] if args.ids else tasks.values():
        found = validate(task)
        print(f"{task.id:28} {'ok' if not found else 'FAIL'}")
        problems += [f"{task.id}: {x}" for x in found]
    print("\n".join(problems) or "All tasks valid.")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
