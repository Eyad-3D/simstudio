"""Who may talk to the engine: only its own origin on a loopback host and, in
the desktop app, only a client holding the per-launch token."""
import importlib

import pytest
from fastapi.testclient import TestClient
from helpers import bev_axle, sig_port
from starlette.websockets import WebSocketDisconnect

from app.main import app
from app.schemas import ElementInstance

TOKEN = "f" * 64
EVIL = "https://evil.example"


def _marker_project(marker) -> dict:
    """A run of this project writes `marker` if its script ever gets to run.
    (It cannot: scripts may not open files. The rejection must come first.)"""
    proj = bev_axle()
    proj.systems[0].elements.append(ElementInstance(
        id="scr", componentDefId="signal.script", label="Probe",
        position={"x": 0, "y": 0},
        parameterOverrides={"code": f"open({str(marker)!r}, 'w')\n"
                                    "def step(t, dt, inputs, state, params):\n    return {}\n"},
        dynamicPorts=[sig_port("cmd_out", "output")],
    ))
    return proj.model_dump()


def _ws_url(client: TestClient) -> str:
    # websocket_connect ignores base_url for relative paths (Host: testserver)
    return str(client.base_url).replace("http", "ws", 1).rstrip("/") + "/api/simulate/run"


def _ws_refused(client: TestClient, **kwargs) -> bool:
    try:
        with client.websocket_connect(_ws_url(client), **kwargs) as ws:
            ws.send_json({"type": "start", "project": bev_axle().model_dump(), "caseId": "case"})
            ws.receive_json()
    except WebSocketDisconnect as e:
        return e.code == 1008
    return False


def _ws_runs(client: TestClient, **kwargs) -> str:
    with client.websocket_connect(_ws_url(client), **kwargs) as ws:
        ws.send_json({"type": "start", "project": bev_axle().model_dump(), "caseId": "case"})
        while True:
            msg = ws.receive_json()
            if msg["type"] in ("done", "error"):
                return msg.get("result", {}).get("status", msg["type"])


# ---- Host: loopback names only (DNS rebinding) ----------------------------------

def test_forged_host_is_refused():
    client = TestClient(app, base_url="http://127.0.0.1:8912")
    for host in ("attacker.example:8912", "attacker.example", "127.0.0.1.nip.io:8912", "localhost.:8912"):
        r = client.get("/api/library", headers={"Host": host})
        assert r.status_code == 400, host
    assert _ws_refused(client, headers={"Host": "attacker.example:8912"})


@pytest.mark.parametrize("base", ["http://127.0.0.1:8912", "http://localhost:8000"])
def test_loopback_hosts_are_accepted(base):
    client = TestClient(app, base_url=base)
    assert client.get("/api/library").status_code == 200
    assert _ws_runs(client) in ("success", "warning")


# ---- Origin: the engine's own page (and the Vite dev server) only ----------------

def test_foreign_origin_is_refused(tmp_path):
    client = TestClient(app, base_url="http://127.0.0.1:8912")
    marker = tmp_path / "marker.txt"
    for origin in (EVIL, "null", "http://127.0.0.1:9999", "http://localhost:8912"):
        r = client.post("/api/validate", json={"project": _marker_project(marker)},
                        headers={"Origin": origin})
        assert r.status_code == 403, origin
    # a form-style simple request is refused before its body is even read
    r = client.post("/api/simulate", content=b"{}",
                    headers={"Origin": EVIL, "Content-Type": "text/plain"})
    assert r.status_code == 403
    # the preflight no longer grants anything
    r = client.options("/api/validate", headers={
        "Origin": EVIL, "Access-Control-Request-Method": "POST",
        "Access-Control-Request-Headers": "content-type"})
    assert r.status_code == 403
    assert "access-control-allow-origin" not in r.headers
    assert not marker.exists()


def test_websocket_with_foreign_origin_is_refused(tmp_path):
    client = TestClient(app, base_url="http://127.0.0.1:8912")
    marker = tmp_path / "marker.txt"
    try:
        with client.websocket_connect(_ws_url(client), headers={"Origin": EVIL}) as ws:
            ws.send_json({"type": "start", "project": _marker_project(marker), "caseId": "case"})
            ws.receive_json()
        refused = False
    except WebSocketDisconnect as e:
        refused = e.code == 1008
    assert refused
    assert not marker.exists()


def test_own_origin_and_dev_server_are_accepted():
    client = TestClient(app, base_url="http://127.0.0.1:8912")
    project = bev_axle().model_dump()
    for origin in ("http://127.0.0.1:8912", "http://localhost:5173", "http://127.0.0.1:5173"):
        r = client.post("/api/validate", json={"project": project}, headers={"Origin": origin})
        assert r.status_code == 200, origin
        assert _ws_runs(client, headers={"Origin": origin}) in ("success", "warning")
    r = client.options("/api/validate", headers={
        "Origin": "http://localhost:5173", "Access-Control-Request-Method": "POST",
        "Access-Control-Request-Headers": "content-type"})
    assert r.headers["access-control-allow-origin"] == "http://localhost:5173"


def test_requests_without_origin_are_accepted():
    # Non-browser clients (the desktop shell, scripts, the smoke tests) send
    # no Origin; without a launch token that is development, so they may call.
    client = TestClient(app, base_url="http://127.0.0.1:8912")
    assert client.get("/api/projects").status_code == 200


# ---- Launch token: the desktop app ------------------------------------------------

@pytest.fixture
def desktop(tmp_path, monkeypatch):
    """The engine as desktop/src/main.js starts it: a launch token and a UI."""
    dist = tmp_path / "dist"
    (dist / "assets").mkdir(parents=True)
    (dist / "index.html").write_text("<!doctype html><title>SimStudio</title>")
    (dist / "assets" / "app.js").write_text("export default 1;")
    monkeypatch.setenv("SIMSTUDIO_TOKEN", TOKEN)
    monkeypatch.setenv("SIMSTUDIO_STATIC_DIR", str(dist))

    import app.main

    try:
        yield importlib.reload(app.main).app
    finally:
        monkeypatch.undo()
        importlib.reload(app.main)


def test_desktop_refuses_api_calls_without_the_token(desktop):
    client = TestClient(desktop, base_url="http://127.0.0.1:47815")
    project = bev_axle().model_dump()
    assert client.get("/api/library").status_code == 401
    assert client.post("/api/validate", json={"project": project}).status_code == 401
    assert client.post("/api/simulate", json={"project": project, "caseId": "case"}).status_code == 401
    assert _ws_refused(client)

    wrong = {"Cookie": "simstudio_token=" + "0" * 64}
    assert client.get("/api/library", headers=wrong).status_code == 401
    assert _ws_refused(client, headers=wrong)
    assert client.get("/api/library", headers={"Authorization": "Bearer nope"}).status_code == 401

    # the shell's readiness poll and the UI's static files stay open
    assert client.get("/api/health").status_code == 200
    assert client.get("/assets/app.js").status_code == 200


def test_desktop_page_load_without_the_token_gets_no_cookie(desktop):
    client = TestClient(desktop, base_url="http://127.0.0.1:47815")
    r = client.get("/")
    assert r.status_code == 200 and "SimStudio" in r.text
    assert "set-cookie" not in r.headers
    assert "frame-ancestors 'none'" in r.headers["content-security-policy"]
    assert r.headers["cache-control"] == "no-store"
    assert client.get("/api/library").status_code == 401


def test_desktop_window_gets_the_cookie_and_then_works(desktop):
    client = TestClient(desktop, base_url="http://127.0.0.1:47815")
    # the window's first load carries the token (loadURL extraHeaders)
    r = client.get("/", headers={"Authorization": f"Bearer {TOKEN}"})
    cookie = r.headers["set-cookie"]
    assert cookie.startswith(f"simstudio_token={TOKEN};")
    assert "HttpOnly" in cookie and "SameSite=Strict" in cookie and "Path=/" in cookie

    # every later call is the UI's own: same origin, cookie sent by the browser
    own = {"Origin": "http://127.0.0.1:47815"}
    assert client.get("/api/library").status_code == 200
    project = client.get("/api/projects/bev-car").json()
    r = client.post("/api/validate", json={"project": project}, headers=own)
    assert r.status_code == 200
    assert _ws_runs(client, headers=own) in ("success", "warning")
    # a reload re-sends the cookie and keeps it
    assert "set-cookie" in client.get("/").headers

    # the cookie does not help a foreign page or host, nor the dev server origin
    assert client.get("/api/library", headers={"Origin": EVIL}).status_code == 403
    assert client.get("/api/library", headers={"Origin": "http://localhost:5173"}).status_code == 403
    assert _ws_refused(client, headers={"Origin": EVIL})
    assert client.get("/api/library", headers={"Host": "attacker.example:47815"}).status_code == 400


def test_desktop_accepts_the_token_as_a_bearer_header(desktop):
    client = TestClient(desktop, base_url="http://127.0.0.1:47815")
    r = client.get("/api/library", headers={"Authorization": f"Bearer {TOKEN}"})
    assert r.status_code == 200
