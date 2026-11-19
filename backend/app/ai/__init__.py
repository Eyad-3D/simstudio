"""LightSim's connection for AI assistants (AI-03, AI-08, AI-29, AI-30).

- :mod:`.engine` is the one place that touches the engine's own code
  (projects, runs, Data Checks, the solver). Everything else here goes
  through it, so it can switch to the automation lane's ``lightsim`` Python
  package (AI-02) without touching the tools.
- :mod:`.overview` writes the short Markdown summary of a model and its last
  run: the MCP ``lightsim_overview`` tool and the app's *Copy for AI*
  button both return it (one code path).
- :mod:`.mcp_server` is a local MCP server over stdio (no network port),
  read-only unless the user confirms (see :mod:`.access`).
- :mod:`.install` writes and removes the AI apps' MCP settings.
- ``skills/`` is the skill pack in the open Agent Skills format.
"""
