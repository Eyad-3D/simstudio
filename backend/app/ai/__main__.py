"""``python -m app.ai``: the same as ``lightsim-backend mcp`` (see cli.py)."""
from __future__ import annotations

import sys

from .cli import main

if __name__ == "__main__":
    sys.exit(main())
