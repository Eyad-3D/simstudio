"""The parameter window's sections (U-4.9): a part's parameters under plain
headings, the rarely used ones in a last section that starts folded. Library
data only: a wrong split is worse than none, so a small part has none."""
from __future__ import annotations

from app.library import load_library

# a part with fewer parameters than this shows them as one list
FEW = 10


def test_every_parameter_of_a_sectioned_part_is_under_one_of_its_sections():
    sectioned = [c for c in load_library() if c.sections]
    assert {c.id for c in sectioned} >= {"battery.generic", "vehicle.body", "propulsion.wheel"}
    for comp in sectioned:
        names = [s.name for s in comp.sections]
        assert len(set(names)) == len(names), comp.id
        used = [p.section for p in comp.parameters]
        missing = [p.key for p in comp.parameters if p.section not in names]
        assert not missing, f"{comp.id}: {missing} under no section of the part"
        # every heading has a parameter, and there are two at least
        assert set(used) == set(names) and len(names) >= 2, comp.id
        # only the last one may start folded, and something stays open
        assert not any(s.folded for s in comp.sections[:-1]), comp.id
        assert not comp.sections[0].folded, comp.id


def test_a_part_with_few_parameters_has_no_sections():
    for comp in load_library():
        if len(comp.parameters) < FEW:
            assert not comp.sections, f"{comp.id} has {len(comp.parameters)} parameters and sections"
        if not comp.sections:
            assert all(p.section is None for p in comp.parameters), comp.id
