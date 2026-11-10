"""The ``lightsim`` command a CLI-mode agent gets (``run.py`` puts it first
on the agent's PATH): LightSim's own command, refusing every path outside
the agent's task folder (``lightsim show /any/file`` would otherwise read
it, and ``--out`` write anywhere) and the ``ai`` command (AI access settings
are the user's to change, not the agent's).

    python -I guard.py <task folder> <lightsim arguments>...
"""
from __future__ import annotations

import os
import sys
from pathlib import Path


def outside(arg: str, work: Path) -> bool:
    """True when ``arg``, read as a path (also the value of ``--opt=value``
    and of ``-ovalue``), leads outside ``work``."""
    tries = [arg, arg.split("=", 1)[-1]]
    if arg.startswith("-") and not arg.startswith("--"):
        tries.append(arg[2:])
    for t in filter(None, tries):
        try:
            if not (work / os.path.expanduser(t)).resolve().is_relative_to(work):
                return True
        except (OSError, ValueError, RuntimeError):
            return True
    return False


def main(argv: list[str]) -> int:
    work, args = Path(argv[0]).resolve(), argv[1:]
    refused = [a for a in args if outside(a, work)] + [a for a in args if a == "ai"]
    if refused:
        print(f"lightsim: refused {refused[0]!r}: in this test you may use only the files in "
              f"your folder ({work}), by names relative to it, and not the 'ai' command.",
              file=sys.stderr)
        return 3
    os.chdir(work)
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "backend"))
    from lightsim.cli import main as lightsim

    return lightsim(args)


if __name__ == "__main__":  # the Script sandbox's worker imports this file again
    sys.exit(main(sys.argv[1:]))
