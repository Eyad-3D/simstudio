"""CON-13: each parameter value can say where it comes from (source, kind,
confidence on ADVISOR's 0/1/2 scale); the examples' values all do, and Data
Checks count the values still at the library default."""
from __future__ import annotations

import json
from pathlib import Path

import pytest

from app.library import library_by_id
from app.storage import load_example
from app.validation import validate_project, value_provenance

ROOT = Path(__file__).resolve().parents[1]
KINDS = {"measured", "datasheet", "estimated", "library default", "generated"}


@pytest.mark.parametrize("example", ["aero-bev", "bev-car", "hybrid-car"])
def test_every_example_value_has_a_source_kind(example):
    raw = json.loads((ROOT / "projects" / f"{example}.json").read_text(encoding="utf-8"))
    for system in raw["systems"]:
        for el in system["elements"]:
            sources = el.get("parameterSources", {})
            for key, value in el["parameterOverrides"].items():
                if isinstance(value, (bool, str)):
                    continue  # settings, text and scripts are choices, not data
                src = sources.get(key)
                assert src, f"{example}: {el['label']}.{key} has no source"
                assert src["kind"] in KINDS and src["confidence"] in (0, 1, 2)
                assert src["source"], f"{example}: {el['label']}.{key}"
            assert set(sources) <= set(el["parameterOverrides"]), el["label"]


def test_sources_survive_the_engine_and_count_in_data_checks():
    project = load_example("bev-car")
    vehicle = next(e for s in project.systems for e in s.elements if e.id == "el-vehicle")
    assert vehicle.model_dump()["parameterSources"]["mass_kg"]["kind"] == "datasheet"
    total, at_default = value_provenance(project, library_by_id())
    assert 0 < at_default < total
    # a source recorded for a default value takes it off the count
    vehicle.parameterSources["wheelbase_m"] = {"source": "a drawing", "kind": "measured", "confidence": 1}
    assert value_provenance(project, library_by_id()) == (total, at_default - 1)
    texts = [c.text for c in validate_project(project)]
    if len(texts) == 1 and texts[0].startswith("All data checks passed"):
        assert f"{at_default - 1} of the model's {total} values are still at their library default" in texts[0]
