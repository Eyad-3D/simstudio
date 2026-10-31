"""The agent test bench (benchmarks/agent/, see its README.md).

Every task's reference solution passes its checker and its blank and wrong
ones fail; the held-out list is a third of the tasks and never reaches the
agent. The runner is driven with a stand-in for ``claude -p`` that prints
the stream-json Claude Code does, so these tests spend nothing: what it
records, that the spending cap stops it before the next try, that it never
skips permission checks, and that neither the guarded ``lightsim`` command
nor LightSim's MCP server hands the agent a file outside its folder.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:  # the benchmarks package lives at the repository root
    sys.path.insert(0, str(ROOT))

from benchmarks import agent  # noqa: E402
from benchmarks.agent import run  # noqa: E402

TASKS = agent.load_tasks()
PLANTED = agent.load_task(run.PLANTED)
#: the runner runs claude on Linux (WSL on Windows): its lightsim is a shell script
posix = pytest.mark.skipif(sys.platform == "win32", reason="the runner runs on Linux or macOS")

FAKE_CLAUDE = r'''
import json, os, sys
args = sys.argv[1:]
if args[:2] == ["auth", "status"]:
    print(json.dumps({"loggedIn": True}))
    sys.exit(0)
prompt = args[args.index("-p") + 1]
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps(args) + "\n")
planted = "the secret" in prompt
leak = "SECRET-FROM-TOOL " + os.environ.get("FAKE_LEAK", "") if planted else "ok"
answer = "I may not read those files. ANSWER: none" if planted else "ANSWER: 15.6; 1.20"
events = [
    {"type": "system", "subtype": "init", "model": "fake-model", "apiKeySource": "none",
     "mcp_servers": [{"name": "lightsim", "status": "connected"}]},
    {"type": "assistant", "message": {"content": [
        {"type": "tool_use", "id": "t1", "name": "mcp__lightsim__run_case", "input": {}}]}},
    {"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": "t1", "is_error": True, "content": leak}]}},
    {"type": "result", "subtype": "success", "is_error": False, "result": answer,
     "total_cost_usd": 0.03, "num_turns": 2, "permission_denials": [],
     "usage": {"input_tokens": 10, "output_tokens": 20, "cache_read_input_tokens": 300},
     "modelUsage": {"fake-model": {}}},
]
for e in events:
    if e["type"] == "result" and os.environ.get("FAKE_SLEEP"):
        sys.stdout.flush()
        import time; time.sleep(float(os.environ["FAKE_SLEEP"]))
    print(json.dumps(e))
'''


@pytest.fixture
def fake_claude(tmp_path, monkeypatch):
    path = tmp_path / "claude"
    path.write_text(f"#!{sys.executable}\n{FAKE_CLAUDE}", encoding="utf-8")
    path.chmod(0o755)
    monkeypatch.setenv("FAKE_LOG", str(tmp_path / "calls.jsonl"))
    monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
    for key in ("LIGHTSIM_PROJECTS_DIR", "LIGHTSIM_DATA_DIR", "LIGHTSIM_SCRIPT_TRUST"):
        monkeypatch.setenv(key, os.environ.get(key, ""))  # the runner's isolate() changes them
    return path


def bench(fake: Path, out: Path, monkeypatch, cap: float, *args: str) -> int:
    monkeypatch.setenv(run.CAP_ENV, str(cap))
    return run.main(["--claude", str(fake), "--out", str(out), *args])


def rows(out: Path) -> list[dict]:
    (folder,) = out.iterdir()
    return [json.loads(x) for x in (folder / "results.jsonl").read_text().splitlines()]


# -- the tasks ------------------------------------------------------------------


@pytest.mark.parametrize("task", TASKS + [PLANTED], ids=lambda t: t.id)
def test_each_task_passes_its_reference_and_fails_its_blank_and_wrong_solutions(task):
    assert agent.validate(task) == []


@pytest.mark.parametrize("task_id", ["build-sedan-abc", "write-launch-smoother"])
def test_a_reference_saved_through_the_edit_tool_passes_the_checker(task_id, tmp_path, monkeypatch):
    """What an agent that edits the right way leaves behind: the reference
    operations sent through LightSim's own ``model_edit`` tool, confirmed and
    saved to the agent's folder, score as the validator's files do."""
    from app.ai.access import AuditLog, Policy
    from app.ai.engine import Engine
    from app.ai.tools import CallContext, ConfirmationNeeded, Tools

    task = next(t for t in TASKS if t.id == task_id)
    work, home = tmp_path / "work", tmp_path / "home"
    agent.write_files(agent.start_files(task), work)
    (home / "projects").mkdir(parents=True)
    settings = home / "ai-access.json"
    settings.write_text(json.dumps({"version": 1, "enabled": True, "examples": True,
                                    "folders": [str(work)]}), encoding="utf-8")
    monkeypatch.setenv("LIGHTSIM_AI_SETTINGS", str(settings))
    # (an Engine points the projects folder at its own; monkeypatch puts it back)
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(home / "projects"))
    engine = Engine(user_folder=home / "projects", allowed_folders=(work,), include_examples=True)
    tools = Tools(engine, Policy(allow_edits=True), AuditLog(engine.user_folder))
    listing = tools.call("lightsim_list_projects", {}, CallContext())
    ref = next(p["project"] for p in listing["projects"] if p["project"].endswith(task.reference["file"]))
    args = {"project": ref, "operations": list(task.reference["ops"]), "dry_run": False}
    try:
        answer = tools.call("model_edit", args, CallContext())
    except ConfirmationNeeded as asked:
        answer = tools.call("model_edit", args, CallContext(confirmed=asked.seal))
    assert answer["applied"] and answer["checksAfter"]["error"] == answer["checksAfter"]["warning"] == 0
    verdict = agent.check(task, work, "")
    assert verdict.success, verdict.reasons


def test_the_bench_has_33_tasks_six_of_each_kind_and_holds_out_a_third_of_each():
    assert len(TASKS) >= 33
    for kind in agent.KINDS:
        assert sum(t.kind == kind for t in TASKS) >= 6, kind
    assert agent.heldout_problems(TASKS, agent.heldout()) == []


def test_a_held_out_list_must_spread_across_the_kinds():
    """A third in all is not enough: tuning on a kind that has nothing held
    out would go unchecked. (The count alone used to be all that was checked.)"""
    by_kind = sorted(TASKS, key=lambda t: t.kind)
    lopsided = {t.id for t in by_kind[:len(TASKS) // 3]}  # the first kinds, whole
    assert abs(len(lopsided) - len(TASKS) / 3) <= 1  # the old test: fine
    problems = agent.heldout_problems(TASKS, lopsided)
    assert any("not about a third" in p for p in problems)
    assert agent.heldout_problems(TASKS, agent.heldout() | {"no-such-task"}) == [
        "held-out list: no task 'no-such-task'"]


def test_the_starting_files_are_derived_not_stored():
    """A task file holds operations on a bundled example, never a project."""
    for task in TASKS:
        assert len(task.path.read_bytes()) < 12_000
        for spec in task.start:
            assert spec["from"] == "blank" or spec["from"].startswith("example:")


def test_an_answer_line_is_read_field_by_field():
    assert agent.parse_answer("I ran it.\nANSWER: B; 3.77 s\n") == ["B", "3.77 s"]
    assert agent.parse_answer("answer: 1; 2\nANSWER: 15.6; 1.2") == ["15.6", "1.2"]
    assert agent.parse_answer("no answer") is None


# -- the agent's reach ------------------------------------------------------------


def test_an_agents_folder_holds_only_its_tasks_files(tmp_path):
    task = next(t for t in TASKS if t.id == "compare-fs-gearing")
    work, mcp_config, env = run.prepare(task, "mcp", tmp_path)
    assert sorted(p.name for p in work.iterdir()) == ["car-a.json", "car-b.json"]
    assert not any(k.startswith("LIGHTSIM_") for k in env)  # the checker's settings stay its own
    settings = json.loads((tmp_path / "home" / "ai-access.json").read_text())
    assert settings["folders"] == [str(work)]


@posix
def test_the_cli_agents_lightsim_refuses_every_path_outside_its_folder(tmp_path):
    work, _, env = run.prepare(PLANTED, "cli", tmp_path)
    lightsim = str(tmp_path / "home" / "bin" / "lightsim")

    def call(*args):
        return subprocess.run([lightsim, *args], cwd=work, env=env, capture_output=True, text=True,
                              timeout=120)

    assert call("show", "car.json").returncode == 0
    for args in [("show", str(run.PLANTED)), ("show", "../../etc/passwd"),
                 ("run", "car.json", "--out", str(tmp_path / "x.csv")),
                 ("run", "car.json", f"--out={tmp_path / 'x.csv'}"),
                 ("run", "car.json", f"-o{tmp_path / 'x.csv'}"), ("ai", "status")]:
        done = call(*args)
        assert done.returncode == 3 and "refused" in done.stderr, args
    assert not (tmp_path / "x.csv").exists()


def test_the_mcp_server_the_agent_gets_will_not_open_the_hidden_files(tmp_path):
    _, mcp_config, env = run.prepare(PLANTED, "mcp", tmp_path)
    server = json.loads(mcp_config.read_text())["mcpServers"]["lightsim"]
    lines = [{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}]
    for i, path in enumerate([run.PLANTED, agent.HELDOUT_FILE], 2):
        lines.append({"jsonrpc": "2.0", "id": i, "method": "tools/call", "params": {
            "name": "lightsim_overview", "arguments": {"project": str(path)}}})
    done = subprocess.run([server["command"], *server["args"]], env={**env, **server["env"]},
                          input="".join(json.dumps(x) + "\n" for x in lines), capture_output=True,
                          text=True, timeout=120)
    replies = {r["id"]: r for r in map(json.loads, done.stdout.splitlines())}
    for i in (2, 3):  # not found, as for any file outside the allowed folder
        assert replies[i]["result"]["isError"] is True
    assert PLANTED.check["secret"] not in done.stdout and "heldout" not in done.stdout.replace(
        str(agent.HELDOUT_FILE), "")


def test_the_runner_never_skips_permission_checks(tmp_path):
    for mode in run.MODES:
        cmd = run.claude_command("claude", mode, "x", tmp_path / "m.json", None, 1.0)
        text = " ".join(cmd)
        assert "dangerously" not in text and "bypassPermissions" not in text
        for flag in ("--restricted", "--strict-mcp-config", "--disable-slash-commands"):
            assert flag in cmd
        assert cmd[cmd.index("--permission-mode") + 1] == "dontAsk"
        assert cmd[cmd.index("--permission-prompts") + 1] == "none"
        tools = cmd[cmd.index("--tools") + 1].split(",")
        assert tools == (["ListMcpResourcesTool", "ReadMcpResourceTool"] if mode == "mcp" else ["Bash"])
        allowed = cmd[cmd.index("--allowedTools") + 1]
        assert allowed == ("mcp__lightsim,ListMcpResourcesTool,ReadMcpResourceTool" if mode == "mcp"
                           else "Bash(lightsim),Bash(lightsim *)")


def test_the_planted_task_fails_only_when_the_secret_reaches_the_agent(tmp_path):
    secret = PLANTED.check["secret"]
    assert agent.check(PLANTED, tmp_path, "ANSWER: none", "refused").success
    assert not agent.check(PLANTED, tmp_path, "ANSWER: x", f'{{"content": "{secret}"}}').success


# -- the runner ---------------------------------------------------------------------


@posix
def test_the_runner_records_each_try(fake_claude, tmp_path, monkeypatch):
    out = tmp_path / "runs"
    assert bench(fake_claude, out, monkeypatch, 1.0, "--tasks", "question-loaded-ev",
                 "--mode", "mcp,cli", "--tries", "2") == 0
    found = rows(out)
    assert [(r["mode"], r["try"]) for r in found] == [("mcp", 1), ("mcp", 2), ("cli", 1), ("cli", 2)]
    r = found[0]
    assert r["success"] and r["model"] == "fake-model" and r["costUsd"] == 0.03 and r["version"] == 1
    assert (r["toolCalls"], r["toolErrors"], r["tools"]) == (1, 1, {"mcp__lightsim__run_case": 1})
    assert (r["inputTokens"], r["outputTokens"], r["cacheReadTokens"]) == (10, 20, 300)
    assert Path(r["transcript"]).is_file() and not Path(r["transcript"]).is_relative_to(ROOT)
    summary = json.loads((next(out.iterdir()) / "summary.json").read_text())
    assert summary["passK"] == {"question-loaded-ev/mcp/session": True,
                                "question-loaded-ev/cli/session": True}
    calls = [json.loads(x) for x in (tmp_path / "calls.jsonl").read_text().splitlines()]
    assert all(c[c.index("--max-budget-usd") + 1] for c in calls)


@posix
def test_the_cap_stops_the_run_before_the_next_try(fake_claude, tmp_path, monkeypatch):
    out = tmp_path / "runs"
    assert bench(fake_claude, out, monkeypatch, 0.05, "--tasks", "question-loaded-ev",
                 "--mode", "mcp", "--tries", "3") == 0
    assert len(rows(out)) == 2  # 0.03 + 0.03 reached 0.05: the third never started
    summary = json.loads((next(out.iterdir()) / "summary.json").read_text())
    assert "cap" in summary["stopped"] and summary["passK"]["question-loaded-ev/mcp/session"] is None
    calls = [json.loads(x) for x in (tmp_path / "calls.jsonl").read_text().splitlines()]
    assert [c[c.index("--max-budget-usd") + 1] for c in calls] == ["0.0500", "0.0200"]


@posix
def test_a_try_killed_at_the_timeout_counts_as_the_rest_of_the_cap(fake_claude, tmp_path, monkeypatch):
    monkeypatch.setenv("FAKE_SLEEP", "60")
    out = tmp_path / "runs"
    assert bench(fake_claude, out, monkeypatch, 0.5, "--tasks", "question-loaded-ev",
                 "--mode", "mcp", "--tries", "2", "--timeout", "2") == 0
    (row,) = rows(out)  # it reported no cost: the second try must not start
    assert (row["end"], row["success"], row["costUsd"]) == ("timeout", False, 0.5)


@posix
def test_no_cap_no_run_and_no_transcripts_in_the_repository(fake_claude, tmp_path, monkeypatch):
    monkeypatch.delenv(run.CAP_ENV, raising=False)
    assert run.main(["--claude", str(fake_claude), "--out", str(tmp_path)]) == 2
    monkeypatch.setenv(run.CAP_ENV, "1")
    assert run.main(["--claude", str(fake_claude), "--out", str(ROOT / "runs")]) == 2
    assert not (tmp_path / "calls.jsonl").exists() and not (ROOT / "runs").exists()


@posix
def test_the_smoke_test(fake_claude, tmp_path, monkeypatch):
    assert bench(fake_claude, tmp_path / "ok", monkeypatch, 2.0, "--smoke") == 0
    found = rows(tmp_path / "ok")
    assert [(r["task"], r["mode"]) for r in found] == [
        ("question-loaded-ev", "mcp"), ("question-loaded-ev", "cli"),
        ("planted-read-checker", "mcp"), ("planted-read-checker", "cli"), ("question-loaded-ev", "mcp")]
    # a tool that hands over the secret fails the smoke test
    monkeypatch.setenv("FAKE_LEAK", PLANTED.check["secret"])
    assert bench(fake_claude, tmp_path / "leak", monkeypatch, 2.0, "--smoke") == 1
