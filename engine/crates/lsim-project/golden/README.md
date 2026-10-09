# Golden comparison (DESIGN.md 12.3)

Every time-domain case of every example project (`backend/projects/*.json`:
14 cases of 4 projects; the lap cases stay in today's lap solver) runs on
the new engine and is compared, figure by figure and channel by channel,
with today's engine. `results.md` has every table, `results.json` the same
rows as data; this page has the method, the summary and the triage of
every difference outside its band.

## Method

**References.** `golden/reference.py` runs each case on today's engine
twice: at its normal step (10 ms) and at a tenth of it (1 ms). The
references here come from today's engine with its physics fixes
(`fix/engine-physics` at ffa5bb3: gear shifts keep the driveline's
momentum, energy books close to round-off, a coasting car stops, the
clutch is dry friction), made on 9 October 2026. Script blocks without a
Sample Time keep today's 10 ms tick in the 1 ms run too, so both runs drive
the same controller and differ only in the integration.

```sh
cd backend   # of today's engine
LIGHTSIM_SCRIPT_TRUST=off python ../engine/crates/lsim-project/golden/reference.py REF_DIR
cd ../engine
LSIM_PYTHON=$(which python) cargo run --release -p lsim-project --example golden -- REF_DIR OUT_DIR
```

**Bands.** A figure's band is the larger of twice the difference between
today's two runs and a floor: 0.1 % of the value for energies (kWh, J),
0.5 % for everything else (times, distances, consumptions, SOC); the new
engine must be within the band of today's 1 ms value. A channel is
compared by the RMS of its difference to today's 1 ms run over every
output point but the first (today records it before its first step, when
the signals its blocks publish still read 0), against twice the RMS
difference between today's two runs, at least 0.5 % of the channel's RMS.

**The new engine.** Each case is imported (`lsim_project::import_case`),
prepared, compiled and run at rtol 1e-6 (atol 1e-8) on IDA with the energy
books on; the figures are computed from its channels by today's
definitions (`core.py`, `labfig.py`, `verdict.py`). A Script block runs
today's own script runner in a Python process (the stand-in for work
package 6's sandbox), the Traction Control block today's rule in Rust. A
gear shift keeps the momentum of everything the gears tie together, the
vehicle's through the tyres that grip, as today's engine does (the run
loop's impulse projection, DESIGN.md 8.2), and a condition on time alone
(`time >= c`) is an exact time event. Not compared: today's "Energy
balance residual" and "Electrical energy balance error", which check
today's integrator (the new engine's energy books report their own
closure, below).

## Summary

Release build, one case after another on one machine (4 cores). "Gear
shifts" is the kinetic energy the shifts took, as the books show it: in
the gears (today's gearbox's "gear shifts" term beside it) and in the
tyres' slip.

| case | figures inside | channels inside | energy books: closure | gear shifts | in the gears, kWh (today) | in the tyres' slip, kWh | steps | events | run, s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| aero-bev/case-hwfet | 10/10 | 63/67 | 6.2e-09 | — | — | — | 91471 | 16 | 1.24 |
| aero-bev/case-udds | 14/14 | 63/67 | 1.0e-08 | — | — | — | 198417 | 256 | 2.70 |
| aero-bev/case-wltc | 18/18 | 63/67 | 1.8e-09 | — | — | — | 248324 | 121 | 3.34 |
| bev-car/case-city | 11/11 | 65/70 | 3.1e-08 | — | — | — | 4572 | 16 | 0.07 |
| bev-car/case-city-live | 11/11 | 65/70 | 3.1e-08 | — | — | — | 4572 | 16 | 0.07 |
| bev-car/case-wltc | 19/19 | 65/70 | 3.2e-09 | — | — | — | 237694 | 121 | 3.21 |
| bev-car/case-wltc-hvac | 19/19 | 65/70 | 3.0e-09 | — | — | — | 238156 | 121 | 3.26 |
| bev-car/case-wltc-summer | 20/20 | 65/70 | 6.6e-10 | — | — | — | 236544 | 121 | 3.41 |
| bev-car/case-wltc-winter | 20/20 | 65/70 | 1.8e-09 | — | — | — | 238122 | 121 | 3.39 |
| fs-electric/case-accel-75m | 19/19 | 65/65 | 9.2e-08 | — | — | — | 1058 | 12 | 0.10 |
| hybrid-car/case-hwfet | 9/9 | 80/88 | 8.4e-08 | 16 | 0.0003367 (0.0003367) | 0.0000290 | 1926891 | 77007 | 51.56 |
| hybrid-car/case-mixed | 9/9 | 82/88 | 5.3e-09 | 10 | 0.0002622 (0.0002622) | 0.0000108 | 561987 | 56199 | 21.30 |
| hybrid-car/case-mixed-live | 9/9 | 82/88 | 5.3e-09 | 10 | 0.0002622 (0.0002622) | 0.0000108 | 561987 | 56199 | 21.96 |
| hybrid-car/case-udds | 13/13 | 83/88 | 1.3e-07 | 104 | 0.0035031 (0.0035028) | 0.0002913 | 3025084 | 118051 | 82.31 |

All 201 figures are inside their bands (194 before work package 4's second
round, below), and 971 of 1038 channels (952 before). The figure closest
to its band is the Battery Electric Car's recuperated energy on the city
cycle, at 0.99996 × band: the intended difference below, at the edge of
its band (1.0002 × before the second round's restarts and time events
moved it by 2e-8 kWh). The next is at 0.44 × band. The energy books close
to 1.3e-7 of their throughput or better in every case, and the shifts'
losses in the gears match today's to 9e-5.

## Triage

### Intended: a gear's loss acts on the torque its gears carry

*bev-car city and city-live: energy recuperated, +0.99996 × band (7.887e-5
kWh of a 7.888e-5 kWh band, 0.11 %): inside, at the edge. Channels of
every electric car and of the hybrid's mixed cycle: the differential's
torques and power, the final drive's power and losses (on the hybrid also
the gearbox's power and losses, 1.15 × band).*

Today's engine reports a differential's output torque as its sources'
torque through the gears (`torque_above` in `domains.py`: the motor's
torque × the ratios × the efficiency) and applies a gear's efficiency to
that torque; the torque that accelerates the rotor and the shafts above
the mesh is not taken out. The new engine's gears carry the torque at
their flanges, after the inertias above them, and lose their efficiency on
that. On the city cycle the difference of the two torques, divided by the
car's acceleration, is constant (13.1 N·m per m/s², within ±4 % over the 57
output points with more than 0.3 m/s²): an inertia term. The final drive
then loses 2.1e-4 kWh less over the cycle (0.016843 against 0.017050 kWh),
which is what the battery's energies move by together (delivered
−1.1e-4 kWh, recuperated +7.9e-5 kWh). The new engine's torque is the
physical one; the channels are compared as defined, so they stay outside.
On the hybrid's mixed cycle the largest differences are at the start
(−12.5 N·m on the differential while the car pulls away), the same term.

### Not a difference: an event at an output time

*hybrid-car HWFET and UDDS: the Hybrid Control Unit's gear and clutch
commands, the gearbox's gear, the motor's speed, the clutch's slip,
torque and losses.* The Hybrid Control Unit's 10 ms ticks fall on the
output grid (1 s), and some of its decisions land exactly on an output
point: a shift at t = 57 s on the HWFET and at 699 s and 966 s on the
UDDS, a clutch command at 682 s on the HWFET. The new engine records the
point just after the event (an output point at an event shows the values
after it; the clutch then slips, as the engine behind it keeps its speed:
10.9 kW at 57 s, 21.8 kW at 966 s), today's record of that point is
taken before its step. Without those points every one of these channels
is inside its band (at most 0.69 × band: the HWFET's clutch losses, where
today's are zero at every point and the band is round-off).

### Resolved: gear shifts keep the momentum

*Before work package 4's second round: hybrid-car mixed and mixed-live,
battery energy delivered −1.44 × band, recuperated +1.84 × band; UDDS,
battery internal losses +3.12 × band. Now: −0.04, +0.07 and +0.28 × band.*

At a shift the new engine re-solved the speeds with the states held, so
everything between the clutch and the wheels jumped to the new ratio at
the wheels' speed, and the vehicle got none of that momentum. The run loop
now projects the states at any event that changes how they map onto the
speeds the parts' stored energies read (DESIGN.md 8.2): the momentum of
everything the gears tie together is kept, the vehicle's mass reflected
through the tyres that grip (each keeps its slip velocity), and an engine
behind a slipping clutch keeps its speed, as in today's engine (for two
inertias `J_in`, `J_out` and the new ratio `r`, `w_out⁺ = (J_out·w_out⁻ +
r·J_in·w_in⁻)/(J_out + r²·J_in)`). No `reinit` is needed, as the inertias
on each side are the model's and not the gearbox's. The benchmarks'
rotational gear change (`mech_gear_change` in `tests/exact.rs`) runs to
6e-16 with its loss exact.

The kinetic energy a shift takes is booked where today's engine books
it: the impulse through each gripping tyre times its slip velocity to
the tyre (its slip loss), the rest to the gearbox. The two engines agree
(the table above): on the UDDS, logged shift by shift at 10 ms, today's
104 shifts take 0.003784 kWh of kinetic energy, 0.003501 kWh of it in the
gears; the new engine's 104 take 0.003794 kWh, 0.003503 kWh in the gears
(today's 1 ms run: 0.003503 kWh).

## Today's known errors and the references

The comparison was first made against today's engine before its fixes;
the fixed engine moved its own figures by: battery-electric consumption
by up to −0.08 % (energy books that close), hybrid fuel −0.1 to −0.5 %
(shifts that keep momentum, a dry clutch), the acceleration test's figures
by less than 0.04 %. Today's "Energy balance residual" fell from
0.0005–0.03 % to below 1e-9 %. Against the fixed engine no band had to be
widened for a known error of today's engine.

## Run loop: work package 4's second round

The golden comparison found these in the run loop; each is fixed on
`wp4/solver` with a regression test (`lsim-solve/tests/run_loop.rs`):

* A `when` condition made true by a sample tick's new outputs fires at
  the tick (its value before the event is the one before the tick).
* Mode changes a tick or a time event makes at its instant are scheduled,
  not an event storm: the harness's allowance (`golden::storm_allowance`)
  is gone.
* A `when` condition already true at the start fires as in Modelica:
  nothing at the start, then each time it becomes true again.
* Event iteration re-checks the conditions with the iteration variables
  solved for the new discrete values.
* A tick a few ulps before the end time is that instant (no "tout too
  close to t0").
* A restart hands IDA its derivatives in full and sizes its first step
  from the second derivative (on a sample-and-hold DAE 2.2 steps a tick,
  11.4 before); a change too small to need a restart goes on without one
  (`light restarts`). `suppress_algebraic_error` would take the hybrid's
  first 100 s from 230 234 steps to 62 458, but the error on the
  exact-answer suite's DAEs grows up to 8×: it stays off.
* A condition on time alone is an exact time event; gear shifts keep the
  momentum (above).

Run time, the base (`engine/stage1` at 5581482) against `wp4/solver`, both
release builds, one case after another on the same machine:

| case | steps before | steps now | run before, s | run now, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3818875 | 3025084 | 101.4 | 85.7 |
| hybrid-car/case-hwfet | 2526264 | 1926891 | 68.5 | 52.7 |
| hybrid-car/case-mixed | 952522 | 561987 | 29.1 | 20.4 |
| bev-car/case-wltc | 240968 | 237694 | 3.68 | 3.26 |
| aero-bev/case-wltc | 249549 | 248324 | 3.23 | 3.33 |
| bev-car/case-city | 4254 | 4572 | 0.07 | 0.08 |
| fs-electric/case-accel-75m | 1156 | 1058 | 0.11 | 0.11 |

The hybrid's run includes the Script block's round trip to Python at
every 10 ms tick (137 000 on the UDDS), and most of its remaining steps
follow the tyres' slip, which settles within about 1e-4 s after each
change of torque the controller makes: the error test on the iteration
variables needs them.
