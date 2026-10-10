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
gear shift, which the gearbox declares a rigid engagement, keeps the
momentum of everything the gears tie together (the run loop's impulse
projection, DESIGN.md 8.2), and that loss is the gearbox's; the tyres,
whose forces are bounded by their grip, pass the momentum on to the
vehicle over the time their slip takes to relax, which the integrator
follows (today's engine relaxes a gripping tyre's slip at once: below).
A condition on time alone (`time >= c`) is an exact time event. Not
compared: today's "Energy
balance residual" and "Electrical energy balance error", which check
today's integrator (the new engine's energy books report their own
closure, below).

## Summary

Release build, one case after another on one machine (4 cores). "As the
gears engaged" is the kinetic energy the gear shifts' rigid engagements
took, the gearbox's, with today's gearbox "gear shifts" term at 1 ms
beside it; the tyres pass no impulse, and what their slip loses as it
relaxes after a shift is in their own slip losses (below).

| case | figures inside | channels inside | energy books: closure | gear shifts | as the gears engaged, kWh (today's gearbox term) | steps | events | run, s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| aero-bev/case-hwfet | 10/10 | 63/67 | 1.3e-10 | — | — | 82027 | 17 | 0.87 |
| aero-bev/case-udds | 14/14 | 63/67 | 1.3e-10 | — | — | 179312 | 257 | 1.96 |
| aero-bev/case-wltc | 18/18 | 63/67 | 2.5e-11 | — | — | 223735 | 122 | 2.38 |
| bev-car/case-city | 10/11 | 65/70 | 1.5e-09 | — | — | 3999 | 17 | 0.06 |
| bev-car/case-city-live | 10/11 | 65/70 | 1.5e-09 | — | — | 3999 | 17 | 0.06 |
| bev-car/case-wltc | 19/19 | 65/70 | 2.7e-10 | — | — | 215669 | 122 | 2.52 |
| bev-car/case-wltc-hvac | 19/19 | 65/70 | 5.8e-11 | — | — | 214113 | 122 | 2.48 |
| bev-car/case-wltc-summer | 20/20 | 65/70 | 7.4e-11 | — | — | 214496 | 122 | 2.48 |
| bev-car/case-wltc-winter | 20/20 | 65/70 | 2.6e-11 | — | — | 215305 | 122 | 2.86 |
| fs-electric/case-accel-75m | 19/19 | 65/65 | 1.7e-10 | — | — | 1058 | 13 | 0.09 |
| hybrid-car/case-hwfet | 9/9 | 88/88 | 3.3e-12 | 16 | 0.0002062 (0.0003367) | 1918254 | 76998 | 51.50 |
| hybrid-car/case-mixed | 9/9 | 82/88 | 1.2e-12 | 10 | 0.0001447 (0.0002622) | 572709 | 56209 | 21.41 |
| hybrid-car/case-mixed-live | 9/9 | 82/88 | 1.2e-12 | 10 | 0.0001447 (0.0002622) | 572709 | 56209 | 21.52 |
| hybrid-car/case-udds | 13/13 | 88/88 | 6.8e-12 | 104 | 0.0016634 (0.0035028) | 3023217 | 118094 | 80.89 |

199 of the 201 figures are inside their bands (194 before work package
4's second round, below), and 984 of 1038 channels (949 after its fourth
round, 952 before its second). The two figures outside are the Battery
Electric Car's recuperated energy on the city cycle (city and
city-live), at 1.0013 × band: the intended difference below, which the
converged run puts at 1.0044 × band (the 0.99996 × of earlier rounds was
that much integration error at rtol 1e-6, inside by chance). The next
figure is at 0.44 × band. Every channel outside is that same intended
difference (the differential's and the final drive's torques, power and
losses; on the hybrid's mixed cycle the gearbox's too, below). The output
points where an event happens, which put 22 of the hybrid's channels
outside after the fourth round, are compared as today's engine records
them, before its step (the run keeps both sides: below). The energy books
close to 1.5e-9 of their throughput or better in every case (the
hybrid's to about 1e-11), each stored energy's rate exact. Every figure
moved by at most 1.5e-3 × band since the fourth round, most by less than
5e-4, and none in the sixth or the seventh.

## Triage

### Intended: a gear's loss acts on the torque its gears carry

*bev-car city and city-live: energy recuperated, +1.0013 × band (7.898e-5
kWh against a 7.888e-5 kWh band, 0.11 % of the figure): just outside;
converged (rtol 1e-7, 1e-8, 1e-9: 1.0041, 1.0044, 1.0044 × band), 7.923e-5
kWh. Earlier rounds landed at 0.99996 × band, inside by an integration
error of 3.5e-7 kWh at rtol 1e-6. Channels of
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

### Resolved: an event at an output time

*After the fourth round: 22 of the hybrid's HWFET and UDDS channels
outside their bands (16 and 19 × band at most, the driven wheels' slip
losses 49 ×), earlier the clutch's and the gear commands' channels at
the same points. Now: all inside.*

The Hybrid Control Unit's 10 ms ticks fall on the output grid (1 s), and
some of its decisions land exactly on an output point: a shift at t = 57
s on the HWFET and at 699 s and 966 s on the UDDS, a clutch command at
682 s on the HWFET. An output point at an event showed only the values
just after it (the gears engaged, the clutch slipping as the engine
behind it keeps its speed, the driven tyres' slip not yet relaxed);
today's engine records that point before its step. The run now keeps
both sides of an event at an output time, as Modelica tools write two
rows at that time to their result files (`SimResult::left_limits` beside
`SimResult::values`, which still hold the value just after it), and the
comparison takes the side today's engine records.

### Intended: a tyre passes no impulse

*No compared figure leaves its band. The gearbox's "gear shifts" term:
today 0.003503 kWh on the UDDS (1 ms), the new engine's gears 0.001663
kWh; HWFET 0.000337 against 0.000206; mixed 0.000262 against 0.000145.
The rest of what today books at a shift the new engine's tyres lose in
their slip over the milliseconds after it. Channels at the output points
where a shift happens: below.*

A tyre's force is bounded by its grip (μ × its load), inside its grip as
at it, so it passes no impulse in zero time. At a shift the gears'
rigid engagement moves the wheels' speed (the gearbox's loss); the
tyres' slip then relaxes through their own law, inside the grip over
its relaxation time (0.2 to 7 ms on the hybrid) or sliding at the grip
(half of the hybrid's shifts leave the front tyres past it, 2 m/s of
slip at launch), and the integrator follows that. Today's engine relaxes
the slip of every tyre that gripped before the shift at once, keeping
its slip velocity through the impulse, and books the impulse times the
slip before the shift to the tyre (negative on a downshift while
driving). The new engine did so too until the review of its third round
showed it is not the limit of a resolved tyre. The evidence
(`lsim-solve/tests/run_loop.rs`):

* a tyre with a grip limit, F = clamp(k κ, ±4000 N), on a 7 → 12
  downshift: relaxed at once, the vehicle stayed 0.196 m/s apart from
  the run with the slip integrated 50 ms after the shift at k = 2e5,
  2e6 and 2e7 N·s/m alike (the motor at 737 against 611 rad/s); the
  tyre is far past its grip once the gears have engaged and slides
  (`a_tyre_past_its_grip_after_the_rigid_stage_passes_no_impulse`);
* a car with a motor on each axle and the library's tyre law, against
  the same car fully resolved (its gear mesh a stiff damper, nothing
  projected): as the mesh stiffens tenfold, the new engine's speeds over
  the whole transient come tenfold closer (4.7e-4 m/s at 1000
  N·m·s/rad), and so do the gear's and each tyre's losses; relaxing the
  tyres at once while inside their grip stays 0.67 m/s and 22 J apart,
  as the tyre's slip takes about 25 ms to relax there
  (`a_two_axle_shift_matches_the_fully_resolved_car`).

Today's engine has the error; its figures move little with it (at most
0.17 × band here).

### Resolved: gear shifts keep the momentum

*Before work package 4's second round: hybrid-car mixed and mixed-live,
battery energy delivered −1.44 × band, recuperated +1.84 × band; UDDS,
battery internal losses +3.12 × band. Now: +0.01, +0.08 and +0.11 × band.*

At a shift the new engine re-solved the speeds with the states held, so
everything between the clutch and the wheels jumped to the new ratio at
the wheels' speed, and the vehicle got none of that momentum. The gearbox
now declares that a change of its selected ratio is a rigid engagement,
and the run loop projects the states there (DESIGN.md 8.2): the momentum
of everything the gears tie together is kept, and an engine behind a
slipping clutch keeps its speed, as in today's engine (for two inertias
`J_in`, `J_out` and the new ratio `r`, `w_out⁺ = (J_out·w_out⁻ +
r·J_in·w_in⁻)/(J_out + r²·J_in)`); the tyres then pass the momentum on
to the vehicle as their slip relaxes (above). No `reinit` is needed, as
the inertias on each side are the model's and not the gearbox's;
nothing but a declared engagement starts a projection, and what a
`reinit` sets stays. The benchmarks' rotational gear change
(`mech_gear_change` in `tests/exact.rs`) runs to 5e-16 with its loss
exact.

## Today's known errors and the references

The comparison was first made against today's engine before its fixes;
the fixed engine moved its own figures by: battery-electric consumption
by up to −0.08 % (energy books that close), hybrid fuel −0.1 to −0.5 %
(shifts that keep momentum, a dry clutch), the acceleration test's figures
by less than 0.04 %. Today's "Energy balance residual" fell from
0.0005–0.03 % to below 1e-9 %. Against the fixed engine no band had to be
widened for a known error of today's engine.

## Run loop: work package 4's second to seventh rounds

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
  11.4 before). `suppress_algebraic_error` would take the hybrid's first
  100 s from 230 234 steps to 62 458, but the error on the exact-answer
  suite's DAEs grows up to 8×: it stays off.
* A condition on time alone is an exact time event; gear shifts keep the
  momentum (above).

The review of the second round found more, each fixed with the
reviewer's test (and more):

* Going on with the integration's history after a slight tick (a light
  restart) accumulates an error beyond the tolerance over a long run (57
  tolerance units on a 10 000-tick ramp at rtol 1e-6): it is opt-in now
  (`SolverOptions::light_restarts`). A tick whose outputs reach nothing
  the integrator integrates or watches still goes on without a restart,
  exactly.
* The projection started wherever a stored energy depended on a discrete
  value, a `reinit`'s jump included (a bouncing ball fell through the
  floor): now only at a declared engagement, leaving what a `reinit` set;
  with exact derivatives instead of finite differences; with event
  iteration going on from the moved states (a condition the jump crosses
  fires at the shift); with the loss split in two physical stages (the
  fourth round then found that a tyre passes no impulse at all: above).
* A timer re-armed at its own instant took its relation's value just
  after the old time; `time > t_last` with `t_last := time` never
  switched; a time crossing at the instant SUNDIALS reports a root was
  lost. The right limit now holds only while the crossing's time is still
  now, and what is scheduled at a root's instant joins it.
* A strict `when x > 0` from x = 0 fires as x leaves zero (Modelica).
  With it the Battery Electric Car's friction now registers a backward
  creep of 1e-5 s at the start of each cycle (one event more), which
  moves no figure.
* Alias start values that disagree are told, and the choice no longer
  depends on the order the parts are listed in.

The review of the third round found two more, each fixed with the
reviewer's test (and more):

* The settle loop (iterate, project, iterate on) stopped after 51
  projections at one instant without a word, the last ratio change
  applied but never projected (a shift logic that upshifts again
  whenever a shift leaves the motor at 100 rad/s or more: gear 53 at
  97.96 rad/s instead of gear 132 at 450.42). A cascade longer than
  `SolverOptions::max_event_iterations` now stops the run with an event
  storm naming the engagement and the conditions; raised, the limit lets
  the whole cascade run exactly.
* The tyres' stage relaxed a tyre's slip at once whenever it had gripped
  before the shift, even far past its grip after the gears engaged: a
  tyre passes no impulse at all now (above), and links a model declares
  are judged after the rigid stage.
* `Integrator::consistent_z` fails by default (a DAE backend cannot skip
  it silently), and the run report counts the ticks that reach nothing
  integrated (`inert_ticks`) apart from the opt-in light restarts.

The review of the fourth round found the energy books' closure down to
1.6e-6 (from 2.7e-8) and asked for both sides of an event at an output
time:

* The books took each stored energy's rate by a finite difference whose
  step collapsed whenever an entry of the state passed zero while moving
  (the engine's speed at each start): its round-off grows as the stored
  energy over that step, and a full fuel tank stores some 1.7e9 J. The
  error depended on the step sequence, which the fourth round's tyre
  transients had reshuffled (on the HWFET 10.5, 1.6, −0.8 and 1.9 J for
  rtol 1, 0.999, 1.001 and 0.99 × 1e-6). The rates are now exact
  (forward-mode differentiation through the assignments; on IDA the
  states' rate the model's own x', not IDA's y'): every case closes to
  1.5e-9 or better, and the runs are 5–31 % faster, the smooth
  integrand letting the steps grow.
* Those longer steps then let a car at rest step over a condition: the
  Battery Electric Car in winter stood 21 s at a start, one step spanning
  its motor's switch-on and switch-off. Tables read along time (the
  cycle's target speed) now have their breakpoints as stop times.
* An output time at an event records both sides (above).

The review of the fifth round found three older defects and asked for
two smaller changes, each fixed with a regression test:

* Two sampled blocks ticking together at an output time inside a step:
  the output point showed the state between the two ticks (one block's
  new output beside the other's old one). Every block due at an instant
  now ticks in one event.
* A declared stored energy or loss that read a table made the books NaN
  (they had no tables), and the default run, its books under error
  control, stopped at t = 0. The books read the model's own
  interpolants, the rates through them exact.
* A condition on an explicit function of time was stepped over while
  nothing integrated moved (`sin(2π time / 10) > 0.95`: x(100) = 0
  against 10.108, without a warning). Its sign changes are found ahead
  without integrating and reached exactly; a condition that mixes time
  and states stops the integrator at the extrema of its terms in time;
  what cannot be searched is said in a warning.
* A zero direction has a zero rate in the exact derivatives (`sqrt` at
  zero: no `inf · 0`); the left limits keep only the channels the event
  changed (on the hybrid's UDDS 1.33 MB instead of 4.18 MB); the
  breakpoints of a table whose position moves are recomputed after a
  discrete change, not kept as stale stops.

The review of the sixth round found one regression and asked for more:

* Blocks ticking at one instant read the iteration variables as they
  were before the blocks ahead of them changed their outputs (a block
  reading the battery's current as the motor controller changes its
  torque at the same tick). They are now solved again between the ticks
  when a block reads them; a block's computed input read through its
  chain at a tick where the integrator stopped is evaluated there.
* A condition that mixes time and states could still miss a pulse
  inside a monotone stretch of its time term (`sin(time) > x` with x
  ramping), silently: every step is now checked along the integrator's
  dense output.
* Integer powers are enclosed with a proven bound, the platform's libm
  is tested against correctly rounded references, a model that does not
  give the tables the run reads does not start, and books computed
  without error control are flagged.

Run time, the base (`engine/stage1` at 5581482) against the second round
(fd361a0), both release builds, one case after another on the same
machine:

| case | steps before | steps now | run before, s | run now, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3818875 | 3025084 | 101.4 | 85.7 |
| hybrid-car/case-hwfet | 2526264 | 1926891 | 68.5 | 52.7 |
| hybrid-car/case-mixed | 952522 | 561987 | 29.1 | 20.4 |
| bev-car/case-wltc | 240968 | 237694 | 3.68 | 3.26 |
| aero-bev/case-wltc | 249549 | 248324 | 3.23 | 3.33 |
| bev-car/case-city | 4254 | 4572 | 0.07 | 0.08 |
| fs-electric/case-accel-75m | 1156 | 1058 | 0.11 | 0.11 |

The third round (from the review) against the second, timed the same
way, alternating the two builds case by case, two runs each (the mean,
then each run):

| case | steps, second round | steps, third | run, second round, s | run, third, s | change |
|---|---:|---:|---:|---:|---:|
| hybrid-car/case-udds | 3025084 | 3031936 | 85.82 (86.23, 85.41) | 85.06 (84.59, 85.53) | −0.9 % |
| hybrid-car/case-hwfet | 1926891 | 1927283 | 54.86 (55.04, 54.68) | 53.60 (53.62, 53.58) | −2.3 % |
| hybrid-car/case-mixed | 561987 | 576470 | 23.30 (23.26, 23.33) | 22.75 (22.82, 22.68) | −2.3 % |
| bev-car/case-wltc | 237694 | 237694 | 3.54 (3.50, 3.59) | 3.65 (3.83, 3.47) | +3.0 % |
| aero-bev/case-wltc | 248324 | 248324 | 3.57 (3.52, 3.63) | 3.46 (3.66, 3.25) | −3.3 % |
| bev-car/case-city | 4572 | 4572 | 0.09 (0.10, 0.09) | 0.08 (0.08, 0.08) | −12 % |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.11 (0.11, 0.11) | 0.12 (0.12, 0.11) | +5 % |

Making light restarts opt-in costs the hybrid steps (2.6 % more on the
mixed cycle), but a tick whose outputs reach nothing integrated keeps
the step, and the projection runs only at a shift and without finite
differences: the hybrid's runs are 1–2 % faster, the electric cars'
within the runs' spread (their steps are the same).

The fourth round against the third, one run each of the full comparison
(`results.md`), one case after another:

| case | steps, third round | steps, fourth | run, third round, s | run, fourth, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3031936 | 3032825 | 83.70 | 83.86 |
| hybrid-car/case-hwfet | 1927283 | 1927517 | 53.83 | 53.36 |
| hybrid-car/case-mixed | 576470 | 577834 | 22.36 | 23.32 |
| hybrid-car/case-mixed-live | 576470 | 577834 | 21.95 | 22.15 |
| bev-car/case-wltc | 237694 | 237694 | 3.49 | 3.48 |
| aero-bev/case-wltc | 248324 | 248324 | 3.24 | 3.40 |
| bev-car/case-city | 4572 | 4572 | 0.07 | 0.07 |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.09 | 0.09 |

The tyres' transients after the hybrid's shifts, now integrated, cost
about 900 steps on the UDDS (0.03 %) and 1 400 on the mixed cycle
(0.24 %); the run times move by −1 % to +4 %, within what single runs
spread (mixed and mixed-live are the same computation: 23.32 and 22.15
s); the electric cars, which do not shift, take the same steps.

The fifth round (exact stored-energy rates, the breakpoint stops, both
sides of events) against the fourth, measured the same way:

| case | steps, fourth round | steps, fifth | run, fourth round, s | run, fifth, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3032825 | 3023217 | 83.86 | 76.53 |
| hybrid-car/case-hwfet | 1927517 | 1918254 | 53.36 | 46.70 |
| hybrid-car/case-mixed | 577834 | 572709 | 23.32 | 21.86 |
| hybrid-car/case-mixed-live | 577834 | 572709 | 22.15 | 21.12 |
| bev-car/case-wltc | 237694 | 215669 | 3.48 | 2.53 |
| aero-bev/case-wltc | 248324 | 223735 | 3.40 | 2.34 |
| bev-car/case-city | 4572 | 3999 | 0.07 | 0.09 |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.09 | 0.08 |

The exact rates replace four evaluations of every channel per step, and
the smooth integrand lets the quadratures' error control take longer
steps (the electric cars 9–10 % fewer); the breakpoint stops add a stop
per second of a driving cycle, without a restart.

The sixth round (every block due at an instant ticks in one event, the
books read the model's tables, conditions on explicit functions of time
searched ahead, left limits of the changed channels only) against the
fifth, measured the same way: every figure, channel, closure and step
count is the same to the last digit. None of the example projects has a
condition on a function of time other than a driving cycle's table read
along time, whose breakpoints are stops already (its terms are monotone
between them), no stored energy or loss that reads a table, and no two
blocks ticking together inside a step. The run times move within what
single runs spread (181.6 s in all before, 181.9 s now):

| case | steps, fifth round | steps, sixth | run, fifth round, s | run, sixth, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3023217 | 3023217 | 76.53 | 76.11 |
| hybrid-car/case-hwfet | 1918254 | 1918254 | 46.70 | 47.03 |
| hybrid-car/case-mixed | 572709 | 572709 | 21.86 | 21.28 |
| hybrid-car/case-mixed-live | 572709 | 572709 | 21.12 | 20.97 |
| bev-car/case-wltc | 215669 | 215669 | 2.53 | 2.63 |
| aero-bev/case-wltc | 223735 | 223735 | 2.34 | 2.33 |
| bev-car/case-city | 3999 | 3999 | 0.09 | 0.06 |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.08 | 0.09 |

The seventh round (iteration variables solved again between the ticks
of one instant, mixed conditions checked along every step, a run that
does not start without the tables it reads, books without error control
flagged) against the sixth: every figure, channel, closure and step
count is the same to the last digit again (the hybrid's one sampled
block ticks alone; none of the models has a mixed condition). The full
comparison took 191.1 s against 181.9 s, the hybrid's runs 4–10 % longer;
timed again alternating the two builds, two runs each, the hybrid's UDDS
took 75.74 and 75.04 s at the sixth round and 76.80 and 76.18 s at the
seventh, its HWFET 48.00 and 47.61 s against 46.90 and 47.09 s: the
difference is the machine's, within ±2 %.

| case | steps, sixth round | steps, seventh | run, sixth round, s | run, seventh, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3023217 | 3023217 | 76.11 | 80.89 |
| hybrid-car/case-hwfet | 1918254 | 1918254 | 47.03 | 51.50 |
| hybrid-car/case-mixed | 572709 | 572709 | 21.28 | 21.41 |
| hybrid-car/case-mixed-live | 572709 | 572709 | 20.97 | 21.52 |
| bev-car/case-wltc | 215669 | 215669 | 2.63 | 2.52 |
| aero-bev/case-wltc | 223735 | 223735 | 2.33 | 2.38 |
| bev-car/case-city | 3999 | 3999 | 0.06 | 0.06 |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.09 | 0.09 |

The hybrid's run includes the Script block's round trip to Python at
every 10 ms tick (137 000 on the UDDS), and most of its remaining steps
follow the tyres' slip, which settles within about 1e-4 s after each
change of torque the controller makes: the error test on the iteration
variables needs them.
