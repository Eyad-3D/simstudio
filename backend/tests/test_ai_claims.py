"""What the AI access page and the help say about the AI connection is true.

Connect AI → AI access, the help (*Use an AI assistant*, *Command-line tool*,
*Python API*), Known issues, the README and the skill pack all say what an
AI app can do through LightSim's MCP server today:

- it reads, checks and compares with no question;
- it asks the user, through the AI app, before it saves an edit and before
  it runs a project with Script blocks, each time;
- on Windows it runs such a project only when the connection was started
  with ``--trust-scripts``;
- it does NOT read the trust list that ``lightsim ai trust`` keeps: only
  Python code that goes through ``lightsim.ai_access.AgentSession`` does,
  and the plain ``lightsim`` package and ``lightsim run`` read no AI access
  setting at all;
- LightSim's own approval of Script code (app/script_trust.py) applies to an
  AI app's run, so code nobody approved in the app fails the run;
- LightSim cannot tell who answered a question, and the question for a run
  does not show the code.

The behaviour is held in test_mcp_server.py (``test_reading_asks_the_user_
nothing``, ``test_every_script_run_asks_and_the_trust_list_is_not_read``,
``test_an_app_that_cannot_ask_gets_a_refusal_for_a_script_run``,
``test_script_code_nobody_approved_fails_the_run_even_after_a_yes``, and the
Windows rule in ``test_script_projects_run_only_after_a_yes``) and in
test_ai_access.py (what ``AgentSession`` does with the list). This file holds
the rest of the facts about the Python side, and the words: the page, the
help and the other places that repeat the claim must say it, and must not
say the old claim again (that the trust list governs AI apps).

When the behaviour changes, change these tests with the page and the help.
"""
from __future__ import annotations

import json
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


def test_the_run_case_tool_tells_an_agent_what_it_will_meet():
    description = next(t for t in TOOLS if t["name"] == "run_case")["description"]
    assert "confirms each run" in description
    assert "has not been approved" in description


# -- the words -------------------------------------------------------------------


def text(rel: str) -> str:
    """A file's text with every run of white space as one space and the JSX
    ``{" "}`` gone, so a sentence is found wherever the line breaks fall."""
    raw = (ROOT / rel).read_text(encoding="utf-8").replace('{" "}', " ")
    return re.sub(r"\s+", " ", raw)


#: sentences each place must keep saying (after text())
MUST_SAY = {
    PANEL: [
        "<code>--trust-scripts</code>",
        "Saving an edit: LightSim asks you each time.",
        "LightSim asks you before each run, and the question does not show the code.",
        "LightSim cannot tell who answered.",
        "Trusted projects (Python package only)",
        "Only Python code that uses <code>lightsim.ai_access.AgentSession</code> reads it",
        "The connection for AI apps does not read this list.",
        "refuses Script code you have not approved there",
    ],
    HOWTO: [
        "LightSim asks you each time",
        "makes LightSim ask you before each run, in the AI app",
        "it does not show the code",
        "does not run at all unless you added `--trust-scripts` to the connection",
        "Code that is not code you approved in LightSim makes the run fail",
        "LightSim cannot tell whether you or the assistant answered",
        "an app that sends your answer again repeats a run, or saves another copy of an edited example, without asking you",
        "If an AI app cannot ask you, nothing is saved or run",
        "**Trusted projects (Python package only)**",
        "Only Python code that uses `AgentSession` reads this list",
        "The connection for AI apps does not read it",
    ],
    COMMAND_LINE: [
        "Two things read these settings",
        "The connection for AI apps does not read the list.",
        "makes LightSim ask you before each run, in the AI app",
        "The trust list above plays no part.",
        "code you have not approved there makes the run fail",
        "LightSim cannot tell who answered. If an AI app cannot ask you, nothing is saved or run.",
        "The last calls made through `AgentSession`",
        "It runs a project with Script blocks only if you trusted the project",
    ],
    PYTHON_API: [
        "the `lightsim` package does not read the AI access settings",
        "reads the same settings but does not use `AgentSession`",
    ],
    KNOWN_LIMITS: [
        "**AI apps are asked first; the trust list is not used.**",
        "and it does not read the trust list (`lightsim ai trust`)",
        "(for the whole connection, not per project), and then it still asks",
        "`lightsim run` and the `lightsim` package read no AI access setting at all",
        "LightSim cannot tell who answered a question",
        "an app that sends the answer to a run's question again repeats that run, and one that sends the answer to an edit of an example again saves another copy, without a new question",
        "not the script's code",
        "makes the run fail until you choose **Run scripts** there",
        "`lightsim ai log` lists only the calls made through `AgentSession`",
    ],
    README: [
        "before it saves an edit and before it runs a project with Script blocks, and it does not read the `lightsim ai trust` list",
        "the trust list that only Python code using `AgentSession` reads",
    ],
    RELEASE_NOTES: [
        "the connection for AI apps does not read that list",
    ],
    SKILL: [
        "The `lightsim ai trust` list plays no part.",
        "the run ends *failed* with \"has not been approved to run\"",
        "tell the user to open the project in LightSim and choose **Run scripts**",
    ],
    SKILL_LIMITS: [
        "**AI apps are asked first; the trust list is not used.**",
        "it does not read the trust list (`lightsim ai trust`)",
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
]
#: a mention of the trust command must say, near it, who reads the list
WHO_READS = ("AgentSession", "does not read", "is not read", "plays no part", "not used")


def _places():
    """Every file that could repeat the claim: the docs, the help, the README,
    the page and the engine's and the package's own words (not the tests)."""
    roots = [ROOT / "docs", ROOT / "backend" / "app", ROOT / "backend" / "lightsim",
             ROOT / "frontend" / "src", ROOT / "desktop" / "src"]
    files = [ROOT / "README.md", ROOT / "CONTRIBUTING.md", ROOT / "SECURITY.md"]
    for root in roots:
        for pattern in ("*.md", "*.py", "*.ts", "*.tsx", "*.js"):
            files += [f for f in root.rglob(pattern)
                      if "node_modules" not in f.parts and ".test." not in f.name
                      and f.name != "test_ai_claims.py"]
    return sorted({f for f in files if f.is_file()})


def test_the_places_to_check_are_found():
    names = {f.name for f in _places()}
    assert {"AiAccessPanel.tsx", "use-an-ai-assistant.md", "command-line.md", "KNOWN-LIMITS.md",
            "README.md", "ai_access.py", "mcp_server.py"} <= names


def test_no_place_repeats_the_old_claim():
    found = []
    for f in _places():
        page = re.sub(r"\s+", " ", f.read_text(encoding="utf-8")).lower()
        found += [f"{f.relative_to(ROOT)}: {claim}" for claim in OLD_CLAIMS if claim in page]
    assert not found, "these still say the trust list governs AI apps:\n" + "\n".join(found)


def test_every_mention_of_the_trust_command_says_who_reads_the_list():
    lonely = []
    for f in _places():
        page = re.sub(r"\s+", " ", f.read_text(encoding="utf-8"))
        for m in re.finditer(r"lightsim ai trust", page):
            near = page[max(0, m.start() - 500): m.end() + 500]
            if not any(w in near for w in WHO_READS):
                lonely.append(f"{f.relative_to(ROOT)}: …{page[max(0, m.start() - 60): m.end() + 60]}…")
    assert not lonely, "a mention of `lightsim ai trust` that does not say who reads the list:\n" + "\n".join(lonely)
