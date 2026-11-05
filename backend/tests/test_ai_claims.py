"""What the AI access page and the help say about the AI connection is true.

Connect AI → AI access, the help (*Use an AI assistant*, *Command-line tool*,
*Python API*), Known issues, the README and the skill pack all say what an
AI app can do through LightSim's MCP server today (step 6, the owner's
decision of 1 November 2026: an agent works hands-off):

- it reads, checks, compares, runs a project and saves an edit, all without
  a question — the connection never asks the user to confirm;
- a project with Script blocks or FMUs runs in an OS sandbox (no network,
  file access to the project's folder only); when that sandbox cannot be set
  up (today, on Windows) the run is refused with the stable code
  ``sandbox_unavailable`` rather than run less safely;
- an edit is saved straight away and is recoverable: the previous version is
  kept as a backup, and an example's edit is saved as a new project;
- it does NOT read the trust list that ``lightsim ai trust`` keeps: only
  Python code that goes through ``lightsim.ai_access.AgentSession`` does,
  and the plain ``lightsim`` package and ``lightsim run`` read no AI access
  setting at all;
- LightSim's own approval of Script code (app/script_trust.py) still applies
  to an AI app's run, so a brand-new script the assistant wrote fails the
  run until the user approves it in the app;
- the AI apps' audit log has a line for each call that reaches a tool, but
  not for a run sent as a task that is refused before it starts, or a call
  LightSim cannot read (a tool it does not have, arguments that are not an
  object);
- the connection reads the settings before every tool call, and
  ``AgentSession`` reads them once, when the session starts;
- while AI access is off (the user's switch or the organisation's policy)
  every tool call is refused, but an AI app can still read the skill pack
  and the part library that come with LightSim.

The behaviour is held in test_mcp_server.py (``test_reading_asks_the_user_
nothing``, ``test_edits_are_dry_runs_and_saving_is_direct``,
``test_script_projects_run_in_the_sandbox_or_are_refused``,
``test_every_script_run_is_sandboxed_and_the_trust_list_is_not_read``,
``test_script_code_nobody_approved_fails_the_run``,
``test_what_the_audit_log_leaves_out``,
``test_every_tool_call_is_refused_while_ai_access_is_off``) and in
test_agent_sandbox.py (the strict sandbox and the refusal). This file holds
the rest of the facts and the words: the page, the help and the other places
that repeat the claim must say it, must not say the old claim again (that the
trust list governs AI apps, or that the connection asks before an edit or a
run), and must not say more than the code does.

When the behaviour changes, change these tests with the page and the help.
"""
from __future__ import annotations

import json
import os
import re
from pathlib import Path

import pytest
from conftest import allow_ai

import lightsim as ls
from app.ai.access import AuditLog, Policy
from app.ai.engine import Engine
from app.ai.mcp_server import McpServer
from app.ai.tools import TOOLS
from lightsim.ai_access import AUDIT_NAME, AccessDenied, AgentSession
from lightsim.cli import main

ROOT = Path(__file__).resolve().parents[2]
PANEL = "frontend/src/components/AiAccessPanel.tsx"
HOWTO = "docs/help/how-to/use-an-ai-assistant.md"
COMMAND_LINE = "docs/help/reference/command-line.md"
PYTHON_API = "docs/help/reference/python-api.md"
KNOWN_LIMITS = "docs/KNOWN-LIMITS.md"
RELEASE_NOTES = "docs/RELEASE-NOTES.md"
README = "README.md"
DEPLOY = "docs/help/how-to/deploy-for-it.md"
CONNECT = "frontend/src/components/AiConnectDialog.tsx"
SKILL = "backend/app/ai/skills/lightsim-write-a-script-block/SKILL.md"
SKILL_LIMITS = "backend/app/ai/skills/lightsim-what-it-cannot-do/references/known-limits.md"


# -- the Python side, as the page and the help describe it -----------------------


def test_the_lightsim_package_and_command_read_no_ai_setting():
    # AI access is off in every test (conftest), yet the package and the
    # command work, and only AgentSession refuses
    project = ls.load("bev-car")
    assert project.check() is not None
    assert main(["check", "bev-car"]) == 0
    with pytest.raises(AccessDenied, match="off"):
        AgentSession().open("bev-car")


def test_lightsim_ai_log_lists_only_the_calls_through_agentsession(tmp_path, monkeypatch, capsys):
    projects = tmp_path / "projects"
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(projects))
    allow_ai(projects)
    server = McpServer(Engine(user_folder=projects), Policy(), AuditLog(projects))
    server.handle({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "lightsim_list_projects", "arguments": {},
        "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}}})
    AgentSession(client="python").list_projects()

    assert main(["ai", "log", "--json"]) == 0
    logged = json.loads(capsys.readouterr().out)
    assert [e["tool"] for e in logged] == ["list_projects"]  # AgentSession's name for it
    assert logged[0]["arguments"]["client"] == "python"
    own = (projects / ".ai" / "audit.jsonl").read_text(encoding="utf-8")
    assert "lightsim_list_projects" in own, "an AI app's calls go to the projects folder's log"
    assert AUDIT_NAME == "ai-audit.jsonl"  # the other one, beside the settings


def test_agentsession_reads_the_settings_when_the_session_starts(tmp_path, monkeypatch):
    projects = tmp_path / "projects"
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(projects))
    allow_ai(projects)
    session = AgentSession()
    Path(os.environ["LIGHTSIM_AI_SETTINGS"]).write_text(json.dumps({"version": 1, "enabled": False}),
                                                         encoding="utf-8")
    assert session.list_projects(), "a session keeps the settings it started with"
    with pytest.raises(AccessDenied, match="off"):
        AgentSession().list_projects()


def test_the_run_case_tool_tells_an_agent_what_it_will_meet():
    description = next(t for t in TOOLS if t["name"] == "run_case")["description"]
    assert "not asked to confirm" in description
    assert "OS sandbox" in description and "sandbox_unavailable" in description
    assert "has not been approved" in description
    edit = next(t for t in TOOLS if t["name"] == "model_edit")["description"]
    assert "saved straight away (you are not asked to confirm)" in edit
    assert "backup" in edit and "undone" in edit


# -- the words -------------------------------------------------------------------


def text(rel: str) -> str:
    """A file's text with every run of white space as one space and the JSX
    ``{" "}`` gone, so a sentence is found wherever the line breaks fall."""
    raw = (ROOT / rel).read_text(encoding="utf-8").replace('{" "}', " ")
    return re.sub(r"\s+", " ", raw)


#: an agent works hands-off (the page, the help, Known issues, the README)
HANDS_OFF = "an agent works hands-off"
NO_QUESTION = "the connection never asks you to confirm"
#: a Script/FMU run goes in a sandbox, or is refused (the page, the help, Known issues)
SANDBOXED = "runs in an OS sandbox"
REFUSED = "the run is refused with the code `sandbox_unavailable` rather than run less safely"
#: an edit is saved and recoverable
SAVED = "an edit is saved straight away"
RECOVER = "the previous version is kept as a backup"
#: the engine still refuses unapproved script code
APPROVAL = "code you have not approved there makes the run fail"
#: when the settings are read (the page, the help)
FRESH = "before every tool call"
STARTS = "reads them when it starts a session"
#: what an AI app still gets while access is off (the page, the help, the IT page)
STILL = "can still read the skill pack and the part library that come with LightSim"
#: what the AI apps' log leaves out (the help, Known issues)
UNLOGGED = "a run sent as a task that LightSim refuses before it starts"
UNREAD = "a call LightSim cannot read, such as one of a tool it does not have"


#: sentences each place must keep saying (after text())
MUST_SAY = {
    PANEL: [
        "An AI app you connected reads these settings " + FRESH + ".",
        "Python code that uses <code>lightsim.ai_access.AgentSession</code> " + STARTS + ".",
        "AI access is off: every tool call from an AI tool is refused, though a connected AI app " + STILL + ".",
        "An AI agent works hands-off: the connection never asks you to confirm.",
        "saved straight away. An example is saved as a new project, and the previous version is kept as a backup",
        "it runs in an OS sandbox with no network and access to the project's folder only",
        "the run is refused with the code <code>sandbox_unavailable</code> rather than run less safely",
        "a brand-new script an assistant wrote fails until you approve it",
        "refuses Script code you have not approved there",
        "Only Python code that uses <code>lightsim.ai_access.AgentSession</code> reads it",
        "The connection for AI apps does not read this list.",
    ],
    HOWTO: [
        "AI agent works hands-off: the connection never asks you to confirm",
        "and run a project, with no question to you",
        "an edit is saved straight away, and is recoverable",
        "runs in an OS sandbox with no network and access to the project's folder only",
        "the run is refused with the code `sandbox_unavailable` rather than run less safely",
        "Code that is not code you approved in LightSim makes the run fail",
        "brand-new script the assistant wrote fails until you approve it",
        UNLOGGED, UNREAD,
        "An AI app you connected reads them " + FRESH + ", and Python code that goes through "
        "`lightsim.ai_access.AgentSession` " + STARTS + ".",
        "Off, every tool call from an AI tool is refused; a connected AI app " + STILL + ".",
        "an assistant already connected can use none of LightSim's tools",
        "**Trusted projects (Python package only)**",
        "Only Python code that uses `AgentSession` reads this list",
        "The connection for AI apps does not read it",
    ],
    COMMAND_LINE: [
        "Two things read these settings",
        "(`lightsim-backend mcp`, the MCP server) " + FRESH + ", and Python code that goes through "
        "`lightsim.ai_access.AgentSession`, when it starts a session",
        "The connection for AI apps does not read the list.",
        "An agent works hands-off: the connection never asks you to confirm",
        "runs a project with Script blocks or FMUs in an OS sandbox",
        "refused with the code `sandbox_unavailable`",
        "code you have not approved there makes the run fail",
        "The last calls made through `AgentSession`",
    ],
    PYTHON_API: [
        "the `lightsim` package does not read the AI access settings",
        "reads the same settings (all but the trust list) before every tool call, and does not use "
        "`AgentSession`",
        "It reads the settings when you create it, so a change takes effect in the next session.",
        "The connection for AI apps works hands-off: it never asks you to confirm",
    ],
    KNOWN_LIMITS: [
        "**AI apps work hands-off; the trust list is not used.**",
        "The connection for AI apps (the MCP server) never asks you to confirm",
        "runs a project with Script blocks or FMUs in an OS sandbox (no network, file access to "
        "the project's folder only)",
        "the run is refused with the code `sandbox_unavailable` rather than run less safely",
        "edit it saves is recoverable (the previous version kept as a backup, an example saved "
        "as a new project)",
        "`lightsim run` and the `lightsim` package read no AI access setting at all",
        "makes the run fail until you choose **Run scripts** there",
        "`lightsim ai log` lists only the calls made through `AgentSession`",
        "That log leaves out " + UNLOGGED + ", and " + UNREAD + ".",
    ],
    README: [
        "it works hands-off (it never asks you to confirm), running a project's Script blocks and "
        "FMUs in an OS sandbox or refusing the run when that sandbox cannot be set up",
        "it does not read the `lightsim ai trust` list",
        "the trust list that only Python code using `AgentSession` reads",
    ],
    RELEASE_NOTES: [
        "the connection for AI apps does not read that list",
        "it works hands-off: it never asks you to confirm",
        "its tool calls go into a local log, `.ai/audit.jsonl` in the projects folder (*Known issues* "
        "lists the calls it leaves out)",
    ],
    DEPLOY: [
        "`off`: AI apps get none of your projects from LightSim. Every tool call from a connected "
        "assistant is refused (it " + STILL + ")",
    ],
    CONNECT: [
        "It works hands-off: it does not ask you to confirm.",
        "It runs a project's Script blocks and FMUs in a sandbox, or refuses the run",
    ],
    SKILL: [
        "The `lightsim ai trust` list plays no part",
        "the run ends *failed* with \"has not been approved to run\"",
        "tell the user to open the project in LightSim and choose **Run scripts**",
        "The connection does not ask the user to confirm a run.",
    ],
    SKILL_LIMITS: [  # only the first sentence of the entry reaches the skill pack
        "**AI apps work hands-off; the trust list is not used.**",
        "The connection for AI apps (the MCP server) never asks you to confirm",
    ],
}


@pytest.mark.parametrize("rel", MUST_SAY)
def test_every_place_says_what_the_connection_does_today(rel):
    page = text(rel)
    missing = [s for s in MUST_SAY[rel] if s not in page]
    assert not missing, f"{rel} no longer says: {missing}"


#: the claim these places used to make: the trust list decides what an AI
#: app may run. It is false (the connection never reads the list).
OLD_CLAIMS = [
    "only once you trust it",
    "enforced by the engine for every ai tool",
    "hold for every ai tool",
    "runs only after you trust it",
    "lets ai tools run this project's script blocks",
    "projects whose script blocks ai tools may run",
    "a project file whose script blocks ai tools may run",
    "a run of a trusted project with script blocks",
    "go through `lightsim.ai_access.agentsession` instead, which keeps",
    "reach projects only through :class:`agentsession`",
    "replaces this with a per-project",
    "untrust a project's script blocks",
    "trusting a project's script blocks (`lightsim ai trust <file>`) and hiding",
    "let ai tools run this project's script blocks (after you confirm)",
    "rules every ai tool works under",
    "the trusted script projects",
]
#: the confirmation the connection no longer asks for, and the old --trust-scripts
#: gate the sandbox replaced; and claims stronger than the code
OVERSTATED = [
    # the MCP connection's removed confirmation claims (the Python AgentSession
    # still confirms, so a generic "needs confirmation" is not forbidden)
    "asks you before it saves",
    "asks you before it runs",
    "lightsim asks you first",
    "a yes sent again",
    "an answer the ai app sends again",
    "sends your answer again",
    "cannot tell who answered",
    "does not show the code",
    "cannot ask you is refused",
    "does not check whether the app can ask",
    "--trust-scripts",
    "input_required",
    "elicitation",
    "asks you each time",
    "before each run",
    "confirm each run",
    "confirm each change",
    "every call is logged",
    "every tool call an ai app makes, allowed or refused",
    "every call from an ai tool is refused",
    "get nothing from lightsim",
]
#: a mention of the trust command must say, near it, who reads the list
WHO_READS = ("AgentSession", "does not read", "is not read", "plays no part", "not used")


def _places():
    """Every file that could repeat the claim: the docs, the help, the README,
    the page and the engine's and the package's own words (not the tests)."""
    roots = [ROOT / "docs", ROOT / "backend" / "app", ROOT / "backend" / "lightsim",
             ROOT / "frontend" / "src", ROOT / "frontend" / "e2e", ROOT / "desktop" / "src"]
    files = [ROOT / "README.md", ROOT / "CONTRIBUTING.md", ROOT / "SECURITY.md"]
    for root in roots:
        for pattern in ("*.md", "*.py", "*.ts", "*.tsx", "*.js"):
            files += [f for f in root.rglob(pattern)
                      if "node_modules" not in f.parts and ".test." not in f.name
                      and f.name != "test_ai_claims.py"]
    return sorted({f for f in files if f.is_file()})


#: a comment marker at the start of a line (``//``, ``#``, ``/**``, `` * ``)
_MARKER = re.compile(r"^[ \t]*(?://+|#+|/\*+|\*/|\*(?=\s))[ \t]?", re.M)


def flat(f: Path) -> str:
    """A file's words as one line: comment markers at line starts gone and
    every run of white space one space, so a sentence is found across the
    lines of a comment too."""
    return _flat(f.read_text(encoding="utf-8"))


def _flat(raw: str) -> str:
    return re.sub(r"\s+", " ", _MARKER.sub("", raw))


def test_the_places_to_check_are_found():
    names = {f.name for f in _places()}
    assert _flat("// the rules every AI tool\n// works under") == "the rules every AI tool works under"
    assert _flat("/** Trust:\n *  only here */") == "Trust: only here */"
    assert {"AiAccessPanel.tsx", "AiConnectDialog.tsx", "use-an-ai-assistant.md", "command-line.md",
            "deploy-for-it.md", "KNOWN-LIMITS.md", "README.md", "ai_access.py", "mcp_server.py",
            "ai01-access.spec.ts", "SKILL.md", "known-limits.md", "tools.py", "policy.js"} <= names


def test_no_place_repeats_the_old_claim():
    found = []
    for f in _places():
        page = flat(f).lower()
        found += [f"{f.relative_to(ROOT)}: {claim}" for claim in OLD_CLAIMS if claim in page]
    assert not found, "these still say the trust list governs AI apps:\n" + "\n".join(found)


def test_no_place_says_more_than_the_connection_does():
    found = []
    for f in _places():
        page = flat(f).lower()
        found += [f"{f.relative_to(ROOT)}: {claim}" for claim in OVERSTATED if claim in page]
    assert not found, "these say more than the AI connection does:\n" + "\n".join(found)


def test_every_mention_of_the_trust_command_says_who_reads_the_list():
    lonely = []
    for f in _places():
        page = flat(f)
        for m in re.finditer(r"lightsim ai trust", page):
            near = page[max(0, m.start() - 500): m.end() + 500]
            if not any(w in near for w in WHO_READS):
                lonely.append(f"{f.relative_to(ROOT)}: …{page[max(0, m.start() - 60): m.end() + 60]}…")
    assert not lonely, "a mention of `lightsim ai trust` that does not say who reads the list:\n" + "\n".join(lonely)
