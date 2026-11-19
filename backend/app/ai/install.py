"""Add LightSim to an AI app's MCP settings, or take it out again (AI-29).

``lightsim-backend mcp install --client <app>`` writes one entry, named
``lightsim``, into the app's settings file and leaves everything else in
that file as it was. A copy of the file as it was is kept beside it
(``<file>.lightsim-backup``). A file that cannot be read safely (not valid
JSON or TOML) is never overwritten: the command stops and says so.

The settings files and their shapes are those the apps documented in 2026;
an app that moves its file needs an update here.
"""
from __future__ import annotations

import json
import os
import re
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Optional

ENTRY = "lightsim"
BACKUP_SUFFIX = ".lightsim-backup"
_TOML_BEGIN = "# >>> lightsim (added by 'lightsim-backend mcp install'; remove with 'mcp uninstall')"
_TOML_END = "# <<< lightsim"


class InstallError(Exception):
    """The settings could not be changed safely; nothing was written."""


#: Tests point every AI app's settings at a scratch folder with this.
SANDBOX_ENV = "LIGHTSIM_AI_CONFIG_HOME"


def _sandbox() -> Optional[Path]:
    value = os.environ.get(SANDBOX_ENV)
    return Path(value) if value else None


def _home() -> Path:
    return _sandbox() or Path.home()


def _appdata() -> Path:
    if _sandbox():
        return _home() / "AppData" / "Roaming"
    return Path(os.environ.get("APPDATA") or _home() / "AppData" / "Roaming")


def _app_config(*parts: str) -> Path:
    """An app's folder in the system's per-user settings place."""
    if _sandbox():
        return _home().joinpath(".config", *parts)
    if sys.platform == "win32":
        return _appdata().joinpath(*parts)
    if sys.platform == "darwin":
        return _home().joinpath("Library", "Application Support", *parts)
    return Path(os.environ.get("XDG_CONFIG_HOME") or _home() / ".config").joinpath(*parts)


@dataclass(frozen=True)
class Client:
    title: str
    config_path: Callable[[], Path]
    key: str  # where the servers live in the file ("mcpServers", "servers", "mcp_servers")
    shape: Callable[[list[str]], dict]
    toml: bool = False


def _plain(cmd: list[str]) -> dict:
    return {"command": cmd[0], "args": cmd[1:]}


def _typed(cmd: list[str]) -> dict:
    return {"type": "stdio", "command": cmd[0], "args": cmd[1:]}


CLIENTS: dict[str, Client] = {
    "claude": Client("Claude Desktop", lambda: _app_config("Claude") / "claude_desktop_config.json",
                     "mcpServers", _plain),
    "claude-code": Client("Claude Code", lambda: _home() / ".claude.json", "mcpServers", _typed),
    "vscode": Client("VS Code (GitHub Copilot)", lambda: _app_config("Code", "User") / "mcp.json",
                     "servers", _typed),
    "copilot": Client("GitHub Copilot CLI", lambda: _home() / ".copilot" / "mcp-config.json",
                      "mcpServers",
                      lambda cmd: {"type": "local", "command": cmd[0], "args": cmd[1:], "tools": ["*"]}),
    "codex": Client("OpenAI Codex",
                    lambda: (_home() / ".codex" if _sandbox() else
                             Path(os.environ.get("CODEX_HOME") or _home() / ".codex")) / "config.toml",
                    "mcp_servers", _plain, toml=True),
    "gemini": Client("Gemini CLI", lambda: _home() / ".gemini" / "settings.json", "mcpServers", _plain),
    "cursor": Client("Cursor", lambda: _home() / ".cursor" / "mcp.json", "mcpServers", _plain),
}


def server_command() -> list[str]:
    """The command an AI app runs to start LightSim's MCP server: the
    installed engine, or (from a source checkout) this Python running the
    engine's entry script."""
    if getattr(sys, "frozen", False):
        return [sys.executable, "mcp"]
    entry = Path(__file__).resolve().parents[2] / "run_backend.py"
    return [sys.executable, str(entry), "mcp"]


def command_warning() -> Optional[str]:
    """Why the command may stop working, or None: an AppImage's engine
    lives in a folder that exists only while LightSim runs."""
    appdir = os.environ.get("APPDIR")
    if os.environ.get("APPIMAGE") and appdir and sys.executable.startswith(appdir):
        return ("LightSim runs from an AppImage, whose files exist only while it is open: "
                "the AI app can start LightSim only while LightSim is running. Install the "
                ".deb package for a connection that always works.")
    return None


def _client(name: str) -> Client:
    try:
        return CLIENTS[name]
    except KeyError:
        raise InstallError(f"Unknown AI app '{name}' (known: {', '.join(sorted(CLIENTS))}).") from None


def _read_json(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        text = path.read_text(encoding="utf-8")
        data = json.loads(text) if text.strip() else {}
    except (OSError, ValueError) as e:
        raise InstallError(f"{path} is not valid JSON ({e}); fix or move it first.") from None
    if not isinstance(data, dict):
        raise InstallError(f"{path} does not hold a JSON object; fix or move it first.")
    return data


def _write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        shutil.copy2(path, path.with_name(path.name + BACKUP_SUFFIX))
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        os.replace(tmp, path)
    except BaseException:
        Path(tmp).unlink(missing_ok=True)
        raise


def _toml_block(cmd: list[str]) -> str:
    args = ", ".join(json.dumps(a) for a in cmd[1:])
    return (f"{_TOML_BEGIN}\n[mcp_servers.{ENTRY}]\ncommand = {json.dumps(cmd[0])}\n"
            f"args = [{args}]\n{_TOML_END}\n")


_TOML_RE = re.compile(re.escape(_TOML_BEGIN) + r".*?" + re.escape(_TOML_END) + r"\n?", re.S)


def _read_toml(path: Path) -> tuple[str, dict]:
    import tomllib

    text = path.read_text(encoding="utf-8") if path.exists() else ""
    try:
        return text, tomllib.loads(text)
    except tomllib.TOMLDecodeError as e:
        raise InstallError(f"{path} is not valid TOML ({e}); fix or move it first.") from None


def install(name: str, extra_args: Optional[list[str]] = None,
            command: Optional[list[str]] = None) -> Path:
    """Add (or update) the ``lightsim`` entry; returns the file written."""
    client = _client(name)
    cmd = (command or server_command()) + list(extra_args or [])
    path = client.config_path()
    if client.toml:
        text, data = _read_toml(path)
        ours = _TOML_RE.search(text)
        if not ours and ENTRY in (data.get(client.key) or {}):
            raise InstallError(f"{path} already has an [{client.key}.{ENTRY}] entry not added by "
                               "LightSim; remove it first.")
        body = _TOML_RE.sub("", text)
        if body and not body.endswith("\n"):
            body += "\n"
        new = body + ("\n" if body.strip() else "") + _toml_block(cmd)
        import tomllib
        tomllib.loads(new)  # never write a file the app could not read
        _write(path, new)
        return path
    data = _read_json(path)
    servers = data.get(client.key)
    if servers is None:
        servers = data[client.key] = {}
    if not isinstance(servers, dict):
        raise InstallError(f"'{client.key}' in {path} is not an object; fix it first.")
    servers[ENTRY] = client.shape(cmd)
    _write(path, json.dumps(data, indent=2, ensure_ascii=False) + "\n")
    return path


def uninstall(name: str) -> Optional[Path]:
    """Remove the ``lightsim`` entry; returns the file changed, or None if
    there was nothing to remove."""
    client = _client(name)
    path = client.config_path()
    if not path.exists():
        return None
    if client.toml:
        text, _ = _read_toml(path)
        if not _TOML_RE.search(text):
            return None
        _write(path, _TOML_RE.sub("", text).rstrip("\n") + "\n" if text.strip() else "")
        return path
    data = _read_json(path)
    servers = data.get(client.key)
    if not isinstance(servers, dict) or ENTRY not in servers:
        return None
    del servers[ENTRY]
    _write(path, json.dumps(data, indent=2, ensure_ascii=False) + "\n")
    return path


def is_installed(name: str) -> bool:
    client = _client(name)
    path = client.config_path()
    try:
        if client.toml:
            return bool(_TOML_RE.search(path.read_text(encoding="utf-8")))
        servers = _read_json(path).get(client.key)
    except (OSError, InstallError):
        return False
    return isinstance(servers, dict) and ENTRY in servers
