"""FMU parts (STD-01): models from other tools, run as part of a LightSim model.

An FMU (Functional Mock-up Unit) is a zip file in the FMI standard that tools
such as Simulink, Dymola or GT-SUITE export: an XML description of the model's
variables plus the model compiled to native code for one or more operating
systems. LightSim runs Co-Simulation FMUs (FMI 2.0 and 3.0) as an "FMU" signal
block whose pins are the FMU variables the user chose to expose.

The pieces:

* ``store``   where an imported FMU file is kept and how a block finds it
              (``locate`` — the one function the project-attachment store,
              STD-02, replaces later), the per-computer list of FMUs the user
              allowed to run, and the unpacked copy a run loads.
* ``info``    reads an FMU without running it (variables, FMI version, kind,
              exporting tool, which operating systems it has code for) and
              checks it with FMPy's validation, in plain words.
* ``sandbox`` / ``worker``  run the FMU's native code in a separate,
              locked-down process (never in the engine), one per FMU block.

FMPy (BSD-2-Clause) does the FMI work. It is optional: without it the engine
still runs every other model and Data Checks say how to add FMU support.
"""
from __future__ import annotations

import importlib.util


def fmpy_available() -> bool:
    """Whether FMPy (the FMU pack) is installed in this engine."""
    try:
        return importlib.util.find_spec("fmpy") is not None
    except (ImportError, ValueError):
        return False


class FmuError(Exception):
    """An FMU failed to load or step; the text is for the user."""


class FmuFileError(Exception):
    """An FMU file is missing, unreadable or not a valid FMU archive."""


#: Shown wherever FMU support is missing.
NOT_INSTALLED = ("FMU support is not installed in this copy of LightSim: the FMU pack "
                 "(FMPy) is missing.")
