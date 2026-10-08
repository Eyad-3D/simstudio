# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller build of the LightSim backend.

Produces a self-contained ``lightsim-backend`` folder (one directory, not one
file — it starts faster and electron-builder ships directories happily) that
the desktop app launches as a child process. Users never install Python.
"""
import os

from PyInstaller.utils.hooks import collect_data_files, collect_submodules

datas = [
    ("app/library/components.json", "app/library"),
    ("app/library/tracks.json", "app/library"),  # the Race Track's layouts
    ("app/library/tyres.json", "app/library"),  # the Wheel's tyre codes (MOD-48)
    ("app/library/sources.json", "app/library"),  # the data register, per run (VAL-37)
    ("app/cycles/*", "app/cycles"),  # the drive-cycle library (CON-16)
    ("app/templates/*", "app/templates"),  # the built-in vehicle templates (CON-18)
    ("projects/*.json", "projects"),  # not projects/runs/: a dev's stored runs
    ("projects/reference/*.json", "projects/reference"),  # examples' stored results (CON-15)
    ("../VERSION", "."),  # single source of truth, read by app/version.py
    # the skill pack for AI assistants, served by `lightsim-backend mcp` (AI-08)
    ("app/ai/skills", "app/ai/skills"),
]
binaries = []

# uvicorn picks its event loop and HTTP parser by name at runtime, so those
# modules are never seen by the import scanner. This list matches the
# pure-Python combination pinned in app/server.py; keep the two in step.
hiddenimports = [
    "uvicorn.logging",
    "uvicorn.loops.asyncio",
    "uvicorn.protocols.http.h11_impl",
    "uvicorn.protocols.websockets.websockets_sansio_impl",
    "uvicorn.lifespan.on",
    "uvicorn.lifespan.off",
]

# Live simulation streams over a WebSocket, so the protocol implementation and
# the websockets package behind it have to travel with the frozen build.
hiddenimports += collect_submodules("websockets")

# "lightsim-backend run …" is the lightsim command-line tool (app/server.py),
# which reaches the engine through importlib (lightsim/_engine.py): name its
# modules so the scanner keeps them all.
hiddenimports += collect_submodules("lightsim")
# `lightsim-backend mcp` (app/ai) is imported only when that command runs;
# list it so the frozen build carries it and the MCP wire types it uses.
hiddenimports += collect_submodules("app.ai") + collect_submodules("mcp_types")

# GNU Readline is GPL-3.0: on Linux the stdlib readline module would pull
# libreadline into the bundle, which a proprietary app cannot ship. Nothing
# here reads a terminal, and every importer (site, rlcompleter,
# websockets.cli) falls back when it is missing.
excludes = ["tkinter", "matplotlib", "numpy.testing", "pytest", "readline"]

# The FMU pack (STD-01, backend/requirements-fmu.txt) goes in only when the
# build asks for it: LIGHTSIM_FREEZE_FMU_PACK=1. OWNER DECISION PENDING: the
# NumPy and lxml wheels it needs carry LGPL code (NumPy's libquadmath, and
# GNU libiconv linked into lxml), which scripts/licenses/allowed.txt does
# not allow, so a build that takes the pack fails the licence check
# (scripts/licenses/clarifications.json) until the owner decides
# (docs/KNOWN-LIMITS.md). Without the switch the pack stays out even when it
# is installed here (developers install it to run the tests): app/fmu and
# the lightsim package import FMPy and pandas inside functions, which
# PyInstaller would otherwise follow. With it, only what running an FMU
# needs comes along: the FMI XML schemas FMPy checks against and its small
# logging helper library, not its GUI, web app, compiler templates or
# bundled solvers.
if os.environ.get("LIGHTSIM_FREEZE_FMU_PACK") == "1":
    try:
        import fmpy  # noqa: F401
    except ImportError:
        raise SystemExit("LIGHTSIM_FREEZE_FMU_PACK=1, but the FMU pack is not installed "
                         "(backend/requirements-fmu.txt)") from None
    datas += collect_data_files("fmpy", includes=["schema/**/*.xsd", "logging/**/*"])
    hiddenimports += ["fmpy.fmi2", "fmpy.fmi3", "fmpy.validation", "app.fmu.worker"]
    excludes += ["fmpy.gui", "fmpy.webapp", "fmpy.ssp", "fmpy.cross_check",
                 "fmpy.container_fmu", "fmpy.sundials", "fmpy.template", "jinja2",
                 "nbformat", "cmake"]
else:
    excludes += ["fmpy", "numpy", "lxml", "pandas"]

a = Analysis(
    ["run_backend.py"],
    pathex=["."],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    runtime_hooks=[],
    excludes=excludes,
    noarchive=False,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="lightsim-backend",
    debug=False,
    strip=False,
    upx=False,
    console=True,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    name="lightsim-backend",
)
