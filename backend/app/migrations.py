"""Upgrade project files written by older LightSim versions (PLT-07).

A project file says which version of the file format it is in
(``schemaVersion``; a file without it is version 1) and which LightSim saved
it (``savedWith``). Every load passes the file's JSON through
:func:`migrate`, which applies one step per version, in order, until the
data is in :data:`CURRENT_VERSION`. Each step is a pure function of the
parsed JSON: it changes only what that version changed and keeps every
other field, known or not, as it was.

A file from a newer LightSim (a higher ``schemaVersion`` than this build
knows) is not upgraded or rewritten: :func:`migrate` raises
:class:`NewerFileError`, and the engine opens it read-only so that a save
cannot drop what this build does not understand.

The format versions:

* **1** (0.1.0 to 0.3.0 pre-releases): one JSON file; parameter studies and
  their results tables are kept in the file (``studies``).
* **2** (0.3.0): studies leave the model file (PLT-34) and are kept with the
  project's runs; the file records ``savedWith`` and may list attached files
  (``attachments``, STD-02).

Add a step for every change to the shape of the file, with a stored fixture
file in ``tests/fixtures/migrations/`` and a test, and list the change in
docs/RELEASE-NOTES.md.
"""
from __future__ import annotations

import copy
from dataclasses import dataclass, field
from typing import Callable

#: The format this build reads and writes.
CURRENT_VERSION = 2


class NewerFileError(Exception):
    """The file was written by a LightSim that knows a newer format."""

    def __init__(self, version: int, saved_with: str | None):
        self.version = version
        self.saved_with = saved_with
        by = f"LightSim {saved_with}" if saved_with else "a newer LightSim"
        install = f"LightSim {saved_with} or newer" if saved_with else "a newer LightSim"
        super().__init__(
            f"This project was saved by {by} (file format {version}; this LightSim "
            f"reads format {CURRENT_VERSION} and older). It is open read-only so "
            f"nothing in it is lost: install {install} to edit it."
        )


@dataclass
class Migrated:
    """A project's JSON in the current format, and what the upgrade did."""

    data: dict
    #: the version the file was in (CURRENT_VERSION when nothing was done)
    from_version: int
    #: parameter studies the upgrade took out of the file, to be kept with
    #: the project's runs (version 1 files only)
    studies: list[dict] = field(default_factory=list)

    @property
    def upgraded(self) -> bool:
        return self.from_version < CURRENT_VERSION


def file_version(raw: dict) -> int:
    """The format version a project's JSON says it is in (1 when unset)."""
    version = raw.get("schemaVersion", 1)
    if isinstance(version, bool) or not isinstance(version, int) or version < 1:
        raise ValueError(f"Not a LightSim project: schemaVersion is {version!r}")
    return version


def _v1_to_v2(data: dict, out: Migrated) -> dict:
    """Studies (a sweep's definition and its results table) move out of the
    model file, so running a sweep no longer changes it (PLT-34)."""
    studies = data.pop("studies", None)
    if isinstance(studies, list):
        out.studies.extend(s for s in studies if isinstance(s, dict))
    return data


#: version → the step that upgrades a file from it to the next version
STEPS: dict[int, Callable[[dict, Migrated], dict]] = {
    1: _v1_to_v2,
}


def migrate(raw: dict) -> Migrated:
    """`raw`, a project file's parsed JSON, upgraded to :data:`CURRENT_VERSION`.

    `raw` itself is not changed. Raises :class:`NewerFileError` for a file in
    a newer format and ValueError for JSON that is not a project.
    """
    if not isinstance(raw, dict):
        raise ValueError("Not a LightSim project: the file does not hold a JSON object")
    version = file_version(raw)
    if version > CURRENT_VERSION:
        saved_with = raw.get("savedWith")
        raise NewerFileError(version, saved_with if isinstance(saved_with, str) else None)
    out = Migrated(data=copy.deepcopy(raw), from_version=version)
    while version < CURRENT_VERSION:
        out.data = STEPS[version](out.data, out)
        version += 1
        out.data["schemaVersion"] = version
    return out
