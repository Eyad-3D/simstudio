"""The tool problems the baseline found, fixed (step 6, benchmarks/agent/
BASELINE.md "What agents got wrong" and "What this means for step 6"):

- ``model_edit``'s description says each operation has an ``op`` key, and its
  input schema lists the operations and their fields;
- an edit error names the right key (an operation without ``op`` says which
  keys it does have);
- ``compare_runs`` says plainly that it cannot compare runs of two different
  projects;
- the tool names an agent sees are consistent between the server's
  instructions, the skill pack and the tools themselves.

Each check fails on the code before the fix and passes after it.
"""
from __future__ import annotations

import json
import re
from pathlib import Path

import pytest
from conftest import allow_ai

from app.ai import skillpack
from app.ai.access import AuditLog, Policy
from app.ai.edits import EditError, apply_operations
from app.ai.engine import Engine
from app.ai.mcp_server import INSTRUCTIONS, McpServer
from app.ai.tools import TOOL_NAMES, TOOLS
from app.storage import load_example

BACKEND = Path(__file__).resolve().parent.parent
MODERN = "2026-07-28"
META = {"io.modelcontextprotocol/protocolVersion": MODERN}


def _tool(name: str) -> dict:
    return next(t for t in TOOLS if t["name"] == name)


# -- model_edit: the op key, in the description and the schema -------------------


def test_model_edit_description_says_each_operation_has_an_op_key():
    desc = _tool("model_edit")["description"]
    assert '"op" key' in desc
    assert '{"op": "set"' in desc, "an example with the op key"


def test_model_edit_schema_lists_the_operations_and_their_fields():
    ops = _tool("model_edit")["inputSchema"]["properties"]["operations"]
    variants = ops["items"]["oneOf"]
    assert ops["items"]["required"] == ["op"]
    by_op = {v["properties"]["op"]["const"]: v for v in variants}
    assert set(by_op) == {"set", "add", "remove", "connect", "disconnect",
                          "add_port", "add_case", "set_case"}
    # each names the fields that kind needs
    assert by_op["set"]["required"] == ["op", "element", "param", "value"]
    assert "param" in by_op["set"]["properties"]
    assert by_op["add"]["required"] == ["op", "type"]
    assert set(by_op["connect"]["required"]) == {"op", "from", "to"}
    assert by_op["add_port"]["properties"]["direction"]["enum"] == ["input", "output"]
    assert by_op["set_case"]["properties"]["kind"]["enum"] == \
        ["cycle", "performance", "acceleration", "lap"]


# -- an edit error names the key, not a bare "None" ------------------------------


def test_an_operation_without_an_op_key_says_so_and_names_its_keys():
    bev = load_example("bev-car")
    # the baseline: an agent put the kind in as a key (set/add/type), not as "op"
    with pytest.raises(EditError) as e:
        apply_operations(bev, [{"set": "Vehicle", "param": "mass_kg", "value": 1927}])
    text = str(e.value)
    assert '"op"' in text and "no \"op\" key" in text
    assert "'set'" in text and "'param'" in text, "it names the keys the operation does have"
    assert "(None)" not in text, "the operation header does not read '(None)'"
    assert "set, add, remove" in text, "and lists the kinds"


def test_an_operation_with_an_unknown_op_names_the_op():
    bev = load_example("bev-car")
    with pytest.raises(EditError, match=r"unknown op 'frobnicate'"):
        apply_operations(bev, [{"op": "frobnicate", "element": "Vehicle"}])


# -- a 'no part' error names the parts, closest first (BASELINE item 5) ----------


def test_no_part_error_lists_the_parts_by_id_and_label_and_type():
    # the item agents recovered from least: "No part 'Battery'" named no part.
    # The real part is el-battery, "HV Battery Pack" (battery.generic).
    bev = load_example("bev-car")
    with pytest.raises(EditError) as e:
        apply_operations(bev, [{"op": "set", "element": "Battery", "param": "capacity_kwh", "value": 50}])
    text = str(e.value)
    assert 'el-battery ("HV Battery Pack", battery.generic)' in text
    # el-battery is the closest to "Battery", so it is listed first
    listed = text.split("closest first):", 1)[1].strip()
    assert listed.startswith('el-battery ("HV Battery Pack", battery.generic)'), listed


def test_no_part_error_says_when_the_name_is_a_case_or_a_part_type():
    bev = load_example("bev-car")
    with pytest.raises(EditError, match=r"'case-city' is a case, not a part"):
        apply_operations(bev, [{"op": "set", "element": "case-city", "param": "x", "value": 1}])
    with pytest.raises(EditError, match=r"'battery.generic' is a part type, not a part"):
        apply_operations(bev, [{"op": "set", "element": "battery.generic", "param": "x", "value": 1}])


def test_element_read_no_part_names_the_parts(tmp_path, monkeypatch):
    # the same help over the MCP server's element_read, by a label that is not
    # a part (BASELINE: element_read 'Battery' named no part)
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path / "projects"))
    allow_ai(tmp_path / "projects")
    server = _server(tmp_path / "projects")
    answer = _call(server, "element_read", {"project": "example:bev-car", "element": "Battery"})
    assert answer["isError"]
    text = answer["content"][0]["text"]
    assert 'el-battery ("HV Battery Pack", battery.generic)' in text
    # a type id passed as a part is named as such
    typed = _call(server, "element_read", {"project": "example:bev-car", "element": "battery.generic"})
    assert typed["isError"] and "is a part type, not a part" in typed["content"][0]["text"]


def test_a_non_object_operation_says_what_an_operation_is():
    bev = load_example("bev-car")
    with pytest.raises(EditError, match=r"an operation is an object with an \"op\" key"):
        apply_operations(bev, ["set Vehicle mass 1900"])


# -- compare_runs: runs of two projects cannot be compared -----------------------


def test_compare_runs_description_says_it_needs_one_project():
    desc = _tool("compare_runs")["description"]
    assert "cannot compare runs of two different" in desc
    assert "two cases of one project" in desc


def _server(folder: Path, allowed=()) -> McpServer:
    return McpServer(Engine(user_folder=folder, allowed_folders=tuple(allowed)),
                     Policy(), AuditLog(folder))


def _call(server: McpServer, name: str, args: dict) -> dict:
    reply = server.handle({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                           "params": {"name": name, "arguments": args, "_meta": META}})
    assert "error" not in reply, reply
    return reply["result"]


def _project_file(path: Path, name: str) -> Path:
    bev = json.loads((BACKEND / "projects" / "bev-car.json").read_text(encoding="utf-8"))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({**bev, "id": path.stem, "name": name}, indent=2), encoding="utf-8")
    return path


def test_compare_runs_on_a_run_of_another_project_says_so(tmp_path, monkeypatch):
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path / "projects"))
    a = _project_file(tmp_path / "a" / "car.json", "Car A")
    b = _project_file(tmp_path / "b" / "car.json", "Car B")
    allow_ai(tmp_path / "projects", a.parent, b.parent)
    server = _server(tmp_path / "projects", allowed=(a.parent, b.parent))
    ref_a, ref_b = str(a.resolve()), str(b.resolve())
    run_a = _call(server, "run_case", {"project": ref_a, "case": "case-city"})["structuredContent"]["run"]
    run_b = _call(server, "run_case", {"project": ref_b, "case": "case-city"})["structuredContent"]["run"]
    # the agent ran two project files and tries to compare the two run ids
    answer = _call(server, "compare_runs", {"project": ref_a, "run_a": run_a, "run_b": run_b})
    assert answer["isError"]
    text = answer["content"][0]["text"]
    assert "cannot compare runs of two different project files" in text
    assert "two cases of one project" in text
    # the baseline bug: a run of the same project still compares fine
    run_a2 = _call(server, "run_case", {"project": ref_a, "case": "case-wltc"})["structuredContent"]["run"]
    ok = _call(server, "compare_runs", {"project": ref_a, "run_a": run_a, "run_b": run_a2})
    assert not ok["isError"]


# -- the tool names are consistent across what an agent reads --------------------

#: wrong spellings of a tool name that would be a bug if an agent met them
_NEAR_MISSES = [
    "list_projects", "lightsim_run_case", "lightsim_run_checks", "lightsim_results_query",
    "lightsim_compare_runs", "lightsim_element_read", "lightsim_model_edit",
    "lightsim_explain_message", "run-case", "model-edit", "edit_model", "read_element",
    "query_results", "check_run",
]


def _agent_facing_text() -> dict[str, str]:
    """Everything an agent reads that could name a tool: the server's
    instructions, every tool's title/description/next hints, and the skill
    pack."""
    texts = {"INSTRUCTIONS": INSTRUCTIONS}
    for t in TOOLS:
        texts[f"tool:{t['name']}"] = t["title"] + " " + t["description"]
    for f in skillpack.skill_files():
        texts[str(f.relative_to(skillpack.SKILLS_DIR))] = f.read_text(encoding="utf-8")
    return texts


def test_every_tool_name_an_agent_reads_is_a_real_tool():
    # the baseline item 4: the names in the instructions, the skills and the
    # tools must be the real tool names, not a near-miss an agent would then
    # call and be told "no such tool"
    found = []
    for where, text in _agent_facing_text().items():
        for miss in _NEAR_MISSES:
            if re.search(rf"\b{re.escape(miss)}\b", text):
                found.append(f"{where}: {miss}")
    assert not found, "a near-miss tool name an agent reads:\n" + "\n".join(found)


def test_each_tool_is_named_in_the_instructions_or_the_skill_pack():
    blob = "\n".join(_agent_facing_text().values())
    missing = [n for n in TOOL_NAMES if n not in blob]
    assert not missing, f"tools no agent-facing text names: {missing}"


def test_the_instructions_tell_the_agent_the_names_may_be_prefixed():
    # so an agent does not call a bare name the way the baseline's did and get
    # "no such tool"
    assert "mcp__lightsim__" in INSTRUCTIONS
    assert "lightsim_list_projects" in INSTRUCTIONS and "run_case" in INSTRUCTIONS
