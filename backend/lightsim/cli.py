"""The ``lightsim`` command: run, check and read LightSim models from a
terminal or a CI pipeline, without opening the app.

    lightsim run car.json --case "City Cycle" --out city.csv
    lightsim check car.json --json

Exit codes (docs/help/reference/command-line.md):
  0  done: the checks passed, or the run is a success with every figure valid
  1  the Data Checks found an error (``check``; ``run`` before it starts)
  2  the run finished but is not valid (warning, failed or cancelled, or a
     figure marked not valid)
  3  a usage or file error (unknown option, missing file, case or part)

The desktop app's engine takes the same commands:
``lightsim-backend run …`` (see command-line.md for where it is installed).
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Optional, Sequence

EXIT_OK, EXIT_CHECKS, EXIT_NOT_VALID, EXIT_USAGE = 0, 1, 2, 3

#: the subcommands, for the engine's entry point to tell them from its own options
COMMANDS = ("run", "check", "edit", "export", "show", "params", "parts", "examples", "schema",
            "notebook", "ai", "version")


class UsageError(Exception):
    pass


class _Parser(argparse.ArgumentParser):
    def error(self, message: str):  # argparse's own usage errors exit with 2
        self.print_usage(sys.stderr)
        raise UsageError(message)


def _out(args, data, text: str) -> None:
    if getattr(args, "json", False):
        print(json.dumps(data, ensure_ascii=False, indent=1))
    elif text:
        print(text)


def _fmt(v: float) -> str:
    return f"{v:,.6g}" if abs(v) < 1e6 else f"{v:,.0f}"


def _run_exit(result) -> int:
    if result.checks and any(c.level == "error" for c in result.checks):
        return EXIT_CHECKS
    return EXIT_OK if result.valid else EXIT_NOT_VALID


def _write_outputs(result, outs: Sequence[str]) -> list[str]:
    written = []
    for out in outs:
        path = Path(out.replace("{case}", _safe(result.case_name or result.case_id)))
        suffix = path.suffix.lower()
        if suffix in (".csv", ".mat", ".json", ".parquet"):
            _keep_what_it_replaces(path)
        if suffix == ".csv":
            result.to_csv(path)
        elif suffix == ".mat":
            result.to_mat(path)
        elif suffix == ".json":
            result.to_json(path)
        elif suffix == ".parquet":
            try:
                result.to_parquet(path)
            except ImportError as e:
                raise UsageError(f"--out '{out}': {e}")
        else:
            raise UsageError(f"--out '{out}': give a .csv, .mat, .json or .parquet file.")
        written.append(str(path))
    return written


def _keep_what_it_replaces(path: Path) -> None:
    """Before an --out file replaces one that is there: a LightSim project
    is never replaced (results over the model are a slip, or an instruction
    planted in a file an agent read); any other file is kept as it was in
    its backups (``<file>-backups``, the newest storage.KEEP_BACKUPS), so an
    agent's --out can be undone to the exact earlier file."""
    from ._engine import engine

    if not path.is_file():
        return
    storage = engine("storage")
    old = path.read_bytes()
    try:
        storage.read_project(old)
    except storage.NewerFileError:
        pass  # a project of a newer LightSim
    except Exception:  # not a project: any other error reading it as one
        loc = storage.location_of_file(path)
        with storage.project_lock(loc):
            storage._keep_backup(loc, path, old)
        return
    raise UsageError(f"'{path}' is a LightSim project; --out does not replace it. "
                     "Give another file name.")


def _safe(name: str) -> str:
    return "".join(c if c.isalnum() or c in "-_." else "_" for c in name).strip("_") or "case"


def _result_text(result, written: list[str]) -> str:
    lines = [f"{result.project_name} — {result.case_name or result.case_id}: {result.status}"]
    for c in result.checks:
        if c.level != "info":
            lines.append(f"  {c.level}: {c.text}" + (f" How to fix: {c.fix}" if c.fix else ""))
    for m in result.messages:
        if m.level != "info":
            lines.append(f"  {m.level}: {m.text}")
    if result.summary:
        width = max(len(k.label) for k in result.summary)
        for k in result.summary:
            note = f"  (not valid: {k.not_valid})" if k.not_valid else ""
            mark = "" if k.passed is None else ("  pass" if k.passed else "  FAIL")
            lines.append(f"  {k.label:<{width}}  {_fmt(k.value):>12} {k.unit}{mark}{note}")
    lines += [f"  wrote {w}" for w in written]
    return "\n".join(lines)


# -- commands -------------------------------------------------------------------
def cmd_run(args) -> int:
    from . import Project
    from ._engine import engine

    project = Project.load(args.project)
    cases = [c.id for c in project.cases] if args.all_cases else [project.case(args.case).id]
    _apply_sets(project, args.set or [], cases)
    if len(cases) > 1 and any("{case}" not in o for o in args.out or []):
        raise UsageError("With --all-cases, put {case} in each --out file name.")
    # Hands-off, as the MCP server's runs: Script blocks and FMUs run in the
    # strict OS sandbox (project-only files, network held off) with no per-user
    # approval, and the run is refused where that sandbox cannot be set up.
    # LIGHTSIM_STRICT_SANDBOX=0 gives a person the app's rules instead.
    sandbox = engine("solver.sandbox")
    token = sandbox.STRICT.set(sandbox.command_line_strict())
    try:
        code, payload, texts = EXIT_OK, [], []
        for case_id in cases:
            result = project.run(case_id, check=not args.no_check, time_limit_s=args.time_limit)
            written = _write_outputs(result, args.out or [])
            rc = _run_exit(result)
            code = max(code, rc)
            payload.append({**result.to_dict(channels=False), "exitCode": rc, "wrote": written})
            texts.append(_result_text(result, written))
    finally:
        sandbox.STRICT.reset(token)
    _out(args, payload[0] if len(payload) == 1 else payload, "\n\n".join(texts))
    return code


def _apply_sets(project, assignments: Sequence[str], cases: Sequence[str]) -> None:
    """Apply --set values. Each is set on the part and as the own value of
    every case that runs: a case's own value wins over the part's, so
    setting the part alone would leave a case that sets that key (the
    hybrid's start charge, the HVAC case's power) running on its own."""
    for assignment in assignments:
        ref, sep, value = assignment.partition("=")
        if not sep:
            raise UsageError(f"--set '{assignment}': write Part.parameter=value.")
        ref, typed = ref.strip(), _typed(value.strip())
        project.set(ref, typed)
        for case_id in cases:
            project.set(ref, typed, case=case_id)


def _typed(text: str):
    """A --set value: true/false, a number, JSON (a table), else text (which
    may carry a unit: "150 kW")."""
    low = text.lower()
    if low in ("true", "false"):
        return low == "true"
    try:
        return float(text) if any(c in text for c in ".eE") else int(text)
    except ValueError:
        pass
    if text[:1] in "{[":
        try:
            return json.loads(text)
        except ValueError:
            pass
    return text


def cmd_check(args) -> int:
    from . import Project

    project = Project.load(args.project)
    checks = project.check()
    errors = sum(c.level == "error" for c in checks)
    warnings = sum(c.level == "warning" for c in checks)
    code = EXIT_CHECKS if errors or (args.strict and warnings) else EXIT_OK
    lines = [f"{project.name}: {errors} error(s), {warnings} warning(s)"]
    lines += [f"  {c.level}: {c.text}" + (f" How to fix: {c.fix}" if c.fix else "")
              for c in checks if c.level != "info" or args.verbose]
    _out(args, {"project": project.name, "errors": errors, "warnings": warnings,
                "checks": [c.to_dict() for c in checks], "exitCode": code}, "\n".join(lines))
    return code


def cmd_edit(args) -> int:
    """Apply operations to a project and save it (AI-02). The same operations
    as the AI plug's ``model_edit`` tool, so an edit works the same from the
    command line as over MCP. The save is one step in the project's history
    (with the reason, client and session), with the previous version kept
    as a backup, so the open app shows the edit and can undo it."""
    from . import Project
    from ._engine import engine

    edits = engine("ai.edits")
    project = Project.load(args.project)
    ops: list = []
    for assignment in args.set or []:
        ref, sep, value = assignment.partition("=")
        if not sep:
            raise UsageError(f"--set '{assignment}': write Part.parameter=value.")
        element, _, param = ref.strip().rpartition(".")
        if not element or not param:
            raise UsageError(f"--set '{assignment}': write Part.parameter=value.")
        ops.append({"op": "set", "element": element, "param": param, "value": _typed(value.strip())})
    for one in args.op or []:
        try:
            ops.append(json.loads(one))
        except ValueError as e:
            raise UsageError(f"--op is one JSON operation object: {e}") from None
    if args.ops:
        try:
            batch = json.loads(args.ops)
        except ValueError as e:
            raise UsageError(f"--ops is a JSON array of operations: {e}") from None
        if not isinstance(batch, list):
            raise UsageError("--ops is a JSON array of operations.")
        ops += batch
    if not ops:
        raise UsageError("Give at least one change: --set, --op or --ops.")
    target = Path(args.out) if args.out else project.path
    if target is None:
        raise UsageError("This project is an example: give --out, a file to save the edit to.")
    storage = engine("storage")
    loc = storage.location_of_file(target)
    client = (args.client or os.environ.get("LIGHTSIM_CLIENT")
              or os.environ.get("AI_AGENT") or "command line")
    meta = {"by": "agent", "tool": "lightsim edit", "reason": args.reason, "client": client,
            "session": args.session or os.environ.get("LIGHTSIM_SESSION")
            or os.environ.get("CLAUDE_CODE_SESSION_ID") or _go_on_session(loc, client)}
    # Saved the way the app saves: the version it replaces kept in the
    # backups (the newest storage.KEEP_BACKUPS) and one line in the
    # project's history, which the open window reads to show the change and
    # undo it. A save from the app between the read and the write is not
    # overwritten: the operations are applied again to the newer file.
    in_place = project.path is not None and target == project.path
    for attempt in range(3):
        revision = None
        if in_place:
            project, revision = _load_unchanged(target, storage)
        try:
            edited, changes = edits.apply_operations(project.model, ops)
        except edits.EditError as e:
            raise UsageError(str(e)) from None
        project.model = edited
        data = edited.model_dump_json(indent=2).encode("utf-8")
        try:
            storage.write_file(loc, data, revision, meta={**meta, "changes": changes})
            break
        except storage.ConflictError:
            if attempt == 2:
                raise UsageError(f"{target} kept changing while it was being saved; "
                                 "nothing was saved. Try again.") from None
    project.path = target
    checks = project.check()
    errors = sum(c.level == "error" for c in checks)
    backups = loc.backups
    where = (f"one step in the project's history ({backups / 'history.jsonl'}), which the open "
             f"app shows and undoes in one click; the previous versions are kept in {backups} "
             f"(the newest {storage.KEEP_BACKUPS})")
    if args.out and not in_place:
        where += "; the original project is unchanged"
    lines = [f"Saved {target} ({len(changes)} change{'s' if len(changes) != 1 else ''})"]
    lines += [f"  {c}" for c in changes]
    lines.append(f"  ({where})")
    lines += [f"  {c.level}: {c.text}" + (f" How to fix: {c.fix}" if c.fix else "")
              for c in checks if c.level != "info"]
    _out(args, {"saved": str(target), "changes": changes, "backups": str(backups),
                "history": str(backups / "history.jsonl"), "session": meta["session"],
                "recoverable": where, "errors": errors,
                "checks": [c.to_dict() for c in checks]}, "\n".join(lines))
    return EXIT_OK


#: Saves by ``lightsim edit`` this close together, with no other save between,
#: are one session when none is named.
SESSION_GAP_MS = 15 * 60 * 1000


def _go_on_session(loc, client: str) -> str:
    """The session of an edit that names none. An agent's shell tool starts
    a new shell for each call, so the shell says nothing: go on with the
    session of the project's newest save when that is this client's
    ``lightsim edit`` with no session named, made less than SESSION_GAP_MS
    ago; else start one."""
    import time
    import uuid

    from ._engine import engine

    lines = engine("history").entries(loc.backups)
    last = lines[-1] if lines else {}
    # ponytail: two agents with the same client editing one file within the
    # gap share a session; they can name their own with LIGHTSIM_SESSION
    if (last.get("tool") == "lightsim edit" and last.get("client") == client
            and str(last.get("session") or "").startswith("cli-")
            and time.time() * 1000 - (last.get("t") or 0) < SESSION_GAP_MS):
        return last["session"]
    return f"cli-{uuid.uuid4().hex[:12]}"


def _load_unchanged(path: Path, storage):
    """The project at ``path`` and the revision of the bytes it was read from."""
    from . import Project

    while True:
        revision = storage.revision_of(path.read_bytes())
        project = Project.load(path)
        if storage.revision_of(path.read_bytes()) == revision:
            return project, revision


def cmd_export(args) -> int:
    from . import read_run

    try:
        result = read_run(args.run)
    except FileNotFoundError:
        raise UsageError(f"No run file '{args.run}'.") from None
    except (ValueError, OSError) as e:
        raise UsageError(f"'{args.run}' is not a LightSim run: {e}") from None
    written = _write_outputs(result, args.out)
    _out(args, {"wrote": written}, "\n".join(f"wrote {w}" for w in written))
    return EXIT_OK


def cmd_show(args) -> int:
    from . import Project

    project = Project.load(args.project)
    data = {
        "name": project.name, "id": project.id, "schemaVersion": project.model.schemaVersion,
        "description": project.model.description,
        "parts": [{"id": e.id, "label": e.label, "type": e.componentDefId}
                  for e in project.elements],
        "cases": [{"id": c.id, "name": c.name, "kind": c.kind, "duration": c.duration}
                  for c in project.cases],
        "hasScripts": project.has_scripts(),
    }
    lines = [f"{project.name} ({len(data['parts'])} parts)", "Cases:"]
    lines += [f"  {c['name']}  [{c['id']}, {c['kind']}, {c['duration']:g} s]" for c in data["cases"]]
    lines.append("Parts:")
    lines += [f"  {p['label']}  [{p['id']}, {p['type']}]" for p in data["parts"]]
    _out(args, data, "\n".join(lines))
    return EXIT_OK


def cmd_params(args) -> int:
    from . import Project
    from .project import iter_params

    project = Project.load(args.project)
    rows = [{"part": label, "key": key, "value": value, "unit": unit}
            for label, key, value, unit in iter_params(project)
            if args.part is None or label == args.part]
    if args.part is not None and not rows:
        project.element(args.part)  # says which parts there are
    lines = []
    for r in rows:
        shown = r["value"] if not isinstance(r["value"], (dict, list)) else "(table)"
        if isinstance(shown, str) and "\n" in shown:
            shown = "(code)"
        lines.append(f"{r['part']}.{r['key']} = {shown} {'' if r['unit'] == '-' else r['unit']}")
    _out(args, rows, "\n".join(lines))
    return EXIT_OK


def cmd_parts(args) -> int:
    from . import library

    defs = library()
    data = [{"type": cid, "name": d.name, "category": d.category,
             "parameters": [{"key": p.key, "label": p.label, "unit": p.unit, "type": p.type}
                            for p in d.parameters],
             "ports": [{"id": p.id, "name": p.name, "kind": p.kind, "direction": p.direction}
                       for p in d.ports]}
            for cid, d in sorted(defs.items())]
    if args.type:
        data = [d for d in data if d["type"] == args.type]
        if not data:
            raise UsageError(f"No part type '{args.type}'.")
        d = data[0]
        text = "\n".join([f"{d['type']}: {d['name']}", "Parameters:"]
                         + [f"  {p['key']} [{p['unit']}] {p['label']}" for p in d["parameters"]]
                         + ["Ports:"]
                         + [f"  {p['id']} ({p['kind']} {p['direction']}) {p['name']}"
                            for p in d["ports"]])
    else:
        text = "\n".join(f"{d['type']:<28} {d['name']}" for d in data)
    _out(args, data, text)
    return EXIT_OK


def cmd_examples(args) -> int:
    from . import examples

    names = examples()
    _out(args, names, "\n".join(names))
    return EXIT_OK


def cmd_schema(args) -> int:
    from .spec import json_schemas, write_schemas

    if args.out:
        for name in json_schemas():
            _keep_what_it_replaces(Path(args.out) / f"{name}.schema.json")
        paths = write_schemas(args.out)
        _out(args, [str(p) for p in paths], "\n".join(f"wrote {p}" for p in paths))
        return EXIT_OK
    schemas = json_schemas()
    if args.name:
        if args.name not in schemas:
            raise UsageError(f"No schema '{args.name}'. Schemas: {', '.join(schemas)}.")
        print(json.dumps(schemas[args.name], indent=2, ensure_ascii=False))
    else:
        print("\n".join(schemas))
    return EXIT_OK


def cmd_notebook(args) -> int:
    from . import Project
    from .notebook import notebook

    project = Project.load(args.project)
    case = project.case(args.case)
    out = Path(args.out or f"{_safe(project.name)}.ipynb")
    if out.exists() and not args.force:
        raise UsageError(f"'{out}' exists; add --force to replace it.")
    _keep_what_it_replaces(out)
    out.write_text(json.dumps(notebook(str(args.project), case.name), indent=1), encoding="utf-8")
    _out(args, {"wrote": str(out)}, f"wrote {out}")
    return EXIT_OK


def cmd_version(args) -> int:
    from . import API_VERSION, __version__
    from .spec import FORMAT_VERSIONS

    _out(args, {"version": __version__, "apiVersion": API_VERSION, "formats": FORMAT_VERSIONS},
         f"LightSim {__version__} (Python API {API_VERSION})")
    return EXIT_OK


def cmd_ai(args) -> int:
    from .ai_access import cli_ai

    return cli_ai(args, _out)


# -- parser -----------------------------------------------------------------------
def parser() -> argparse.ArgumentParser:
    p = _Parser(prog="lightsim", description="Run, check and read LightSim models.",
                epilog="Exit codes: 0 done, 1 Data Checks failed, 2 run not valid, "
                       "3 usage or file error.")
    p.add_argument("--version", action="store_true", help="print the version and exit")
    sub = p.add_subparsers(dest="command", parser_class=_Parser)

    def add(name: str, help_: str, json_out: bool = True):
        sp = sub.add_parser(name, help=help_, description=help_)
        if json_out:
            sp.add_argument("--json", action="store_true", help="print JSON instead of text")
        return sp

    r = add("run", "Run a case of a project and print its summary.")
    r.add_argument("project", help="project file, or an example id (bev-car)")
    r.add_argument("--case", "-c", help="case id or name (default: the first case)")
    r.add_argument("--all-cases", action="store_true", help="run every case")
    r.add_argument("--out", "-o", action="append",
                   help="write the results: .csv, .mat, .json or .parquet (Parquet needs "
                        "pyarrow; repeatable; {case} in the name is replaced by the case name)")
    r.add_argument("--set", action="append", metavar="PART.KEY=VALUE",
                   help="change a parameter for this run only, e.g. 'Vehicle.mass_kg=1900 kg'")
    r.add_argument("--no-check", action="store_true", help="skip the Data Checks")
    r.add_argument("--time-limit", type=float, metavar="S",
                   help="stop the run after S seconds of wall-clock time")
    r.set_defaults(func=cmd_run)

    c = add("check", "Run the Data Checks of a project.")
    c.add_argument("project")
    c.add_argument("--strict", action="store_true", help="warnings fail too (exit 1)")
    c.add_argument("--verbose", "-v", action="store_true", help="list info findings too")
    c.set_defaults(func=cmd_check)

    ed = add("edit", "Change a project and save it (the same operations as the AI plug).")
    ed.add_argument("project", help="project file, or an example id (needs --out)")
    ed.add_argument("--set", action="append", metavar="PART.KEY=VALUE",
                    help="set a parameter, e.g. 'Vehicle.mass_kg=1900 kg' (repeatable)")
    ed.add_argument("--op", action="append", metavar="JSON",
                    help="one operation as JSON, e.g. '{\"op\":\"add\",\"type\":\"mech.brake\","
                         "\"label\":\"Brake\"}' (repeatable)")
    ed.add_argument("--ops", metavar="JSON", help="a JSON array of operations")
    ed.add_argument("--out", "-o", help="save to this file (default: the project file in place; "
                                        "the previous versions are kept beside it in <file>-backups)")
    ed.add_argument("--reason", help="why, in one line: kept with the change in the project's "
                                     "history, which the open app shows")
    ed.add_argument("--client", help="who is editing (default: LIGHTSIM_CLIENT, AI_AGENT or "
                                     "'command line')")
    ed.add_argument("--session", help="group edits into one session that the app undoes in one "
                                      "click (default: LIGHTSIM_SESSION, CLAUDE_CODE_SESSION_ID, "
                                      "else the session of the file's last lightsim edit if "
                                      "it named none and was under 15 minutes ago)")
    ed.set_defaults(func=cmd_edit)

    e = add("export", "Write a stored run (.json.gz from the app, or .json) as CSV, MAT, JSON "
                      "or Parquet.")
    e.add_argument("run", help="the run file")
    e.add_argument("--out", "-o", action="append", required=True,
                   help=".csv, .mat, .json or .parquet (Parquet needs pyarrow)")
    e.set_defaults(func=cmd_export)

    s = add("show", "List a project's cases and parts.")
    s.add_argument("project")
    s.set_defaults(func=cmd_show)

    pa = add("params", "List a project's parameter values.")
    pa.add_argument("project")
    pa.add_argument("--part", help="only this part (its label)")
    pa.set_defaults(func=cmd_params)

    t = add("parts", "List the part types of the library, or one type's parameters and ports.")
    t.add_argument("type", nargs="?", help="a part type, e.g. motor.emotor")
    t.set_defaults(func=cmd_parts)

    x = add("examples", "List the examples that come with LightSim.")
    x.set_defaults(func=cmd_examples)

    sc = add("schema", "Print or write the JSON Schemas of LightSim's files.", json_out=False)
    sc.add_argument("name", nargs="?", help="one schema (project, run, result, …)")
    sc.add_argument("--out", help="write every schema into this folder")
    sc.set_defaults(func=cmd_schema)

    n = add("notebook", "Write a Jupyter notebook that runs a project and plots its results.")
    n.add_argument("project")
    n.add_argument("--case", "-c")
    n.add_argument("--out", "-o", help="the .ipynb file (default: <project name>.ipynb)")
    n.add_argument("--force", action="store_true", help="replace an existing file")
    n.set_defaults(func=cmd_notebook)

    from .ai_access import add_ai_parser

    add_ai_parser(add("ai", "Show or change what AI assistants may see and do (off by default)."))
    sub.choices["ai"].set_defaults(func=cmd_ai)

    v = add("version", "Print the version of LightSim, its Python API and its file formats.")
    v.set_defaults(func=cmd_version)
    return p


def main(argv: Optional[Sequence[str]] = None) -> int:
    # Windows gives a pipe (a script, MATLAB's system()) the ANSI code page,
    # which has no "CO₂" or "•": a pipe gets UTF-8, which its reader decodes;
    # a console replaces what it cannot show
    for stream in (sys.stdout, sys.stderr):
        try:
            if stream.isatty():
                stream.reconfigure(errors="replace")  # type: ignore[union-attr]
            else:
                stream.reconfigure(encoding="utf-8")  # type: ignore[union-attr]
        except (AttributeError, ValueError, OSError):
            pass
    from .project import LightSimError
    from .units import UnitError

    p = parser()
    try:
        args = p.parse_args(argv)
        if args.version:
            return cmd_version(argparse.Namespace(json=False))
        if not args.command:
            p.print_help()
            return EXIT_USAGE
        return args.func(args)
    except UsageError as e:
        print(f"lightsim: {e}", file=sys.stderr)
        return EXIT_USAGE
    except (LightSimError, UnitError) as e:
        print(f"lightsim: {e}", file=sys.stderr)
        return EXIT_USAGE
    except OSError as e:
        print(f"lightsim: {e}", file=sys.stderr)
        return EXIT_USAGE
