# Run a cycle in quick mode

Quick mode answers "what would this cycle take?" in a fraction of a full
run's time. It runs the new engine's fast mode: the cycle's speed is
prescribed, so the car drives it exactly, and the Driver's pedal is worked
out back from the motion. The parts' states (the battery's charge, its
voltages, the temperatures) are stepped once a second. A limit the car
would have met (a motor's torque at its speed, a battery's power) is
flagged, not acted on.

## Run a case in quick mode

1. Open *Cases & Parameters* and pick a drive-cycle case.
2. Set **Run mode** to **Quick**.
3. Click **Run**. The result's first message says it ran in quick mode.

Set **Run mode** back to **Full** for the full dynamic run. The choice is
the case's own (its `mode` in the project file), so a study of the case
runs each point in quick mode too.

## What it gives

Quick mode gives the totals of the run: the summary rows that add up over
it.

- each battery's final SOC, energy delivered, energy recuperated and
  internal losses;
- the Climate Control's energy used;
- the distance driven and the simulated duration;
- the consumptions (at the battery and at the socket), MPGe and the range;
- each phase's distance and consumption;
- an engine's fuel used, the fuel consumption, CO₂ per km and the battery's
  energy change as a share of the fuel's.

Its channels are recorded once a second.

On the example cars' nine drive-cycle cases the charge used is within
0.2 % of the full run's, the energy delivered within 0.5 % and the
consumptions within 0.2 % (each phase's consumption within 0.31 %). The energy recuperated and the internal losses
come out up to 1.8 % and 1.4 % lower than the full run's (measured on
1 November 2026 by `python -m benchmarks.back_to_back`, its QUICK line).

## What it does not give

- **No peaks, minima or times at a limit.** A battery's peak power, a
  motor's time limited by its supply and the like follow the full run's
  Driver, so quick mode leaves those rows out.
- **No verdict on the speed trace.** The car drives the cycle exactly, so
  there is nothing to judge: a quick run never says *Cycle not followed*.
  It does make today's checks of what the run read: an Ambient outside the
  usual air, and a table read past its data (once a second, at its
  points) for longer than the run allows, which marks the consumptions,
  MPGe and the range not valid as a full run does. A table whose axis is
  set to *Error* read past its data stops fast mode as it stops a full
  run: the case runs in full instead (`QUICK-FAILED`, with
  `FAST-TABLE-ERROR`), and fails there as today's run does. So does an
  E-Motor's loss map whose *Torque* axis is set to *Error* and starts
  above 0 N·m, when the supply holds the motor back: a full run reads it
  at 0 N·m there (`QUICK-FAILED`). The tables of a part connected to
  nothing (a battery, a fuel cell, a motor or an engine) are not judged,
  as a full run leaves the part out.
- **No checks of the motion.** The wheels lifting off the road, an
  E-Motor with no live supply and a supply limit holding a motor back are
  not told as a full run tells them; fast mode flags a limit instead (see
  below).
- **No energy report, duty or limit band**, and no hand checks.
- **No live view.** The result arrives when the run ends; Stop and live
  edits do nothing, and a paced case (*Pacing*) is not paced.
- **A limit is flagged, not acted on.** Where the car could not have
  followed the cycle, a warning says from when to when; the totals then
  take more than the car could have given. Run the case in full to see what
  it does.

## When it runs in full instead

A case quick mode does not run runs as a full run, with a warning that
starts *Not run in quick mode* and gives a code:

| Code | Why |
|---|---|
| `QUICK-KIND` | the case drives no cycle: an acceleration or performance test, a lap, a Formula Student event |
| `QUICK-BALANCE` | a hybrid's charge-balanced case (set *Charge balance* off to run it once) |
| `QUICK-END` | the case ends at a *Distance* or after a number of *Laps*: quick mode runs the cycle's whole duration |
| `QUICK-SAMPLED` | the model has a Script, FMU or Traction Control part, which fast mode does not run, or a PID or Lookup block with a *Sample Time* longer than a full run's solver step (10 ms), whose output a full run holds between samples |
| `QUICK-CELLS` | a battery built from cells |
| `QUICK-LIMITS` | a value outside its parameter's limits (a study's point can hold one) |
| `QUICK-MISSING` | the new engine is not installed |
| `QUICK-BUILD` | the model could not be built, or a text component's file could not be read |
| `QUICK-FAILED` | the new engine's fast mode refused the case or stopped; its own codes follow (`FAST-NO-CYCLE`, `FAST-SAMPLED-CONTROL`, `FAST-TABLE-ERROR` for a table whose axis is set to Error read past its data, where a full run stops too, …) |
| `QUICK-TABLE-TIME` | a table was read past its data for a time too near what the run allows to judge at quick mode's points once a second: a full run counts it every solver step |
| `QUICK-NOT-FOUND` | the case is not in the project |
| `QUICK-ERROR` | a fault of LightSim's own: please report it |

With `LIGHTSIM_ENGINE_STRICT=1` such a case fails with its code instead.

[Known issues](../../KNOWN-LIMITS.md) lists what quick mode does not do.
