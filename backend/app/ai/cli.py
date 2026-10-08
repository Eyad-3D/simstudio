"""``lightsim-backend mcp ...``: the AI connection's commands (AI-03, AI-29).

    lightsim-backend mcp [--allow-folder DIR]... [--read-only] [--trust-scripts]
        Serve MCP over stdin/stdout. AI apps start this; you do not.
    lightsim-backend mcp install --client claude|claude-code|vscode|copilot|codex|gemini|cursor
        Add LightSim to that AI app's MCP settings (a backup of the file is kept).
    lightsim-backend mcp uninstall --client ...
        Remove it again.
    lightsim-backend mcp status
        Which AI apps have LightSim set up, and when an assistant last used it.

From a source checkout, ``python -m app.ai`` (in ``backend/``) is the same
command. When the automation lane's CLI (AI-02) lands, ``lightsim mcp``
becomes its subcommand and calls :func:`main`.
"""
from __future__ import annotations

import argparse
import datetime as _dt
import io
import sys
from pathlib import Path
from typing import Optional

from . import install


def _serve(args: argparse.Namespace) -> int:
    from .access import AuditLog, Policy
    from .engine import Engine
    from .mcp_server import McpServer

    # stdout carries the protocol and nothing else: anything the engine
    # prints goes to stderr, which AI apps show in their logs
    out = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", newline="\n", line_buffering=True)
    inp = io.TextIOWrapper(sys.stdin.buffer, encoding="utf-8")
    sys.stdout = sys.stderr
    engine = Engine(
        user_folder=Path(args.projects_dir) if args.projects_dir else None,
        allowed_folders=tuple(Path(f) for f in args.allow_folder),
        include_examples=not args.no_examples,
    )
    policy = Policy(allow_edits=not args.read_only, trust_scripts=args.trust_scripts,
                    max_run_seconds=args.max_run_seconds)
    print(f"LightSim MCP server: projects in {engine.user_folder}"
          + (f", also {', '.join(map(str, engine.allowed_folders))}" if engine.allowed_folders else ""),
          file=sys.stderr)
    McpServer(engine, policy, AuditLog(engine.user_folder)).serve(inp, out)
    return 0


def _install(args: argparse.Namespace) -> int:
    from ..machine_policy import AI_OFF, ai_off

    if ai_off():
        print(f"Not installed: {AI_OFF}", file=sys.stderr)
        return 3
    extra = []
    for f in args.allow_folder:
        extra += ["--allow-folder", str(Path(f).expanduser().resolve())]
    if args.read_only:
        extra.append("--read-only")
    try:
        path = install.install(args.client, extra_args=extra)
    except install.InstallError as e:
        print(f"Not installed: {e}", file=sys.stderr)
        return 3
    # connecting is the user's yes to AI access for their projects (AI-01)
    from .engine import default_projects_dir, grant_folder

    grant_folder(default_projects_dir())
    print(f"LightSim added to {install.CLIENTS[args.client].title}: {path}")
    print("AI access is on for your projects folder ('lightsim ai off' turns it off).")
    print("Restart the AI app (or reload its MCP servers) to connect.")
    return 0


def _uninstall(args: argparse.Namespace) -> int:
    try:
        path = install.uninstall(args.client)
    except install.InstallError as e:
        print(f"Not removed: {e}", file=sys.stderr)
        return 3
    print(f"LightSim removed from {install.CLIENTS[args.client].title}" + (f": {path}" if path else "."))
    return 0


def _status(args: argparse.Namespace) -> int:
    from .access import last_audit_entry
    from .engine import default_projects_dir

    for key, client in install.CLIENTS.items():
        state = "set up" if install.is_installed(key) else "not set up"
        print(f"{client.title:<26} {state:<11} {client.config_path()}")
    last = last_audit_entry(default_projects_dir())
    if last:
        when = _dt.datetime.fromtimestamp(float(last.get("t", 0))).strftime("%Y-%m-%d %H:%M")
        who = f" by {last['client']}" if last.get("client") else ""
        print(f"Last used{who}: {when} ({last.get('tool')})")
    else:
        print("No assistant has used LightSim yet.")
    print("Command AI apps run:", " ".join(install.server_command()))
    return 0


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(prog="lightsim-backend mcp",
                                     description="LightSim's connection for AI assistants (MCP).")
    parser.add_argument("--allow-folder", action="append", default=[], metavar="DIR",
                        help="also let the assistant see the project files in DIR")
    parser.add_argument("--projects-dir", help=argparse.SUPPRESS)
    parser.add_argument("--no-examples", action="store_true", help="hide the shipped examples")
    parser.add_argument("--read-only", action="store_true", help="refuse every edit")
    parser.add_argument("--trust-scripts", action="store_true",
                        help="let the assistant run projects with Script blocks on Windows "
                             "(after you confirm each run)")
    parser.add_argument("--max-run-seconds", type=float, default=300.0,
                        help="stop an assistant's run after this long (default 300)")
    sub = parser.add_subparsers(dest="command")
    for name, helptext in (("install", "add LightSim to an AI app"),
                           ("uninstall", "remove LightSim from an AI app")):
        p = sub.add_parser(name, help=helptext)
        p.add_argument("--client", required=True, choices=sorted(install.CLIENTS))
        if name == "install":
            p.add_argument("--allow-folder", action="append", default=[], metavar="DIR")
            p.add_argument("--read-only", action="store_true")
    sub.add_parser("status", help="which AI apps have LightSim set up")
    args = parser.parse_args(argv)
    if args.command == "install":
        return _install(args)
    if args.command == "uninstall":
        return _uninstall(args)
    if args.command == "status":
        return _status(args)
    return _serve(args)
