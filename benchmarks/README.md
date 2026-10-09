# LightSim benchmarks: the Stage 0 yardstick

What "correct" and "fast" mean for every stage of LightSim's new engine,
measured the same way for any engine: today's Python one (the baseline) and
the equation-based Rust one being built in `engine/`.

- **Correct**: small, physically meaningful reference problems with exact
  (analytical) answers. Each states its equations, parameters and initial
  state, derives its answer, names the quantities to compare and their
  tolerances, and balances its energy.
- **Fast**: the example cars' main cases, timed warm, as multiples of real
  time and solver steps per second, with the machine's load recorded.

The targets are in [`targets.toml`](targets.toml); the first measurement of
today's engine is in [`results/`](results/).

## Running it

From the repository root, with the backend's Python environment
(`backend/requirements-dev.txt`):

```sh
python -m benchmarks.run                    # everything, about 14 minutes on one core
python -m benchmarks.run --skip-speed       # the reference problems only (about a minute)
python -m benchmarks.run --skip-reference --repeats 3 --cases "BEV City,FS autocross"
python -m benchmarks.run --speed-from results/X.json   # re-run the problems, keep X's timings
python -m benchmarks.run --render results/X.json       # re-write X.md from X.json
python -m benchmarks.reference.verify       # the exact answers against a fine integration
cd backend && python -m pytest tests/test_reference_problems.py -q
```

`benchmarks.run` writes `results/<engine>-<date>.md` (the report) and
`.json` (every number in it). Time the engine on a quiet machine: the report
prints the load average around every case and each run's CPU share.

## The reference problems

One TOML file each in [`reference/problems/`](reference/problems/); SI
units (temperatures in °C), SOC as a fraction.

| Problem | What it checks |
|---|---|
| `elec_rc_step` | RC circuit: a 400 V source precharges a 1 mF DC link through 50 Ω. Exponential charge, the 95 % time τ ln 20, and half the source's energy lost in R. |
| `elec_rl_step` | RL circuit: a 12 V supply energises a 24 Ω, 0.6 H contactor coil. Exponential current rise and the pull-in time. |
| `batt_cc_rc` | Battery as OCV(SOC) + R0 + one RC pair at a constant 120 A. Linear OCV, so V(t), SOC(t) and every energy term are closed forms. |
| `batt_cp_rc` | The same circuit with a flat OCV at a constant 60 kW. The current rises as the RC pair charges; t(I) is an integral of a rational function (logs), inverted to the last bit. |
| `batt_voltage_limit` | Constant current until the terminal voltage hits V_min (the event time by Lambert's W), then held at V_min while the current decays (a linear 2 × 2 system). |
| `motor_dc_spinup` | Permanent-magnet DC motor with winding inductance on a 48 V step: two real eigenvalues, current peak, spin-up, 90 % time. |
| `motor_dc_spinup_l0` | The same motor with L = 0, as system-level models usually have it: one time constant J R / k². |
| `mech_inertia_coastdown` | A rotor coasting against viscous and Coulomb friction: it stops at exactly (J/c) ln(1 + c ω0 / T_c), then stays stopped. |
| `mech_clutch_lockup` | A dry clutch engaging two inertias at constant friction torque: linear slip, exact lock-up time, then locked with 16 N·m through it; the clutch's heat. |
| `mech_gear_change` | A motor at constant torque through a 12:1 then 7:1 gear to a vehicle-sized inertia: the speed jump and the kinetic energy lost at a rigid, instantaneous shift. |
| `veh_coastdown` | A 1500 kg car coasting from 108 km/h against A + C v²: speed (tan) and distance (log cos) in closed form, the half-speed time, the stop time and stopping distance. |
| `veh_constant_power` | 60 kW at the wheels from 18 km/h: v = √(v0² + 2Pt/m), distance, 0-100 km/h time. |
| `therm_lumped_mass` | A stator heated at 1.5 kW for 20 minutes, then cooling: two exponentials, the time it reaches 80 °C and the time it is back at 50 °C. |
| `therm_two_masses` | Battery cells on a cooled plate: two coupled masses, two time constants (1947 s and 103 s), the time the cells reach 50 °C. |

Every problem's energy balance (`[energy]`: sources = sinks, the stored
energy's change among the sinks) holds in the exact answer to 1e-15.

### A problem file

```toml
id = "veh_coastdown"           # = the file name
model = "vehicle_coastdown"    # the exact solution in reference/exact.py
version = 1                    # raised whenever the problem changes
domain = "vehicle"
title, summary, statement, equations, assumptions, solution   # the text
[parameters] / [initial]       # name = { value = …, unit = "…", note = "…" }
[run]                          # t_end, and output_dt: the comparison grid
[[compare]]                    # name, kind (signal | event | energy), unit,
                               # optional rtol / atol / scale; an event also
                               # names the signal and level (and direction) it is
[energy]                       # sources = [...], sinks = [...]
[checkpoints]                  # generated: the exact answer at four times, and the events
```

### How an engine is measured

`reference/compare.py`, the same for every engine:

- **signals** at the problem's output times (t > 0, skipping the instant a
  signal jumps): the largest error, the RMS error, and the largest as a
  share of the scale, which is the largest |exact value| unless the problem
  gives one (a temperature is scaled by its rise, not by its Celsius value).
  It must be within `atol + rtol × scale`.
- **events**: the engine's own event time if it reports one, else the first
  crossing of the event's signal and level in the engine's finest output,
  interpolated linearly. Within `atol + rtol × t_exact`.
- **energy terms** at t_end, as shares of the problem's energy scale (the
  largest exact term at t_end).
- **energy balance**: sources − sinks from the engine's energy terms when
  it gives them all, else the residual its own books state, as a share of
  the energy scale. A change of stored energy is taken from the engine's
  states where it has them (½ J ω², ½ m v²), so books that miss a jump
  cannot hide it.

Default tolerances (targets.toml): 1e-4 for signals, 0.1 ms + 1e-5 for
events, 1e-4 for energy terms, 1e-6 for the energy balance.

### Exact means exact

`reference/exact.py` uses the standard library only: closed forms built
from sums of tⁿ e^(λt) terms that are added, multiplied and integrated
exactly (`mathx.ExpPoly`), linear 2 × 2 systems through their eigenvalues,
Lambert's W, and integrals of rational functions by partial fractions.
Root finding (an event, an inverse) bisects to the last bit.
`reference/verify.py` integrates every problem's stated equations a second
time, written independently (SciPy's DOP853 at a tolerance of 1e-13, or
RK4 at 20 000 steps per phase without SciPy), with the energies as extra
states; every quantity agrees to better than 3e-9 of its scale. The gear
change's jump rule is checked against a dry clutch of 10¹² times the motor
torque engaging the new gear, not against the momentum rule it was derived
from.

## Adding a problem

1. Write `reference/problems/<id>.toml` (copy a similar one). State the
   equations as an engine would be given them, every assumption, and how
   the answer is derived.
2. Add its exact solution to `reference/exact.py` under a `@model` name:
   every compared signal and energy term as a function of time (energies 0
   at t = 0), every event time, and `breaks` where a signal jumps.
3. Add the same equations, integrated numerically, to `reference/verify.py`
   (`NUMERIC`), and run `python -m benchmarks.reference.verify`.
4. Run `python -m benchmarks.reference.export --checkpoints`.
5. Express it for today's engine in `engines/lightsim_py.py` (`EXPRESSIONS`),
   or say in `CANNOT` why it cannot be, then
   `python -m benchmarks.run --skip-speed --write-baseline`.
6. Run `tests/test_reference_problems.py`.

Never change a problem to suit an engine: raise its `version` when the
problem itself changes.

## Adding an engine

An engine adapter (`engines/<name>.py`) turns a problem into a run and
returns a `compare.Trace`: its output times, each problem quantity at those
times in SI units, the energy terms at t_end, optionally its own event
times and energy residual, the wall-clock time and the number of steps. The
problem files and `exact.py` are all it needs; `export.py --json` writes
every exact answer at every output time for an engine that cannot run
Python.

## Today's engine (the baseline)

`engines/lightsim_py.py` builds a LightSim project for each problem it can
express and runs it through the `lightsim` package, at its default 10 ms
solver step (and at 5 ms, for the order of convergence). What it cannot
express (RC and RL circuits, a motor's winding inductance, anything
thermal) is listed with the reason. Every project it builds goes through
LightSim's Data Checks first, as a user's would: an error in one stops the
benchmark. Starting a part spinning needs a
preamble (the E-Motor spins it up first), constant current needs a Lookup
trick, and a DC motor or a constant power is written as a full-load curve;
each expression says how it is built.

### What the first measurement showed

From [`results/lightsim-py-2026-10-08.md`](results/lightsim-py-2026-10-08.md)
(engine code as of commit 61c34e0, unchanged at 1b6ea5f):

- **Expressible**: 9 of the 14 problems. Not expressible: RC and RL
  circuits, the DC motor with winding inductance, both thermal problems.
- **Batteries are accurate**: the constant-current and constant-power
  problems meet the targets (worst signal 6e-5 of scale, energy books
  closed to 1e-13); the voltage limit is reached 1.7 ms late (target
  1.2 ms).
- **First order everywhere**: errors halve with the step (order 1.00)
  wherever the step is what limits them; events land on step boundaries
  (the coast-down rotor stops at 45.81 s instead of 45.8145 s).
- **Energy books that do not close**: a motor spinning up an inertia at
  10 ms leaves 1.3 % of the energy unaccounted for (explicit Euler books
  T·ω at the start of each step while the inertia stores T²·dt²/2J more);
  the clutch's lock-up rings for a step (its torque −100 N·m instead of
  +16 N·m) and leaves 0.2 % open.
- **A gear shift creates or destroys energy, and the books do not see
  it**: with the load on a propeller shaft, the shift keeps the motor's
  speed and the load's speed jumps from 67.5 to 115.8 rad/s; the run ends
  with 1683 kJ of kinetic energy for the 1085 kJ the motor delivered, while
  the energy books say 1086 kJ stored and close to 0.1 %. With a vehicle on
  wheels the shift keeps the vehicle's speed instead (0.8 % off the
  rigid-engagement answer) and the motor's lost kinetic energy is missing
  from the books (816 kJ booked, 805 kJ held). This is why the adapter
  takes stored kinetic energy from the engine's speeds, not its books.
- **A car never stops coasting**: the rolling resistance fades out below
  0.3 m/s, so the speed decays exponentially and never reaches 0.
- **Tyre slip and stiffness**: anything driven through a tyre carries its
  slip loss (0.17 % of the energy in the constant-power run) and, with a
  tyre stiff enough to keep that small, a 1.43 ms solver step.
- **Speed**: 55-56x real time for the BEV (about 5,600 steps/s), 32-33x for
  the hybrid, 14.5x for the FS acceleration run, 150-180x for the FS lap
  cases (quasi-steady): 18 to 70 times short of the 1000x target, and
  18,000 times short of the WLTC fast-mode target.

`baselines/lightsim-py.toml` holds twice today's measured errors;
`tests/test_reference_problems.py` fails when one grows past it. When a
change makes today's engine more accurate, regenerate it with
`python -m benchmarks.run --skip-speed --write-baseline`.

## Licences

Everything here is LightSim's own; the problems use no outside data. The
tools are the standard library, and SciPy (BSD-3-Clause, already a test
dependency of the backend) for the verification.
