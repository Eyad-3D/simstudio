# Script cookbook

Ready-to-copy recipes for a **Script** part: control logic you can paste
into its code, then change. The [Script API](script-api.md) says what
`step` gets and gives back; this page gives working examples. LightSim's
tests run every recipe on this page with the inputs its test lists, so the
code here works as shown.

To use a recipe:

1. Add a **Script** part from the library and select it.
2. In *Properties*, under *Signal Ports*, add the input and output ports
   the recipe lists (**+ input**, **+ output**, then type the name).
3. Open its code (**Edit…**), replace it with the recipe and close.
4. Link the ports in *Data Bus Connections*
   ([how](../how-to/wire-control-signals.md)), then run.

Each recipe keeps what it must remember from one call to the next in
`state`, and puts its settings in capitals at the top, where you change
them.

## Rate limiter

Limits how fast a signal may change: here a traction command may rise by
at most 2 per second (0 to full in 0.5 s) and fall at any rate.
**Ports:** input `cmd`; output `cmd_out`.

```python
RISE = 2.0   # most the output may rise per second
FALL = 1e9   # most it may fall per second (no limit)


def step(t, dt, inputs, state, params):
    target = inputs.get("cmd", 0.0)
    last = state.get("out", 0.0)
    out = clamp(target, last - FALL * dt, last + RISE * dt)
    state["out"] = out
    return {"cmd_out": out}
```

With `dt` 0.01 s and `cmd` stepping from 0 to 1, the output goes 0.02,
0.04, 0.06 and so on; when `cmd` drops back to 0 it follows at once.

## PI controller

Holds a measured value at a target, here a motor speed in 1/min, with a
command from 0 to 1. The integral stops growing while the output is at
its limit (anti-windup), so it does not overshoot when the limit lets go.
**Ports:** inputs `target`, `actual`; output `cmd`.

```python
KP = 0.002   # command per 1/min of error
KI = 0.001   # command per 1/min of error per second


def step(t, dt, inputs, state, params):
    error = inputs.get("target", 0.0) - inputs.get("actual", 0.0)
    i = state.get("i", 0.0)
    raw = KP * error + KI * (i + error * dt)
    out = clamp(raw, 0.0, 1.0)
    if out == raw:  # only integrate while the output is not limited
        i += error * dt
    state["i"] = i
    return {"cmd": out}
```

For a PI or PID with no code, the library's **PID** part does the same.

## Table lookup

Reads a value from a table, in a straight line between its points and
holding the end values outside them: here an auxiliary load in kW against
the outside temperature in °C, for a heater and air-con.
**Ports:** input `temp`; output `load_kw`.

```python
LOAD = {-10: 4.0, 0: 2.5, 15: 0.5, 25: 0.5, 35: 2.0}   # °C: kW


def step(t, dt, inputs, state, params):
    return {"load_kw": interp(LOAD, inputs.get("temp", 20.0))}
```

At 5 °C it gives 1.833 kW, a third of the way from 2.5 to 0.5; at
-20 °C it holds 4.0 kW. The library's **Lookup** part does this with no
code; use a script when you need several tables or some logic around
them.

## Switch with hysteresis

Turns something on below one level and off only above a higher one, so it
does not switch on and off at every step near a single level: here a
range extender that charges the battery from 25 % SOC until it is back
at 35 %. It is the core of a rule-based hybrid strategy; the P2 Hybrid
Car example's *Hybrid Control Unit* is a complete one.
**Ports:** input `soc`; output `engine_on`.

```python
ON_BELOW = 25.0    # % SOC
OFF_ABOVE = 35.0   # % SOC


def step(t, dt, inputs, state, params):
    soc = inputs.get("soc", 50.0)
    on = state.get("on", 0.0)
    if soc < ON_BELOW:
        on = 1.0
    elif soc > OFF_ABOVE:
        on = 0.0
    state["on"] = on
    return {"engine_on": on}
```

Between the two levels the output stays what it was: at 30 % it is on
while the engine charges the battery back up from 25 %, and off while the
battery runs down from 35 %.

## Gear-shift schedule

Picks a gear from the vehicle speed, with a gap between the up and down
shift speeds so that it does not hunt between two gears.
**Ports:** input `speed` (km/h); output `gear`.

```python
UP = (20.0, 40.0, 60.0, 80.0, 100.0)  # km/h: shift 1-2, 2-3, ... 5-6
GAP = 5.0                             # km/h: shift down this much lower


def step(t, dt, inputs, state, params):
    v = inputs.get("speed", 0.0)
    gear = state.get("gear", 1)
    if gear < len(UP) + 1 and v > UP[gear - 1]:
        gear += 1
    elif gear > 1 and v < UP[gear - 2] - GAP:
        gear -= 1
    state["gear"] = gear
    return {"gear": gear}
```

It moves one gear per call, so it climbs through the gears one step at a
time. Link `gear` to a Gearbox's *Gear Select* input.

## Regenerative braking by speed

Lets the motor brake as a generator only above walking pace, and fades
it in between 5 and 15 km/h, as many cars do. Negative commands are
braking; positive ones pass through.
**Ports:** inputs `cmd`, `speed` (km/h); output `cmd_out`.

```python
FADE = {5.0: 0.0, 15.0: 1.0}   # km/h: share of the braking the motor does


def step(t, dt, inputs, state, params):
    cmd = inputs.get("cmd", 0.0)
    if cmd < 0:
        cmd *= interp(FADE, inputs.get("speed", 0.0))
    return {"cmd_out": cmd}
```

Put it between the Driver's *Traction Command* and the E-Motor's: at
10 km/h a braking command of -0.6 becomes -0.3.

## Low-pass filter

Smooths a noisy signal with a first-order filter: the output follows the
input with a time constant `TAU`, reaching 63 % of a step after `TAU`
seconds.
**Ports:** input `x`; output `y`.

```python
TAU = 0.5   # s


def step(t, dt, inputs, state, params):
    x = inputs.get("x", 0.0)
    y = state.get("y", x)
    y += (x - y) * dt / (TAU + dt)
    state["y"] = y
    return {"y": y}
```

On its first call the output starts at the input, so it does not ramp up
from 0.

## Power limiter

Holds a motor's mechanical power under a limit by scaling its command
down, as a Formula Student car must stay under 80 kW: the command a
motor needs for a power is about the power over the most it can give at
that speed. Set `LIMIT_W` a little under the rule, because the battery's
power also covers the motor's losses.
**Ports:** inputs `cmd`, `rpm` (the E-Motor's *Shaft Speed*); output
`cmd_out`.

```python
LIMIT_W = 72000.0   # W at the shaft, under the 80 kW at the battery
T_MAX = 230.0       # N·m: the motor's full-load torque


def step(t, dt, inputs, state, params):
    cmd = inputs.get("cmd", 0.0)
    w = max(inputs.get("rpm", 0.0), 1.0) * math.pi / 30.0   # rad/s
    p = cmd * T_MAX * w
    if p > LIMIT_W:
        cmd = LIMIT_W / (T_MAX * w)
    return {"cmd_out": cmd}
```

At 5,000 1/min full command would ask 230 N·m × 523.6 rad/s = 120 kW, so
it gives 0.598. The battery's own *Output Power Limit* is the simpler
way to hold a battery to a limit; this recipe holds one motor to its own.

## Launch control

A simple traction control for a standing start: it cuts the command while
a driven wheel slips more than a target, and gives it back slowly.
Formula Student cars use one to keep the tyres at their grip limit; the
FS example has none, so its rear tyres spin at the launch.
**Ports:** inputs `cmd`, `slip` (a driven Wheel's *Longitudinal Slip*,
where 0.1 means the wheel turns 10 % faster than the car moves); output
`cmd_out`.

```python
SLIP = 0.10     # target slip, 10 %
CUT = 0.5       # share of the command kept while slipping
RECOVER = 2.0   # how fast the share comes back, per second


def step(t, dt, inputs, state, params):
    share = state.get("share", 1.0)
    if inputs.get("slip", 0.0) > SLIP:
        share = min(share, CUT)
    else:
        share = min(1.0, share + RECOVER * dt)
    state["share"] = share
    return {"cmd_out": inputs.get("cmd", 0.0) * share}
```

Put it between the Driver's *Traction Command* and the E-Motor's. At
full command and 30 % slip it gives 0.5; once the slip is back under
10 %, the command comes back by 0.02 a step and is whole again after
0.25 s.

## Timer: hold for a while

Gives 1 only once its input has been on for a set time, and drops at once
when it goes off: a debounce, so one step's reading does not decide. The
P2 Hybrid Car's script uses the same idea before it starts or stops its
engine.
**Ports:** input `ask`; output `go`.

```python
HOLD = 0.2   # s the input must stay on


def step(t, dt, inputs, state, params):
    if inputs.get("ask", 0.0) > 0.5:
        state["on_for"] = state.get("on_for", 0.0) + dt
    else:
        state["on_for"] = 0.0
    return {"go": 1.0 if state["on_for"] >= HOLD - 1e-9 else 0.0}
```

With the solver's 0.01 s step, `go` turns to 1 on the 20th call in a row
with `ask` on.
