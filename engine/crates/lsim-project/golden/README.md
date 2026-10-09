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
clutch is dry friction). Script blocks without a Sample Time keep today's
10 ms tick in the 1 ms run too, so both runs drive the same controller and
differ only in the integration.

```sh
cd backend   # of today's engine
LIGHTSIM_SCRIPT_TRUST=off python ../engine/crates/lsim-project/golden/reference.py REF_DIR
cd ../engine
LSIM_PYTHON=$(which python) cargo run -p lsim-project --example golden -- REF_DIR OUT_DIR
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
package 6's sandbox), the Traction Control block today's rule in Rust.
Not compared: today's "Energy balance residual" and "Electrical energy
balance error", which check today's integrator (the new engine's energy
books report their own closure, below).

## Summary

| case | figures inside | channels inside | figures outside | energy books: closure | lost at events, kWh | steps | events |
|---|---:|---:|---|---:|---:|---:|---:|
| aero-bev/case-hwfet | 10/10 | 63/67 | — | 1.8e-09 | 0.0000 | 91725 | 16 |
| aero-bev/case-udds | 14/14 | 63/67 | — | 4.6e-09 | 0.0000 | 195780 | 256 |
| aero-bev/case-wltc | 18/18 | 63/67 | — | 1.9e-09 | 0.0000 | 249549 | 121 |
| bev-car/case-city | 10/11 | 65/70 | energy recuperated (+1.00 × band) | 9.7e-09 | 0.0000 | 4254 | 16 |
| bev-car/case-city-live | 10/11 | 65/70 | energy recuperated (+1.00 × band) | 9.7e-09 | 0.0000 | 4254 | 16 |
| bev-car/case-wltc | 19/19 | 65/70 | — | 6.2e-09 | 0.0000 | 240968 | 121 |
| bev-car/case-wltc-hvac | 19/19 | 65/70 | — | 7.4e-10 | 0.0000 | 238215 | 121 |
| bev-car/case-wltc-summer | 20/20 | 65/70 | — | 3.5e-09 | 0.0000 | 241263 | 121 |
| bev-car/case-wltc-winter | 20/20 | 65/70 | — | 4.0e-10 | 0.0000 | 239109 | 121 |
| fs-electric/case-accel-75m | 19/19 | 65/65 | — | 4.8e-07 | 0.0000 | 1156 | 9 |
| hybrid-car/case-hwfet | 9/9 | 74/88 | — | 3.8e-08 | 0.0004 | 2526264 | 76724 |
| hybrid-car/case-mixed | 7/9 | 77/88 | battery energy delivered (−1.44 × band), recuperated (+1.84 × band) | 2.3e-08 | 0.0003 | 952522 | 56217 |
| hybrid-car/case-mixed-live | 7/9 | 77/88 | as case-mixed | 2.3e-08 | 0.0003 | 952522 | 56217 |
| hybrid-car/case-udds | 12/13 | 80/88 | battery internal losses (+3.12 × band) | 1.8e-07 | 0.0051 | 3818875 | 117288 |

194 of 201 figures are inside their bands. Every fuel, consumption, range,
distance, SOC, time and acceleration figure is inside; the seven outside
are battery energies (the triage below). The energy books close to 1e-6 of
their throughput or better in every case ("lost at events" is the kinetic
energy gear shifts change, below).

## Triage

### Intended: a gear's loss acts on the torque its gears carry

*bev-car city and city-live: energy recuperated, +1.00 × band (7.89e-5 kWh
of a 7.89e-5 kWh band, 0.11 %). Channels of every electric car: the
differential's torques and power, the final drive's power and losses.*

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

### Not a difference: an event at an output time

*hybrid-car HWFET: the clutch's losses (one output point).* The Hybrid
Control Unit shifts at t = 57 s exactly; the new engine records the point
just after the shift (the clutch slipping, 10.9 kW, as the engine behind it
keeps its speed), today's record of that point is taken before its step.
The clutch shows no losses at any other output point.

### Pending: gear shifts that keep the momentum need `reinit`

*hybrid-car mixed and mixed-live: battery energy delivered −0.14 %,
recuperated +0.18 % (bands 0.1 %); UDDS: battery internal losses +1.2 %
(band 0.38 %). Channels: the Hybrid Control Unit's gear and clutch
commands (a tick earlier or later at some shifts and engine starts), the
motor's speed, the clutch's and the tyres' slip losses, the brakes' power,
the driver's commands.*

Today's fixed engine treats a shift as a rigid, instantaneous engagement
that keeps the angular momentum of everything the gears tie together, the
vehicle's included. The new engine cannot yet: preparation drops `reinit`
(DESIGN.md 5.8), so at a ratio change the run loop re-solves the speeds
with the states held, and which speeds are states decides what jumps. The
library makes that choice physical: the wheels' speeds are the
driveline's states (the other driveline speeds are not fixed at the start,
so index reduction keeps the wheels), and an engine behind a clutch keeps
its speed while the clutch slips (as in today's engine) until its sides
meet again. What still jumps is everything between the clutch and the
wheels, the E-Motor's rotor and the gearbox's input, to the new ratio at
the wheels' speed: its kinetic-energy change is booked as "lost at events"
(5.1 Wh on UDDS, 0.3–0.4 Wh on the others). Today's engine passes that
momentum to the vehicle and the new one does not; the charge-sustaining
controller answers with slightly different motor and engine torques, and
the battery's gross energies move by amounts of the same size (UDDS:
delivered +4.5 Wh, recuperated +5.3 Wh, both inside; the losses, which
weigh the current squared, +0.26 Wh). The fuel, the SOC and the distance
are inside their bands in every hybrid case. The bands of the mixed cycle are
at their 0.1 % floor (0.21 Wh of 0.2 kWh), as today's two runs make the
same controller decisions there.

Where `reinit` is needed: in `Blocks.Gearbox` at each change of the
selected ratio, a re-initialisation of the speeds the gears tie together
that keeps their angular momentum (an impulse through the gears: for an
input side `J_in`, an output side reflecting `J_out`, and the new ratio
`r`, `w_out⁺ = (J_out·w_out⁻ + r·J_in·w_in⁻)/(J_out + r²·J_in)`), the
vehicle's mass reflected through the wheels that grip included. Because
the inertias on each side are the model's and not the gearbox's, this
needs preparation to pass the re-initialisation (or an impulse
projection of the states at the event) to the run loop. The same
re-initialisation makes the benchmarks' rotational gear change exact
(`mech_gear_change`, ignored in `tests/exact.rs` until then).

## Today's known errors and the references

The comparison was first made against today's engine before its fixes;
the fixed engine moved its own figures by: battery-electric consumption
by up to −0.08 % (energy books that close), hybrid fuel −0.1 to −0.5 %
(shifts that keep momentum, a dry clutch), the acceleration test's figures
by less than 0.04 %. Today's "Energy balance residual" fell from
0.0005–0.03 % to below 1e-9 %. Against the fixed engine no band had to be
widened for a known error of today's engine.

## Run-loop observations for work package 4

* A `when` condition made true by a sample tick's new outputs never
  fires: `iterate` evaluates the conditions' previous values after the
  tick has set the outputs. Modes are re-checked
  against their relations, so the library's friction holds its conditions
  as modes.
* Mode changes a tick makes counted towards the event-storm limit, though
  the clock schedules them: the Hybrid Control Unit switching the engine's
  throttle on and off from one tick to the next stopped the UDDS and HWFET
  runs. The run loop no longer counts what a tick or a time event changes
  at its instant, and the harness's allowance for it is gone.
* Each tick whose outputs change restarts IDA (about 30 steps per 10 ms
  tick on the hybrid: most of its run time, with the Script's round trip
  to Python). On the WLTC the error test on the iteration variables
  takes 3 times as many steps as with `suppress_algebraic_error` (the
  first 50 s of the Battery Electric Car's WLTC: 6928 steps against 2341).
