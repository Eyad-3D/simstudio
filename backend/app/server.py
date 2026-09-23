"""Entrypoint for the packaged backend.

The desktop shell starts this as a child process, so it binds to loopback
only, takes its port from the command line, and prints a single READY line the
shell waits for before opening a window.
"""
from __future__ import annotations

import argparse
import sys

import uvicorn


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="lightsim-backend")
    parser.add_argument("--port", type=int, default=8000)
    parser.add_argument("--host", default="127.0.0.1")
    args = parser.parse_args(argv)

    from .main import app  # imported late so --help stays instant

    class _AnnouncingServer(uvicorn.Server):
        """Prints READY only once the socket is actually accepting requests."""

        async def startup(self, sockets=None):  # type: ignore[override]
            await super().startup(sockets=sockets)
            print(f"LIGHTSIM_READY http://{args.host}:{args.port}", flush=True)

    # Pinned to the pure-Python event loop and HTTP parser: the frozen build
    # ships exactly these, so it behaves the same as a source checkout. The
    # optional C accelerators buy nothing for a single local user. WebSockets
    # stay on — live simulation streams its steps over /api/simulate/run.
    config = uvicorn.Config(
        app,
        host=args.host,
        port=args.port,
        log_level="info",
        loop="asyncio",
        http="h11",
        ws="websockets",
    )
    _AnnouncingServer(config).run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
