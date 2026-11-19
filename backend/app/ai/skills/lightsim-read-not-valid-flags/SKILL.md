---
name: lightsim-read-not-valid-flags
description: Read a LightSim run's status (success, warning, cancelled, failed) and the 'not valid' notes on its summary rows, and decide what may be reported. Use whenever a run_case or results answer has a status other than success, a notValid field, or a pass/fail limit.
license: See README.md of the LightSim skill pack
metadata:
  version: "0.3.0"
  app: LightSim 0.3.0
---

# Run status and *not valid* figures

## The four statuses

- **success**: the vehicle followed its target speed (within ±2 km/h and
  ±1 s for all but 1 % of the run, at least 2 s), covered the distance, no
  motor, engine, battery or fuel cell ran outside its table data or above
  its maximum speed for longer than that allowance, and nothing warned.
  It does **not** mean the model is realistic: it checks the trace and
  the data edges, not plausibility.
- **warning**: something warned. Most often *Cycle not followed* (the
  car could not keep up: too little power or traction) or a part outside
  its data (the message names it, how far past and for how long).
- **cancelled**: a stop cut the run short (in the app, or the MCP
  server's time limit). Its per-distance figures are *not valid*.
- **failed**: an error: the model could not be built, a script failed,
  the car covered less than 5 % of the distance, a value went NaN, or a
  map was read outside its data with its axis set to Error.

## *Not valid* notes

A summary row with a `notValid` note (in the app: *not valid: …*) is a
number the run's verdict does not stand behind, for example *Consumption
— not valid: cycle not followed*. Never report such a number on its own:
quote the note with it, or fix the cause and run again.

## Limits and pass or fail

Some rows carry a `limit` and `passed` (a battery's power limit, a Formula
Student acceleration time). Report the pass or fail with the limit.

## Where to look next

- `run_case` lists the run's warnings and errors; `explain_message` finds
  the parts a message is about and how to fix it.
- `results_query` on the named part's channels shows when and how far it
  went outside its data.
