"""The published file-format specification (AI-07): the committed JSON
Schemas are what the engine generates, the examples and their results
validate against them, every field is described in docs/spec, and every
example on the Python API help page runs."""
from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

import jsonschema
import pytest

from app.schemas import StoredRun
from app.solver import simulate
from app.storage import load_example
from lightsim.spec import json_schemas

ROOT = Path(__file__).parent.parent.parent
SPEC = ROOT / "docs" / "spec"
EXAMPLES = sorted((ROOT / "backend" / "projects").glob("*.json"))


def _validator(name: str):
    schema = json.loads((SPEC / "schemas" / f"{name}.schema.json").read_text(encoding="utf-8"))
    return jsonschema.Draft202012Validator(schema)


def test_committed_schemas_are_up_to_date():
    for name, schema in json_schemas().items():
        path = SPEC / "schemas" / f"{name}.schema.json"
        assert path.is_file(), f"missing {path}"
        assert json.loads(path.read_text(encoding="utf-8")) == schema, (
            f"{path.name} is out of date: run 'python -m lightsim schema --out ../docs/spec/schemas' "
            f"in backend/ and note the change in docs/spec/CHANGELOG.md")


@pytest.mark.parametrize("path", EXAMPLES, ids=lambda p: p.stem)
def test_example_projects_validate(path):
    _validator("project").validate(json.loads(path.read_text(encoding="utf-8")))


@pytest.mark.parametrize("example,case", [("bev-car", "case-city"), ("hybrid-car", "case-mixed")])
def test_golden_case_results_and_stored_runs_validate(example, case):
    project = load_example(example)
    result = simulate(project, case)
    _validator("result").validate(result.model_dump(mode="json"))
    run = StoredRun(id="r", caseId=case, caseName=case, startedAt=0, status=result.status,
                    result=result, snapshot={"project": project, "case": project.cases[0]})
    _validator("run").validate(json.loads(run.model_dump_json()))


def _fields(schema: dict) -> set[str]:
    names = set(schema.get("properties", {}))
    for sub in schema.get("$defs", {}).values():
        names |= set(sub.get("properties", {}))
    return names


@pytest.mark.parametrize("schema,pages", [("project", ["project.md"]),
                                          ("run", ["results.md", "project.md"])])
def test_every_field_is_described(schema, pages):
    text = "".join((SPEC / page).read_text(encoding="utf-8") for page in pages)
    missing = sorted(f for f in _fields(json_schemas()[schema]) if f"`{f}`" not in text)
    assert not missing, f"docs/spec/{pages[0]} does not describe: {missing}"


def test_every_summary_key_is_listed():
    text = (SPEC / "results.md").read_text(encoding="utf-8")
    for ex in ("bev-car", "hybrid-car"):
        project = load_example(ex)
        for s in simulate(project, project.cases[0].id).summary:
            listed = f"`{s.key}`" if "." not in s.key else f"`<id>.{s.key.rpartition('.')[2]}`"
            assert listed in text, f"{s.key} is not in docs/spec/results.md"


def _examples() -> list[str]:
    page = (ROOT / "docs" / "help" / "reference" / "python-api.md").read_text(encoding="utf-8")
    return re.findall(r"```python\n(.*?)```", page, re.S)


def test_the_api_page_has_at_least_ten_examples():
    assert len(_examples()) >= 10


@pytest.mark.parametrize("code", _examples(), ids=lambda c: c.split("\n")[2][:40])
def test_api_page_examples_run(code, tmp_path):
    env_path = str(ROOT / "backend")
    r = subprocess.run([sys.executable, "-c", f"import sys; sys.path.insert(0, {env_path!r})\n"
                        + code], cwd=tmp_path, capture_output=True, text=True, timeout=600)
    assert r.returncode == 0, r.stderr[-2000:]


def test_every_format_that_works_names_a_test_that_exists():
    """docs/FORMATS.md (STD-34): no format is listed as working without a
    test in CI that reads or writes it."""
    text = (ROOT / "docs" / "FORMATS.md").read_text(encoding="utf-8")
    works = text.split("## Works today", 1)[1].split("\n## ", 1)[0]
    rows = [r for r in works.splitlines() if r.startswith("| ") and not r.startswith("| Format")]
    assert len(rows) >= 5
    for row in rows:
        m = re.search(r"`([^`]+)::([^`]+)`\s*\|\s*$", row)
        assert m, f"no test named in: {row}"
        path, name = ROOT / m.group(1), m.group(2)
        assert path.is_file(), f"{m.group(1)} does not exist"
        assert name in path.read_text(encoding="utf-8"), f"{name} is not in {m.group(1)}"
