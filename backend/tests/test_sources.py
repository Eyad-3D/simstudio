"""Each run lists the data and methods it rests on, and exports them as
citations (VAL-37)."""
from __future__ import annotations

import json

from fastapi.testclient import TestClient

from app import sources
from app.main import app
from app.storage import load_example

client = TestClient(app)


def test_the_shipped_copy_is_the_register():
    """Run `python -m app.sources` from backend/ after changing the register."""
    assert json.loads(sources.CATALOGUE.read_text(encoding="utf-8")) == sources.build_catalogue()


def _ids(project_id: str, case_id: str) -> dict:
    return {s.id: s for s in sources.sources_of(load_example(project_id), case_id).sources}


def test_the_battery_electric_car_cites_its_cycle_its_fastsim_values_and_its_defaults():
    found = _ids("bev-car", "case-wltc")
    assert "DR-25" in found  # the WLTC trace
    assert "cycle-wltc-3b" in found and "fastsim" in found
    assert "Vehicle · Vehicle Mass" in found["DR-01"].usedBy
    assert found["DR-04"].confidence == 0  # library defaults of unknown source
    assert "own" not in found  # the example as shipped has no values of its own


def test_a_changed_value_is_your_own_and_unknown_defaults_are_flagged():
    project = load_example("bev-car")
    vehicle = next(e for e in project.systems[0].elements if e.id == "el-vehicle")
    vehicle.parameterOverrides["mass_kg"] = 2100
    rs = sources.sources_of(project, "case-city")
    own = next(s for s in rs.sources if s.id == "own")
    assert own.usedBy == ["Vehicle · Vehicle Mass"]
    assert rs.unknownProvenance


def test_the_hybrid_cites_epa_and_its_case_values():
    found = _ids("hybrid-car", "case-udds")
    assert {"DR-02", "DR-34", "epa-test-car-list", "cycle-udds"} <= set(found)
    assert "own" not in found  # its cases' starting charges are the example's too


def test_the_fs_example_cites_its_track_layouts_and_the_rules():
    found = _ids("fs-electric", load_example("fs-electric").cases[0].id)
    assert {"DR-38", "DR-41", "fs-rules-2026"} <= set(found)


def test_bibtex_and_csl_json_cite_lightsim_and_every_source():
    rs = sources.sources_of(load_example("bev-car"), "case-wltc")
    assert rs.bibtex.startswith("@software{lightsim,")
    assert f"version = {{{sources.VERSION}}}" in rs.bibtex
    for s in rs.sources:
        if s.kind != "own":
            assert f"{{{s.id}," in rs.bibtex
            assert any(item["id"] == s.id for item in rs.cslJson)
    assert rs.bibtex.count("{") == rs.bibtex.count("}")
    assert all({"id", "type", "title", "author"} <= set(i) for i in rs.cslJson)
    assert any("FASTSim" in c for c in rs.credits)


def test_the_engine_answers_for_a_run():
    project = load_example("bev-car")
    body = client.post("/api/sources", json={"project": project.model_dump(),
                                             "caseId": "case-wltc"}).json()
    assert body["unknownProvenance"] is True
    assert body["bibtex"].startswith("@software{lightsim")


def test_road_cars_do_not_cite_the_formula_student_rules():
    """The catalogue-wide row (DR-04) names the FS battery preset's source;
    that is no reason to cite the FS rules for a car that does not use it."""
    assert "fs-rules-2026" not in _ids("bev-car", "case-wltc")
    assert "fs-rules-2026" not in _ids("hybrid-car", "case-udds")
    project = load_example("bev-car")  # with the battery's FS preset, it does
    battery = next(e for e in project.systems[0].elements if e.componentDefId == "battery.generic")
    preset = next(p for p in sources.library_by_id()["battery.generic"].presets
                  if p.name == "Formula Student Electric")
    battery.parameterOverrides.update(preset.values)
    assert "fs-rules-2026" in {s.id for s in sources.sources_of(project, "case-wltc").sources}


def test_bibtex_typesets_in_latex_for_both_road_examples():
    """Authors are organisations, every field but the URL is escaped, so
    '2021_Cupra_Born.csv' or '95 % peak' cannot break LaTeX."""
    import re
    for project_id, case_id in (("bev-car", "case-wltc"), ("hybrid-car", "case-udds")):
        bib = sources.sources_of(load_example(project_id), case_id).bibtex
        for line in bib.splitlines():
            if not line.startswith("  url = "):
                assert not re.search(r"(?<!\\)[%_&#$]", line), line
        authors = re.findall(r"author = \{\{(.*)\}\},", bib)
        assert authors and all(len(a) <= 90 for a in authors), authors
    bev = sources.sources_of(load_example("bev-car"), "case-wltc").bibtex
    assert "author = {{National Renewable Energy Laboratory" in bev  # DR-01, FASTSim's values
    assert "2021\\_Cupra\\_Born.csv" in bev and "author = {{European Union}}" in bev  # the WLTC
    assert sources._bib_escape("a\\b ~^ {x}") == r"a\textbackslash{}b \textasciitilde{}\textasciicircum{} \{x\}"
