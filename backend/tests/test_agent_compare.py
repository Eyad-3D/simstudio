"""The model and effort comparison (benchmarks/agent/compare.py and the try
in run.py, see benchmarks/agent/README.md).

Driven with a stand-in for ``claude -p`` that prints the stream-json Claude
Code 2.1.273 prints (the shapes of its errors were recorded against a local
stand-in for the API), so these tests spend nothing: the failure classes, the
plan-only environment, resuming, the order of the tries, side-by-side jobs,
the stage rules, the list-price cost and both answers.
"""
from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:  # the benchmarks package lives at the repository root
    sys.path.insert(0, str(ROOT))

from benchmarks import agent  # noqa: E402
from benchmarks.agent import compare, run  # noqa: E402

TASKS = {t.id: t for t in agent.load_tasks()}
posix = pytest.mark.skipif(sys.platform == "win32", reason="the runner runs on Linux or macOS")

FAKE = r'''
import json, os, sys, time
args = sys.argv[1:]
if args[:2] == ["auth", "status"]:
    print(json.dumps({"loggedIn": True})); sys.exit(0)
opt = lambda name: args[args.index(name) + 1] if name in args else None
prompt, model, effort = opt("-p"), opt("--model"), opt("--effort")
answers = json.load(open(os.environ["FAKE_ANSWERS"]))
answer = next((a for k, a in answers.items() if k in prompt), "ANSWER: none")
task = next((k for k in answers if k in prompt), "?")
rules = json.load(open(os.environ["FAKE_RULES"])) if os.environ.get("FAKE_RULES") else []
state = os.environ["FAKE_LOG"] + ".state"
used = json.load(open(state)) if os.path.exists(state) else {}
do = "ok"
for i, r in enumerate(rules):
    if r.get("model", model) == model and r.get("task", task) == task and used.get(str(i), 0) < r.get("times", 1e9):
        used[str(i)] = used.get(str(i), 0) + 1; do = r["do"]; break
json.dump(used, open(state, "w"))
log = {"t0": time.time(), "task": task, "model": model, "effort": effort, "do": do,
       "budget": opt("--max-budget-usd"), "partial": "--include-partial-messages" in args,
       "keys": sorted(k for k in os.environ if k.startswith(("ANTHROPIC_", "CLAUDE_CODE_USE_")))}
source = "ANTHROPIC_API_KEY" if "ANTHROPIC_API_KEY" in os.environ else "none"
def out(e): print(json.dumps(e), flush=True)
def fail(error, status, text):
    out({"type": "assistant", "error": error, "is_api_error_message": True,
         "message": {"model": "<synthetic>", "content": [{"type": "text", "text": text}]}})
    out({"type": "result", "subtype": "success", "is_error": True, "terminal_reason": "api_error",
         "api_error_status": status, "result": text, "total_cost_usd": 0, "modelUsage": {}})
if do == "noresult":
    sys.exit(1)
out({"type": "system", "subtype": "init", "model": model, "apiKeySource": source, "claude_code_version": "2.1.273",
     "mcp_servers": [{"name": "lightsim", "status": "failed" if do == "nomcp" else "connected"}]})
if do == "effort":
    open(opt("--debug-file"), "w").write(
        f"[WARN] [effort] model {model} rejected output_config.effort; latching unsupported and retrying without it.\n")
if do == "limit":
    out({"type": "rate_limit_event", "rate_limit_info": {"status": "rejected", "resetsAt": int(time.time()) + 2,
                                                          "rateLimitType": "five_hour"}})
    fail("rate_limit", 429, "You've hit your session limit")
elif do == "529":
    out({"type": "system", "subtype": "api_retry", "attempt": 1, "error_status": 529, "error": "overloaded"})
    fail("server_error", 529, "API Error: 529 Overloaded.")
elif do == "notfound":
    fail("model_not_found", 404, f"There's an issue with the selected model ({model}).")
elif do == "signedout":
    fail("authentication_failed", None, "Failed to authenticate: OAuth session expired")
else:
    answered = "claude-opus-5" if do == "othermodel" else model
    usage = {"input_tokens": 1000, "cache_read_input_tokens": 20000, "cache_creation_input_tokens": 2000,
             "cache_creation": {"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": 2000}, "output_tokens": 1}
    out({"type": "stream_event", "event": {"type": "message_start", "message": {"id": "m1", "model": answered, "usage": usage}}})
    out({"type": "assistant", "message": {"model": answered, "content": [
        {"type": "tool_use", "id": "t1", "name": "mcp__lightsim__run_case", "input": {}}]}})
    out({"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": "t1", "is_error": True, "content": "a LightSim error"}]}})
    out({"type": "stream_event", "event": {"type": "message_delta", "usage": {"output_tokens": 500}}})
    if do == "sleep":
        time.sleep(float(os.environ.get("FAKE_SLEEP", "60")))
    out({"type": "result", "subtype": "success", "is_error": False, "terminal_reason": "completed",
         "result": "ANSWER: 0; 0" if do == "wrong" else answer, "num_turns": 2, "total_cost_usd": 0.0815,
         "usage": {**usage, "output_tokens": 500}, "permission_denials": [],
         "modelUsage": {answered: {"inputTokens": 1000, "outputTokens": 500, "cacheReadInputTokens": 20000,
                                   "cacheCreationInputTokens": 2000, "costUSD": 0.0815, "costBasis": "unknown"}}})
log["t1"] = time.time()
open(os.environ["FAKE_LOG"], "a").write(json.dumps(log) + "\n")
'''
#: the list price of one ok try on Sonnet 5.5: 1000 in, 500 out, 20 000 cache read, 2000 written for 1 h
SONNET_TRY = (1000 * 2 + 500 * 10 + 20000 * 0.1 + 2000 * 4) / 1e6
QUESTIONS = ["question-loaded-ev", "question-winter-range", "compare-bev-gearing"]


@pytest.fixture
def fake(tmp_path, monkeypatch):
    path = tmp_path / "claude"
    path.write_text(f"#!{sys.executable}\n{FAKE}", encoding="utf-8")
    path.chmod(0o755)
    answers = {t.statement[:60]: agent.answer_text(t) for t in TASKS.values() if t.answer}
    (tmp_path / "answers.json").write_text(json.dumps(answers), encoding="utf-8")
    monkeypatch.setenv("FAKE_ANSWERS", str(tmp_path / "answers.json"))
    monkeypatch.setenv("FAKE_LOG", str(tmp_path / "calls.jsonl"))
    monkeypatch.delenv("FAKE_RULES", raising=False)
    monkeypatch.delenv(run.CAP_ENV, raising=False)
    for key in ("LIGHTSIM_PROJECTS_DIR", "LIGHTSIM_DATA_DIR", "LIGHTSIM_SCRIPT_TRUST"):
        monkeypatch.setenv(key, os.environ.get(key, ""))  # isolate() changes them
    monkeypatch.setattr(compare, "BACKOFF_S", 0.2)
    monkeypatch.setattr(compare, "LIMIT_MARGIN_S", 0.0)
    return path


def rules(tmp_path, monkeypatch, *items):
    (tmp_path / "rules.json").write_text(json.dumps(list(items)), encoding="utf-8")
    monkeypatch.setenv("FAKE_RULES", str(tmp_path / "rules.json"))


def go(fake: Path, out: Path, *args: str) -> int:
    return compare.main(["--claude", str(fake), "--out", str(out), "--jobs", "1", *args])


def calls(tmp_path) -> list[dict]:
    path = tmp_path / "calls.jsonl"
    return [json.loads(x) for x in path.read_text().splitlines()] if path.exists() else []


def tries(out: Path, name: str = "comparison") -> dict:
    return {p.stem: json.loads(p.read_text()) for p in (out / name / "tries").glob("*.json")}


def notruns(out: Path, name: str = "comparison") -> list[dict]:
    path = out / name / "notrun.jsonl"
    return [json.loads(x) for x in path.read_text().splitlines()] if path.exists() else []


# -- the try --------------------------------------------------------------------------


@posix
def test_a_try_runs_on_the_plan_with_no_key_and_no_cap_and_records_its_setting(fake, tmp_path, monkeypatch):
    for key in ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL",
                "CLAUDE_CODE_USE_BEDROCK", "AWS_BEARER_TOKEN_BEDROCK", "CLAUDE_CODE_EFFORT_LEVEL"):
        monkeypatch.setenv(key, "set-in-the-parent")
    monkeypatch.setenv(run.CAP_ENV, "0.001")  # ignored on the plan: no cap
    out = tmp_path / "runs"
    assert go(fake, out, "--tasks", "question-loaded-ev", "--settings", "sonnet-medium,opus-high",
              "--stage", "A") == 0
    (c1, c2) = calls(tmp_path)
    assert c1["keys"] == c2["keys"] == [] and c1["budget"] is None and c1["partial"]
    assert [(c["model"], c["effort"]) for c in (c1, c2)] == [("claude-sonnet-5-5", "medium"),
                                                             ("claude-opus-5-5", "high")]
    r = tries(out)["question-loaded-ev.mcp.sonnet-medium.1"]
    assert (r["class"], r["billing"], r["model"], r["effort"], r["claudeCode"]) == (
        "solved", "plan", "claude-sonnet-5-5", "medium", "2.1.273")
    assert r["reported"] == {"init": "claude-sonnet-5-5", "answered": ["claude-sonnet-5-5"],
                             "result": ["claude-sonnet-5-5"]}
    assert r["tokens"] == {"input": 1000, "output": 500, "cacheRead": 20000, "cacheWrite": 2000}
    assert r["costUsd"] == pytest.approx(SONNET_TRY) and r["cliCostUsd"] == 0.0815
    assert (r["toolCalls"], r["toolErrors"], r["retries"]) == (1, 1, 0)
    assert not Path(r["transcript"]).is_relative_to(ROOT)


def test_billing_env_keeps_only_what_the_try_needs():
    env = {"PATH": "/bin", "HOME": "/h", "ANTHROPIC_API_KEY": "k", "ANTHROPIC_AUTH_TOKEN": "t",
           "ANTHROPIC_BASE_URL": "u", "ANTHROPIC_DEFAULT_SONNET_MODEL": "m", "CLAUDE_CODE_USE_VERTEX": "1",
           "AWS_BEARER_TOKEN_BEDROCK": "b", "CLAUDE_CODE_EFFORT_LEVEL": "max"}
    assert run.billing_env(env) == {"PATH": "/bin", "HOME": "/h"}
    assert run.billing_env(env, "api") == {"PATH": "/bin", "HOME": "/h", "ANTHROPIC_API_KEY": "k"}


@posix
def test_api_mode_alone_may_have_a_cap(fake, tmp_path, monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "not-a-key")  # read by the stand-in only
    monkeypatch.setenv(run.CAP_ENV, "0.05")
    out = tmp_path / "runs"
    assert go(fake, out, "--auth", "api", "--tasks", ",".join(QUESTIONS + ["question-socket-mpge"]),
              "--settings", "sonnet-medium", "--stage", "A") == 1  # stopped by the cap
    found = calls(tmp_path)
    assert len(found) == 3  # 3 x 0.019 USD passed 0.05: the fourth never started
    assert [float(c["budget"]) for c in found] == pytest.approx([0.05, 0.05 - SONNET_TRY, 0.05 - 2 * SONNET_TRY],
                                                               abs=1e-4)
    assert all(c["keys"] == ["ANTHROPIC_API_KEY"] for c in found)
    assert {r["billing"] for r in tries(out).values()} == {"api (ANTHROPIC_API_KEY)"}


def test_the_try_never_skips_permission_checks(tmp_path):
    for mode in run.MODES:
        cmd = run.claude_command("claude", mode, "x", tmp_path / "m.json", "claude-sonnet-5-5", "medium")
        assert "dangerously" not in " ".join(cmd) and "bypassPermissions" not in " ".join(cmd)
        assert "--max-budget-usd" not in cmd  # no cap unless one is set on purpose (API mode)
        assert cmd[cmd.index("--effort") + 1] == "medium" and cmd[cmd.index("--model") + 1] == "claude-sonnet-5-5"
    assert run.claude_command("claude", "mcp", "x", tmp_path, None, None, 1.5)[-2:] == ["--max-budget-usd", "1.5000"]


# -- the failure classes ------------------------------------------------------------------


def seen_of(tmp_path, *events) -> dict:
    path = tmp_path / "t.jsonl"
    path.write_text("".join(json.dumps(e) + "\n" for e in events), encoding="utf-8")
    return run.read_transcript(path)


INIT = {"type": "system", "subtype": "init", "model": "claude-sonnet-5-5", "apiKeySource": "none",
        "mcp_servers": [{"name": "lightsim", "status": "connected"}]}


def api_failure(error, status, text="x"):
    return [{"type": "assistant", "error": error, "message": {"model": "<synthetic>", "content": []}},
            {"type": "result", "is_error": True, "terminal_reason": "api_error", "api_error_status": status,
             "result": text}]


def done(**extra):
    return {"type": "result", "subtype": "success", "is_error": False, "terminal_reason": "completed",
            "result": "ANSWER: 1", **extra}


@pytest.mark.parametrize("events, debug, timed_out, passed, want", [
    ([INIT, {"type": "assistant", "message": {"model": "claude-sonnet-5-5", "content": []}}, done()], "", False,
     True, "solved"),
    # LightSim's errors and approval refusals the agent gave up on are part of the score
    ([INIT, {"type": "user", "message": {"content": [{"type": "tool_result", "is_error": True,
                                                      "content": "the user declined the change"}]}}, done()],
     "", False, False, "failed"),
    ([INIT, done(is_error=True, terminal_reason="prompt_too_long")], "", False, False, "failed"),
    ([INIT], "", True, False, "failed"),  # killed at the timeout: scored on what it left
    ([INIT, *api_failure("rate_limit", 429)], "", False, False, "not-run:limit"),
    ([INIT, {"type": "rate_limit_event", "rate_limit_info": {"status": "rejected", "resetsAt": 1}},
      *api_failure("unknown", None)], "", False, False, "not-run:limit"),
    ([INIT, *api_failure("billing_error", 400)], "", False, False, "not-run:limit"),
    ([INIT, *api_failure("server_error", 529)], "", False, False, "not-run:service"),
    ([INIT, {"type": "system", "subtype": "api_retry", "error": "overloaded"}], "", True, False,
     "not-run:service"),  # killed while Claude Code waited to retry
    ([INIT, *api_failure("authentication_failed", None)], "", False, False, "not-run:stop"),
    ([{**INIT, "apiKeySource": "ANTHROPIC_API_KEY"}, done()], "", False, True, "not-run:stop"),
    ([], "", False, False, "not-run:runner"),
    ([INIT], "", False, False, "not-run:runner"),
    ([{**INIT, "mcp_servers": [{"name": "lightsim", "status": "failed"}]}, done()], "", False, True,
     "not-run:runner"),
    ([INIT, *api_failure("model_not_found", 404)], "", False, False, "unavailable"),
    ([INIT, done()], "[WARN] [effort] model claude-sonnet-5-5 rejected output_config.effort; latching", False,
     True, "unavailable"),  # Claude Code 2.1.273
    ([INIT, done()], "[WARN] [effort] the server refused effort for model claude-sonnet-5-5 (the "
     "output_config.effort field). Sending the request again without either", False, True, "unavailable"),
    ([INIT, done()], "[DEBUG] [effort] Effort 'xhigh' exceeds the cap for claude-sonnet-5-5 set by your settings "
     "or organization; using 'high'.", False, True, "unavailable"),  # 2.1.316
    # some events carry a text as their message (a permission denial in the shell)
    ([INIT, {"type": "system", "subtype": "permission_denied", "message": "Permission to use Bash has been denied"},
      done()], "", False, True, "solved"),
    ([INIT, {"type": "assistant", "message": {"model": "claude-opus-5", "content": []}}, done()], "", False, True,
     "unavailable"),
    ([INIT, {"type": "assistant", "message": {"model": "claude-sonnet-5-5-20261001", "content": []}}, done()],
     "", False, True, "solved"),
])
def test_every_try_ends_in_one_class(tmp_path, events, debug, timed_out, passed, want):
    cls, why = run.classify(seen_of(tmp_path, *events), debug, timed_out, mode="mcp", auth="session",
                            model="claude-sonnet-5-5", passed=passed)
    assert (f"{cls}:{why.split(':')[0]}" if cls == "not-run" else cls) == want, why


def test_the_list_price_is_worked_out_per_request(tmp_path):
    big = {"input_tokens": 5000, "cache_read_input_tokens": 100_000, "cache_creation_input_tokens": 0}
    seen = seen_of(
        tmp_path,
        {"type": "stream_event", "event": {"type": "message_start", "message": {
            "id": "a", "model": "claude-haiku-5-5", "usage": {"input_tokens": 1000, "output_tokens": 1}}}},
        {"type": "stream_event", "event": {"type": "message_delta", "usage": {"output_tokens": 100}}},
        {"type": "stream_event", "event": {"type": "message_start", "message": {
            "id": "b", "model": "claude-haiku-5-5", "usage": big}}},  # over 100 000: the second rate card
        {"type": "stream_event", "event": {"type": "message_delta", "usage": {"output_tokens": 200}}},
        {"type": "result", "modelUsage": {
            "claude-haiku-5-5": {"inputTokens": 6000, "outputTokens": 300, "cacheReadInputTokens": 100_000},
            "claude-haiku-4-5-20251001": {"inputTokens": 400, "outputTokens": 10},  # Claude Code's own call
            "claude-new-9": {"inputTokens": 1, "costUSD": 0.5}}})
    cost, unknown = run.list_cost(seen)
    want = ((1000 * 0.1 + 100 * 0.5) + (5000 * 0.5 + 100_000 * 0.05 + 200 * 2.5) + (400 * 1 + 10 * 5)) / 1e6 + 0.5
    assert cost == pytest.approx(want) and unknown == ["claude-new-9"]


# -- running the comparison ---------------------------------------------------------------


@posix
def test_a_limit_is_waited_out_and_the_try_retried_never_scored(fake, tmp_path, monkeypatch):
    rules(tmp_path, monkeypatch, {"model": "claude-sonnet-5-5", "do": "limit", "times": 1},
          {"model": "claude-opus-5-5", "do": "529", "times": 2})
    out = tmp_path / "runs"
    assert go(fake, out, "--tasks", "question-loaded-ev", "--settings", "sonnet-medium,opus-high",
              "--stage", "A") == 0
    found = calls(tmp_path)
    assert [c["do"] for c in found] == ["limit", "ok", "529", "529", "ok"]
    assert found[1]["t0"] >= found[0]["t0"] + 0.9  # it waited for the reset time the limit gave
    recs = tries(out)
    assert {k: (r["class"], r["retries"]) for k, r in recs.items()} == {
        "question-loaded-ev.mcp.sonnet-medium.1": ("solved", 1), "question-loaded-ev.mcp.opus-high.1": ("solved", 2)}
    assert [n["why"].split(":")[0] for n in notruns(out)] == ["limit", "service", "service"]
    data = json.loads((out / "comparison" / "report.json").read_text())
    assert data["scoredTries"] == 2 and data["notRunTries"] == 3
    assert data["settings"]["sonnet-medium/mcp"]["notRunRetries"] == 1


@posix
def test_a_stopped_comparison_goes_on_where_it_stopped(fake, tmp_path, monkeypatch):
    rules(tmp_path, monkeypatch, {"task": TASKS["question-winter-range"].statement[:60], "model": "claude-haiku-5-5",
                                  "do": "noresult"})
    out = tmp_path / "runs"
    args = ["--tasks", "question-loaded-ev,question-winter-range", "--settings", "haiku-low,sonnet-medium",
            "--stage", "A"]
    assert go(fake, out, *args) == 1  # the runner's fault, three times: it stops
    assert sorted(tries(out)) == ["question-loaded-ev.mcp.haiku-low.1", "question-loaded-ev.mcp.sonnet-medium.1"]
    assert len(notruns(out)) == 3
    monkeypatch.delenv("FAKE_RULES")  # fixed; the same command again
    before = len(calls(tmp_path))
    assert go(fake, out, *args) == 0
    again = calls(tmp_path)[before:]
    assert [(c["model"], c["effort"]) for c in again] == [("claude-haiku-5-5", "low"), ("claude-sonnet-5-5", "medium")]
    assert tries(out)["question-winter-range.mcp.haiku-low.1"]["attempt"] == 4
    assert go(fake, out, *args) == 0 and len(calls(tmp_path)) == before + 2  # all done: nothing runs


@posix
def test_the_tries_go_task_by_task_across_the_settings(fake, tmp_path):
    assert go(fake, tmp_path / "runs", "--tasks", ",".join(QUESTIONS[:2]),
              "--settings", "haiku-low,sonnet-medium,opus-high", "--stage", "A") == 0
    order = [(c["task"], c["model"]) for c in calls(tmp_path)]
    first, second = (TASKS[t].statement[:60] for t in QUESTIONS[:2])
    assert order == [(first, "claude-haiku-5-5"), (first, "claude-sonnet-5-5"), (first, "claude-opus-5-5"),
                     (second, "claude-haiku-5-5"), (second, "claude-sonnet-5-5"), (second, "claude-opus-5-5")]


@posix
def test_jobs_run_tries_side_by_side(fake, tmp_path, monkeypatch):
    rules(tmp_path, monkeypatch, {"do": "sleep"})
    monkeypatch.setenv("FAKE_SLEEP", "1.5")
    assert compare.main(["--claude", str(fake), "--out", str(tmp_path / "runs"), "--jobs", "3",
                         "--tasks", ",".join(QUESTIONS), "--settings", "haiku-low,sonnet-medium",
                         "--stage", "A"]) == 0
    spans = [(c["t0"], c["t1"]) for c in calls(tmp_path)]
    most = max(sum(a <= t < b for a, b in spans) for t, _ in spans)
    assert len(spans) == 6 and most == 3


@posix
def test_a_setting_the_plan_refuses_is_unavailable_and_not_run_again(fake, tmp_path, monkeypatch):
    rules(tmp_path, monkeypatch, {"model": "claude-haiku-5-5", "do": "notfound"},
          {"model": "claude-opus-5-5", "do": "othermodel"})
    out = tmp_path / "runs"
    assert go(fake, out, "--tasks", ",".join(QUESTIONS), "--settings", "haiku-low,sonnet-medium,opus-high",
              "--stage", "A") == 0
    assert [c["model"] for c in calls(tmp_path)] == [
        "claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5", "claude-sonnet-5-5", "claude-sonnet-5-5"]
    text = (out / "comparison" / "report.md").read_text()
    assert "Unavailable settings: Haiku 5.5, low: the plan refused the model" in text
    assert "Opus 5.5, high: claude-opus-5 answered, not claude-opus-5-5" in text


@posix
def test_a_try_killed_at_the_timeout_is_scored_on_what_it_left(fake, tmp_path, monkeypatch):
    rules(tmp_path, monkeypatch, {"do": "sleep"})
    out = tmp_path / "runs"
    assert go(fake, out, "--tasks", "question-loaded-ev", "--settings", "sonnet-medium", "--stage", "A",
              "--timeout", "2") == 0
    (r,) = tries(out).values()
    assert (r["class"], r["end"], r["costComplete"]) == ("failed", "timeout", False)
    assert r["costUsd"] == pytest.approx(SONNET_TRY)  # what its streamed request showed


def test_transcripts_stay_outside_the_repository_and_names_must_exist(fake, tmp_path):
    assert compare.main(["--claude", str(fake), "--out", str(ROOT / "runs")]) == 2
    for bad in (["--tasks", "question-loaded-ev,no-such-task"], ["--settings", "sonnet-mid"]):
        assert compare.main(["--claude", str(fake), "--out", str(tmp_path / "runs"), *bad]) == 2
    assert not (ROOT / "runs").exists() and not calls(tmp_path)


# -- the stages and the answers -----------------------------------------------------------
ALL = list(TASKS.values())
NAMES = [s.name for s in compare.SETTINGS]


def rec(task: str, setting: str, n: int = 1, solved: bool = True, cost: float = 0.1, mode: str = "mcp") -> dict:
    return {"id": f"{task}.{mode}.{setting}.{n}", "task": task, "setting": setting, "mode": mode, "try": n,
            "class": "solved" if solved else "failed", "costUsd": cost, "cliCostUsd": 0.0, "toolCalls": 1,
            "toolErrors": 0, "seconds": 10.0, "tokens": {"input": 1, "output": 1, "cacheRead": 1, "cacheWrite": 1}}


def records(*rs) -> dict:
    return {r["id"]: r for r in rs}


def stage_a_records(solved: dict[str, int]) -> list[dict]:
    """Stage A: setting -> how many tasks (the first ones) it solved."""
    return [rec(t.id, s, solved=i < solved.get(s, 0)) for s in NAMES for i, t in enumerate(ALL)]


def test_stage_b_takes_the_ceiling_the_target_and_those_within_five_of_the_ceiling():
    rs = records(*stage_a_records({"opus-high": 30, "sonnet-xhigh": 25, "sonnet-high": 24, "sonnet-medium": 3,
                                   "haiku-high": 26}),
                 {**rec(ALL[0].id, "haiku-high", 2), "class": "unavailable", "why": "x"})
    picked = [s.name for s in compare.stage_b_settings(rs, ALL, list(compare.SETTINGS))]
    assert picked == ["sonnet-medium", "sonnet-xhigh", "opus-high"]
    specs, why = compare.plan("B", rs, ALL, list(compare.SETTINGS))
    assert not why and len(specs) == 3 * len(ALL) * 2
    assert [(s.task.id, s.setting.name, s.n) for s in specs[:3]] == [
        (ALL[0].id, "sonnet-medium", 2), (ALL[0].id, "sonnet-medium", 3), (ALL[0].id, "sonnet-xhigh", 2)]
    assert compare.plan("B", records(*list(rs.values())[1:]), ALL, list(compare.SETTINGS)) == (
        [], "stage B waits for stage A to finish")


def three_tries(solved: dict[str, int], cost: dict[str, float]) -> dict:
    """Three tries of every task: setting -> how many tasks (the first ones) it solves on all three."""
    return records(*[rec(t.id, s, n, solved=i < k, cost=cost[s])
                     for s, k in solved.items() for i, t in enumerate(ALL) for n in (1, 2, 3)])


def test_the_cheapest_working_setting_is_the_cheapest_within_two_of_the_ceiling():
    rs = three_tries({"opus-high": 30, "sonnet-xhigh": 29, "sonnet-medium": 28, "haiku-high": 27},
                     {"opus-high": 1.0, "sonnet-xhigh": 0.5, "sonnet-medium": 0.2, "haiku-high": 0.01})
    rs[f"{ALL[31].id}.mcp.sonnet-medium.2"]["class"] = "solved"  # one try more is still not all three
    summary = compare.summarize(rs, [], ALL, list(compare.SETTINGS))
    a1 = compare.cheapest_working(summary, ALL)
    assert (a1["setting"], a1["pass3"], a1["ceilingPass3"]) == ("sonnet-medium", [28, 33], [30, 33])
    assert a1["candidates"] == ["sonnet-medium", "sonnet-xhigh", "opus-high"]
    assert a1["onlyCeilingSolves"] == sorted(t.id for t in ALL[28:30]) and a1["onlyItSolves"] == []
    specs, why = compare.plan("C", rs, ALL, [s for s in compare.SETTINGS if s.name in
                                             ("haiku-high", "sonnet-medium", "sonnet-xhigh", "opus-high")])
    assert not why and {(s.mode, s.setting.name) for s in specs} == {("cli", "sonnet-medium"), ("cli", "opus-high")}
    assert compare.cheapest_working(compare.summarize(records(*stage_a_records({})), [], ALL,
                                                      list(compare.SETTINGS)), ALL)["setting"] is None


def test_hands_off_needs_nine_in_ten_on_all_tasks_and_on_the_held_out_ones():
    held = agent.heldout()
    misses_visible = sorted(ALL, key=lambda t: t.id not in held)  # the tasks it misses come last
    misses_held = sorted(ALL, key=lambda t: t.id in held)

    def three(setting, order, k):
        return [rec(t.id, setting, n, solved=i < k) for i, t in enumerate(order) for n in (1, 2, 3)]

    rs = records(*three("opus-high", misses_visible, 31), *three("sonnet-medium", misses_held, 31))
    summary = compare.summarize(rs, [], ALL, list(compare.SETTINGS))
    assert summary["sonnet-medium/mcp"]["pass3"]["heldout"] == [9, 11]  # 31 of 33, but 82 % held out
    assert compare.hands_off(summary, ALL)["settings"] == ["opus-high"]
    rs = records(*three("opus-high", misses_held, 31), *three("sonnet-medium", misses_held, 20))
    text, data = compare.report(rs, [], ALL, list(compare.SETTINGS))
    assert data["handsOff"]["settings"] == []
    assert "No setting is good enough for hands-off work" in text
    assert "with Sonnet 5.5, medium as the target (it has 20/33 and 0/11)" in text
