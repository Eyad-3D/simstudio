# New engine: status of the work packages (9 October 2026)

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
| WP2 preparation | `wp2/prep` (merged) | Done: index-2/3 models match exact answers to 1e-9 or better; tearing matches the brute-force minimum on 8 loops; 10⁵-equation networks prepare in 0.49–0.69 s; a test model per fault code; inverse models of stand-ins of the example cars; `reinit` as a when-assignment to a state's jump part (gear change exact to 2e-14, its loss booked to 4e-14) | Index reduction through tables; a start value carried from an alias to its representative |
| WP3 code generation | `wp3/codegen` (work in progress) | Rewrite: coloured sparse Jacobians, parallel chunked compiles, monotone cubic 1-D/2-D tables, compiled modes, guards and initialisation | 10⁴ equations under 100 ms (≈3× over); acceptance tests; machine-code caching decision |
| WP4 solver | `wp4/solver` (merged) | Done: SUNDIALS built with only a C compiler (Linux); SUNDIALS and diffsol backends agree within 4.2·rtol; faer sparse LU; events, initialisation, energy books, accuracy check, sweeps (3.44–4.09× on 4 cores) | Run-loop fixes from WP5 (below); exact time events; an idle 10 ms block costs 19.5 % on the smallest models (target 5 %); Windows/macOS CI |
| WP5 library, import | `wp5/library` (merged) | Done: physical library and the 36 vehicle blocks; project importer; 194 of 201 golden figures inside their bands, energy books closed to 1e-6 or better | Gear shifts with `reinit` (5 battery figures wait on it); 2 intended differences documented in `lsim-project/golden/README.md` |
| WP6 fast mode, Python | `wp6/fast-engine-python` (work in progress, on the old base) | Rosenbrock-W fast stepper; limit flags match the forward run's limit hits; engine facade | Fast mode 2.6×10⁵× real time (target 10⁶×), full dynamic 500× (target 1000×); Python API; app flag; wheels |

## Open run-loop issues (from the golden comparison)

- A `when` condition made true by a sample tick's new outputs never fires.
- Mode changes a tick makes count towards the event-storm limit.
- A `when` condition already true at the start never fires.
- IDA stops with "tout too close to t0" when a tick falls just before the
  end time.
- Each tick whose outputs change restarts IDA (about 30 steps per tick on
  the hybrid car).
- A shift at `time = c` is found by root finding (t = c + 1.4e-13), so the
  output point at exactly c shows the value before the shift.
