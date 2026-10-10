"""Command-line export and import (STD-09, STD-10, STD-36).

The desktop app's engine runs these when its first argument names one:

  lightsim-backend run PROJECT.json|EXAMPLE [--case NAME] [--out FILE.mat|.csv] [--json]
  lightsim-backend export RUN.json[.gz] [--format mat|csv|json] [--out FILE]
  lightsim-backend import-table FILE --part TYPE --param KEY [--mode M]
                                [--sheet NAME] [--range A1:D20]
                                [--decimal comma|point] [--json]
  lightsim-backend params export PROJECT.json --out FILE.xlsx|.csv
  lightsim-backend params import PROJECT.json SHEET.xlsx|.csv [--out NEW.json] [--json]

From a source checkout: ``python run_backend.py run ...`` in backend/.

Exit codes (as the planned ``lightsim`` command, AI-02, will use them):
0 done; 1 Data Checks found errors, or the file has problems; 2 the run
finished but a result is "not valid" or the run failed; 3 a usage or file
error.

This is the export and import part of the command-line tool; running
studies, checks and the Python package come with AI-02.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import time
from pathlib import Path
from typing import Any, Optional

from ..schemas import Project, RunSnapshot, StoredRun
from ..version import VERSION

COMMANDS = ("run", "export", "import-table", "params")

OK, CHECKS_FAILED, NOT_VALID, USAGE = 0, 1, 2, 3


class _Usage(Exception):
    pass


def _load_project(path: str) -> Project:
    if not Path(path).exists():  # an example's id (bev-car), as the lightsim tool takes
        from ..storage import load_example

        try:
            return load_example(path)
        except (FileNotFoundError, ValueError):
            pass  # not an example either: say the file is missing
    try:
        raw = json.loads(Path(path).read_text(encoding="utf-8"))
    except OSError as e:
        raise _Usage(f"cannot read {path}: {e.strerror or e}")
    except ValueError as e:
        raise _Usage(f"{path} is not a LightSim project (not JSON: {e})")
    try:
        return Project.model_validate(raw)
    except ValueError as e:
        raise _Usage(f"{path} is not a LightSim project: {str(e).splitlines()[0]}")


def model_hash(project: Project) -> str:
    """SHA-256 of the project without its studies as JSON with sorted keys:
    the fingerprint runs made in the app carry (frontend/src/provenance.ts)."""
    model = project.model_dump(mode="json", exclude={"studies"})
    text = json.dumps(model, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _pick_case(project: Project, wanted: Optional[str]):
    if not project.cases:
        raise _Usage("the project has no cases")
    if not wanted:
        return project.cases[0]
    w = wanted.strip().lower()
    for c in project.cases:
        if c.id.lower() == w or c.name.lower() == w:
            return c
    starts = [c for c in project.cases if c.name.lower().startswith(w)]
    if len(starts) == 1:
        return starts[0]
    names = ", ".join(f"'{c.name}'" for c in project.cases)
    raise _Usage(f"no case named '{wanted}'; the project's cases are {names}")


def _write(path: Path, data: bytes) -> None:
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    except OSError as e:
        raise _Usage(f"cannot write {path}: {e.strerror or e}")


def _print(obj: Any, as_json: bool, text: str) -> None:
    if as_json:  # ASCII (CO₂ as ₂): any reader decodes it, whatever its code page
        print(json.dumps(obj, indent=2))
    else:
        print(text)


def cmd_run(args: argparse.Namespace) -> int:
    from ..solver import simulate
    from ..validation import run_blockers, validate_project
    from . import results

    project = _load_project(args.project)
    case = _pick_case(project, args.case)
    errors = run_blockers(validate_project(project), case.id)
    if errors:
        out = {"status": "checks failed", "case": case.name,
               "errors": [c.text for c in errors]}
        _print(out, args.json, "Data Checks found errors:\n" + "\n".join(
            f"  - {c.text}" for c in errors))
        return CHECKS_FAILED
    started = int(time.time() * 1000)
    result = simulate(project, case.id)
    model = project.model_copy(update={"studies": []})
    run = StoredRun(
        id=f"cli-{started}", caseId=case.id, caseName=case.name, startedAt=started,
        status=result.status, result=result,
        snapshot=RunSnapshot(project=model, case=case, appVersion=VERSION,
                             modelHash=model_hash(project)))
    rt = results.table(run)
    fmt = args.format or (Path(args.out).suffix.lstrip(".").lower() if args.out else "mat")
    if fmt == "parquet":
        raise _Usage("the app's engine writes .mat and .csv; Parquet comes with the lightsim "
                     "Python package when pyarrow is installed (lightsim run --out x.parquet)")
    if fmt not in ("mat", "csv"):
        raise _Usage(f"--format must be mat or csv, not '{fmt}'")
    out_path = Path(args.out) if args.out else Path(
        f"{results.file_stem(run)}.{fmt}")
    _write(out_path, results.to_mat(rt) if fmt == "mat" else results.to_csv(rt))
    files = [str(out_path)]
    if fmt == "csv":
        card_path = out_path.with_suffix(".runcard.json")
        _write(card_path, results.run_card_json(rt))
        files.append(str(card_path))
    not_valid = rt.card["notValid"]
    summary = {s["label"]: s["value"] for s in rt.card["summary"]}
    out = {"status": result.status, "project": project.name, "case": case.name,
           "files": files, "summary": summary, "summaryUnits": {
               s["label"]: s["unit"] for s in rt.card["summary"]},
           "notValid": not_valid,
           "messages": [m.model_dump() for m in result.messages if m.level != "info"]}
    lines = [f"{case.name}: {result.status}. Wrote {', '.join(files)}."]
    lines += [f"  {s['label']}: {s['value']:g} {s['unit']}" for s in rt.card["summary"]]
    lines += [f"  not valid: {x['label']} ({x['why']})" for x in not_valid]
    _print(out, args.json, "\n".join(lines))
    if result.status in ("failed", "cancelled") or not_valid:
        return NOT_VALID
    return OK


def cmd_export(args: argparse.Namespace) -> int:
    import gzip

    from . import results

    path = Path(args.run)
    try:
        data = path.read_bytes()
    except OSError as e:
        raise _Usage(f"cannot read {path}: {e.strerror or e}")
    if data[:2] == b"\x1f\x8b":
        data = gzip.decompress(data)
    try:
        run = StoredRun.model_validate_json(data)
    except ValueError as e:
        raise _Usage(f"{path} is not a stored LightSim run: {str(e).splitlines()[0]}")
    rt = results.table(run)
    fmt = args.format
    body = {"mat": results.to_mat, "csv": results.to_csv,
            "json": results.run_card_json}[fmt](rt)
    ext = ".runcard.json" if fmt == "json" else f".{fmt}"
    out = Path(args.out) if args.out else Path(results.file_stem(run) + ext)
    _write(out, body)
    print(out)
    return OK


def cmd_import_table(args: argparse.Namespace) -> int:
    from . import tables
    from .sheets import SheetError, read_file

    try:
        data = Path(args.file).read_bytes()
    except OSError as e:
        raise _Usage(f"cannot read {args.file}: {e.strerror or e}")
    try:
        target = tables.target_for(args.part, args.param, args.mode)
    except KeyError:
        raise _Usage(f"'{args.part}' has no parameter '{args.param}'")
    except ValueError as e:
        raise _Usage(str(e))
    try:
        sheets = read_file(data, args.file, args.decimal)
    except SheetError as e:
        raise _Usage(str(e))
    sheet = next((s for s in sheets if s.name == args.sheet), None) if args.sheet else sheets[0]
    if sheet is None:
        raise _Usage(f"no sheet '{args.sheet}'; the file has "
                     + ", ".join(f"'{s.name}'" for s in sheets))
    opts = {"range": args.range} if args.range else {}
    res = tables.import_table(sheet, target, opts).as_dict()
    other = "point" if res["decimal"] == "comma" else "comma"
    question = [f"{res['decimalQuestion']} Give --decimal {other} if that is wrong."] \
        if res["decimalQuestion"] else []
    text = "\n".join(res["notes"] + question + [e["text"] for e in res["errors"]]
                     + ([json.dumps(res["value"])] if res["ok"] else []))
    _print(res, args.json, text)
    return OK if res["ok"] else CHECKS_FAILED


def cmd_params(args: argparse.Namespace) -> int:
    from . import params
    from .sheets import SheetError

    project = _load_project(args.project)
    if args.action == "export":
        if not args.out:
            raise _Usage("give the file to write with --out (.xlsx or .csv)")
        out = Path(args.out)
        data = params.export_csv(project) if out.suffix.lower() == ".csv" else \
            params.export_xlsx(project)
        _write(out, data)
        print(out)
        return OK
    if not args.sheet:
        raise _Usage("give the parameter sheet to import")
    try:
        data = Path(args.sheet).read_bytes()
    except OSError as e:
        raise _Usage(f"cannot read {args.sheet}: {e.strerror or e}")
    try:
        res = params.import_sheet(project, data, args.sheet)
    except SheetError as e:
        raise _Usage(str(e))
    d = res.as_dict()
    lines = [e["text"] for e in d["errors"]] + [w["text"] for w in d["warnings"]]
    lines += [f"{c['element']} · {c['parameter']}: {json.dumps(c['old'])} -> "
              f"{json.dumps(c['new'])} {c['unit']}" for c in d["changes"]]
    lines.append(f"{len(d['changes'])} change(s), {d['unchanged']} unchanged, "
                 f"{len(d['errors'])} problem(s).")
    if res.errors:
        _print(d, args.json, "\n".join(lines))
        return CHECKS_FAILED
    if args.out:
        raw = project.model_dump(mode="json")
        for c in res.changes:
            for s in raw["systems"]:
                for e in s["elements"]:
                    if e["id"] == c.elementId:
                        e.setdefault("parameterOverrides", {})[c.key] = c.new
        _write(Path(args.out), (json.dumps(raw, indent=2, ensure_ascii=False) + "\n")
               .encode("utf-8"))
        lines.append(f"Wrote {args.out}.")
    _print(d, args.json, "\n".join(lines))
    return OK


class _Parser(argparse.ArgumentParser):
    def error(self, message: str):  # usage errors exit 3, not argparse's 2
        self.print_usage(sys.stderr)
        self.exit(USAGE, f"{self.prog}: error: {message}\n")


def parser() -> argparse.ArgumentParser:
    p = _Parser(
        prog="lightsim-backend",
        description="LightSim's engine: run a case and export its results, or read tables "
                    "and parameter sheets.")
    sub = p.add_subparsers(dest="command", required=True, parser_class=_Parser)
    r = sub.add_parser("run", help="run a case of a project and write its results")
    r.add_argument("project", help="the project file (.json), or an example id (bev-car)")
    r.add_argument("--case", help="the case's name or id (default: the first case)")
    r.add_argument("--out", help="the results file (.mat or .csv)")
    r.add_argument("--format", choices=["mat", "csv"], help="default: from --out, else mat")
    r.add_argument("--json", action="store_true", help="print the outcome as JSON")
    r.set_defaults(func=cmd_run)
    e = sub.add_parser("export", help="export a stored run (runs/<project>/<run>.json.gz)")
    e.add_argument("run")
    e.add_argument("--format", choices=["mat", "csv", "json"], default="mat")
    e.add_argument("--out")
    e.set_defaults(func=cmd_export)
    t = sub.add_parser("import-table", help="read a table, map or profile from CSV or .xlsx")
    t.add_argument("file")
    t.add_argument("--part", required=True, help="the part type, e.g. motor.emotor")
    t.add_argument("--param", required=True, help="the parameter key, e.g. power_loss")
    t.add_argument("--mode", help="Road Profile: distance or time")
    t.add_argument("--sheet")
    t.add_argument("--range", help="the cells to read, e.g. B3:F20")
    t.add_argument("--decimal", choices=["comma", "point"],
                   help="a CSV file's decimal mark (default: as its cells show)")
    t.add_argument("--json", action="store_true")
    t.set_defaults(func=cmd_import_table)
    pa = sub.add_parser("params", help="a project's parameters as a spreadsheet, and back")
    pa.add_argument("action", choices=["export", "import"])
    pa.add_argument("project")
    pa.add_argument("sheet", nargs="?", help="import: the sheet to read")
    pa.add_argument("--out", help="export: the sheet to write; import: write the changed "
                                  "project here")
    pa.add_argument("--json", action="store_true")
    pa.set_defaults(func=cmd_params)
    return p


def _plain_streams() -> None:
    """Never stop on a character the output cannot encode. Windows gives a
    pipe (MATLAB's system(), a script) the ANSI code page, which has no
    "₂": a pipe gets UTF-8, a console replaces what it cannot show."""
    for stream in (sys.stdout, sys.stderr):
        try:
            if stream.isatty():
                stream.reconfigure(errors="replace")  # type: ignore[union-attr]
            else:
                stream.reconfigure(encoding="utf-8")  # type: ignore[union-attr]
        except (AttributeError, ValueError, OSError):
            pass


def main(argv: Optional[list[str]] = None) -> int:
    _plain_streams()
    args = parser().parse_args(argv)
    try:
        return args.func(args)
    except _Usage as e:
        print(f"lightsim: {e}", file=sys.stderr)
        return USAGE


if __name__ == "__main__":
    sys.exit(main())
