"""``python -m lightsim …`` runs the command-line tool (lightsim/cli.py)."""
import sys

from .cli import main

if __name__ == "__main__":
    # Script blocks run in a worker started with "spawn": it imports this
    # module again, so nothing may start before this guard.
    sys.exit(main())
