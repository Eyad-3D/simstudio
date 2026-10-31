"""One try of the bench: a task given to Claude through the headless
command-line tool (``claude -p``) and scored by the task's hidden checker.
``compare.py`` runs the comparison of models and effort levels with it;
this module's own command is the runner's smoke test. README.md in this
folder says how to run both.

    python -m benchmarks.agent.run --smoke --out DIR

Each try gets a fresh temporary folder outside the repository holding only
the task's starting files, and one way to work:

- ``mcp``: LightSim's MCP server from this checkout, over stdio, is the
  agent's only tool (with the two tools that read its resources, the skills);
- ``cli``: the shell, allowed only the ``lightsim`` command (``guard.py``:
  no path outside the folder).

Every other tool is removed and denied, Claude Code's own settings, hooks,
plugins and skills are not loaded (``--restricted``), and anything that
would ask for permission is refused (``dontAsk``); permission checks are
never skipped.

Billing. A try runs on the Claude plan ``claude`` is signed in to: every
variable that would make it bill an API account instead (``ANTHROPIC_*``,
``CLAUDE_CODE_USE_*``, ``AWS_BEARER_TOKEN_BEDROCK``) is removed from its
environment, and a try that still used a key is the runner's fault. There is
no spending cap. ``auth="api"`` runs on ``ANTHROPIC_API_KEY`` instead, and
only there a cap may be set (``LSIM_BENCH_CAP_USD``, ``--max-budget-usd``).

Every try ends in one class: ``solved`` or ``failed`` (the checker's verdict
on what the agent left; only these are scored), ``not-run`` (a plan usage
limit, a rate limit, an overloaded or network error from the service, or the
runner's own fault: retried, never scored) or ``unavailable`` (the plan
refused the model, or the tool refused the effort or ran another model).

Cost. Each try records ``total_cost_usd`` as the tool reports it, and the
cost at Anthropic's API list price, worked out here from the tokens of each
request, so it does not hang on the tool's own price list: Claude Code
2.1.273 did not know the 5.5 models and priced them as Claude Opus 5
(``costBasis: "unknown"``); 2.1.316 knows them (``"list"``).
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
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

CAP_ENV = "LSIM_BENCH_CAP_USD"  # API mode only
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

#: USD per million tokens at Anthropic's API list price (platform.claude.com
#: pricing page, 31 October 2026): input, output, cache write (5 minutes),
#: cache write (1 hour), cache read; Haiku 5.5 has a second rate card for
#: prompts over 100 000 tokens.
PRICES = {
    "claude-opus-5-5": (4.0, 20.0, 5.0, 8.0, 0.20),
    "claude-sonnet-5-5": (2.0, 10.0, 2.5, 4.0, 0.10),
    "claude-haiku-5-5": (0.10, 0.50, 0.125, 0.20, 0.01),
    "claude-haiku-4-5": (1.0, 5.0, 1.25, 2.0, 0.10),  # Claude Code's own small calls
}
LONG_PROMPT = {"claude-haiku-5-5": (100_000, (0.50, 2.50, 0.625, 1.0, 0.05))}

#: the classes a try ends in; only the first two are scored
SCORED = ("solved", "failed")
_CHECK = threading.Lock()  # the checker runs LightSim in this process: one at a time


def billing_env(env: dict, auth: str = "session") -> dict:
    """The agent's environment without anything that would bill an API
    account (or pick another provider, model or effort than the try's); with
    ``auth="api"``, ``ANTHROPIC_API_KEY`` alone is put back."""
    out = {k: v for k, v in env.items()
           if not k.startswith(("ANTHROPIC_", "CLAUDE_CODE_USE_"))
           and k not in ("AWS_BEARER_TOKEN_BEDROCK", "CLAUDE_CODE_EFFORT_LEVEL")}
    if auth == "api":
        out["ANTHROPIC_API_KEY"] = env["ANTHROPIC_API_KEY"]
    return out


def claude_command(claude: str, mode: str, prompt: str, mcp_config: Path, model: Optional[str],
                   effort: Optional[str] = None, budget: Optional[float] = None,
                   debug_file: Optional[Path] = None, env: Optional[dict] = None) -> list[str]:
    """The ``claude -p`` command line of one try (``budget`` only in API mode)."""
    settings: dict = {"permissions": {"deny": DENY + (["Bash"] if mode == "mcp" else [])}}
    if mode == "mcp":
        tools = ["--tools", "ListMcpResourcesTool,ReadMcpResourceTool",
                 "--allowedTools", "mcp__lightsim,ListMcpResourcesTool,ReadMcpResourceTool"]
    else:
        tools = ["--tools", "Bash", "--allowedTools", "Bash(lightsim),Bash(lightsim *)"]
        settings["env"] = {"PATH": (env or os.environ).get("PATH", "")}  # the guarded lightsim first
    return [claude, "-p", prompt, "--output-format", "stream-json", "--verbose",
            "--include-partial-messages",  # each request's tokens, for its list price
            "--restricted", "--strict-mcp-config", "--mcp-config", str(mcp_config), *tools,
            "--settings", json.dumps(settings),
            "--permission-mode", "dontAsk", "--permission-prompts", "none",
            "--disable-slash-commands", "--no-session-persistence",
            *(["--model", model] if model else []), *(["--effort", effort] if effort else []),
            *(["--debug-file", str(debug_file)] if debug_file else []),
            *(["--max-budget-usd", f"{budget:.4f}"] if budget is not None else [])]


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
    tool calls in order, how many tool results were errors, the service
    errors and retries Claude Code reported, the models that answered, a
    plan limit's reset time, and each request's tokens (from its
    ``message_start`` and ``message_delta`` stream events)."""
    seen = {"init": {}, "result": {}, "calls": [], "errors": 0, "apiErrors": [], "models": set(),
            "apiRetries": 0, "lastRetry": False, "limitReset": None, "requests": {}}
    current = None
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            ev = json.loads(line)
        except ValueError:
            continue
        kind = ev.get("type")
        seen["lastRetry"] = kind == "system" and ev.get("subtype") == "api_retry"
        message = ev.get("message") if isinstance(ev.get("message"), dict) else {}  # a text on some events
        blocks = message.get("content") if isinstance(message.get("content"), list) else []
        if kind == "system" and ev.get("subtype") == "init":
            seen["init"] = ev
        elif seen["lastRetry"]:
            seen["apiRetries"] += 1
        elif kind == "assistant":
            seen["calls"] += [b.get("name", "") for b in blocks if b.get("type") == "tool_use"]
            if ev.get("error"):
                seen["apiErrors"].append(ev["error"])
            if message.get("model") not in (None, "<synthetic>"):
                seen["models"].add(message["model"])
        elif kind == "user":
            seen["errors"] += sum(1 for b in blocks if b.get("type") == "tool_result" and b.get("is_error"))
        elif kind == "result":
            seen["result"] = ev
        elif kind == "rate_limit_event":
            info = ev.get("rate_limit_info") or {}
            if info.get("status") == "rejected":
                seen["limitReset"] = info.get("resetsAt") or seen["limitReset"]
        elif kind == "stream_event":
            e = ev.get("event") or {}
            if e.get("type") == "message_start":
                m = e.get("message") or {}
                current = m.get("id")
                seen["requests"][current] = {"model": m.get("model"), **(m.get("usage") or {})}
            elif e.get("type") == "message_delta" and current in seen["requests"]:
                seen["requests"][current]["output_tokens"] = (e.get("usage") or {}).get("output_tokens", 0)
    return seen


def _price(model: str, prompt: int) -> Optional[tuple]:
    model = re.sub(r"-\d{8}$", "", model or "")
    long = LONG_PROMPT.get(model)
    return long[1] if long and prompt > long[0] else PRICES.get(model)


def list_cost(seen: dict) -> tuple[float, list[str]]:
    """(USD at API list price, the models with no list price here). Each
    streamed request is priced on its own rate card; what ``modelUsage`` has
    beyond them (Claude Code's own small calls, or a try with no stream
    events) at the base one; a model with no price here at the tool's own
    ``costUSD``."""
    def cost(p, inp, out, w5, w1, read):
        return (inp * p[0] + out * p[1] + w5 * p[2] + w1 * p[3] + read * p[4]) / 1e6

    total, unknown = 0.0, []
    streamed: dict = collections.defaultdict(lambda: [0, 0, 0, 0])
    for r in seen["requests"].values():
        read, write = r.get("cache_read_input_tokens", 0), r.get("cache_creation_input_tokens", 0)
        p = _price(r["model"], r.get("input_tokens", 0) + read + write)
        if p is None:
            continue
        split = r.get("cache_creation") or {}
        w1 = split.get("ephemeral_1h_input_tokens", 0)
        total += cost(p, r.get("input_tokens", 0), r.get("output_tokens", 0), write - w1, w1, read)
        s = streamed[re.sub(r"-\d{8}$", "", r["model"])]
        for i, n in enumerate((r.get("input_tokens", 0), r.get("output_tokens", 0), read, write)):
            s[i] += n
    usage = seen["result"].get("usage") or {}
    split = usage.get("cache_creation") or {}
    share_1h = split.get("ephemeral_1h_input_tokens", 0) / max(1, sum(split.values()) if split else 0)
    for model, u in (seen["result"].get("modelUsage") or {}).items():
        s = streamed[re.sub(r"-\d{8}$", "", model)]
        rest = [max(0, u.get(k, 0) - s[i]) for i, k in enumerate(
            ("inputTokens", "outputTokens", "cacheReadInputTokens", "cacheCreationInputTokens"))]
        p = _price(model, 0)
        if p is None:
            unknown.append(model)
            total += float(u.get("costUSD") or 0.0)
        elif any(rest):
            total += cost(p, rest[0], rest[1], rest[3] * (1 - share_1h), rest[3] * share_1h, rest[2])
    return total, unknown


def tokens(seen: dict) -> dict:
    """The try's tokens over every model (``modelUsage``), else the result's
    usage, else what the streamed requests showed (a try killed early)."""
    keys = ("input", "output", "cacheRead", "cacheWrite")
    mu = (seen["result"].get("modelUsage") or {}).values()
    if mu:
        return {k: sum(u.get(f, 0) for u in mu) for k, f in zip(keys, (
            "inputTokens", "outputTokens", "cacheReadInputTokens", "cacheCreationInputTokens"))}
    rows = [seen["result"].get("usage")] if seen["result"].get("usage") else seen["requests"].values()
    return {k: sum(r.get(f, 0) for r in rows) for k, f in zip(keys, (
        "input_tokens", "output_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"))}


#: the ``error`` of Claude Code's assistant message for a failed request
_LIMIT = ("rate_limit", "billing_error")
#: what Claude Code's debug log says when it sends another effort than asked
#: (the API refused it, or a cap lowered it): 2.1.316 writes "[effort] ...",
#: 2.1.273 "... rejected output_config.effort ..."
_EFFORT_CHANGED = re.compile(r"\[effort\][^\n]*|[^\n]*rejected output_config\.effort[^\n]*")
_STOP = ("authentication_failed", "oauth_org_not_allowed", "account_on_hold", "verification_required")


def classify(seen: dict, debug: str, timed_out: bool, *, mode: str, auth: str,
             model: Optional[str], passed: bool) -> tuple[str, str]:
    """(class, why) of a try. ``why`` of a not-run try starts with its
    kind: ``limit``, ``service``, ``runner`` or ``stop`` (the comparison
    cannot go on: signed out, or billed to the wrong account). Everything
    else the agent met, LightSim's errors and approval refusals included,
    leaves a scored try: ``passed`` (the checker's verdict) decides."""
    init, res = seen["init"], seen["result"]
    errors = set(seen["apiErrors"])
    if not init:
        return "not-run", "runner: claude gave no init event (see the .stderr file)"
    if (init.get("apiKeySource") in (None, "none")) != (auth == "session"):
        return "not-run", f"stop: the try used the wrong account (apiKeySource {init.get('apiKeySource')!r})"
    servers = {s.get("name"): s.get("status") for s in init.get("mcp_servers") or []}
    if mode == "mcp" and servers.get("lightsim") != "connected":
        return "not-run", f"runner: LightSim's MCP server did not connect ({servers})"
    changed = _EFFORT_CHANGED.search(debug)
    if changed:
        return "unavailable", f"the tool changed the effort: {changed.group().strip()[:200]}"
    if "model_not_found" in errors or res.get("api_error_status") == 404:
        return "unavailable", f"the plan refused the model: {str(res.get('result'))[:200]}"
    others = {m for m in seen["models"] if re.sub(r"-\d{8}$", "", m) != model} if model else set()
    if others:
        return "unavailable", f"{', '.join(sorted(others))} answered, not {model}"
    failed_call = res.get("is_error") and res.get("terminal_reason") == "api_error" or not res and seen["lastRetry"]
    if errors & set(_STOP) and failed_call:
        return "not-run", f"stop: {str(res.get('result'))[:200]}"
    if failed_call:
        text = str(res.get("result") or "no result")[:200]
        if errors & set(_LIMIT) or res.get("api_error_status") == 429 or seen["limitReset"]:
            return "not-run", f"limit: {text}"
        return "not-run", f"service: {text}"
    if not res and not timed_out:
        return "not-run", "runner: claude gave no result (see the .stderr file)"
    if res.get("subtype") == "error_during_execution" and not timed_out:
        return "not-run", f"runner: claude stopped with error_during_execution: {str(res.get('result'))[:200]}"
    return ("solved" if passed else "failed"), ("timeout" if timed_out else res.get("terminal_reason") or "")


def run_try(task: Task, mode: str, model: Optional[str], effort: Optional[str], out: Path, name: str, *,
            claude: str, timeout: float, auth: str = "session", budget: Optional[float] = None) -> dict:
    """Run one try and return its record (``out/name.*`` keeps the
    transcript, stderr, debug log and the files as the agent left them)."""
    base = Path(tempfile.mkdtemp(prefix="lsim-bench-"))  # outside the repository
    transcript, debug = out / f"{name}.jsonl", out / f"{name}.debug.log"
    try:
        work, mcp_config, env = prepare(task, mode, base)
        env = billing_env(env, auth)
        prompt = PROMPT.format(tools=TOOLS[mode], work=work, files=", ".join(task.files),
                               statement=task.statement.replace("{root}", str(ROOT)))
        cmd = claude_command(claude, mode, prompt, mcp_config, model, effort, budget, debug, env)
        started = time.monotonic()
        with open(transcript, "wb") as so, open(out / f"{name}.stderr", "wb") as se:
            proc = subprocess.Popen(cmd, cwd=work, env=env, stdin=subprocess.DEVNULL,
                                    stdout=so, stderr=se, start_new_session=True)
            try:
                proc.wait(timeout=timeout)
                timed_out = False
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)  # the group of the claude we started
                proc.wait()
                timed_out = True
        wall = time.monotonic() - started
        seen = read_transcript(transcript)
        debug_text = debug.read_text(encoding="utf-8", errors="replace") if debug.exists() else ""
        cls, why = classify(seen, debug_text, timed_out, mode=mode, auth=auth, model=model, passed=False)
        verdict = None
        if cls in SCORED:  # the agent ran: the checker scores what it left
            with _CHECK:
                verdict = check(task, work, str(seen["result"].get("result") or ""),
                                transcript.read_text(encoding="utf-8", errors="replace"))
            cls = "solved" if verdict.success else "failed"
        shutil.copytree(work, out / f"{name}-files")
    finally:
        shutil.rmtree(base, ignore_errors=True)
    res, init = seen["result"], seen["init"]
    cost, unpriced = list_cost(seen)
    if budget is not None and not res:
        cost = budget  # API mode: a try that never reported its cost counts as all it was allowed
    mu = res.get("modelUsage") or {}
    return {
        "task": task.id, "version": task.version, "kind": task.kind, "heldout": task.id in heldout(),
        "mode": mode, "model": model, "effort": effort, "class": cls, "why": why,
        "reasons": verdict.reasons if verdict else [], "figures": verdict.figures if verdict else {},
        "reported": {"init": init.get("model"), "answered": sorted(seen["models"]), "result": sorted(mu)},
        "billing": None if not init else "plan" if init.get("apiKeySource") in (None, "none")
        else f"api ({init.get('apiKeySource')})",
        "claudeCode": init.get("claude_code_version"),
        "toolCalls": len(seen["calls"]), "tools": dict(collections.Counter(seen["calls"])),
        "toolErrors": seen["errors"], "permissionDenials": len(res.get("permission_denials") or []),
        "apiRetries": seen["apiRetries"], "tokens": tokens(seen),
        "costUsd": round(cost, 6), "costComplete": bool(res) and not unpriced, "unpriced": unpriced,
        "cliCostUsd": res.get("total_cost_usd"), "cliCostBasis": sorted({u.get("costBasis") for u in mu.values()} - {None}),
        "seconds": round(wall, 1), "turns": res.get("num_turns"), "limitReset": seen["limitReset"],
        "end": "timeout" if timed_out else res.get("terminal_reason") or res.get("subtype") or "no result",
        "transcript": str(transcript), "finished": time.time(),
    }


def preflight(claude: str, auth: str) -> Optional[str]:
    """Why no try can run, or None."""
    if auth == "api":
        return None if os.environ.get("ANTHROPIC_API_KEY") else "--auth api needs ANTHROPIC_API_KEY"
    try:
        out = subprocess.run([claude, "auth", "status"], capture_output=True, text=True, timeout=60,
                             env=billing_env(dict(os.environ)))
        if json.loads(out.stdout).get("loggedIn") is True:
            return None
    except (OSError, ValueError, subprocess.TimeoutExpired) as e:
        return f"'{claude} auth status' failed: {e}"
    return f"'{claude}' is not signed in to a Claude plan: run it once and use /login"


def smoke(out: Path, claude: str, model: str, effort: str, timeout: float) -> list[str]:
    """One task in each mode and the planted task in each mode, on the plan.
    Returns the problems."""
    task, planted = next(t for t in load_tasks() if t.id == SMOKE_TASK), load_task(PLANTED)
    problems = []
    for t in (task, planted):
        for mode in MODES:
            r = run_try(t, mode, model, effort, out, f"{t.id}.{mode}", claude=claude, timeout=timeout)
            with open(out / "results.jsonl", "a", encoding="utf-8") as f:
                f.write(json.dumps(r) + "\n")
            print(f"{t.id} {mode}: {r['class']} ({r['why']}), {r['toolCalls']} tool calls, "
                  f"{r['costUsd']:.4f} USD at list price, {r['seconds']:.0f} s", flush=True)
            if r["class"] not in SCORED:
                problems.append(f"{t.id} did not run in {mode} mode: {r['why']}")
            elif t is planted and r["class"] != "solved":
                problems.append(f"the planted task got the secret in {mode} mode")
            elif r["billing"] != "plan":
                problems.append(f"{t.id} in {mode} mode was billed as {r['billing']}")
    return problems


def main(argv=None) -> int:
    p = argparse.ArgumentParser(prog="python -m benchmarks.agent.run", description=__doc__.split("\n\n")[0])
    p.add_argument("--smoke", action="store_true", required=True, help="the runner's own smoke test")
    p.add_argument("--model", default="claude-sonnet-5-5")
    p.add_argument("--effort", default="medium")
    p.add_argument("--claude", default=shutil.which("claude") or "claude")
    p.add_argument("--out", default=str(Path(tempfile.gettempdir()) / "lightsim-bench-runs"),
                   help="where transcripts and results go (outside the repository)")
    p.add_argument("--timeout", type=float, default=1800, help="seconds per try")
    args = p.parse_args(argv)
    isolate()
    if Path(args.out).resolve().is_relative_to(ROOT):
        print("--out must be outside the repository: transcripts are never committed.", file=sys.stderr)
        return 2
    problem = preflight(args.claude, "session")
    if problem:
        print(f"Not run: {problem}.", file=sys.stderr)
        return 2
    out = Path(args.out).resolve() / f"{time.strftime('%Y%m%d-%H%M%S')}-smoke"
    out.mkdir(parents=True)
    problems = smoke(out, args.claude, args.model, args.effort, args.timeout)
    print("\n".join(f"Problem: {x}" for x in problems) or "Smoke test passed.")
    return 1 if problems else 0


if __name__ == "__main__":  # the checker's Script runs import this module again
    sys.exit(main())
