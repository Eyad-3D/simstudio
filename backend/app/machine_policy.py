"""The machine-wide policy file IT can put on a PC (PLT-36), as the engine
reads it.

The desktop app reads the file (desktop/src/policy.js) and gives the engine
it starts what the file fixes, as LIGHTSIM_POLICY. An engine an AI app starts
(``lightsim-backend mcp``) has no desktop app around it, so it reads the file
itself: a packaged engine always does, and never takes LIGHTSIM_POLICY from
its environment, which the user or the AI app sets. A source checkout or the
Python package takes LIGHTSIM_POLICY when it is set (the desktop app in
development, the tests) and the file otherwise.

The file's keys and their checks are policy.js's; a key with a value
LightSim does not know fixes nothing (the desktop app logs it).
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Optional

ENV = "LIGHTSIM_POLICY"

# each key's allowed values, or the kind of value it takes (policy.js KEYS)
KEYS: dict[str, object] = {
    "updates": ("off", "notify", "auto"),
    "ai": ("off", "mcp-only", "allowed"),
    "aiProviders": "string-list",
    "licenceFile": "string",
    "projectsRoots": "string-list",
    "scriptTrust": ("prompt", "always-prompt"),
    "examples": "boolean",
}

AI_OFF = ("Your organisation's policy turns AI access to LightSim off on this computer "
          "(the \"ai\" key of its policy file).")


def _windows_drive() -> Optional[str]:
    """The drive Windows is installed on, from Windows itself, not from
    %SystemDrive% or %ProgramData%, which a user can set for their account."""
    try:
        import ctypes

        buf = ctypes.create_unicode_buffer(260)
        n = ctypes.windll.kernel32.GetSystemWindowsDirectoryW(buf, 260)  # type: ignore[attr-defined]
    except (ImportError, AttributeError, OSError):
        return None
    win = buf.value if 0 < n < 260 else ""
    return win[:2].upper() if win[1:2] == ":" else None


def policy_path(platform: str = sys.platform) -> Path:
    """Where the policy file lives on this platform (policy.js policyPath)."""
    if platform == "win32":
        drive = _windows_drive()
        base = (f"{drive}\\ProgramData" if drive
                else os.environ.get("ProgramData") or "C:\\ProgramData")
        return Path(base) / "LightSim" / "policy.json"
    if platform == "darwin":
        return Path("/Library/Application Support/LightSim/policy.json")
    return Path("/etc/lightsim/policy.json")


def validate(raw: object) -> dict:
    """The settings a parsed policy fixes: only known keys with valid values."""
    if not isinstance(raw, dict):
        return {}
    out = {}
    for key, value in raw.items():
        rule = KEYS.get(key)
        if isinstance(rule, tuple):
            ok = isinstance(value, str) and value in rule
        elif rule == "boolean":
            ok = isinstance(value, bool)
        elif rule == "string":
            ok = isinstance(value, str) and value.strip() != ""
        elif rule == "string-list":
            ok = isinstance(value, list) and all(isinstance(v, str) and v.strip() for v in value)
        else:
            ok = False
        if ok:
            out[key] = value
    return out


def read_file(path: Optional[Path] = None) -> dict:
    """What the policy file fixes; {} when there is none or it cannot be read."""
    try:
        text = (path or policy_path()).read_text(encoding="utf-8-sig")  # Notepad may add a BOM
    except OSError:
        return {}
    try:
        return validate(json.loads(text))
    except ValueError:
        return {}


def settings() -> dict:
    """The settings the machine-wide policy fixes; {} without a policy."""
    if not getattr(sys, "frozen", False):
        given = os.environ.get(ENV)
        if given is not None:
            try:
                return validate(json.loads(given or "{}"))
            except ValueError:
                return {}
    return read_file()


def ai_off() -> bool:
    """True when the policy turns AI access off."""
    return settings().get("ai") == "off"
