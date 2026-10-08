#!/usr/bin/env python3
"""Build LightSim's MCP Bundle (AI-29): ``LightSim-<version>.mcpb``.

An .mcpb is a zip with a ``manifest.json``; AI apps such as Claude Desktop
install it with a double-click. This one carries no engine: its entry
point (``server/launcher.js``) runs the LightSim installed on the computer,
or says where to download it. It also carries the skill pack (AI-08).

    python scripts/mcp/build-mcpb.py [--out DIR]

The tool list in the manifest is read from the engine's own tool
definitions, so the two cannot drift.
"""
from __future__ import annotations

import argparse
import json
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "backend"))

from app.ai.skillpack import SKILLS_DIR  # noqa: E402
from app.ai.tools import TOOLS  # noqa: E402


def manifest(version: str) -> dict:
    return {
        "manifest_version": "0.3",
        "name": "lightsim",
        "display_name": "LightSim",
        "version": version,
        "description": "Open, check and run LightSim vehicle simulation models from your AI assistant.",
        "long_description": (
            "Lets your AI assistant list your LightSim projects, outline a model, run its Data "
            "Checks and simulations, and read back the key results. It runs the LightSim installed "
            "on this computer, with no network connection, and asks you before it saves a change "
            "or runs a project that contains Script blocks. LightSim's results are not validated "
            "against measured vehicles."
        ),
        "author": {"name": "Eyad Abualkhair", "url": "https://github.com/Eyad-3D/simstudio"},
        "homepage": "https://github.com/Eyad-3D/simstudio",
        "license": "LicenseRef-LightSim-EULA",
        "keywords": ["simulation", "vehicle", "electric", "engineering", "lightsim"],
        "server": {
            "type": "node",
            "entry_point": "server/launcher.js",
            "mcp_config": {
                "command": "node",
                "args": ["${__dirname}/server/launcher.js", "${user_config.allowed_folders}"],
            },
        },
        "tools": [{"name": t["name"], "description": t["description"]} for t in TOOLS],
        "user_config": {
            "allowed_folders": {
                "type": "directory",
                "title": "Extra project folders",
                "description": "Folders of LightSim project files the assistant may also open "
                               "(your saved projects and the examples are always available).",
                "multiple": True,
                "required": False,
                "default": [],
            }
        },
        "compatibility": {"platforms": ["win32", "linux", "darwin"]},
    }


def build(out_dir: Path) -> Path:
    version = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
    out_dir.mkdir(parents=True, exist_ok=True)
    target = out_dir / f"LightSim-{version}.mcpb"
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("manifest.json", json.dumps(manifest(version), indent=2) + "\n")
        z.write(Path(__file__).with_name("launcher.js"), "server/launcher.js")
        for f in sorted(SKILLS_DIR.rglob("*")):
            if f.is_file():
                z.write(f, "skills/" + f.relative_to(SKILLS_DIR).as_posix())
    return target


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default=str(ROOT / "dist"), help="where to write the .mcpb")
    print(build(Path(parser.parse_args().out)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
