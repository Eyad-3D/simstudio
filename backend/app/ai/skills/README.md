# LightSim skill pack

Short guides that teach an AI assistant the right way to build, wire,
check and explain LightSim models. Each folder is one skill in the open
[Agent Skills](https://agentskills.io) format: a `SKILL.md` with a name
and a description, and sometimes a `references/` folder the assistant
reads only when it needs it. The same pack works in Claude, Codex, GitHub
Copilot, Gemini, Cursor and VS Code.

Pack version 0.3.0, for LightSim 0.3.0.

| Skill | Use it to |
|---|---|
| `lightsim-build-a-bev` | build a battery electric car from scratch |
| `lightsim-build-a-p2-hybrid` | build a parallel (P2) hybrid with a control script |
| `lightsim-wiring-rules` | wire parts and link signals the way the solver expects |
| `lightsim-units-and-parameters` | set values in the right units; the full component reference |
| `lightsim-write-a-script-block` | write a Script block's Python `step` function |
| `lightsim-verify-a-model` | check a model before trusting it: checks, a smoke run, targets |
| `lightsim-read-not-valid-flags` | read run statuses and *not valid* figures |
| `lightsim-what-it-cannot-do` | know LightSim's limits before you answer |
| `lightsim-explain-a-result` | explain a result to a student in plain words |

## Where the pack comes from

- Inside LightSim: the MCP server serves every file as a resource,
  `lightsim://skills/<skill>/SKILL.md`.
- In the installer, next to the engine (`backend/app/ai/skills/`).
- Two reference files are generated from the app so they never drift:
  `lightsim-units-and-parameters/references/components.md` (from the
  component library) and `lightsim-what-it-cannot-do/references/known-limits.md`
  (from `docs/KNOWN-LIMITS.md`). Regenerate them with
  `python -m app.ai.skillpack --write` in `backend/`; a test fails when
  they are out of date.

## Licence

Draft: the pack is meant to be published under CC-BY-4.0 in a public
repository, so that anyone's assistant can use it. Until the owner decides,
it ships with LightSim under LightSim's own licence (LICENSE, EULA.txt).
