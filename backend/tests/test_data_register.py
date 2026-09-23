"""The data licence register (docs/data-register.csv) must cover every bundled
dataset: each data file in the repo, and inside the component catalogue and the
example projects each map, curve and drive/grade profile. A file or dataset
added without a row (source, licence, credit, shipped or not) fails here, and
so does a row whose file or dataset no longer exists. See docs/DATA-REGISTER.md.
"""
from __future__ import annotations

import csv
import json
import os
import subprocess
from fnmatch import fnmatch
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
REGISTER = ROOT / "docs" / "data-register.csv"
LIBRARY = "backend/app/library/components.json"

COLUMNS = [
    "id", "file", "dataset", "kind", "description", "source", "history",
    "licence", "credit", "ships_in_installer", "notes",
]
REQUIRED = ["id", "file", "dataset", "kind", "description", "source", "licence", "credit"]

# File types that hold data rather than code.
DATA_SUFFIXES = {
    ".json", ".csv", ".tsv", ".yaml", ".yml", ".xlsx", ".xls", ".mat", ".dat",
    ".parquet", ".h5", ".hdf5", ".mf4", ".npy", ".npz",
}
# Files of those types that are tooling or configuration, not datasets.
NOT_DATA = [
    "**/package.json", "**/package-lock.json", "**/tsconfig*.json", ".claude/*",
    ".github/*", "desktop/electron-builder.yml", "docs/data-register.csv",
    "frontend/e2e/a11y-baseline.json",
]
# What the installer carries: the engine bundle takes backend/projects and the
# catalogue (simstudio-backend.spec); the UI bundle inlines frontend/src/data.
SHIPPED = ["backend/projects/*", "backend/app/library/*", "frontend/src/data/*"]

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
        if Path(f).suffix.lower() in DATA_SUFFIXES and not _matches(f, NOT_DATA)
        and (ROOT / f).is_file()
    )


def _is_dataset(ptype: str | None, key: str, value) -> bool:
    """Maps, curves and profiles: the values someone could have copied."""
    return ptype in ("table1d", "table2d") or key == "profile" or isinstance(value, dict)


def embedded_datasets() -> set[tuple[str, str]]:
    """(file, "<component or element id>.<parameter>") for every map, curve and
    profile in the catalogue defaults and the example projects' overrides."""
    library = json.loads((ROOT / LIBRARY).read_text(encoding="utf-8"))["components"]
    types = {c["id"]: {p["key"]: p["type"] for p in c["parameters"]} for c in library}
    found = {
        (LIBRARY, f"{c['id']}.{p['key']}")
        for c in library for p in c["parameters"]
        if _is_dataset(p["type"], p["key"], p["default"])
    }
    for path in sorted((ROOT / "backend" / "projects").glob("*.json")):
        project = json.loads(path.read_text(encoding="utf-8"))
        rel = path.relative_to(ROOT).as_posix()
        for system in project["systems"]:
            for el in system["elements"]:
                ptypes = types.get(el["componentDefId"], {})
                for key, value in el.get("parameterOverrides", {}).items():
                    if _is_dataset(ptypes.get(key), key, value):
                        found.add((rel, f"{el['id']}.{key}"))
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
