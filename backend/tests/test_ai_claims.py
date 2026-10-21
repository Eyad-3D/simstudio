"""What the AI access page and the help say about the AI connection is true.

Connect AI → AI access, the help (*Use an AI assistant*, *Command-line tool*,
*Python API*), Known issues, the README and the skill pack all say what an
AI app can do through LightSim's MCP server today:

- it reads, checks and compares, and runs a project without Script blocks,
  with no question;
- it asks the user, through the AI app, before it saves an edit and before
  it runs a project with Script blocks; but the answer the app sends back
  is not single-use: sent again, it repeats a run, or saves another copy of
  an edited example or new project, with no new question;
- an app on the older handshake that cannot ask is refused what needs a
  yes; on the 2026-07-28 protocol the server does not check whether the app
  can ask, so an app that answers itself can save and run;
- on Windows it runs such a project only when the connection was started
  with ``--trust-scripts``;
- it does NOT read the trust list that ``lightsim ai trust`` keeps: only
  Python code that goes through ``lightsim.ai_access.AgentSession`` does,
  and the plain ``lightsim`` package and ``lightsim run`` read no AI access
  setting at all;
- LightSim's own approval of Script code (app/script_trust.py) applies to an
  AI app's run, so code nobody approved in the app fails the run;
- LightSim cannot tell who answered a question, and the question for a run
  does not show the code;
- the AI apps' audit log has a line for each call that reaches a tool, but
  not for a call that brings back the user's no, a run sent as a task that
  is asked about or refused before it starts, or a call LightSim cannot read
  (a tool it does not have, arguments that are not an object);
- the connection reads the settings before every tool call, and
  ``AgentSession`` reads them once, when the session starts;
- while AI access is off (the user's switch or the organisation's policy)
  every tool call is refused, but an AI app can still read the skill pack
  and the part library that come with LightSim, and the question for a
  yes does not promise that it counts once.

The behaviour is held in test_mcp_server.py (``test_reading_asks_the_user_
nothing``, ``test_every_script_run_asks_and_the_trust_list_is_not_read``,
``test_a_yes_sent_again_repeats_a_run_or_saves_another_copy_without_a_new_question``,
``test_whether_an_app_can_ask_is_checked_only_on_the_older_handshake``,
``test_an_app_that_cannot_ask_gets_a_refusal_for_a_script_run``,
``test_script_code_nobody_approved_fails_the_run_even_after_a_yes``,
``test_what_the_audit_log_leaves_out``,
``test_every_tool_call_is_refused_while_ai_access_is_off``, and the Windows
rule in ``test_script_projects_run_only_after_a_yes``) and in
test_ai_access.py (what ``AgentSession`` does with the list). This file holds
the rest of the facts about the Python side, and the words: the page, the
help and the other places that repeat the claim must say it, must not
say the old claim again (that the trust list governs AI apps), and must not
say more than the code does (that LightSim asks before *each* run, that
nothing is saved or run for an app that cannot ask, that every call is
logged, that an AI app gets nothing while access is off).

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
    # the page and the help: the connection for AI apps reads the settings
    # before every tool call (test_mcp_server holds that), AgentSession when
    # it starts a session
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
    assert "says yes to LightSim's question" in description
    assert "sent again it repeats the run without asking the user" in description
    assert "has not been approved" in description
    edit = next(t for t in TOOLS if t["name"] == "model_edit")["description"]
    assert "a yes sent again saves another copy" in edit


# -- the words -------------------------------------------------------------------


def text(rel: str) -> str:
    """A file's text with every run of white space as one space and the JSX
    ``{" "}`` gone, so a sentence is found wherever the line breaks fall."""
    raw = (ROOT / rel).read_text(encoding="utf-8").replace('{" "}', " ")
    return re.sub(r"\s+", " ", raw)


#: what an answer sent again does (the page, the help, Known issues)
AGAIN = ("app that sends your answer again repeats a run, or saves another copy of an edited "
         "example or new project, without asking you")
#: what an app that cannot ask can do (the page, the help, Known issues)
OLDER = "that cannot ask you is refused anything that needs your yes"
NEWER = ("LightSim does not check whether the app can ask, so an app that answers for you can "
         "save edits and run Script projects")
#: what the AI apps' log leaves out (the help, Known issues)
UNLOGGED = "a call that brings back your no"
UNREAD = "a call LightSim cannot read, such as one of a tool it does not have"
#: when the settings are read (the page, the help)
FRESH = "before every tool call"
STARTS = "reads them when it starts a session"
#: what an AI app still gets while access is off (the page, the help, the IT page)
STILL = "can still read the skill pack and the part library that come with LightSim"
#: what an answer sent again does, in short (the README, Connect AI, the release notes)
SHORT = "an answer the AI app sends again is not asked about again"

#: sentences each place must keep saying (after text())
MUST_SAY = {
    PANEL: [
        "An AI app you connected reads these settings " + FRESH + ".",
        "Python code that uses <code>lightsim.ai_access.AgentSession</code> " + STARTS + ".",
        "AI access is off: every tool call from an AI tool is refused, though a connected AI app " + STILL + ".",
        "<code>--trust-scripts</code>",
        "running a project without Script blocks: no question.",
        "Saving an edit: LightSim asks you first.",
        "LightSim asks you first, and the question does not show the code.",
        "LightSim cannot tell who answered.",
        AGAIN, OLDER, NEWER,
        "Connect only an AI app that shows you each question.",
        "Trusted projects (Python package only)",
        "Only Python code that uses <code>lightsim.ai_access.AgentSession</code> reads it",
        "The connection for AI apps does not read this list.",
        "refuses Script code you have not approved there",
    ],
    HOWTO: [
        "and run a project without Script blocks, with no question to you",
        "to save one, LightSim asks you first",
        "makes LightSim ask you before it runs, in the AI app",
        "it does not show the code",
        "does not run at all unless you added `--trust-scripts` to the connection",
        "Code that is not code you approved in LightSim makes the run fail",
        "LightSim cannot tell whether you or the assistant answered",
        AGAIN, OLDER, NEWER, UNLOGGED, UNREAD,
        "that LightSim asks about or refuses before it starts",
        "(not the calls the log leaves out, above)",
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
        "makes LightSim ask you before it runs, in the AI app",
        "The trust list above plays no part.",
        "code you have not approved there makes the run fail",
        "only after you say yes to the change in the AI app",
        "LightSim cannot tell who answered, and an " + AGAIN,
        OLDER, NEWER, UNLOGGED,
        "that LightSim asks about or refuses before it starts, and " + UNREAD,
        "The last calls made through `AgentSession`",
        "It runs a project with Script blocks only if you trusted the project",
    ],
    PYTHON_API: [
        "the `lightsim` package does not read the AI access settings",
        "reads the same settings (all but the trust list) before every tool call, and does not use "
        "`AgentSession`",
        "It reads the settings when you create it, so a change takes effect in the next session.",
        AGAIN,
    ],
    KNOWN_LIMITS: [
        "**AI apps pass on LightSim's questions; the trust list is not used.**",
        "does not read the trust list (`lightsim ai trust`), and its questions before an edit or a run "
        "of a project with Script blocks go through the AI app, which can answer for you or send your "
        "answer again.",
        AGAIN, "(an edit of a saved project asks again)", OLDER, NEWER,
        "(for the whole connection, not per project), and then it still asks",
        "`lightsim run` and the `lightsim` package read no AI access setting at all",
        "LightSim cannot tell who answered a question",
        "That log leaves out " + UNLOGGED + ", a run sent as a task that LightSim asks about or "
        "refuses before it starts, and " + UNREAD + ".",
        "not the script's code",
        "makes the run fail until you choose **Run scripts** there",
        "`lightsim ai log` lists only the calls made through `AgentSession`",
    ],
    README: [
        "before it saves an edit and before it runs a project with Script blocks (it cannot tell who "
        "answered, and " + SHORT + ": see [Known issues](docs/KNOWN-LIMITS.md)), and it does not read the "
        "`lightsim ai trust` list",
        "the trust list that only Python code using `AgentSession` reads",
    ],
    RELEASE_NOTES: [
        "the connection for AI apps does not read that list",
        "LightSim cannot tell who answered, and " + SHORT,
        "its tool calls go into a local log, `.ai/audit.jsonl` in the projects folder (*Known issues* "
        "lists the calls it leaves out)",
    ],
    DEPLOY: [
        "`off`: AI apps get none of your projects from LightSim. Every tool call from a connected "
        "assistant is refused (it " + STILL + ")",
    ],
    CONNECT: [
        "It asks you, in the AI app, before it saves a change or runs a project that contains Script blocks.",
        "LightSim cannot tell who answered, and " + SHORT + ": see <b>AI access</b>.",
    ],
    SKILL: [
        "The `lightsim ai trust` list plays no part.",
        "the run ends *failed* with \"has not been approved to run\"",
        "tell the user to open the project in LightSim and choose **Run scripts**",
        "an answer sent again repeats the run, or saves another copy of an edited example or new "
        "project, without asking the user",
        "Only the user answers LightSim's questions.",
    ],
    SKILL_LIMITS: [  # only the first sentence of the entry reaches the skill pack
        "**AI apps pass on LightSim's questions; the trust list is not used.**",
        "does not read the trust list (`lightsim ai trust`), and its questions before an edit or a run "
        "of a project with Script blocks go through the AI app, which can answer for you or send your "
        "answer again.",
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
#: what the code does not do: ask before *each* run whatever the app sends,
#: refuse every app that cannot ask, log every call
OVERSTATED = [
    "asks you each time",
    "script blocks, each time",
    "before each run",
    "confirms each run",
    "confirm each run",
    "confirm each change",
    "nothing is saved or run",
    "cannot ask you cannot save",
    "every tool call an ai app makes, allowed or refused",
    "every request is written to a log",
    "every tool call goes into a local audit log",
    "every call is logged",
    "every request goes into",
    "before every request",
    "every request from",
    "every call from an ai tool is refused",
    "get nothing from lightsim",
    "already connected gets nothing",
    "do this once",
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
