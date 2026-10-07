"""The MCP Bundle (.mcpb) for one-click installs (AI-29): its manifest lists
the engine's tools, it carries the skill pack, and its launcher runs the
installed LightSim or says where to get it."""
from __future__ import annotations

import importlib.util
import json
import os
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

import pytest

from app.ai.tools import TOOLS

ROOT = Path(__file__).resolve().parents[2]
LAUNCHER = ROOT / "scripts" / "mcp" / "launcher.js"
needs_node = pytest.mark.skipif(shutil.which("node") is None, reason="needs Node.js")


def _build(tmp_path: Path) -> Path:
    spec = importlib.util.spec_from_file_location("build_mcpb", ROOT / "scripts" / "mcp" / "build-mcpb.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.build(tmp_path)


def test_bundle_manifest_and_contents(tmp_path):
    bundle = _build(tmp_path)
    with zipfile.ZipFile(bundle) as z:
        names = z.namelist()
        manifest = json.loads(z.read("manifest.json"))
    assert "server/launcher.js" in names
    assert "skills/lightsim-build-a-bev/SKILL.md" in names
    assert manifest["name"] == "lightsim" and manifest["server"]["entry_point"] == "server/launcher.js"
    assert [t["name"] for t in manifest["tools"]] == [t["name"] for t in TOOLS]
    assert manifest["user_config"]["allowed_folders"]["type"] == "directory"
    assert manifest["version"] == (ROOT / "VERSION").read_text().strip()


def _talk(env: dict, lines: list[dict]) -> list[dict]:
    proc = subprocess.run(["node", str(LAUNCHER)], input="".join(json.dumps(m) + "\n" for m in lines),
                          capture_output=True, text=True, timeout=120, env=env)
    return [json.loads(x) for x in proc.stdout.splitlines() if x.strip()]


@needs_node
def test_launcher_without_lightsim_says_where_to_get_it():
    env = {k: v for k, v in os.environ.items() if k != "LIGHTSIM_ENGINE"}
    env["LIGHTSIM_ENGINE"] = "/nonexistent/lightsim-backend"
    if Path("/opt/LightSim/resources/backend/lightsim-backend").exists():
        pytest.skip("LightSim is installed here")
    out = _talk(env, [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "lightsim_setup"}}])
    assert out[0]["result"]["serverInfo"]["name"] == "lightsim"
    assert "releases/latest" in out[1]["result"]["content"][0]["text"]


@needs_node
@pytest.mark.skipif(sys.platform == "win32", reason="uses a shell script as the engine")
def test_launcher_runs_the_installed_engine(tmp_path):
    engine = tmp_path / "lightsim-backend"
    engine.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{ROOT / "backend" / "run_backend.py"}" "$@"\n')
    engine.chmod(0o755)
    env = {**os.environ, "LIGHTSIM_ENGINE": str(engine), "LIGHTSIM_PROJECTS_DIR": str(tmp_path / "p")}
    out = _talk(env, [{"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {
        "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}}}])
    assert out[0]["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"] == "lightsim"
