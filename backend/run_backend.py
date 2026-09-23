"""PyInstaller entry script — thin wrapper so the spec has a single target."""
from __future__ import annotations

import multiprocessing
import sys

if __name__ == "__main__":
    # The Script sandbox starts a worker with the "spawn" start method (see
    # app/solver/sandbox.py). In the frozen one-folder build that worker is a
    # fresh copy of this executable; freeze_support() lets it run as the worker
    # instead of starting a second engine. A no-op in a normal launch and when
    # running from source.
    multiprocessing.freeze_support()

    from app.server import main

    sys.exit(main())
