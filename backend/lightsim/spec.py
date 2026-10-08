"""The JSON Schemas of LightSim's files, generated from the engine's own
data models so they cannot drift from what the app reads and writes.

The committed copies live in docs/spec/schemas/ (``lightsim schema --out
docs/spec/schemas`` writes them; a test fails when they are out of date).
Each schema carries the format's version in its ``$id``; docs/spec/CHANGELOG.md
says what changed in each version.
"""
from __future__ import annotations

import json
from pathlib import Path

from ._engine import engine

#: Version of each format. The project file's is its ``schemaVersion`` field;
#: the files lane's migrations (PLT-07) raise it when the shape changes.
FORMAT_VERSIONS = {
    "project": 1,
    "run": 1,
    "result": 1,
    "study": 1,
    "library": 1,
    "data-checks": 1,
    "lightsim-result": 1,
}

#: What each schema says about its licence. docs/spec/README.md holds the
#: draft licence, which waits for the owner's decision; until the owner
#: confirms it, a schema names no licence of its own. When it is confirmed,
#: name it here and regenerate the schemas (``lightsim schema --out``).
LICENCE_NOTE = "Licence: a draft waiting for the owner's decision, see docs/spec/README.md."

_TITLES = {
    "project": "LightSim project file",
    "run": "LightSim stored run (runs/<project id>/<run id>.json.gz, gzip-compressed)",
    "result": "LightSim simulation result (SimResult)",
    "study": "LightSim parameter study (an entry of a project's 'studies')",
    "library": "LightSim component library (GET /api/library 'components' entries)",
    "data-checks": "LightSim Data Check findings (POST /api/validate)",
    "lightsim-result": "LightSim result export (lightsim run --out result.json)",
}


def _lightsim_result_schema() -> dict:
    """The CLI's and Python package's JSON export (Result.to_dict)."""
    num_or_null = {"type": ["number", "null"]}
    return {
        "type": "object",
        "required": ["format", "formatVersion", "caseId", "status", "kpis", "summary",
                     "messages"],
        "properties": {
            "format": {"const": "lightsim-result"},
            "formatVersion": {"const": 1},
            "project": {"type": "string"},
            "caseId": {"type": "string"},
            "caseName": {"type": "string"},
            "status": {"enum": ["success", "warning", "failed", "cancelled"]},
            "valid": {"type": "boolean"},
            "kpis": {"type": "object", "additionalProperties": {"type": "number"}},
            "units": {"type": "object", "additionalProperties": {"type": "string"}},
            "notValid": {"type": "object", "additionalProperties": {"type": "string"}},
            "summary": {"type": "array", "items": {
                "type": "object", "required": ["key", "label", "value", "unit"],
                "properties": {"key": {"type": "string"}, "label": {"type": "string"},
                               "value": {"type": "number"}, "unit": {"type": "string"},
                               "notValid": {"type": "string"}, "limit": {"type": "number"},
                               "passed": {"type": "boolean"}}}},
            "messages": {"type": "array", "items": {
                "type": "object", "required": ["level", "text"],
                "properties": {"level": {"enum": ["info", "warning", "error"]},
                               "text": {"type": "string"}}}},
            "checks": {"type": "array", "items": {"type": "object"}},
            "time": {"type": "array", "items": {"type": "number"}},
            "channels": {"type": "object", "additionalProperties": {
                "type": "object", "required": ["label", "unit", "values"],
                "properties": {"label": {"type": "string"}, "unit": {"type": "string"},
                               "values": {"type": "array", "items": num_or_null}}}},
        },
    }


def json_schemas() -> dict[str, dict]:
    """Every schema by name (the keys of :data:`FORMAT_VERSIONS`)."""
    from pydantic import TypeAdapter

    s = engine("schemas")
    raw = {
        "project": s.Project.model_json_schema(),
        "run": s.StoredRun.model_json_schema(),
        "result": s.SimResult.model_json_schema(),
        "study": s.Study.model_json_schema(),
        "library": TypeAdapter(list[s.ComponentDef]).json_schema(),
        "data-checks": TypeAdapter(list[s.DataCheck]).json_schema(),
        "lightsim-result": _lightsim_result_schema(),
    }
    out = {}
    for name, schema in raw.items():
        version = FORMAT_VERSIONS[name]
        out[name] = {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": f"urn:lightsim:schema:{name}:{version}",
            "title": _TITLES[name],
            "description": (f"Format version {version}. Generated from LightSim's data models; "
                            f"the specification is docs/spec/README.md. {LICENCE_NOTE}"),
            **{k: v for k, v in schema.items() if k not in ("title", "description")},
        }
    return out


def write_schemas(folder: str | Path) -> list[Path]:
    """Write each schema to ``<folder>/<name>.schema.json``; returns the paths."""
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    paths = []
    for name, schema in json_schemas().items():
        path = folder / f"{name}.schema.json"
        path.write_text(json.dumps(schema, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        paths.append(path)
    return paths
