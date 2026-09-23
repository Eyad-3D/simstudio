"""API tests: REST round-trip and the live WebSocket run channel."""
from fastapi.testclient import TestClient
from helpers import bev_axle

from app.main import app

client = TestClient(app)


def test_rest_roundtrip():
    assert client.get("/api/health").json()["status"] == "ok"

    lib = client.get("/api/library").json()["components"]
    by_id = {c["id"]: c for c in lib}
    assert "vehicle.body" in by_id and "signal.script" in by_id
    # units are mandatory everywhere, including dimensionless ones
    for c in lib:
        for p in c["parameters"]:
            assert p["unit"], f"{c['id']}.{p['key']} has no unit"

    project = client.get("/api/examples/bev-car").json()
    assert project["name"] == "Battery Electric Car"

    checks = client.post("/api/validate", json={"project": project}).json()
    assert not [c for c in checks if c["level"] == "error"]

    result = client.post(
        "/api/simulate", json={"project": project, "caseId": "case-city"}
    ).json()
    assert result["status"] == "success"
    assert len(result["channels"]) > 30


def test_ws_stream_and_live_set_param():
    project = bev_axle().model_dump()
    with client.websocket_connect("/api/simulate/run") as ws:
        ws.send_json({"type": "start", "project": project, "caseId": "case"})
        steps = 0
        result = None
        while True:
            msg = ws.receive_json()
            if msg["type"] == "step":
                steps += 1
                if steps == 10:  # live change: silence the motor via q4/full-load scale
                    ws.send_json({"type": "set_param", "elementId": "batt",
                                  "key": "max_charge_power_kW", "value": 1.0})
            elif msg["type"] == "done":
                result = msg["result"]
                break
        assert steps >= 60
        assert result["status"] in ("success", "warning")


def test_ws_cancel_stops_early():
    project = bev_axle().model_dump()
    # Long enough that the solver cannot finish before the cancel arrives:
    # a fast CI runner completed a 600 s case while the first five steps
    # were still in flight.
    project["cases"][0]["duration"] = 36000
    project["cases"][0]["timeStep"] = 1.0
    with client.websocket_connect("/api/simulate/run") as ws:
        ws.send_json({"type": "start", "project": project, "caseId": "case"})
        n = 0
        while True:
            msg = ws.receive_json()
            if msg["type"] == "step":
                n += 1
                if n == 5:
                    ws.send_json({"type": "cancel"})
            elif msg["type"] == "done":
                result = msg["result"]
                break
        assert n < 36000
        assert any("cancelled" in m["text"] for m in result["messages"])


def test_ws_rejects_bad_start():
    with client.websocket_connect("/api/simulate/run") as ws:
        ws.send_json({"type": "start", "project": {"nonsense": True}, "caseId": "x"})
        msg = ws.receive_json()
        assert msg["type"] == "error"
