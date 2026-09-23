# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller build of the LightSim backend.

Produces a self-contained ``lightsim-backend`` folder (one directory, not one
file — it starts faster and electron-builder ships directories happily) that
the desktop app launches as a child process. Users never install Python.
"""
from PyInstaller.utils.hooks import collect_submodules

datas = [
    ("app/library/components.json", "app/library"),
    ("projects/*.json", "projects"),  # not projects/runs/: a dev's stored runs
    ("../VERSION", "."),  # single source of truth, read by app/version.py
]
binaries = []

# uvicorn picks its event loop and HTTP parser by name at runtime, so those
# modules are never seen by the import scanner. This list matches the
# pure-Python combination pinned in app/server.py; keep the two in step.
hiddenimports = [
    "uvicorn.logging",
    "uvicorn.loops.asyncio",
    "uvicorn.protocols.http.h11_impl",
    "uvicorn.protocols.websockets.websockets_impl",
    "uvicorn.lifespan.on",
    "uvicorn.lifespan.off",
]

# Live simulation streams over a WebSocket, so the protocol implementation and
# the websockets package behind it have to travel with the frozen build.
hiddenimports += collect_submodules("websockets")

# GNU Readline is GPL-3.0: on Linux the stdlib readline module would pull
# libreadline into the bundle, which a proprietary app cannot ship. Nothing
# here reads a terminal, and every importer (site, rlcompleter,
# websockets.cli) falls back when it is missing.
excludes = ["tkinter", "matplotlib", "numpy.testing", "pytest", "readline"]

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
