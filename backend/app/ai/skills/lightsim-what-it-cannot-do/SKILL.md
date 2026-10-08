---
name: lightsim-what-it-cannot-do
description: What LightSim does not model and which results can be wrong today (no thermal model, longitudinal drive cycles only, one battery per bus, not validated against measurements). Use before answering any question about absolute numbers, and when a user asks whether LightSim can model something.
license: See README.md of the LightSim skill pack
metadata:
  version: "0.3.0"
  app: LightSim 0.3.0
---

# What LightSim cannot do

**Nothing in LightSim has been validated against measured vehicles yet.**
Use it to learn the workflow and to compare variants of one model with
each other. Do not present its absolute numbers (consumption, range, top
speed, acceleration) as facts about a real vehicle.

The most common limits:

- **No heat or cooling**: temperatures do not change and do not affect
  batteries, motors or engines; the Ambient part sets only the air density.
- **Forward driving only**: no reverse, no rolling back.
- **Drive cycles are longitudinal**: no cornering except in *Lap* cases,
  which are a quasi-steady-state estimate. Tyre force does not peak and
  drop.
- **A simple driver**: a PI speed follower that does not look ahead or
  shift gears (gear and clutch logic come from Script blocks).
- **Structure**: one battery or voltage source per electrical bus;
  DC-DC converters work one way; one E-Motor and one differential per
  driveline.
- **A battery has a power limit but no current limit**.
- **Signal units are not checked** (a Battery's SOC is 0-100).
- **A success checks the speed trace and the data edges, not
  plausibility.**

`references/known-limits.md` lists every known limit by section, generated
from LightSim's *Known issues and limits* page for this version. Read the
section that matches the user's question. The full page, with workarounds
and the roadmap item that fixes each one:
https://github.com/Eyad-3D/simstudio/blob/main/docs/KNOWN-LIMITS.md

When a question needs something LightSim does not model, say so plainly,
name the limit, and suggest what the user can still learn from the model.
