"""The lightsim wheel (scripts/build-wheel.py, AI-02) works on its own: an
example runs from it with nothing of the repository on the path."""
from __future__ import annotations

import importlib.util
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).parent.parent.parent


def test_the_wheel_runs_an_example_on_its_own(tmp_path):
    spec = importlib.util.spec_from_file_location("build_wheel", ROOT / "scripts" / "build-wheel.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    wheel = mod.build(tmp_path / "dist")
    names = zipfile.ZipFile(wheel).namelist()
    assert "lightsim/_app/solver/core.py" in names and "lightsim/projects/bev-car.json" in names
    assert not any(n.startswith("app/") or n.endswith("_app/main.py") for n in names)
    site = tmp_path / "site"
    zipfile.ZipFile(wheel).extractall(site)
    code = ("import sys; sys.path.insert(0, sys.argv[1]); import lightsim as ls; "
            "assert ls._engine.BASE == 'lightsim._app', ls._engine.BASE; "
            "r = ls.run('bev-car', case='City Cycle'); assert r.ok, r.status; "
            "print(ls.__version__, r.kpis['distance_km'])")
    r = subprocess.run([sys.executable, "-I", "-c", code, str(site)], cwd=tmp_path,
                       capture_output=True, text=True, timeout=300)
    assert r.returncode == 0, r.stderr
    assert r.stdout.split()[0] == (ROOT / "VERSION").read_text(encoding="utf-8").strip()
