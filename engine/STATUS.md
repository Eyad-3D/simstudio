# New engine: status of the work packages (10 October 2026)

The six packages of DESIGN.md §16 are built on their own branches and
merged onto `engine/stage1` as they finish. The shared types in `lsim-ir`
are unified (`engine/ir-unify`, DESIGN.md §5.8); every package builds on
them.

Build setup for every package: its own target directory, and
`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=line-tables-only
CARGO_PROFILE_TEST_DEBUG=line-tables-only`, `cargo -j 2`. A target
directory shared between worktrees mixes their workspace crates silently.

## Package by package

| Package | Branch | State | Next |
|---|---|---|---|
| WP1 language, units | `engine/ir-unify` (merged) | Done: parser and printer with plain errors and units checked at parse time; Base Modelica import (13 models match exact answers); 63 malformed inputs; round trip of the library; `engine/docs/text-format.md`; unified shared types | The gear-change model in the text format, now that preparation carries `reinit` |
| WP2 preparation | `wp2/prep` (merged) | Done: index-2/3 models match exact answers to 1e-9 or better; tearing matches the brute-force minimum on 8 loops; 10⁵-equation networks prepare in 0.49–0.69 s; a test model per fault code; inverse models of stand-ins of the example cars; `reinit` as a when-assignment to a state's jump part (gear change exact to 2e-14, its loss booked to 4e-14); a start value carried from an alias to its representative (`wp4/solver`) | Index reduction through tables |
| WP3 code generation | `wp3/codegen` (work in progress) | Rewrite: coloured sparse Jacobians, parallel chunked compiles, monotone cubic 1-D/2-D tables, compiled modes, guards and initialisation | 10⁴ equations under 100 ms (≈3× over); acceptance tests; machine-code caching decision |
| WP4 solver | `wp4/solver` | Done: SUNDIALS built with only a C compiler (Linux); SUNDIALS and diffsol backends agree within 4.2·rtol; faer sparse LU; events, initialisation, energy books, accuracy check, sweeps (3.44–4.09× on 4 cores). Second round: the run-loop fixes from WP5's golden comparison, exact time events, cheap restarts, the impulse projection that keeps the momentum at a gear shift (DESIGN.md 8.2). Third round, from the review: the projection only at declared engagements, in two physical stages with exact derivatives; event iteration after it; light restarts opt-in; time crossings re-armed, set to now or at a root's instant; strict `when` conditions; alias start conflicts told. Fourth round: tyres pass no impulse (their slip after a shift integrated, checked against a fully resolved two-axle car); links judged after the rigid stage; cascades of engagements projected to their end or stopped with a named event storm. Fifth round: the energy books' stored-energy rates exact (closure back to round-off), tables read along time stop the integrator at their breakpoints, both sides of an event at an output time recorded. Sixth round: blocks ticking at one instant tick in one event; the books read the model's tables; conditions on explicit functions of time found ahead and reached exactly (the rest warned about); left limits of the changed channels only. Seventh round: iteration variables solved again between the ticks of one instant; mixed conditions checked along every step's dense output; integer powers enclosed with a proven bound, the platform's libm tested; no run without the tables it reads; books without error control flagged | An idle 10 ms block costs 19.5 % on the smallest models (target 5 %); Windows/macOS CI |
| WP5 library, import | `wp5/library` (merged) | Done: physical library and the 36 vehicle blocks; project importer; 199 of 201 golden figures inside their bands, 984 of 1038 channels; energy books closed to 1e-6 or better (1.5e-9 or better on every case) | The intended difference (a gear's loss on the torque its gears carry) puts the city cycle's recuperated energy just outside its band (1.0013 ×, converged 1.0044 ×, city and city-live) and every channel still outside; documented in `lsim-project/golden/README.md` |
| WP6 fast mode, Python | `wp6/fast-engine-python` (work in progress, on the old base) | Rosenbrock-W fast stepper; limit flags match the forward run's limit hits; engine facade | Fast mode 2.6×10⁵× real time (target 10⁶×), full dynamic 500× (target 1000×); Python API; app flag; wheels; lsim-engine's `Started` wrapper must forward every `ModelFunctions` method (it forwards seven: `eval_table`, `modes`, the table guards, the Jacobians and `init` are missing; DESIGN.md §5.8) |

## Run-loop issues from the golden comparison: resolved (`wp4/solver`)

- A `when` condition made true by a sample tick's new outputs fires at the
  tick (its value before the event is the one before the tick).
- Mode changes a tick or a time event makes at its instant are scheduled,
  not an event storm; the golden harness's allowance is gone.
- A `when` condition already true at the start: decided as Modelica does
  (nothing fires at the start; it fires once it has been false and becomes
  true again), documented and tested.
- Event iteration re-checks the conditions with the iteration variables
  solved for the new discrete values.
- A tick a few ulps before the end time is that instant (no "tout too
  close to t0"); the output grid ends at the end time exactly.
- A restart hands IDA y' in full (z' too), skips the second consistency
  solve and sizes its first step from x''. Going on with the history after
  a slight change (a light restart) is opt-in: its error accumulates over
  a long run (the review: 57 tolerance units on a 10 000-tick ramp); a tick
  whose outputs reach nothing integrated goes on exactly.
  `suppress_algebraic_error` was measured and stays off (its errors grow up
  to 8× on the exact-answer suite's DAEs).
- A condition on time alone (`time >= c`) is an exact time event at c; its
  right limit holds only while its time is still now (a timer re-armed at
  its own instant, `t_last := time`), and a root SUNDIALS reports at a
  scheduled time joins it.
- A strict `when x > 0` from x = 0 fires as x leaves zero (Modelica).
- A gear shift, declared by the gearbox as a rigid engagement, keeps the
  momentum of everything the gears tie together, and that loss is the
  gearbox's. A tyre passes no impulse (its force is bounded by its grip):
  its slip relaxes after the shift through its own law, inside its grip
  or sliding at it, integrated, passing the momentum on to the vehicle
  over that time, its loss its own (today's engine relaxes a gripping
  tyre's slip at once; a fully resolved two-axle car agrees with the new
  engine, not with that). Couplings a model declares stiff and unbounded
  still pass an impulse on, judged after the rigid stage. Nothing else
  starts a projection, what a `reinit` set stays, and a cascade of
  engagements at one instant is projected to its end or stops the run
  naming it.
- `Integrator::consistent_z` fails by default (a DAE backend cannot skip
  it silently); the run report counts inert ticks apart from light
  restarts.
- The energy books take each stored energy's rate exactly (forward-mode
  differentiation through the assignments; the model's x' on IDA): the
  finite difference before it lost precision whenever an entry of the
  state passed zero while moving, and the hybrid's books closed to only
  1.6e-6; now 1.5e-9 or better on every golden case.
- Tables read along time (a driving cycle) stop the integrator at their
  breakpoints, so a condition they drive is not stepped over while
  nothing integrated moves.
- An output time at an event records both sides (`SimResult::left_limits`
  beside the values after it, only the channels the event changed); the
  golden comparison takes the side today's engine records.
- Every sampled block due at an instant ticks in one event: an output
  point there shows the values after all of them.
- A declared stored energy or loss may read a table: the books read the
  model's own interpolants (they were NaN, and the default run stopped at
  its start), the rates through tables and time exact; a zero direction
  has a zero rate (no `inf · 0`).
- A condition on an explicit function of time (`sin(2π time / T) >
  0.95`) is not stepped over while nothing integrated moves: its sign
  changes are found ahead and reached exactly; one that also reads
  states is checked along every step's dense output and the step ends
  at a pulse root finding did not see; what cannot be searched is named
  in a warning.
- Blocks ticking at one instant read the iteration variables as the
  blocks before them left them (solved again when needed), and a
  computed input at the instant it is read.
- A model that does not give the tables its books or conditions read
  does not start; books computed without error control are flagged.
- The breakpoints of a table whose position moves are recomputed after
  a discrete change (no stale stops).
