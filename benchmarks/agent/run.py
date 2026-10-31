"""The bench's runner: gives each task to Claude through the headless
command-line tool (``claude -p``) and scores the attempt with the task's
hidden checker. README.md in this folder says how to run it.

    LSIM_BENCH_CAP_USD=5 python -m benchmarks.agent.run --mode mcp --tries 3 --out DIR
    LSIM_BENCH_CAP_USD=2 python -m benchmarks.agent.run --smoke --out DIR

Each try gets a fresh temporary folder outside the repository holding only
the task's starting files, and one way to work:

- ``mcp``: LightSim's MCP server from this checkout, over stdio, is the
  agent's only tool (with the two tools that read its resources, the skills);
- ``cli``: the shell, allowed only the ``lightsim`` command (``guard.py``:
  no path outside the folder).

Every other tool is removed and denied, Claude Code's own settings, hooks,
plugins and skills are not loaded (``--restricted``), and anything that
would ask for permission is refused (``dontAsk``); permission checks are
never skipped. ``--auth api`` runs the same with ``ANTHROPIC_API_KEY``.

The spending cap ``LSIM_BENCH_CAP_USD`` (USD at list price, summed from the
cost each run reports) is required: the runner stops before the next try
once it is reached, and gives each try only what is left of it
(``--max-budget-usd``). Per try it records success, tool calls, tool
errors, tokens, cost, time, the model and the transcript, in ``--out``
(never inside the repository); per task, pass^k over k tries.
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Optional

from . import (
    DIR,
    ROOT,
    SMOKE_DIR,
    Task,
    check,
    heldout,
    isolate,
    load_task,
    load_tasks,
    start_files,
    write_files,
)

CAP_ENV = "LSIM_BENCH_CAP_USD"
MODES = ("mcp", "cli")
SMOKE_TASK = "question-loaded-ev"
PLANTED = SMOKE_DIR / "planted-read-checker.toml"

PROMPT = """You are helping an engineer with LightSim, a vehicle simulation app. {tools}
Your folder is {work}; it holds {files}. Work on your own: nobody will answer questions during the task.

{statement}"""
TOOLS = {
    "mcp": "Your only tools are LightSim's, from its MCP server \"lightsim\"; its skills are "
           "resources of that server.",
    "cli": "Your only tool is the shell, and in it only the `lightsim` command (`lightsim --help` "
           "says what it does). Name files relative to your folder.",
}
#: built-in tools denied as well as left out (--tools)
DENY = ["Read", "Write", "Edit", "NotebookEdit", "Glob", "Grep", "WebFetch", "WebSearch", "Task"]


def claude_command(claude: str, mode: str, prompt: str, mcp_config: Path,
                   model: Optional[str], budget: float, env: Optional[dict] = None) -> list[str]:
    """The ``claude -p`` command line of one try."""
    settings: dict = {"permissions": {"deny": DENY + (["Bash"] if mode == "mcp" else [])}}
    if mode == "mcp":
        tools = ["--tools", "ListMcpResourcesTool,ReadMcpResourceTool",
                 "--allowedTools", "mcp__lightsim,ListMcpResourcesTool,ReadMcpResourceTool"]
    else:
        tools = ["--tools", "Bash", "--allowedTools", "Bash(lightsim),Bash(lightsim *)"]
        settings["env"] = {"PATH": (env or os.environ).get("PATH", "")}  # the guarded lightsim first
    return [claude, "-p", prompt, "--output-format", "stream-json", "--verbose",
            "--restricted", "--strict-mcp-config", "--mcp-config", str(mcp_config), *tools,
            "--settings", json.dumps(settings),
            "--permission-mode", "dontAsk", "--permission-prompts", "none",
            "--disable-slash-commands", "--no-session-persistence",
            "--max-budget-usd", f"{budget:.4f}", *(["--model", model] if model else [])]


def prepare(task: Task, mode: str, base: Path) -> tuple[Path, Path, dict]:
    """The try's folders, MCP config and environment: ``base/work`` holds
    only the task's files; ``base/home`` the engine's settings and data, the
    MCP server's projects folder and (cli) the guarded ``lightsim``."""
    work, home = base / "work", base / "home"
    write_files(start_files(task), work)
    (home / "projects").mkdir(parents=True)
    settings = home / "ai-access.json"
    settings.write_text(json.dumps({"version": 1, "enabled": True, "examples": True,
                                    "folders": [str(work)]}), encoding="utf-8")
    engine_env = {"PYTHONPATH": str(ROOT / "backend"), "LIGHTSIM_AI_SETTINGS": str(settings),
                  "LIGHTSIM_PROJECTS_DIR": str(home / "projects"), "LIGHTSIM_DATA_DIR": str(home)}
    servers = {}
    if mode == "mcp":
        servers["lightsim"] = {"type": "stdio", "command": sys.executable,
                               "args": ["-m", "app.ai", "--allow-folder", str(work),
                                        "--projects-dir", str(home / "projects")],
                               "env": engine_env}
    mcp_config = home / "mcp.json"
    mcp_config.write_text(json.dumps({"mcpServers": servers}), encoding="utf-8")
    # none of the checker's own settings (isolate) reach the agent's tools:
    # they run as the app would, with its approval of Script code
    env = {k: v for k, v in os.environ.items() if not k.startswith("LIGHTSIM_")}
    if mode == "cli":
        (home / "bin").mkdir()
        wrapper = home / "bin" / "lightsim"
        assign = " ".join(f"{k}={shlex.quote(v)}" for k, v in engine_env.items() if k != "PYTHONPATH")
        wrapper.write_text(
            "#!/bin/sh\n"
            f"exec /usr/bin/env -i PATH=/usr/bin:/bin HOME={shlex.quote(str(home))} {assign} "
            f"{shlex.quote(sys.executable)} -I {shlex.quote(str(DIR / 'guard.py'))} "
            f"{shlex.quote(str(work))} \"$@\"\n", encoding="utf-8")
        wrapper.chmod(0o755)
        env["PATH"] = f"{home / 'bin'}{os.pathsep}{env.get('PATH', '')}"
    return work, mcp_config, env


def read_transcript(path: Path) -> dict:
    """What a stream-json transcript says: the init and result events, the
    tool calls in order, and how many tool results were errors."""
    init, result, calls, errors = {}, {}, [], 0
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            ev = json.loads(line)
        except ValueError:
            continue
        content = (ev.get("message") or {}).get("content")
        blocks = content if isinstance(content, list) else []
        if ev.get("type") == "system" and ev.get("subtype") == "init":
            init = ev
        elif ev.get("type") == "assistant":
            calls += [b.get("name", "") for b in blocks if b.get("type") == "tool_use"]
        elif ev.get("type") == "user":
            errors += sum(1 for b in blocks if b.get("type") == "tool_result" and b.get("is_error"))
        elif ev.get("type") == "result":
            result = ev
    return {"init": init, "result": result, "calls": calls, "errors": errors}


class Runner:
    def __init__(self, args: argparse.Namespace, cap: float) -> None:
        self.args, self.cap, self.spent = args, cap, 0.0
        self.stopped: Optional[str] = None
        self.held = heldout()
        stamp = time.strftime("%Y%m%d-%H%M%S")
        self.out = Path(args.out).resolve() / f"{stamp}-{'smoke' if args.smoke else 'bench'}"
        self.out.mkdir(parents=True, exist_ok=True)
        self.rows: list[dict] = []

    def attempt(self, task: Task, mode: str, n: int, auth: str) -> Optional[dict]:
        """One try, or None when the cap (or a failure of the tooling) stops the run."""
        if self.stopped:
            return None
        left = self.cap - self.spent
        if left <= 0:
            self.stopped = f"the cap of {self.cap:g} USD was reached (spent {self.spent:.4f} USD)"
            return None
        name = f"{task.id}-{mode}-{auth}-{n}"
        base = Path(tempfile.mkdtemp(prefix="lsim-bench-"))  # outside the repository
        try:
            work, mcp_config, env = prepare(task, mode, base)
            if auth == "session":
                env.pop("ANTHROPIC_API_KEY", None)
            files = ", ".join(task.files)
            prompt = PROMPT.format(tools=TOOLS[mode], work=work, files=files,
                                   statement=task.statement.replace("{root}", str(ROOT)))
            cmd = claude_command(self.args.claude, mode, prompt, mcp_config, self.args.model, left, env)
            transcript = self.out / f"{name}.jsonl"
            started = time.monotonic()
            with open(transcript, "wb") as out, open(self.out / f"{name}.stderr", "wb") as err:
                proc = subprocess.Popen(cmd, cwd=work, env=env, stdin=subprocess.DEVNULL,
                                        stdout=out, stderr=err, start_new_session=True)
                try:
                    proc.wait(timeout=self.args.timeout)
                    timed_out = False
                except subprocess.TimeoutExpired:
                    os.killpg(proc.pid, signal.SIGKILL)  # the group of the claude we started
                    proc.wait()
                    timed_out = True
            wall = time.monotonic() - started
            seen = read_transcript(transcript)
            text = str(seen["result"].get("result") or "")
            verdict = check(task, work, text, transcript.read_text(encoding="utf-8", errors="replace"))
            shutil.copytree(work, self.out / f"{name}-files")
        finally:
            shutil.rmtree(base, ignore_errors=True)
        res, init = seen["result"], seen["init"]
        usage = res.get("usage") or {}
        # a try that never reported its cost (killed at the timeout) is counted at
        # the most it was allowed to spend
        cost = float(res.get("total_cost_usd") or 0.0) if res else left
        self.spent += cost
        row = {
            "task": task.id, "version": task.version, "kind": task.kind,
            "heldout": task.id in self.held, "mode": mode, "auth": auth, "try": n,
            "model": init.get("model"),
            "models": sorted(res.get("modelUsage") or {}), "success": verdict.success,
            "reasons": verdict.reasons, "figures": verdict.figures,
            "toolCalls": len(seen["calls"]), "tools": dict(collections.Counter(seen["calls"])),
            "toolErrors": seen["errors"], "permissionDenials": len(res.get("permission_denials") or []),
            "inputTokens": usage.get("input_tokens", 0), "outputTokens": usage.get("output_tokens", 0),
            "cacheReadTokens": usage.get("cache_read_input_tokens", 0),
            "cacheWriteTokens": usage.get("cache_creation_input_tokens", 0),
            "costUsd": cost, "seconds": round(wall, 1), "turns": res.get("num_turns"),
            "end": "timeout" if timed_out else res.get("subtype") or "no result",
            "apiKeySource": init.get("apiKeySource"), "transcript": str(transcript),
        }
        problem = None if timed_out else self._tooling_problem(row, init, res, auth, mode)
        if problem:
            row["end"], self.stopped = "tooling error", problem
        self.rows.append(row)
        with open(self.out / "results.jsonl", "a", encoding="utf-8") as f:
            f.write(json.dumps(row) + "\n")
        print(f"{name}: {'pass' if row['success'] else 'fail'}, {row['toolCalls']} tool calls, "
              f"{cost:.4f} USD, {wall:.0f} s" + (f" ({problem})" if problem else ""), flush=True)
        return None if problem else row

    @staticmethod
    def _tooling_problem(row: dict, init: dict, res: dict, auth: str, mode: str) -> Optional[str]:
        """Why this try says nothing about the agent (and the run stops)."""
        if not res:
            return "claude gave no result (see the .stderr file)"
        if res.get("terminal_reason") == "api_error" or res.get("is_error") and not init:
            return f"the Claude API failed: {str(res.get('result'))[:200]}"
        if (row["apiKeySource"] in (None, "none")) != (auth == "session"):
            return f"the run used the wrong account (apiKeySource {row['apiKeySource']!r})"
        servers = {s.get("name"): s.get("status") for s in init.get("mcp_servers") or []}
        if mode == "mcp" and servers.get("lightsim") != "connected":
            return f"LightSim's MCP server did not connect ({servers})"
        return None

    def bench(self, tasks: list[Task], modes: list[str], tries: int, auth: str) -> None:
        for task in tasks:
            for mode in modes:
                for n in range(1, tries + 1):
                    self.attempt(task, mode, n, auth)

    def smoke(self) -> tuple[list[str], list[str]]:
        """One task in each mode, the planted task in each mode, then a cap
        that must stop a second try. Returns (problems, notes)."""
        task, planted = next(t for t in load_tasks() if t.id == SMOKE_TASK), load_task(PLANTED)
        problems, notes = [], []
        for mode in MODES:
            if self.attempt(task, mode, 1, "session") is None:
                problems.append(f"{task.id} did not run in {mode} mode")
        if os.environ.get("ANTHROPIC_API_KEY"):
            if self.attempt(task, "mcp", 1, "api") is None:
                problems.append(f"{task.id} did not run in API mode")
        else:
            notes.append("API mode not run: ANTHROPIC_API_KEY is not set")
        for mode in MODES:
            row = self.attempt(planted, mode, 1, "session")
            if row is None or not row["success"]:
                problems.append(f"the planted task {'did not run' if row is None else 'got the secret'} "
                                f"in {mode} mode")
        if self.stopped:
            return problems + [f"stopped before the cap test: {self.stopped}"], notes
        self.cap = self.spent + 0.001  # the next try may spend 0.001 USD: the one after must not start
        first, second = self.attempt(task, "mcp", 2, "session"), self.attempt(task, "mcp", 3, "session")
        if first is None or second is not None:
            problems.append("the cap did not stop the run before the next try")
        else:
            notes.append(f"the cap stopped the run before the next try: {self.stopped}")
        return problems, notes

    def report(self, tries: int, notes: list[str]) -> str:
        groups: dict[tuple, list[dict]] = collections.defaultdict(list)
        for r in self.rows:
            groups[r["task"], r["mode"], r["auth"]].append(r)
        lines = ["| Task | Held out | Mode | Tries | Passed | pass^k | Tool calls | Tool errors | "
                 "Tokens in/out | Cost, USD | Time, s |", "|" + "---|" * 11]
        for (task, mode, auth), rs in groups.items():
            k = len(rs)
            pk = "–" if k < tries else ("1" if all(r["success"] for r in rs) else "0")
            mean = lambda key: sum(r[key] for r in rs) / k  # noqa: E731
            lines.append(f"| {task} | {'yes' if rs[0]['heldout'] else 'no'} | {mode}/{auth} | {k} | "
                         f"{sum(r['success'] for r in rs)} | {pk} | {mean('toolCalls'):.1f} | "
                         f"{mean('toolErrors'):.1f} | {mean('inputTokens') + mean('cacheReadTokens') + mean('cacheWriteTokens'):.0f}"
                         f"/{mean('outputTokens'):.0f} | {mean('costUsd'):.4f} | {mean('seconds'):.0f} |")
        models = sorted({m for r in self.rows for m in r["models"] or [r["model"]] if m})
        summary = {"cap": self.cap, "spent": round(self.spent, 4), "stopped": self.stopped,
                   "models": models, "tries": tries, "notes": notes,
                   "passK": {f"{t}/{m}/{a}": (all(r["success"] for r in rs) if len(rs) == tries else None)
                             for (t, m, a), rs in groups.items()}}
        (self.out / "summary.json").write_text(json.dumps(summary, indent=1), encoding="utf-8")
        text = "\n".join([f"# Agent bench run {self.out.name}", "",
                          f"Models: {', '.join(models) or 'none'}. Spent {self.spent:.4f} of "
                          f"{self.cap:g} USD." + (f" Stopped: {self.stopped}." if self.stopped else ""),
                          "", *lines, "", *[f"- {n}" for n in notes]]) + "\n"
        (self.out / "summary.md").write_text(text, encoding="utf-8")
        return text


def preflight(claude: str, auth: str) -> Optional[str]:
    """Why no try can run, or None."""
    if auth == "api":
        return None if os.environ.get("ANTHROPIC_API_KEY") else "--auth api needs ANTHROPIC_API_KEY"
    try:
        out = subprocess.run([claude, "auth", "status"], capture_output=True, text=True, timeout=60,
                             env={k: v for k, v in os.environ.items() if k != "ANTHROPIC_API_KEY"})
        if json.loads(out.stdout).get("loggedIn") is True:
            return None
    except (OSError, ValueError, subprocess.TimeoutExpired) as e:
        return f"'{claude} auth status' failed: {e}"
    return f"'{claude}' is not signed in to a Claude plan: run it once and use /login"


def main(argv=None) -> int:
    p = argparse.ArgumentParser(prog="python -m benchmarks.agent.run", description=__doc__.split("\n\n")[0])
    p.add_argument("--mode", default="mcp,cli", help="mcp, cli or both (mcp,cli)")
    p.add_argument("--tries", type=int, default=3, help="tries per task and mode (pass^k's k)")
    p.add_argument("--tasks", help="task ids, comma-separated (default: all)")
    p.add_argument("--set", choices=("all", "visible", "heldout"), default="all")
    p.add_argument("--auth", choices=("session", "api"), default="session",
                   help="session: the Claude plan claude is signed in to; api: ANTHROPIC_API_KEY")
    p.add_argument("--model", help="the model (claude's default if not given; recorded either way)")
    p.add_argument("--claude", default=shutil.which("claude") or "claude")
    p.add_argument("--out", default=str(Path(tempfile.gettempdir()) / "lightsim-bench-runs"),
                   help="where transcripts and results go (outside the repository)")
    p.add_argument("--timeout", type=float, default=1800, help="seconds per try")
    p.add_argument("--smoke", action="store_true", help="the runner's own smoke test")
    args = p.parse_args(argv)
    isolate()
    try:
        cap = float(os.environ[CAP_ENV])
        assert cap > 0
    except (KeyError, ValueError, AssertionError):
        print(f"Set {CAP_ENV} to the most this run may spend, in USD at list price.", file=sys.stderr)
        return 2
    if Path(args.out).resolve().is_relative_to(ROOT):
        print("--out must be outside the repository: transcripts are never committed.", file=sys.stderr)
        return 2
    problem = preflight(args.claude, args.auth)
    if problem:
        print(f"Not run: {problem}.", file=sys.stderr)
        return 2
    runner = Runner(args, cap)
    problems: list[str] = []
    notes: list[str] = []
    if args.smoke:
        problems, notes = runner.smoke()
    else:
        tasks = load_tasks()
        if args.tasks:
            wanted = args.tasks.split(",")
            tasks = [t for t in tasks if t.id in wanted]
        if args.set != "all":
            tasks = [t for t in tasks if (t.id in runner.held) == (args.set == "heldout")]
        # ponytail: one try at a time, so the cap is checked before every one; a pool would
        # need the cap checked per start and could overshoot by the tries in flight
        runner.bench(tasks, [m for m in args.mode.split(",") if m in MODES], args.tries, args.auth)
    if not args.smoke and runner.stopped and "cap" not in runner.stopped:
        problems.append(f"stopped: {runner.stopped}")
    print(runner.report(1 if args.smoke else args.tries, [f"Problem: {x}" for x in problems] + notes))
    return 1 if problems else 0


if __name__ == "__main__":  # the checker's Script runs import this module again
    sys.exit(main())
