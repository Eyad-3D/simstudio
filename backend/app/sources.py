"""Where a run's numbers come from (VAL-37).

``sources_of(project, case_id)`` lists the data and methods a run rests on:
the drive cycles it drives, the example's vehicle values and maps it uses,
the library's defaults it falls back on and the values the user typed, each
with its row of the data register (docs/data-register.csv), its licence,
the credit it asks for and how far it can be trusted:

- 0: source unknown (most library defaults today),
- 1: a known source (published data, or created for LightSim), not validated,
- 2: validated against measurements (nothing in the library yet).

It also writes them as BibTeX and CSL-JSON, with an entry to cite LightSim
itself. The register ships as ``library/sources.json``, a copy this module
makes from the CSV (``python -m app.sources``, from backend/); a test keeps
the two the same.
"""
from __future__ import annotations

import csv
import json
import re
from pathlib import Path
from typing import Optional

from pydantic import BaseModel, Field

from . import cycles
from .library import library_by_id
from .schemas import Project
from .version import VERSION

CATALOGUE = Path(__file__).parent / "library" / "sources.json"
REGISTER_CSV = Path(__file__).resolve().parents[2] / "docs" / "data-register.csv"
REGISTER_FIELDS = ("id", "file", "dataset", "kind", "description", "source", "licence",
                   "credit", "cleared")

# Published works a row's source names, cited as such (BibTeX and CSL-JSON).
REFERENCES: dict[str, dict] = {
    "fastsim": {
        "type": "software", "title": "FASTSim: Future Automotive Systems Technology Simulator",
        "author": "National Renewable Energy Laboratory (now the National Laboratory of the Rockies)",
        "url": "https://github.com/NatLabRockies/fastsim", "note": "Apache-2.0",
        "match": "FASTSim",
    },
    "epa-test-car-list": {
        "type": "dataset", "title": "Data on Cars Used for Testing Fuel Economy",
        "author": "U.S. Environmental Protection Agency",
        "url": "https://www.epa.gov/compliance-and-fuel-economy-data/data-cars-used-testing-fuel-economy",
        "match": "Test Car List",
    },
    "fs-rules-2026": {
        "type": "document", "title": "FS Rules 2026, version 1.1",
        "author": "Formula Student Germany", "url": "https://www.formulastudent.de/fsg/rules/",
        # as the register's rows and the battery preset's note name them
        "match": ("Formula Student Rules 2026", "FS Rules 2026"),
    },
}
# A dataset's citation author: the organisation its register source names
# first (the full source text goes in the note). A row LightSim made, or
# whose source is unknown, says so instead.
PUBLISHERS: tuple[tuple[str, str], ...] = (
    ("provenance unknown", "Source unknown"),
    ("created for LightSim", "LightSim authors"),
    ("LightSim's own", "LightSim authors"),
    ("Copy of", "LightSim authors"),
    ("FASTSim", REFERENCES["fastsim"]["author"]),
    ("EPA", "U.S. Environmental Protection Agency"),
    ("40 CFR", "U.S. Environmental Protection Agency"),
    ("Regulation (EU)", "European Union"),
    ("UN/ECE", "United Nations Economic Commission for Europe"),
    ("UN GTR", "United Nations Economic Commission for Europe"),
    ("Project Chrono", "Project Chrono Development Team"),
    ("Formula Student Rules", "Formula Student Germany"),
)
# the regulations behind the bundled drive cycles, by cycle id
CYCLE_REFERENCES: dict[str, dict] = {
    "wltc-3b": {"type": "legislation", "title": "Global technical regulation No. 15: Worldwide "
                "harmonized Light vehicles Test Procedure (WLTP), class 3b cycle",
                "author": "United Nations Economic Commission for Europe"},
    "udds": {"type": "legislation", "title": "Urban Dynamometer Driving Schedule (UDDS), 40 CFR "
             "Part 86, Appendix I", "author": "U.S. Environmental Protection Agency"},
    "hwfet": {"type": "legislation", "title": "Highway Fuel Economy Driving Schedule (HWFET), "
              "40 CFR Part 600, Appendix I", "author": "U.S. Environmental Protection Agency"},
}

CONFIDENCE_TEXT = {0: "source unknown", 1: "known source, not validated", 2: "validated"}


class RunSource(BaseModel):
    """One dataset or method a run rests on."""

    id: str  # a register row (DR-nn), "own" for the user's values, or a reference key
    title: str
    source: str = ""
    licence: str = ""
    credit: str = ""
    confidence: int = 1  # 0 unknown, 1 known source, 2 validated
    usedBy: list[str] = Field(default_factory=list)  # "Vehicle · Mass", "Driving Task · cycle"
    kind: str = "data"  # "data" | "method" | "own"


class RunSources(BaseModel):
    sources: list[RunSource]
    # a headline number may rest on values whose source is unknown
    unknownProvenance: bool = False
    credits: list[str] = Field(default_factory=list)  # attribution sentences required
    bibtex: str = ""
    cslJson: list[dict] = Field(default_factory=list)


# ---- the shipped copy of the register -------------------------------------------------

def build_catalogue(csv_path: Path = REGISTER_CSV) -> dict:
    with csv_path.open(encoding="utf-8", newline="") as f:
        rows = [{k: r[k] for k in REGISTER_FIELDS} for r in csv.DictReader(f)
                if r["ships_in_installer"] == "yes"]
    return {"_about": "Made by `python -m app.sources` from docs/data-register.csv: the rows "
                      "of the data that ships, for each run's Sources & credits (VAL-37). Do "
                      "not edit; edit the register and run it again.",
            "rows": rows}


def _catalogue() -> list[dict]:
    return json.loads(CATALOGUE.read_text(encoding="utf-8"))["rows"]


def short_title(description: str) -> str:
    """A register row's description up to its first colon, full stop or
    bracket: "Battery Electric Car example, rebuilt on the 2021 Cupra Born
    58 kWh"."""
    head = re.split(r"[:(]|\. ", description, maxsplit=1)[0].strip().rstrip(",;.")
    return head if len(head) <= 120 else head[:117].rstrip() + "..."


def publisher(source: str, licence: str) -> str:
    """The organisation to cite as a register row's author (PUBLISHERS):
    LightSim's authors for data LightSim made, else the one its source
    names first."""
    if licence.startswith("LightSim's own"):
        return "LightSim authors"
    text = source.lower()
    found = [(text.find(marker.lower()), who) for marker, who in PUBLISHERS
             if marker.lower() in text]
    return min(found)[1] if found else "LightSim authors"


def _matches(ref: dict) -> tuple[str, ...]:
    m = ref["match"]
    return (m,) if isinstance(m, str) else tuple(m)


def confidence(row: dict) -> int:
    if row["licence"].startswith("Unknown") or row["source"].startswith("Provenance unknown"):
        return 0
    return 1


# ---- what a project uses ------------------------------------------------------------

def _example_value(example: Project, case_id: Optional[str], el, key: str, default):
    """A parameter as the example's case of that id runs it."""
    case = next((c for c in example.cases if c.id == case_id), None)
    own = case.parameterOverrides.get(el.id, {}) if case else {}
    return own.get(key, el.parameterOverrides.get(key, default))


def _examples() -> dict[str, Project]:
    from .storage import load_example  # storage imports the solver; keep this lazy
    out = {}
    for path in sorted((Path(__file__).resolve().parents[1] / "projects").glob("*.json")):
        try:
            out[path.stem] = load_example(path.stem)
        except Exception:  # noqa: BLE001 - a broken example cites nothing
            continue
    return out


def sources_of(project: Project, case_id: Optional[str] = None) -> RunSources:
    rows = _catalogue()
    by_key = {(r["file"], r["dataset"]): r for r in rows}
    defs = library_by_id()
    examples = _examples()
    case = next((c for c in project.cases if c.id == case_id), None)
    overrides = case.parameterOverrides if case else {}
    used: dict[str, RunSource] = {}
    own: list[str] = []

    # what the cited rows (and the presets in use) say, to find the works
    # they name; not the catalogue-wide row, which names every preset's
    # source whether it is used or not
    cited_text: list[str] = []
    lib_file = "backend/app/library/components.json"

    def cite(row: dict, what: str) -> None:
        if row["id"] not in used and (row["file"], row["dataset"]) != (lib_file, "*"):
            cited_text.append(f"{row['description']} {row['source']}")
        s = used.setdefault(row["id"], RunSource(
            id=row["id"], title=short_title(row["description"]), source=row["source"], licence=row["licence"],
            credit=row["credit"], confidence=confidence(row)))
        if what not in s.usedBy:
            s.usedBy.append(what)

    for system in project.systems:
        for el in system.elements:
            cdef = defs.get(el.componentDefId)
            if cdef is None:
                continue
            values = {**el.parameterOverrides, **overrides.get(el.id, {})}
            for preset in cdef.presets:  # a preset in use: its note names its source
                if preset.values and set(preset.values) & set(values) and all(
                        values.get(k, next((p.default for p in cdef.parameters if p.key == k), None))
                        == v for k, v in preset.values.items()):
                    cited_text.append(preset.note or "")
            # the same part in the examples, the project's own example first
            twins = [(eid, e) for eid, p in sorted(examples.items(),
                                                   key=lambda kv: kv[0] != project.id)
                     for s in p.systems for e in s.elements
                     if e.id == el.id and e.componentDefId == el.componentDefId]
            for pdef in cdef.parameters:
                key = pdef.key
                what = f"{el.label} · {pdef.label}"
                value = values.get(key, pdef.default)
                if cdef.id == "signal.driving_task" and key == "cycle":
                    if value:
                        row = by_key.get((f"backend/app/cycles/{value}.csv", "*"))
                        if row:
                            cite(row, what)
                    continue
                if cdef.id == "track.lap" and key == "layout" and value != "Custom":
                    cite(by_key[("backend/app/library/tracks.json", "*")], what)
                    continue
                if cdef.id == "signal.driving_task" and key == "profile" and values.get("cycle"):
                    continue  # a cycle replaces the typed profile
                table = pdef.type in ("table1d", "table2d") or key == "profile"
                if key not in values:  # the library's default
                    row = by_key.get((lib_file, f"{cdef.id}.{key}") if table else ("", ""))
                    cite(row or by_key[(lib_file, "*")], what)
                else:
                    ex = next((eid for eid, e in twins
                               if _example_value(examples[eid], case_id, e, key, pdef.default)
                               == value), None)
                    ex_file = f"backend/projects/{ex}.json"
                    row = (by_key.get((ex_file, f"{el.id}.{key}")) if table else None) \
                        or by_key.get((ex_file, "*")) if ex else None
                    if row:
                        cite(row, what)
                    else:
                        own.append(what)

    out = sorted(used.values(), key=lambda s: int(re.sub(r"\D", "", s.id) or 0))
    if own:
        out.append(RunSource(id="own", title="Values typed into this project", kind="own",
                             source="You (or whoever made the project)", confidence=1,
                             usedBy=own))
    # the methods and regulations behind the data
    methods: list[RunSource] = []
    texts = " ".join(cited_text)
    for key, ref in REFERENCES.items():
        if any(m in texts for m in _matches(ref)):
            methods.append(RunSource(id=key, title=ref["title"], source=ref["author"],
                                     licence=ref.get("note", ""), kind="method"))
    for cycle_id, ref in CYCLE_REFERENCES.items():
        if any(s.id == cycles.CYCLES.get(cycle_id, {}).get("register") for s in out):
            methods.append(RunSource(id=f"cycle-{cycle_id}", title=ref["title"],
                                     source=ref["author"], kind="method"))
    out += methods
    credits = sorted({s.credit for s in out if s.credit and not s.credit.startswith(("None", "Same as"))})
    result = RunSources(sources=out, credits=credits,
                        unknownProvenance=any(s.confidence == 0 for s in out))
    result.bibtex = bibtex(result)
    result.cslJson = csl_json(result)
    return result


# ---- citations ------------------------------------------------------------------------

_BIB_SPECIAL = {"\\": r"\textbackslash{}", "~": r"\textasciitilde{}", "^": r"\textasciicircum{}",
                **{c: "\\" + c for c in "{}%&$#_"}}


def _bib_escape(text: str) -> str:
    """Text LaTeX typesets as written: its special characters escaped."""
    return re.sub(r"[\\~^{}%&$#_]", lambda m: _BIB_SPECIAL[m.group()], text)


def lightsim_entry() -> dict:
    return {"id": "lightsim", "type": "software", "title": "LightSim",
            "author": "LightSim authors", "version": VERSION,
            "note": "Vehicle simulation app; free for non-commercial use"}


def _entries(rs: RunSources) -> list[dict]:
    out = [lightsim_entry()]
    for s in rs.sources:
        if s.kind == "own":
            continue
        if s.kind == "method":
            ref = REFERENCES.get(s.id) or CYCLE_REFERENCES.get(s.id.removeprefix("cycle-"), {})
            out.append({"id": s.id, "type": ref.get("type", "document"), "title": s.title,
                        "author": s.source, "url": ref.get("url"), "note": ref.get("note")})
        else:
            out.append({"id": s.id, "type": "dataset", "title": s.title, "author": publisher(s.source, s.licence),
                        "note": "; ".join(x for x in (f"Source: {s.source}",
                                                       f"Licence: {s.licence}",
                                                       f"Credit: {s.credit}",
                                                       CONFIDENCE_TEXT[s.confidence]) if x)})
    return out


BIB_TYPE = {"software": "software", "dataset": "misc", "legislation": "misc", "document": "manual"}


def bibtex(rs: RunSources) -> str:
    """BibTeX entries; every field is escaped for LaTeX but the URL, which
    BibTeX styles print verbatim. An author in double braces is an
    organisation, not a list of names."""
    blocks = []
    for e in _entries(rs):
        fields = [("title", _bib_escape(e["title"])), ("author", "{" + _bib_escape(e["author"]) + "}")]
        if e.get("version"):
            fields.append(("version", _bib_escape(e["version"])))
        if e.get("url"):
            fields.append(("url", e["url"]))
        if e.get("note"):
            fields.append(("note", _bib_escape(e["note"])))
        key = re.sub(r"[^A-Za-z0-9:-]", "-", e["id"])
        body = ",\n".join(f"  {k} = {{{v}}}" for k, v in fields)
        blocks.append(f"@{BIB_TYPE[e['type']]}{{{key},\n{body}\n}}")
    return "\n\n".join(blocks) + "\n"


CSL_TYPE = {"software": "software", "dataset": "dataset", "legislation": "legislation",
            "document": "document"}


def csl_json(rs: RunSources) -> list[dict]:
    out = []
    for e in _entries(rs):
        item = {"id": e["id"], "type": CSL_TYPE[e["type"]], "title": e["title"],
                "author": [{"literal": e["author"]}]}
        for k, csl in (("version", "version"), ("url", "URL"), ("note", "note")):
            if e.get(k):
                item[csl] = e[k]
        out.append(item)
    return out


if __name__ == "__main__":
    CATALOGUE.write_text(json.dumps(build_catalogue(), indent=2, ensure_ascii=False) + "\n",
                         encoding="utf-8")
    print(f"wrote {CATALOGUE}")
