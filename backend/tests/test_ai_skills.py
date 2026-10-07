"""The skill pack for AI assistants (AI-08): valid Agent Skills, generated
references up to date, every name it uses exists, and its build recipe
really builds a car that drives."""
from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

from app.ai import skillpack
from app.ai.access import AuditLog, Policy
from app.ai.engine import Engine
from app.ai.tools import TOOLS, CallContext, ConfirmationNeeded, Tools
from app.library import load_library

NAME = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
SKILL_FILES = sorted(skillpack.SKILLS_DIR.glob("*/SKILL.md"))


def test_the_pack_has_the_planned_skills():
    names = {f.parent.name for f in SKILL_FILES}
    assert names == {
        "lightsim-build-a-bev", "lightsim-build-a-p2-hybrid", "lightsim-wiring-rules",
        "lightsim-units-and-parameters", "lightsim-write-a-script-block",
        "lightsim-verify-a-model", "lightsim-read-not-valid-flags",
        "lightsim-what-it-cannot-do", "lightsim-explain-a-result",
    }


@pytest.mark.parametrize("path", SKILL_FILES, ids=lambda p: p.parent.name)
def test_each_skill_follows_the_agent_skills_format(path: Path):
    text = path.read_text(encoding="utf-8")
    meta = skillpack.front_matter(text)
    assert meta["name"] == path.parent.name
    assert NAME.match(meta["name"]) and len(meta["name"]) <= 64
    assert 40 <= len(meta["description"]) <= 1024
    assert "Use " in meta["description"], "the description says when to use it"
    assert f'version: "{skillpack.PACK_VERSION}"' in text
    assert len(text.splitlines()) < 500, "SKILL.md stays short; detail goes in references/"


def test_generated_references_are_up_to_date():
    stale = [p.name for p, text in skillpack.generated().items()
             if not p.exists() or p.read_text(encoding="utf-8") != text]
    assert not stale, "run `python -m app.ai.skillpack --write` in backend/"


def test_every_name_the_skills_use_exists():
    lib = load_library()
    types = {c.id for c in lib}
    keys = {p.key for c in lib for p in c.parameters}
    ports = {p.id for c in lib for p in c.ports}
    tools = {t["name"] for t in TOOLS}
    words = {"add_port", "add_case", "set_case", "dry_run", "t_from", "t_to", "full_tables",
             "hide_values", "motor_rpm", "engine_rpm", "motor_cmd", "engine_on", "clutch_cmd",
             "references/components.md", "references/known-limits.md", "wltc-3b"}
    for path in SKILL_FILES:
        for token in re.findall(r"`([^`\s]+)`", path.read_text(encoding="utf-8")):
            if re.fullmatch(r"[a-z]+\.[a-z0-9_]+", token):
                assert token in types, f"{path.parent.name}: no library part {token}"
            elif re.fullmatch(r"[a-z][a-z0-9]*(_[a-z0-9A-Z]+)+", token):
                assert token in keys | ports | tools | words, f"{path.parent.name}: unknown {token}"


def test_the_bev_recipe_builds_a_car_that_drives(tmp_path, monkeypatch):
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path))
    text = (skillpack.SKILLS_DIR / "lightsim-build-a-bev" / "SKILL.md").read_text(encoding="utf-8")
    ops = json.loads(re.search(r"```json\n(.*?)```", text, re.S).group(1))
    engine = Engine(user_folder=tmp_path)
    tools = Tools(engine, Policy(), AuditLog(tmp_path))
    args = {"project": "new:My EV", "operations": ops, "dry_run": False}
    dry = tools.call("model_edit", {**args, "dry_run": True}, CallContext())
    assert dry["checksAfter"]["error"] == 0 and dry["checksAfter"]["warning"] == 0
    with pytest.raises(ConfirmationNeeded) as need:
        tools.call("model_edit", args, CallContext())
    saved = tools.call("model_edit", args, CallContext(confirmed=need.value.seal))
    run = tools.call("run_case", {"project": saved["project"], "case": "Case 1"}, CallContext())
    rows = {r["key"]: r["value"] for r in run["summary"]}
    assert run["status"] == "success"
    assert rows["distance_driven"] == pytest.approx(7.292, abs=0.001)
    assert rows["consumption"] == pytest.approx(11.0, abs=0.1), "the number the skill quotes"


def test_mcp_resources_serve_every_file_of_the_pack(tmp_path, monkeypatch):
    from app.ai.mcp_server import McpServer

    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path))
    server = McpServer(Engine(user_folder=tmp_path))
    listed = server.handle({"jsonrpc": "2.0", "id": 1, "method": "resources/list"})["result"]
    uris = {r["uri"] for r in listed["resources"]}
    for f in skillpack.skill_files():
        rel = f.relative_to(skillpack.SKILLS_DIR).as_posix()
        if rel != "README.md":
            assert f"lightsim://skills/{rel}" in uris
    read = server.handle({"jsonrpc": "2.0", "id": 2, "method": "resources/read", "params": {
        "uri": "lightsim://skills/lightsim-wiring-rules/SKILL.md"}})["result"]
    assert "Target Speed" in read["contents"][0]["text"]
    assert skillpack.known_limit_sections("Signal units SOC Script percent")
