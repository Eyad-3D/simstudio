"""Where the engine's modules come from.

In a source checkout the engine is the ``app`` package next to this one
(``backend/app``). The wheel (scripts/build-wheel.py) ships its own copy as
``lightsim._app``, so installing LightSim never puts a package called
``app`` on anyone's path. Every module of this package imports the engine
through :func:`engine`.
"""
from __future__ import annotations

import importlib
from types import ModuleType

try:
    from . import _app as _root  # type: ignore[attr-defined]  # the wheel
except ImportError:
    import app as _root  # a source checkout

BASE = _root.__name__


def engine(name: str) -> ModuleType:
    """The engine module ``name``, e.g. ``engine("schemas")``."""
    return importlib.import_module(f"{BASE}.{name}")
