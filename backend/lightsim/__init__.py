"""LightSim from Python: load a project, check it, run a case, read the
results. Everything runs in this process: no server, no window, nothing on
the network.

    import lightsim as ls
    r = ls.run("bev-car", case="City Cycle")   # a file path or an example id
    print(r.status, r.kpis["consumption_kwh_per_100km"])
    r.to_csv("city.csv"); r.to_mat("city.mat")

The API (docs/help/reference/python-api.md):

- :func:`run` (project, case) → :class:`Result`
- :func:`check` (project) → list of :class:`Check`
- :func:`load` (path or example id) → :class:`Project` (``get``, ``set``,
  ``add``, ``remove``, ``connect``, ``route``, ``add_case``, ``check``,
  ``run``, ``save``)
- :func:`read_run` (a stored run file) → :class:`Result`
- :func:`examples`, :func:`library`, :func:`schemas`
"""
from __future__ import annotations

from typing import Optional

from ._engine import engine
from .project import LightSimError, Project, examples, library
from .result import Channel, Check, Kpi, Message, Result, read_run
from .units import UnitError

__all__ = [
    "Channel", "Check", "Kpi", "LightSimError", "Message", "Project", "Result", "UnitError",
    "__version__", "check", "examples", "library", "load", "read_run", "run", "schemas",
]

__version__: str = engine("version").VERSION
#: The version of the API below; it changes only when a call changes in a way
#: that breaks scripts (docs/spec/CHANGELOG.md).
API_VERSION = 1


def load(source) -> Project:
    """A project from a file path, or an example by id (``"bev-car"``)."""
    return Project.load(source)


def check(project) -> list[Check]:
    """The Data Checks of a project (a path, an example id or a Project)."""
    return Project.load(project).check()


def run(project, case: Optional[str] = None, **options) -> Result:
    """Run one case of a project (a path, an example id or a Project) and
    return its Result. ``case`` is a case's id or name (the first case if
    None); ``options`` are those of :meth:`Project.run`."""
    return Project.load(project).run(case, **options)


def schemas() -> dict[str, dict]:
    """The JSON Schemas of LightSim's files (docs/spec/), by name."""
    from .spec import json_schemas

    return json_schemas()
