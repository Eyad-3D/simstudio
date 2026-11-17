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


def pytest_configure(config):
    config.addinivalue_line(
        "markers", "todays_engine(why): run on today's engine (LIGHTSIM_ENGINE=today), not the app's "
                   "new engine: a feature the new engine does not run yet, whose tests are the oracle "
                   "for its port, or today's engine's own internals")


def _matches(node: str, key: str) -> bool:
    return node == key or node.startswith(key + "[") or ("[" in key and node.startswith(key))


def pytest_collection_modifyitems(config, items):
    """tests/engine_marks.py: the tests that run today's engine (marker
    todays_engine) and those the new engine fails (xfail, strict), each with
    its reason; a key no collected test of its file matches is an error."""
    import engine_marks as em

    used, files = set(), set()
    for item in items:
        node = item.nodeid.split("tests/", 1)[-1]
        files.add(node.split("::")[0])
        if why := em.TODAY_FILES.get(node.split("::")[0]):
            item.add_marker(pytest.mark.todays_engine(why))
            used.add(node.split("::")[0])
        for table, mark in ((em.XFAIL, "xfail"), (em.TODAY, "today")):
            hits = [k for k in table if _matches(node, k)]
            if not hits:
                continue
            used.update(hits)
            why = table[max(hits, key=len)]
            item.add_marker(pytest.mark.xfail(strict=True, reason=why) if mark == "xfail"
                            else pytest.mark.todays_engine(why))
            break
    stale = [k for k in (*em.TODAY_FILES, *em.TODAY, *em.XFAIL) if k.split("::")[0] in files and k not in used]
    if stale:
        raise pytest.UsageError(f"tests/engine_marks.py names tests that do not exist: {stale}")


@pytest.hookimpl(tryfirst=True)
def pytest_runtest_setup(item):
    """The app's engine (the new one) for every test, whatever the shell
    says; today's for a test marked ``todays_engine`` (the marker says why).
    Set before the test's fixtures, a module's among them, and not through
    monkeypatch (a test's monkeypatch.undo() keeps it); a test that sets
    LIGHTSIM_ENGINE itself does so after this. The shared caches of runs
    (helpers.example_result, validation/suite.py) are kept per engine."""
    if item.get_closest_marker("todays_engine"):
        os.environ["LIGHTSIM_ENGINE"] = "today"
    else:
        os.environ.pop("LIGHTSIM_ENGINE", None)


@pytest.fixture(autouse=True)
def _ai_settings(tmp_path_factory, monkeypatch):
    """Every test gets its own AI access settings file (off until a test
    turns it on), never the user's real one (lightsim/ai_access.py)."""
    monkeypatch.setenv("LIGHTSIM_AI_SETTINGS", str(tmp_path_factory.mktemp("ai") / "ai-access.json"))


@pytest.fixture(autouse=True)
def _no_machine_policy(tmp_path_factory, monkeypatch):
    """No test reads the machine-wide policy file of the computer it runs on
    (app/machine_policy.py); a test that needs one writes its own."""
    from app import machine_policy

    missing = tmp_path_factory.mktemp("policy") / "policy.json"
    monkeypatch.setattr(machine_policy, "policy_path", lambda platform=None: missing)


def allow_ai(*folders, examples: bool = True) -> None:
    """Turn AI access on for ``folders`` in this test's settings file."""
    import json

    path = os.environ["LIGHTSIM_AI_SETTINGS"]
    with open(path, "w", encoding="utf-8") as f:
        json.dump({"version": 1, "enabled": True, "examples": examples,
                   "folders": [str(Path(x).resolve()) for x in folders]}, f)
