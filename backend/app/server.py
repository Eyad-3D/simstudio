"""Entrypoint for the packaged backend.

The desktop shell starts this as a child process, so it binds to loopback
only, takes its port from the command line, and prints a single READY line the
shell waits for before opening a window.

Given a command first (``lightsim-backend run car.json``), it is the
``lightsim`` command-line tool instead (lightsim/cli.py): it runs the model
in this process and starts no server.
"""
from __future__ import annotations

import argparse
import sys

import uvicorn


def main(argv: list[str] | None = None) -> int:
    args_in = sys.argv[1:] if argv is None else argv
    if args_in[:1] == ["mcp"]:  # the AI assistants' connection over stdio (app/ai/cli.py)
        from .ai.cli import main as mcp_main

        return mcp_main(args_in[1:])
    if args_in and not args_in[0].startswith("-"):
        from lightsim.cli import main as cli_main  # the command-line tool

        return cli_main(args_in)
    parser = argparse.ArgumentParser(prog="lightsim-backend")
    parser.add_argument("--port", type=int, default=8000)
    parser.add_argument("--host", default="127.0.0.1")
    args = parser.parse_args(args_in)

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
