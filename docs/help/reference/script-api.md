# Script API

A **Script** part runs a short Python function of your own while the
simulation runs: a hybrid's control strategy, a custom recuperation rule,
signal math. This page says what the function gets and what it must give
back. The P2 Hybrid Car example's *Hybrid Control Unit* is a complete
script to learn from.

## The function

The script must define `step`:

```python
def step(t, dt, inputs, state, params):
    speed = inputs.get("speed", 0.0)          # an input port named speed
    return {"brake_cmd": 1.0 if speed > 120 else 0.0}
```

| Argument | What it holds |
|---|---|
| `t` | The simulated time, in s |
| `dt` | The time since the last call, in s: the solver step (0.01 s, or less when the case's *Step* is shorter), or the block's *Sample Time* when that is longer |
| `inputs` | A dict of the input ports' values, by port name. An input with no source reads 0 |
| `state` | A dict that keeps what you store in it from one call to the next; empty on the first call |
| `params` | A dict of the block's own parameters (its code and *Sample Time*) |

`step` returns a dict of output values by port name, numbers only. A key
that is not one of the block's output ports is ignored with a warning in
*Messages*. Return an empty dict, or nothing, to set no output at that
call.

Code outside `step` runs once, at the start of each run: put constants and
tables there.

## Ports

A new Script has no ports. In its *Properties*, under *Signal Ports*, click
**+ input** or **+ output**, then type the name in the new field. A name is
made lowercase with underscores (`Motor Speed` becomes `motor_speed`), and
that is the key in `inputs` or in the returned dict. Renaming a port
removes its links. Link the ports in *Data Bus Connections*
([how](../how-to/wire-control-signals.md)).

## When it runs

`step` is called at every solver step (every 10 ms, or at the case's *Step*
when that is shorter), after the signal sources and before the physics.
With a *Sample Time* longer than the solver step, it is called only every
*Sample Time* and its outputs hold in between. Inputs are always the latest
values.

## What a script can use

- `math`, which needs no import (`import math` also works; no other import
  does);
- `clamp(x, lo, hi)`, which limits `x` to the range lo to hi;
- `interp(table, x)` and `interp(table, x, y)`, which read a 1D table
  `{x: value}` or a 2D table `{x: {y: value}}` in a straight line between
  its points, holding the edge values outside them;
- plain Python: numbers, strings, lists, dicts, `if`, `for`, `while`,
  functions, and builtins such as `abs`, `min`, `max`, `round`, `sum`,
  `len`, `range` and `sorted`.

A script cannot open files, start programs, reach the network or define
classes, and cannot use Python's internals: names that start with two
underscores, attributes that start with one. Data Checks compile the code
(they never run it) and report what is not allowed.

## Limits

- One call must return within 2 s, or the run fails with a message that
  names the script and the time.
- An error in the script fails the run; *Messages* names the script, the
  time and the error.
- Scripts run in a separate process with a 512 MB memory cap. How much
  else is blocked depends on your system: see
  [Known issues](../../KNOWN-LIMITS.md). Open projects with scripts only
  from people you trust.
