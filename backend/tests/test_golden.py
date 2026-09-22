"""Golden regression: the bundled demo projects must reproduce their stored
fixtures exactly (tight tolerance). This is the safety net for the solver
refactor — any unintended behavioral drift fails here first.

Regenerate deliberately after an intended change: python tests/update_golden.py
"""
import json

import pytest
from update_golden import CASES, fixture_path, snapshot


@pytest.mark.parametrize("project_id,case_id", CASES)
def test_demo_matches_golden(project_id, case_id):
    path = fixture_path(project_id, case_id)
    assert path.exists(), f"missing fixture {path} — run tests/update_golden.py"
    expected = json.loads(path.read_text(encoding="utf-8"))
    actual = snapshot(project_id, case_id)

    assert actual["status"] == expected["status"]
    assert actual["messages"] == expected["messages"]

    assert set(actual["channels"]) == set(expected["channels"])
    for key, exp_ch in expected["channels"].items():
        act_ch = actual["channels"][key]
        assert act_ch["unit"] == exp_ch["unit"], key
        assert len(act_ch["points"]) == len(exp_ch["points"]), key
        for (t_act, v_act), (t_exp, v_exp) in zip(act_ch["points"], exp_ch["points"]):
            assert t_act == pytest.approx(t_exp, abs=1e-9), key
            if v_exp is None:
                assert v_act is None, f"{key} @ t={t_exp}"
            else:
                assert v_act == pytest.approx(v_exp, rel=1e-6, abs=1e-6), f"{key} @ t={t_exp}"

    assert actual["summary"].keys() == expected["summary"].keys()
    for label, (v_exp, unit) in expected["summary"].items():
        v_act, unit_act = actual["summary"][label]
        assert unit_act == unit, label
        assert v_act == pytest.approx(v_exp, rel=1e-6, abs=1e-6), label
