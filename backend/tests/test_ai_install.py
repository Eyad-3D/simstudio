"""`lightsim-backend mcp install|uninstall|status` (AI-29): one command adds
LightSim to an AI app's settings, keeps everything else in the file, and
never overwrites a file it cannot read."""
from __future__ import annotations

import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

from app.ai import install
from app.ai.cli import main

CMD = ["/opt/LightSim/lightsim-backend", "mcp"]
BACKEND = Path(__file__).resolve().parent.parent


def test_the_server_still_starts_for_a_connection_set_up_with_trust_scripts(tmp_path):
    # --trust-scripts was removed with the hands-off change (the owner,
    # 1 Nov 2026); accept and ignore it so a connection whose config still
    # passes it starts, with a note on stderr to drop it. Empty stdin ends
    # the serve loop at once, so the server starts and exits 0.
    env = {**os.environ, "LIGHTSIM_PROJECTS_DIR": str(tmp_path / "projects"),
           "LIGHTSIM_AI_SETTINGS": str(tmp_path / "ai.json")}
    r = subprocess.run([sys.executable, "-m", "app.ai", "--trust-scripts"],
                       cwd=BACKEND, stdin=subprocess.DEVNULL, capture_output=True,
                       text=True, timeout=60, env=env)
    assert r.returncode == 0, r.stderr
    assert "trust-scripts is no longer needed" in r.stderr


@pytest.fixture(autouse=True)
def sandbox(tmp_path, monkeypatch):
    monkeypatch.setenv(install.SANDBOX_ENV, str(tmp_path))
    return tmp_path


@pytest.mark.parametrize("client", sorted(install.CLIENTS))
def test_install_then_uninstall_leaves_other_settings_alone(client):
    spec = install.CLIENTS[client]
    path = spec.config_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    if spec.toml:
        path.write_text('model = "o4"\n\n[mcp_servers.other]\ncommand = "other"\n', encoding="utf-8")
    else:
        path.write_text(json.dumps({"theme": "dark", spec.key: {"other": {"command": "other"}}}),
                        encoding="utf-8")

    assert install.install(client, extra_args=["--read-only"], command=CMD) == path
    assert install.is_installed(client)
    if spec.toml:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
        assert data["model"] == "o4" and data["mcp_servers"]["other"]["command"] == "other"
        entry = data["mcp_servers"]["lightsim"]
    else:
        data = json.loads(path.read_text(encoding="utf-8"))
        assert data["theme"] == "dark" and "other" in data[spec.key]
        entry = data[spec.key]["lightsim"]
    assert entry["command"] == CMD[0] and entry["args"] == ["mcp", "--read-only"]
    assert path.with_name(path.name + install.BACKUP_SUFFIX).exists()

    install.install(client, command=CMD)  # installing again replaces the entry, once
    text = path.read_text(encoding="utf-8")
    assert text.count('"/opt/LightSim/lightsim-backend"') == 1

    assert install.uninstall(client) == path
    assert not install.is_installed(client)
    after = path.read_text(encoding="utf-8")
    assert "other" in after and "lightsim" not in after.replace("lightsim-backend", "")
    assert install.uninstall(client) is None


def test_a_new_settings_file_is_created():
    path = install.install("gemini", command=CMD)
    assert json.loads(path.read_text(encoding="utf-8")) == {
        "mcpServers": {"lightsim": {"command": CMD[0], "args": ["mcp"]}}}


def test_a_broken_settings_file_is_never_overwritten():
    path = install.CLIENTS["cursor"].config_path()
    path.parent.mkdir(parents=True)
    path.write_text("{broken", encoding="utf-8")
    with pytest.raises(install.InstallError, match="not valid JSON"):
        install.install("cursor", command=CMD)
    assert path.read_text(encoding="utf-8") == "{broken"


def test_codex_entry_the_user_wrote_is_not_replaced():
    path = install.CLIENTS["codex"].config_path()
    path.parent.mkdir(parents=True)
    path.write_text('[mcp_servers.lightsim]\ncommand = "mine"\n', encoding="utf-8")
    with pytest.raises(install.InstallError, match="not added by LightSim"):
        install.install("codex", command=CMD)


def test_the_commands(capsys, sandbox):
    assert main(["install", "--client", "vscode"]) == 0
    assert "VS Code" in capsys.readouterr().out
    entry = json.loads(install.CLIENTS["vscode"].config_path().read_text(encoding="utf-8"))["servers"]["lightsim"]
    assert entry["type"] == "stdio" and entry["args"][-1] == "mcp"
    assert main(["status"]) == 0
    out = capsys.readouterr().out
    assert "VS Code (GitHub Copilot)   set up" in out and "No assistant has used LightSim yet." in out
    assert main(["uninstall", "--client", "vscode"]) == 0
    with pytest.raises(SystemExit):
        main(["install", "--client", "nope"])
