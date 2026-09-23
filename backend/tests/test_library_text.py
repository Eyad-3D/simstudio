"""The component descriptions are the app's in-place help text (library
tooltip, Properties panel), so they must not send users to ports that do not
exist."""
from __future__ import annotations

import re

from app.library import load_library

# "the Vehicle's Road Grade input", "the Driver's Target Speed input", …
PORT_REFERENCE = re.compile(
    r"\b([A-Z][\w-]*(?: [A-Z][\w-]*)*)'s ([A-Z][\w-]*(?: [A-Z][\w-]*)*) (input|output)\b"
)


def test_descriptions_name_ports_that_exist():
    by_name = {c.name: c for c in load_library()}
    wrong = []
    found = 0
    for comp in load_library():
        for owner, port, direction in PORT_REFERENCE.findall(comp.description or ""):
            found += 1
            target = by_name.get(owner)
            if target is None or not any(
                p.name == port and p.direction == direction for p in target.ports
            ):
                wrong.append(f"{comp.name}: {owner}'s {port} {direction}")
    assert found, "pattern matched nothing — the regex no longer fits the catalog"
    assert not wrong, "descriptions refer to missing ports: " + "; ".join(wrong)
