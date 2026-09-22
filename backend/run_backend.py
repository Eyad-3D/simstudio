"""PyInstaller entry script — thin wrapper so the spec has a single target."""
from __future__ import annotations

import sys

from app.server import main

if __name__ == "__main__":
    sys.exit(main())
