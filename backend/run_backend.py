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

    # A strict agent run's Script or FMU worker on Windows is this executable
    # again, inside an AppContainer (app/solver/win_sandbox.py): a frozen
    # engine has no `python -c`, so it is started with WORKER_ARG first.
    from app.solver.win_sandbox import WORKER_ARG

    if sys.argv[1:2] == [WORKER_ARG]:
        from app.solver.win_sandbox import child_main

        sys.exit(child_main(sys.argv[2:]))

    # `lightsim-backend run|export|import-table|params ...` is the command
    # line (app/dataio/cli.py); anything else starts the server
    if len(sys.argv) > 1 and sys.argv[1] in ("run", "export", "import-table", "params"):
        from app.dataio.cli import main as cli_main

        sys.exit(cli_main(sys.argv[1:]))

    from app.server import main

    sys.exit(main())
