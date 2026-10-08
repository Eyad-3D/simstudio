# New engine: status of the work packages (paused 8 October 2026)

The six packages of DESIGN.md §16 are being built in parallel on their own
branches, paused mid-way at the owner's request. Each branch is pushed; none
is merged yet. A seventh branch fixes bugs the reference problems found in
today's Python engine.

Build setup for every package: its own target directory, and
`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=line-tables-only
CARGO_PROFILE_TEST_DEBUG=line-tables-only`, `cargo -j 2`. A target
directory shared between worktrees mixes their workspace crates silently.

## Before merging anything: agree the shared types

Three packages added overlapping definitions to `lsim-ir`, each additive on
its own but not with each other:

- tables: WP1 (`f640ff0`) and WP3 (`91bcf76`) both add `TableData`,
  `Interpolation`, `Outside`, `FlatTable` and `FlatSystem::tables`;
- WP2 (`a8bc3e6`, `8b34b4b`, `b4841d0`, `c9913a7`) and WP3 (`91bcf76`) both
  add `Mode`, `InitSystem`, `PreparedModel::{modes, init, jac_pattern}`;
- WP4 reads modes as `RunInfo.modes` with `d = roots[c] > 0`.

WP1 is the custodian of `lsim-ir`: reconcile these first, then merge
WP1 → WP2 → WP3 → WP4 → WP5 → WP6.

## Package by package

| Package | Branch | State | Next |
|---|---|---|---|
| WP1 language, units | `wp1/language` | Done: parser and printer with plain errors and units checked at parse time; Base Modelica import (13 models match exact answers); 63 malformed inputs; round trip of the library; `engine/docs/text-format.md` | Report; the gear-change model once preparation carries `reinit` |
| WP2 preparation | `wp2/prep` (last commit WIP) | Index reduction works: index-2 and index-3 models (locked inertias, capacitor across a source, Cartesian pendulum) match exact answers to 1e-9 or better | 10⁵-equation networks under 1 s (1.5 s now); tearing benchmark; a test model per error code; inverse models of the example cars |
| WP3 code generation | `wp3/codegen` (last commit WIP, builds) | Rewrite: coloured sparse Jacobians, parallel chunked compiles, monotone cubic 1-D/2-D tables, compiled modes, guards and initialisation | 10⁴ equations under 100 ms (≈3× over); acceptance tests; machine-code caching decision |
| WP4 solver | `wp4/solver` (last commit WIP) | SUNDIALS built with only a C compiler (proven on Linux); SUNDIALS and diffsol backends; faer sparse LU; events, initialisation, energy books, accuracy check, sweeps; exact problems pass on both backends | Run the event tests; idle Script block < 5 % cost; sweeps ≥ 3.5× on 4 cores; Windows/macOS CI |
| WP5 library, import | `wp5/library` (last commit WIP) | Physical library and the 36 vehicle blocks; 11 of 14 exact problems pass with energy books closed to 1e-7 | Project importer; battery problems; golden comparison against today's engine |
| WP6 fast mode, Python | `wp6/fast-engine-python` (last commit WIP, check fails) | Rosenbrock-W fast stepper; limit flags match the forward run's limit hits; engine facade | Fast mode 2.6×10⁵× real time (target 10⁶×), full dynamic 500× (target 1000×) on a test car; Python API; app flag; wheels |
| Today's engine fixes | `fix/engine-physics` (last commit WIP) | Gear shifts conserve momentum and book their loss; energy books at full precision | Verify: mean-speed energy booking, exact coast-down stop, dry clutch, warnings; regenerate goldens |

Known risks raised by the packages: preparation drops `reinit`; IDA stops
with "root found at and very near t" when a condition stays at zero after
its event; the first preparation stage rejects relations outside `noEvent`;
`Battery.OcvR0Rc` parameters `r0`, `r1`, `c1` clash with its part names.
