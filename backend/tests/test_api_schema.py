"""The engine's HTTP API against its own OpenAPI schema, with Schemathesis.

Schemathesis generates requests from /openapi.json, valid and invalid ones,
and checks every answer: no server error (5xx), only the status codes the
schema declares, the declared content type, and a body that matches the
declared response schema. Saves and deletes go to a temporary projects folder.

Left out:
- "the API rejects schema-violating data": pydantic's lax mode accepts
  `false` for a number (as 0.0), which the JSON schema forbids. That is a
  deliberate leniency, not a defect.
- POST /api/simulate: a generated case may ask for any duration and time step
  and would run the project's control scripts. The solver tests cover it.
- the live-run WebSocket, which is not in the schema.
"""
from __future__ import annotations

import copy

import pytest
import schemathesis
from hypothesis import HealthCheck, settings
from schemathesis.checks import not_a_server_error
from schemathesis.specs.openapi.checks import (
    content_type_conformance,
    response_schema_conformance,
    status_code_conformance,
)

from app.main import app

# Error answers the routes give on purpose that /openapi.json does not list
# yet (FastAPI declares only 200 and 422 unless a route passes `responses=`).
# They are added to the copy of the schema under test, with the body FastAPI
# sends, so any other status still fails. Declare them in app/main.py and
# drop them here. 400 includes a request body that is not UTF-8; 409 is a
# save that would overwrite newer work (If-Match), once saves check that.
UNDECLARED = {
    ("get", "/api/projects/{project_id}"): {"400": "Invalid project id", "404": "No such project"},
    ("put", "/api/projects/{project_id}"): {
        "400": "Invalid project id, id mismatch or unreadable body",
        "409": "The project changed since it was read",
    },
    ("delete", "/api/projects/{project_id}"): {"400": "Invalid project id", "404": "No such project"},
    ("post", "/api/validate"): {"400": "Unreadable request body"},
    ("get", "/api/projects/{project_id}/runs"): {"400": "Invalid project id"},
    ("delete", "/api/projects/{project_id}/runs"): {"400": "Invalid project id"},
    ("get", "/api/projects/{project_id}/runs/{run_id}"): {"400": "Invalid id", "404": "No such run"},
    ("put", "/api/projects/{project_id}/runs/{run_id}"): {"400": "Invalid id or id mismatch"},
    ("delete", "/api/projects/{project_id}/runs/{run_id}"): {"400": "Invalid id", "404": "No such run"},
    ("get", "/api/projects/{project_id}/backups"): {"400": "Invalid project id"},
    ("get", "/api/projects/{project_id}/backups/{backup_id}"): {
        "400": "Invalid id or unreadable backup",
        "404": "No such backup",
    },
    ("get", "/api/examples/{example_id}"): {"400": "Invalid example id", "404": "No such example"},
    ("post", "/api/examples/{example_id}/hide"): {"400": "Invalid example id", "404": "No such example"},
}
ERROR_BODY = {
    "application/json": {
        "schema": {
            "type": "object",
            "properties": {"detail": {"type": "string"}},
            "required": ["detail"],
        }
    }
}


def _schema_under_test() -> dict:
    raw = copy.deepcopy(app.openapi())
    for (method, path), statuses in UNDECLARED.items():
        responses = raw["paths"][path][method]["responses"]
        for status, description in statuses.items():
            responses.setdefault(status, {"description": description, "content": ERROR_BODY})
    return raw


schema = schemathesis.openapi.from_dict(_schema_under_test())
schema.app = app
CHECKS = [
    not_a_server_error,
    status_code_conformance,
    content_type_conformance,
    response_schema_conformance,
]


@pytest.fixture(scope="module")
def projects_dir(tmp_path_factory):
    # saves, hidden examples and runs land here, not in the user's folder
    with pytest.MonkeyPatch.context() as mp:
        mp.setenv("SIMSTUDIO_PROJECTS_DIR", str(tmp_path_factory.mktemp("projects")))
        yield


@schema.exclude(path="/api/simulate").parametrize()
@settings(max_examples=50, deadline=None, suppress_health_check=[HealthCheck.too_slow])
def test_api_answers_as_its_schema_says(case, projects_dir):
    case.call_and_validate(checks=CHECKS)
