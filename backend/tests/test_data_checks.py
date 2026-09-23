"""Data Checks that catch models which would run 'successfully' but wrongly."""
from fastapi.testclient import TestClient
from helpers import dbc

from app.main import app
from app.schemas import Connection
from app.storage import load_project
from app.validation import validate_project

client = TestClient(app)


def _errors(project):
    return [c.text for c in validate_project(project) if c.level == "error"]


# ---- UX-37: two signals wired into one input ----------------------------------------

def test_two_sources_on_one_input_is_an_error_naming_both_links():
    proj = load_project("bev-car")
    proj.dataBusConnections.append(dbc(99, "el-vehicle", "sig_speed", "el-driver", "sig_target_in"))
    errors = [e for e in _errors(proj) if "sources" in e]
    assert errors == [
        "'Driver.Target Speed' has 2 sources: 'Vehicle Task.Target Speed' and "
        "'Vehicle.Vehicle Speed' — "
        "an input takes one signal and the run would silently use only the last link. "
        "Remove all but one of them."
    ]


def test_the_run_is_refused_instead_of_driving_zero_km():
    proj = load_project("bev-car")
    proj.dataBusConnections.append(dbc(99, "el-vehicle", "sig_speed", "el-driver", "sig_target_in"))
    result = client.post("/api/simulate",
                         json={"project": proj.model_dump(), "caseId": "case-city"}).json()
    assert result["status"] == "failed"
    assert any("2 sources" in m["text"] for m in result["messages"])


def test_fan_in_is_found_across_canvas_wires_and_data_bus_links():
    proj = load_project("hybrid-car")
    # an older file kept a signal link as a canvas wire, input side first
    proj.systems[0].connections.append(Connection(
        id="c-sig", sourceElementId="el-hcu", sourcePortId="soc",
        targetElementId="el-vehicle", targetPortId="sig_speed"))
    errors = [e for e in _errors(proj) if "sources" in e]
    assert len(errors) == 1
    # listed in the order the solver applies them: the last one would win
    assert errors[0].startswith("'Hybrid Control Unit.soc' has 2 sources: 'Vehicle.Vehicle Speed' "
                                "and 'HV Battery.SOC'")


def test_repeated_link_and_fan_out_are_fine():
    proj = load_project("bev-car")
    # the same link twice feeds the input from one source only
    proj.dataBusConnections.append(dbc(98, "el-task", "sig_demand", "el-driver", "sig_target_in"))
    # one output feeding many inputs (brake command → four brakes) is normal
    assert not [e for e in _errors(proj) if "sources" in e]


def test_shipped_examples_have_no_fan_in():
    for name in ("bev-car", "hybrid-car"):
        assert not [e for e in _errors(load_project(name)) if "sources" in e]
