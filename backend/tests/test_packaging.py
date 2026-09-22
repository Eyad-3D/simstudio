"""Covers the behaviour the packaged desktop app depends on: a relocatable
projects folder that seeds itself, and serving the built UI from the API."""
from __future__ import annotations

import importlib

import pytest
from fastapi.testclient import TestClient

from app import paths, storage

EXAMPLES = {"bev-car", "hybrid-car"}


@pytest.fixture
def fresh_projects_dir(tmp_path, monkeypatch):
    target = tmp_path / "userdata" / "projects"
    monkeypatch.setenv("SIMSTUDIO_PROJECTS_DIR", str(target))
    storage._seeded.clear()
    yield target
    storage._seeded.clear()


def test_projects_dir_follows_the_env_var(fresh_projects_dir):
    assert paths.projects_dir() == fresh_projects_dir


def test_a_fresh_dir_is_seeded_with_the_examples(fresh_projects_dir):
    assert {p["id"] for p in storage.list_projects()} == EXAMPLES
    assert (fresh_projects_dir / "bev-car.json").is_file()


def test_saving_writes_to_the_user_dir_not_the_bundle(fresh_projects_dir):
    project = storage.load_project("bev-car")
    project.name = "Renamed"
    storage.save_project(project)

    assert storage.load_project("bev-car").name == "Renamed"
    # The bundled copy must stay pristine, or the next fresh install is wrong.
    seed = paths.SEED_PROJECTS_DIR / "bev-car.json"
    assert "Renamed" not in seed.read_text(encoding="utf-8")


def test_a_deleted_example_stays_deleted(fresh_projects_dir):
    storage.list_projects()  # seeds
    assert storage.delete_project("bev-car")
    storage._seeded.clear()  # simulate the next launch

    assert {p["id"] for p in storage.list_projects()} == EXAMPLES - {"bev-car"}


def test_the_built_ui_is_served_from_the_api_origin(tmp_path, monkeypatch):
    dist = tmp_path / "dist"
    (dist / "assets").mkdir(parents=True)
    (dist / "index.html").write_text("<!doctype html><title>SimStudio</title>")
    (dist / "assets" / "app.js").write_text("export default 1;")
    monkeypatch.setenv("SIMSTUDIO_STATIC_DIR", str(dist))

    import app.main

    client = TestClient(importlib.reload(app.main).app)
    try:
        assert "SimStudio" in client.get("/").text
        assert client.get("/assets/app.js").status_code == 200
        # Mounting the SPA must not shadow the API.
        assert client.get("/api/health").status_code == 200
        assert client.get("/api/nope").status_code == 404
    finally:
        monkeypatch.undo()
        importlib.reload(app.main)
