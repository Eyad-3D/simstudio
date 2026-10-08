---
name: lightsim-write-a-script-block
description: Write or fix the Python step() function of a LightSim Script block (control strategies, custom logic, signal math), within what LightSim's script sandbox allows. Use when a model needs control logic, or a run fails with a script error.
license: See README.md of the LightSim skill pack
metadata:
  version: "0.3.0"
  app: LightSim 0.3.0
---

# Write a Script block

A Script block (`signal.script`) runs a short Python function every solver
step (10 ms) or at its `sample_time_s` if that is longer.

## The function

```python
SOC_TARGET = 55.0  # code outside step runs once per run: constants, tables


def step(t, dt, inputs, state, params):
    soc = inputs.get("soc", SOC_TARGET)   # an input port named soc
    if not state:
        state["on"] = 0.0                 # state keeps values between calls
    return {"engine_on": 1.0 if soc < SOC_TARGET else 0.0}
```

- `t`: simulated time, s; `dt`: time since the last call, s.
- `inputs`: input port values by port name; an unlinked input reads 0.
- `state`: a dict kept from call to call; empty on the first call.
- `params`: the block's own parameters.
- Return a dict of output values by port name, numbers only. A key that
  is not an output port is ignored with a warning.

## Ports

A new Script has no ports. Add them with `model_edit`:
`{"op": "add_port", "element": "HCU", "name": "soc", "direction": "input"}`.
Names become lowercase with underscores; that is the key in `inputs` and
in the returned dict. Then `connect` them (`"HCU:soc"`).

## What a script can use

`math` (no import needed), `clamp(x, lo, hi)`, `interp(table, x)` and
`interp(table, x, y)` for 1D `{x: v}` and 2D tables, and plain Python:
numbers, strings, lists, dicts, `if`, `for`, `while`, functions, `abs`,
`min`, `max`, `round`, `sum`, `len`, `range`, `sorted`.

It cannot import other modules, open files, start programs, reach the
network, define classes, or use names starting with two underscores or
attributes starting with one. Data Checks compile the code (never run it)
and report what is not allowed: run `run_checks` after every change.

## Limits and good habits

- One call must return within 2 s, or the run fails; the script runs in a
  separate process with a 512 MB memory cap.
- Signal units are not checked: a Battery's `sig_soc` is in % (0-100).
- Keep switching decisions steady: minimum on/off times and hysteresis
  stop a decision from flickering every step.
- Running a project with Script blocks over MCP needs the user's
  confirmation, and on Windows the user must allow it when connecting.
- Help page: *Script API* in LightSim's Help.
