"""Live parameter edits honor the catalog's variability metadata:
"fixed" (structural) params defer to the next run with a message,
"tunable" ones apply silently.
"""
from helpers import bev_axle

from app.solver import simulate


def _run_with_edit(element_id: str, key: str, value):
    proj = bev_axle(profile="0:0; 5:60; 10:60")
    proj.cases[0].duration = 10
    sent = {"done": False}

    def control():
        if not sent["done"]:
            sent["done"] = True
            return [{"type": "set_param", "elementId": element_id, "key": key, "value": value}]
        return []

    return simulate(proj, "case", control=control)


def test_fixed_parameter_edit_defers_with_message():
    result = _run_with_edit("fd", "ratio", 5.0)
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    assert any(
        "'Final Drive.ratio' changed" in m.text and "next run" in m.text
        for m in result.messages
    ), [m.text for m in result.messages]


def test_tunable_parameter_edit_applies_silently():
    result = _run_with_edit("drv", "driver_kp", 0.5)
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    assert not any("take effect on the next run" in m.text for m in result.messages)
