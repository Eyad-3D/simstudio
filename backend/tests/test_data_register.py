"""The data licence register (docs/data-register.csv) must cover every bundled
dataset: each data file in the trees the app is built from, and inside the
component catalogue and the example projects each map, curve and drive/grade
profile. A file or dataset added without a row (source, licence, credit,
shipped or not, cleared or not) fails here, and so does a row whose file or
dataset no longer exists. See docs/DATA-REGISTER.md.
"""
from __future__ import annotations

import csv
import json
import os
import subprocess
import warnings
from fnmatch import fnmatch
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
REGISTER = ROOT / "docs" / "data-register.csv"
LIBRARY = "backend/app/library/components.json"

COLUMNS = [
    "id", "file", "dataset", "kind", "description", "source", "history",
    "licence", "credit", "ships_in_installer", "cleared", "notes",
]
REQUIRED = ["id", "file", "dataset", "kind", "description", "source", "licence", "credit"]
# Third-party data credited in THIRD-PARTY-NOTICES.txt (Help > Third-Party
# Notices), with the register rows that use it.
BUNDLED_DATA = ROOT / "scripts" / "licenses" / "bundled-data.json"

# Where datasets live: the engine (its package, the example projects and the
# test fixtures), the UI source and static files, and the desktop shell's
# files. Everything else in the repo (build and licence tooling such as the
# SBOM, CI workflows, docs, editor settings) is not scanned, so a tooling JSON
# needs no exemption. Data that ships from anywhere else must be added here.
DATA_ROOTS = ["backend/", "frontend/src/", "frontend/public/", "desktop/src/"]
# File types that hold data rather than code (.txt: EPA publishes its driving
# schedules as text tables, such as uddscol.txt). Data typed into source code
# (a Python or TypeScript array) is not detected; it needs a row by hand.
DATA_SUFFIXES = {
    ".json", ".csv", ".tsv", ".txt", ".yaml", ".yml", ".xlsx", ".xls", ".mat",
    ".dat", ".parquet", ".h5", ".hdf5", ".mf4", ".npy", ".npz",
}
# Files of those types inside DATA_ROOTS that are configuration, not datasets.
NOT_DATA = [
    "**/package.json", "**/package-lock.json", "**/tsconfig*.json",
    "**/requirements*.txt",
]
# What the installer carries: the engine bundle takes backend/projects and the
# catalogue (lightsim-backend.spec); the UI bundle inlines frontend/src/data
# and copies frontend/public; the shell's asar holds desktop/src.
SHIPPED = [
    "backend/projects/*", "backend/app/library/*", "frontend/src/data/*",
    "frontend/public/*", "desktop/src/*",
]

_SKIP_DIRS = {".git", "node_modules", "dist", "build", "release", "__pycache__",
              ".venv", ".pytest_cache", "test-results", "playwright-report"}


def _repo_files() -> list[str]:
    """Tracked files (what a release is built from); a plain walk without git."""
    try:
        out = subprocess.run(
            ["git", "ls-files"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout
        return out.splitlines()
    except (OSError, subprocess.CalledProcessError):
        files = []
        for dirpath, dirnames, filenames in os.walk(ROOT):
            dirnames[:] = [d for d in dirnames if d not in _SKIP_DIRS]
            rel = Path(dirpath).relative_to(ROOT)
            files += [(rel / f).as_posix() for f in filenames]
        return files


def _matches(path: str, patterns: list[str]) -> bool:
    return any(fnmatch(path, p) or fnmatch(path, p.removeprefix("**/")) for p in patterns)


def data_files() -> list[str]:
    return sorted(
        f for f in _repo_files()
        if f.startswith(tuple(DATA_ROOTS)) and Path(f).suffix.lower() in DATA_SUFFIXES
        and not _matches(f, NOT_DATA) and (ROOT / f).is_file()
    )


def _is_dataset(ptype: str | None, key: str, value) -> bool:
    """Maps, curves and profiles: the values someone could have copied."""
    return ptype in ("table1d", "table2d") or key == "profile" or isinstance(value, dict)


def embedded_datasets() -> set[tuple[str, str]]:
    """(file, "<component or element id>.<parameter>") for every map, curve and
    profile in the catalogue defaults and the example projects' overrides; a
    case's own override is "<case id>/<element id>.<parameter>"."""
    library = json.loads((ROOT / LIBRARY).read_text(encoding="utf-8"))["components"]
    types = {c["id"]: {p["key"]: p["type"] for p in c["parameters"]} for c in library}
    found = {
        (LIBRARY, f"{c['id']}.{p['key']}")
        for c in library for p in c["parameters"]
        if _is_dataset(p["type"], p["key"], p["default"])
    }
    # the tracked examples only: a developer's own files there are not shipped
    # by CI (the engine used to save projects there in development)
    for rel in data_files():
        if not fnmatch(rel, "backend/projects/*.json"):
            continue
        project = json.loads((ROOT / rel).read_text(encoding="utf-8"))
        component_of = {}
        for system in project.get("systems", []):
            for el in system["elements"]:
                component_of[el["id"]] = el["componentDefId"]
                ptypes = types.get(el["componentDefId"], {})
                for key, value in el.get("parameterOverrides", {}).items():
                    if _is_dataset(ptypes.get(key), key, value):
                        found.add((rel, f"{el['id']}.{key}"))
        # a case can carry its own drive cycle or map (parameterOverrides)
        for case in project.get("cases", []):
            for el_id, overrides in (case.get("parameterOverrides") or {}).items():
                ptypes = types.get(component_of.get(el_id), {})
                for key, value in overrides.items():
                    if _is_dataset(ptypes.get(key), key, value):
                        found.add((rel, f"{case['id']}/{el_id}.{key}"))
    return found


@pytest.fixture(scope="module")
def rows() -> list[dict[str, str]]:
    with REGISTER.open(newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        assert reader.fieldnames == COLUMNS, "register columns changed; update this test"
        return list(reader)


def test_every_row_is_complete(rows):
    for row in rows:
        missing = [c for c in REQUIRED if not row[c].strip()]
        assert not missing, f"{row['id'] or row['file']}: empty {missing}"
        assert row["ships_in_installer"] in ("yes", "no"), row["id"]
        assert row["cleared"] in ("yes", "no", "pending"), row["id"]
    ids = [r["id"] for r in rows]
    assert len(ids) == len(set(ids)), "duplicate register ids"
    keys = [(r["file"], r["dataset"]) for r in rows]
    assert len(keys) == len(set(keys)), "a dataset is registered twice"


def test_every_data_file_is_registered(rows):
    registered = {r["file"] for r in rows}
    unregistered = [f for f in data_files() if f not in registered]
    assert not unregistered, (
        f"data files with no row in docs/data-register.csv: {unregistered} "
        "(record source, licence and credit; see docs/DATA-REGISTER.md)"
    )


def test_every_map_curve_and_profile_is_registered(rows):
    registered = {(r["file"], r["dataset"]) for r in rows}
    missing = sorted(embedded_datasets() - registered)
    assert not missing, (
        f"bundled maps/curves/profiles with no row in docs/data-register.csv: {missing}"
    )


def test_no_row_points_at_something_that_is_gone(rows):
    datasets = embedded_datasets()
    for row in rows:
        assert (ROOT / row["file"]).is_file(), f"{row['id']}: {row['file']} does not exist"
        if row["dataset"] != "*":
            assert (row["file"], row["dataset"]) in datasets, (
                f"{row['id']}: {row['file']} has no dataset {row['dataset']}"
            )


def test_shipping_column_matches_the_packaging(rows):
    for row in rows:
        expected = "yes" if _matches(row["file"], SHIPPED) else "no"
        assert row["ships_in_installer"] == expected, (
            f"{row['id']}: {row['file']} ships_in_installer should be {expected}"
        )


def test_nothing_ships_that_is_not_cleared(rows):
    """'cleared' is the owner's sign-off that LightSim may ship the data. A row
    marked 'no' must not ship, and 'yes' needs a known licence. 'pending' is
    allowed until the owner has confirmed provenance; it is listed as a warning
    so the open sign-offs stay visible in every test run."""
    for row in rows:
        if row["cleared"] == "yes":
            assert not row["licence"].startswith("Unknown"), (
                f"{row['id']}: cleared, but its licence is unknown"
            )
        if row["ships_in_installer"] == "yes":
            assert row["cleared"] != "no", (
                f"{row['id']}: {row['file']} ships but is not cleared for shipping"
            )
    pending = [r["id"] for r in rows if r["ships_in_installer"] == "yes" and r["cleared"] == "pending"]
    if pending:
        warnings.warn(
            f"{len(pending)} shipped datasets await the owner's licence sign-off: "
            f"{', '.join(pending)} (docs/DATA-REGISTER.md)",
            stacklevel=1,
        )


def _needs_credit(credit: str) -> bool:
    """A credit text of its own, rather than "None ..." or "Same as DR-nn"
    (that row's credit then covers it)."""
    return not credit.startswith(("None", "Same as"))


def test_credited_data_appears_in_the_third_party_notices(rows):
    """Rule 6: data whose licence asks for credit is credited on the app's
    credits screen, Help > Third-Party Notices. scripts/third-party-notices.py
    writes that from scripts/licenses/bundled-data.json, so every shipped row
    with a credit text must be listed there, and every listed row must exist
    and carry that credit."""
    listed = {}
    for source in json.loads(BUNDLED_DATA.read_text(encoding="utf-8"))["data"]:
        for rid in source["register"]:
            listed[rid] = source["name"]
    by_id = {r["id"]: r for r in rows}
    missing = sorted(r["id"] for r in rows
                     if r["ships_in_installer"] == "yes" and _needs_credit(r["credit"])
                     and r["id"] not in listed)
    assert not missing, (f"shipped rows that need credit but are not in "
                         f"scripts/licenses/bundled-data.json: {missing}")
    for rid in listed:
        assert rid in by_id, f"bundled-data.json lists {rid}, which the register does not have"
        assert _needs_credit(by_id[rid]["credit"]), f"{rid} is listed but credits nothing"
