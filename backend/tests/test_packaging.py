"""Covers the behaviour the packaged desktop app depends on: a relocatable
projects folder, and serving the built UI from the API. (Examples are served
from the bundle: test_examples.py.)"""
from __future__ import annotations

import importlib

from fastapi.testclient import TestClient

from app import paths, storage


def test_projects_dir_follows_the_env_var(tmp_path, monkeypatch):
    target = tmp_path / "userdata" / "projects"
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(target))
    assert paths.projects_dir() == target
    project = storage.load_example("bev-car")
    project.id = "bev-car-copy"
    storage.save_project(project, create_only=True)
    assert (target / "bev-car-copy.json").is_file()


def test_the_built_ui_is_served_from_the_api_origin(tmp_path, monkeypatch):
    dist = tmp_path / "dist"
    (dist / "assets").mkdir(parents=True)
    (dist / "index.html").write_text("<!doctype html><title>LightSim</title>")
    (dist / "assets" / "app.js").write_text("export default 1;")
    monkeypatch.setenv("LIGHTSIM_STATIC_DIR", str(dist))

    import app.main

    client = TestClient(importlib.reload(app.main).app)
    try:
        assert "LightSim" in client.get("/").text
        assert client.get("/assets/app.js").status_code == 200
        # Mounting the SPA must not shadow the API.
        assert client.get("/api/health").status_code == 200
        assert client.get("/api/nope").status_code == 404
    finally:
        monkeypatch.undo()
        importlib.reload(app.main)
