import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

# Starlette's TestClient sends "Host: testserver"; the engine answers only
# 127.0.0.1 and localhost unless told otherwise (app/security.py).
os.environ.setdefault("LIGHTSIM_ALLOWED_HOSTS", "testserver")

# Most tests run models with their own Script code; the check that a user
# approved it (app/script_trust.py) has its own tests, which turn it on.
os.environ.setdefault("LIGHTSIM_SCRIPT_TRUST", "off")

import pytest  # noqa: E402


@pytest.fixture(autouse=True)
def _ai_settings(tmp_path_factory, monkeypatch):
    """Every test gets its own AI access settings file (off until a test
    turns it on), never the user's real one (lightsim/ai_access.py)."""
    monkeypatch.setenv("LIGHTSIM_AI_SETTINGS", str(tmp_path_factory.mktemp("ai") / "ai-access.json"))


def allow_ai(*folders, examples: bool = True) -> None:
    """Turn AI access on for ``folders`` in this test's settings file."""
    import json

    path = os.environ["LIGHTSIM_AI_SETTINGS"]
    with open(path, "w", encoding="utf-8") as f:
        json.dump({"version": 1, "enabled": True, "examples": examples,
                   "folders": [str(Path(x).resolve()) for x in folders]}, f)
