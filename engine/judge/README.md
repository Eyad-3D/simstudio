# The outside judge

The engine's solver is judged on published problems with their own
reference solutions, not on its own probes. `judge.py` downloads the
problems' sources, writes each problem as a model in the text format,
runs it at rtol 1e-4 … 1e-10 and scores the results the way the problems'
sources score solvers: the error against the reference, significant
correct digits (scd), steps, error-test failures, events and wall time,
and how many times a run chose its states again (`switches`).
OpenModelica runs the same models for a second tool's answer.

```text
python3 engine/judge/judge.py fetch    CACHE                  # sources, checked by SHA-256
python3 engine/judge/judge.py cases    CACHE CASES [--sweep]  # models, runs, reference solutions
cargo build --release -p lsim-project --example judge         # the runner (engine/)
python3 engine/judge/judge.py run      CASES RESULTS          # the engine on every case
python3 engine/judge/judge.py score    CACHE CASES RESULTS [--before RESULTS0] [--beside RESULTS2] [--out FILE]
python3 engine/judge/judge.py sundials CACHE WORK             # SUNDIALS' own IDAS examples
python3 engine/judge/judge.py omc      CASES RESULTS [--cache CACHE]  # OpenModelica, if omc is on the PATH

# the library Driver's map: a car at every mass on every driving cycle
cargo build --release -p lsim-project --example driver_map    # (engine/)
driver_map PROJECT CASE RUNS.jsonl --masses 1000,2000 --cycles wltc-3b,own
python3 engine/judge/driver.py cells CACHE RUNS.jsonl --out CELLS.jsonl
python3 engine/judge/driver.py today PROJECT CASE TODAY.jsonl --masses ... --cycles ... \
    [--substep 0.001] [--back-calculation]                     # today's engine: the backend's Python
python3 engine/judge/driver.py score CACHE CELLS.jsonl [--before CELLS0.jsonl] [--today TODAY.jsonl] [--out FILE]
```

`fast.py` judges fast mode (DESIGN.md §10) the same way, from outside:

```text
<backend's Python> engine/judge/fast.py cases CASES     # EPA's reference cars as project files
python3 engine/judge/fast.py cycles CASES               # the BEV example cars on every bundled cycle
cargo build --release -p lsim-project --example fast    # the runner (engine/)
python3 engine/judge/fast.py run   CASES RESULTS        # every example case, cycle and EPA car
python3 engine/judge/fast.py score RESULTS --fastsim FASTSIM_JSON [--before RESULTS0] [--out FILE]
```

`run` runs each drive-cycle case of the example projects, the two battery
electric example cars on each of the 27 bundled cycles (written as
project files by `cycles`), and each of EPA's four reference cars on the
UDDS and HWFET (`backend/validation`'s suite, written as project files by
`cases`), in fast mode, in full dynamic mode (rtol 1e-6, as the golden
comparison), in fast mode again on the speed the full dynamic run drove,
every 0.1 s (which takes the driver's tracking out), and in fast mode
again on the cycle changed at round-off level (every speed 1e-9 of itself
up or down by turns; `--perturb 0` leaves that out, for a runner from
before it had it). `score` puts fast mode's charge used beside the full
dynamic run's (inside its band when fast mode raises no flag, the two
agree within 1 %, and the perturbed cycle gives the same flags and a
charge used within a millionth: fixed before scoring) and its energy at
the wall, counted as the suite counts it, beside FASTSim 3.1.0's on the same cars
(FASTSIM_JSON: `benchmarks/fastsim/compare.py fastsim` in FASTSim's own
virtualenv; its runs given LightSim's inputs are the comparison, its own
vehicle files beside them), with steps, events (those at the cycle's
samples counted apart), flags and run times, and `--before` an earlier
build's results beside them.

`test_judge.py` checks the scoring itself on made-up series whose right
scores are known, the speed-trace rule's included (standard library only;
`engine/check.sh` runs it). `score --beside` puts another tool's runs of
the same cases beside the engine's, run against run at each tolerance
(OpenModelica's of the original model, for `msl-pid-controller`).
`driver.py today` runs the map's cases on today's engine, at its 10 ms
step or at 1 ms (`--substep`, the golden comparison's fine run), and
with `--back-calculation` with its Driver's frozen integral replaced by
`LimPID`'s back-calculation in that process only: a check of how much of
a difference between the engines is the anti-windup (today's engine
itself is not changed).
CACHE, CASES, RESULTS and WORK are folders outside the repository. For
`omc`, OpenModelica's compiler and runtime suffice: conda-forge's
`openmodelica` 1.27.1 package with the libraries `omc` loads (`libgcc`,
`libgfortran5`, `libopenblas`, `liblapack`, `libblas`, `libiconv`)
unpacked into one folder, 230 MB, its `bin/omc` on the PATH; the judge
sets `OPENMODELICAHOME` and the library path from it. The judge needs
Python 3.10+ with mpmath and scipy (the reference solutions), and for
`sundials` a C compiler, CMake and `patch`. `run` takes the runner
from `CARGO_TARGET_DIR` (or `--runner`); `score --before` puts an earlier
build's results beside the current ones. `cases --sweep` adds the pendulum
released at every 0.01 rad from 0.01 to 3.14 rad, in x and y and in its
angle.

## The judges

| case | what it judges | reference | source and terms |
|---|---|---|---|
| `testset-andrews` | Andrews' squeezing mechanism: 7 bodies, index 3, 27 unknowns, t ≤ 0.03 s | the test set's reference solution at the end (PSIDE, rtol = atol = 1e-14); scd over the 7 angles, mescd over all 27, as the test set counts them | Bari IVP Test Set, problem 13: F. Mazzia, C. Magherini, *Test Set for Initial Value Problem Solvers*, release 2.4, University of Bari, 2008; E. Hairer, G. Wanner, *Solving ODEs II*, 1996, pp. 530–536. Free download, no licence stated: the equations are transcribed here with this citation; its code and reference values are not copied |
| `testset-caraxis` | the car axis: index 3, a time-dependent constraint, t ≤ 3 s | the test set's reference (quadruple-precision GAMD, 1e-24) at the end, all 10 components | Bari IVP Test Set, problem 17 (S. Schneider, PhD thesis, Geneva 1994); as above |
| `testset-chemakzo` | the chemical Akzo Nobel problem: stiff, index 1, t ≤ 180 s | the test set's reference (PSIDE, 1e-19) at the end | Bari IVP Test Set, problem 3; as above. SUNDIALS' `idasAkzoNob_dns` is the same problem |
| `iftomm-pendulum` | the planar simple pendulum, released horizontally, in x and y (index 3), 10 s | the closed form (Jacobi elliptic functions, mpmath at 34 digits); the benchmark's own measure, the energy drift, against its limit of 5e-5 J | IFToMM Library of Computational Benchmark Problems: R. Masoudi, T. Uchida, D. Vilela, A. Luaces, J. Cuadrado, J. McPhee, *A library of computational benchmark problems for the multibody dynamics community*, ECCOMAS Multibody Dynamics 2013. Research use only: its descriptions and the results other groups submitted stay in the cache |
| `iftomm-fourbar` | the multiple four-bar mechanism: 5 × 5 four-bar loops, 55 bars, 60 coordinates, 55 constraints, 5 degrees of freedom, through singular positions (a row's bars in line), 10 s | its minimal coordinates (each row's angle; the parallelograms keep their shape) integrated by Gragg–Bulirsch–Stoer extrapolation at 40 digits to an estimated 1e-28 (energy kept to 4e-27; DOP853 at rtol 1e-13 agrees to 6e-11); the benchmark's energy-drift limit of 1 J | IFToMM (A. Callejo); as above. The two submitted results agree with the reference to 3e-7 (Callejo) and 6e-6 (Burkhardt) rad up to 2 s, and drift apart later with their energy errors (0.75 and 0.06 J) |
| `pendulum-swing-θ0`, `pendulum-loop-m` | the same pendulum released at rest from θ0 (swinging), or given the speed at the bottom that makes m = 4gL/v0² < 1 (looping) | the closed form, as above | mathematics (mpmath, BSD-3-Clause) |
| `pendulum-angle-θ0`, `pendulum-angle-loop-m` | the control: the same pendulums in their angle from the vertical, an ODE with no constraint and so no choice of states, the integrator's own error on the problem beside the x-y form's | the closed form, as above | mathematics |
| `pendulum-prefer-x-θ0` | `stateSelect`: the x-y pendulum released at 1.50 and 1.55 rad, where pivoting alone keeps y and vy (which degenerate at the bottom of the swing), with x marked `prefer`; and at atan 99 (1.5607 rad), where the choice that keeps x would start with y moving 99 times as much as x, inside the run's limit (100) but outside the margin a marked choice must start in (50): the mark is refused and told, and the run goes on with the choice by pivoting, choosing its states again where they degrade | the closed form, as above | mathematics |
| `pendulum-event-θ0` | the step after an event watched: the x-y pendulum released at 2.0 and 2.8 rad (states x and vx, which degenerate as the rod passes the horizontal), with a `when` as the rod nears the horizontal (y < 0.02, about 2 ms before) whose assignment changes nothing: each run must go on through the event and choose its states again where it does without it (or, run without the model's choices, stop where it stops without it) | the closed form, as above (the point where y first moves a hundred times as much as x) | mathematics |
| `pendulum-level` | the x-y pendulum released exactly level, x = L and y = 0 to the bit (x and vx fixed): at the start y decides nothing, the rod's velocity constraint x·vx + y·vy = 0 leaves vy 0 / 0, and the release is a double root of x² + y² = L² in y; it was refused at preparation before dynamic state selection | the closed form, as above (the IFToMM pendulum is the same problem released on the other side, 6e-17 m below level) | mathematics |
| `pendulums-k-θ…`, `pendulums-angle-k-θ…` | held out (added after dynamic state selection's rule was chosen on the single pendulums, and not used to choose it): k = 2, 5 and 6 x-y pendulums released at rest at the angles in the name, independent of each other in one model, so that index reduction's choice has a set per rod at each level and each rod's states degrade on its own (1.50 + 1.60, 1.50 + 1.80, 1.70 + 2.00, 1.50 … 1.58, 1.5 … 3.05 rad); and the same pendulums in their angles, the control | each rod's closed form, as above; the error is the worst rod's | mathematics |
| `double-pendulum-1.20-1.60`, `double-angle-1.20-1.60` | held out, as above: a double pendulum in x and y (two unit masses on two 1 m rods, its rods' constraints one coupled set at each level), released at rest at 1.2 and 1.6 rad from the vertical, 5 s; and in its two angles, the control | its angles integrated by DOP853 at rtol = atol = 1e-13 (scipy; the classical Runge–Kutta at 1e-4 s steps, `lsim-solve/tests/state_choice.rs`, agrees to 1e-12 m) | mathematics |
| `msl-planarfourbar` | `stateSelect` in a mechanism: the Modelica Standard Library's `Mechanics.MultiBody.Examples.Loops.PlanarFourbar` transcribed in the plane: four steel BodyBoxes, revolute1 marked `always` (as the example marks it), revolute2 and revolute3 `prefer` (the Revolute's default), the bodies' centres `avoid` (BodyBox's), a damper at revolute1, the cut joint's two loop equations, the joint forces unknown: index 3, a 13-row choice of states, 5 s. Pivoting alone takes the states the marks ask for here (revolute1's angle and speed) | revolute1's angle from the mechanism's one degree of freedom (the loop closed by Newton's method, DOP853 at 1e-13; 1e-12 and 1e-13 agree to 5e-11 rad; OpenModelica 1.27 on MSL 4.1.0's own model, checked once by hand, 7.1e-9 rad from it at rtol 1e-12, 2.1e-7 at 1e-10); MSL's reference result beside it (Dymola 2020x, DASSL at 1e-6: 7.1e-4 rad from it; OpenModelica at 1e-6, 5.6e-4) | Modelica Standard Library 4.1.0 (Modelica Association, BSD-3-Clause): `PlanarFourbar.mo` and `BodyBox.mo`, and the reference result `PlanarFourbar.csv` of its regression tests (`MAP-LIB_ReferenceResults` v4.1.0). The brief named `Fourbar1`; it is spatial (a universal, a spherical and a prismatic joint), and a transcription of its frames without the MultiBody library would judge the transcription more than the engine: `PlanarFourbar` has the same `stateSelect = always` and a cut joint's loop |
| `fmu-stair` | a model with no continuous state: a counter raised by a time event each second | `Stair_out.csv`, point for point; the events at whole seconds exactly | Reference FMUs v0.0.41 (Modelica Association, BSD-2-Clause), `Stair/model.c` |
| `fmu-bouncingball` | state events and `reinit`: a ball that bounces until it rests | the bounces in closed form (each one's time and speed); `BouncingBall_out.csv` beside it, which fmusim wrote with forward Euler at 1 ms: its h is up to 2.4e-2 m off the exact bounces (median 3.4e-3 m), and it bounces 12 times against the exact 11 | Reference FMUs v0.0.41, `BouncingBall/model.c` |
| `kink-spring-one`, `kink-spring-many`, `kink-spring-clamp`, `kink-gear-linear`, `kink-gear-cubic`, `kink-gear-clamp`, `kink-2d`, each beside `…-located` | a table read at a state, where the read kinks or jumps at the table's breakpoints (DESIGN.md §8.2, *Tables read at a state*): a unit mass on a spring whose force is a linear table of its position, crossing one breakpoint once, five of them 234 times in 100 s, or held at the data's edges (`clamp`) and leaving them 30 times; the gear of `lsim-prep`'s tests, its ratio a table of the load's speed (`w1 = r(w2) w2`, index reduction reads `r'`), linear (`r'` jumps: the solved acceleration jumps), a monotone cubic (`r''` jumps) and a cubic held at its edges with the torque reversed (`r'` jumps where the speed leaves the data and comes back); a mass driven by a table of two states, `x'' = T(x, y)` with `y'' = -y`, its cells' edges crossed 64 times in x and y. Each `-located` control is the same function written as `if` branches on the same breakpoints, every crossing a mode: held during a step and located by root finding, the integrator restarted there (Park and Barton's discontinuity locking done by the model); `report` puts each case beside it (error in rtol, their ratio, steps, error-test failures) | closed forms per piece, the crossings solved at 40 digits (mpmath): harmonic motion or a parabola per piece of the springs; for the gears the first integral `J1 r(w2) w2 + J2 ∫ dw / r = ∫ tau dt`, integrated per cell (in closed form, or by quadrature on a cubic's cells) and inverted; for the two-state table a forced oscillator per pair of cells, `y`'s crossings by `acos`, `x`'s found by sampling every 4 ms with each sample interval's extremum. Each reference checked against DOP853 at rtol 1e-13 restarted at its crossings when the cases are written (2e-14 … 2.3e-12) | mathematics |
| `kink-dense-spring-41`, `kink-dense-spring-401`, `kink-dense-gear-41`, `kink-dense-gear-401`, `kink-pass-301`, `kink-pass-default`, each beside `…-held` | a table read at a state, run the engine's way beside held in its cells (the runner's option `table_cells`; the engine's way is stepped over for the springs and the passes, and held for the gears since rule v4, their stepped-over runs `…-over` below): the spring whose force, −x − 2x³, is a linear table of 41 or 401 points (its value kinks; `-held`, held as a read that jumps is, "kinks": 348 and 3 392 crossings in 20 s, each a restart); the gear whose ratio, 2 + w/40 + 0.25 sin(w/8), is a linear table of 41 or 401 points of the load's speed (`r'` jumps; `-held`, "jumps", with the releases); `v' = T(v)` from 0, once through a linear table of 301 points (290 crossings), and through the table as the text format writes one by default (a monotone cubic held at its edges, 41 points: C¹ between them, leaving the data at its last). `report` puts each case beside its other way (error in rtol, their ratio, steps, events) | as the other `kink-` cases; for `v' = T(v)` per piece `v = v∞ + (v₀ − v∞) e^{b t}` where T is affine, `t(v) = ∫ dv / T` by quadrature on a cubic's pieces, inverted (each checked against DOP853: 6.5e-16 … 1.7e-13) | mathematics |
| `kink-gear-linear-held`, `kink-gear-clamp-held`; `kink-osc-41`, `kink-osc-101`, each beside `…-held`, `…-always` and `…-located`; `kink-edge-11` beside `…-held` and `…-located`; `kink-bead-41` beside `…-held` | where holding a read in its cells pays and where it loses (DESIGN.md §8.2): the two gears above held ("jumps"); the same gear with its load on a torsion spring and no torque, its speed swung back and forth from 28 rad/s through a linear ratio table, 2.5 + w/50 + 0.2 sin(w/4) at 41 or 101 points on [−30, 30] rad/s (1 127 and 2 847 crossings in 20 s; `-held` with the releases, `-always` held through every crossing, "table_release" off, `-located` the ratio as `if` branches of `w2 / wu`); the same from 15 rad/s through a monotone cubic of 11 points on [−10, 10] held at its edges (59 crossings of its edges); the A3 review's bead on a wire `y = r(x)`, `r` the default table of x²/2 at 41 points (index reduction reads `r''`), from x = 1.5 at rest for 10 s (204 crossings; `-held` at every point, "kinks", as the engine held it for one round; no `if` form: preparation refuses it, STATUS.md) | DOP853 at rtol 1e-13 cell by cell of the engine's own pieces, each crossing found by its event and the integration restarted there (no closed form); checked by the energy each conserves, ½ J1 w1² + ½ J2 w2² + ½ k φ² and ½ vx² (1 + r'²) + g r, to 1.7e-12 … 1.9e-11 | mathematics |
| `count-implicit-a`, `count-implicit-nostate-a`, `count-explicit-pulse` | a `when` on a variable solved from an implicit equation (`x^3 + x = z`, z = a t, with and without the state), and on an explicit function of a state (`x = z - z^3/4`) above its threshold from 0.539 to 1.675 s, inside one of the integrator's steps: each must fire once, at its crossing | the exact crossing times (`x^3 + x = 0.625`; the root of `t^3 - 4 t + 2`); OpenModelica's counts beside them | none published: the exact answers (an independent check's probes of the solver work's first round) |
| `kink-gear-linear-over`, `kink-dense-gear-41-over`, `kink-dense-gear-401-over`, `kink-osc-41-over`, `kink-osc-101-over`; `kink-gear-linear-imp`, `kink-dense-gear-41-imp`, `kink-osc-41-imp`, each beside `…-imp-held`, `…-imp-always`, `…-imp-located` and (the two gears) `…-imp-over` | the default holds the read where the model's form needs it (DESIGN.md §8.2, rule v4): `…-over` steps over the cases whose default now holds their read (a linear table's slope, on the explicit form), as the engine's default did before; `…-imp` are the gear, the dense gear and the swing with their torques written implicitly, `t2 + 1e-6 t2³/tu² = r t1 + 1e-6 (r t1)³/tu²` (the same solution, t2 an iteration variable: the implicit form, on IDAS, where the run releases nothing, so the default, `-held` and `-always` are one run); stepped over (`-imp-over`) the gears stop at their first breakpoint (the swing gives no answer within minutes: not run) | as the cases they are written from | mathematics |
| `msl-pid-controller` | the library Driver as `LimPID`, in the Modelica Standard Library's `Blocks.Examples.PID_Controller`: a PI controller (k = 100, Ti = 0.1 s, yMax = 12 N·m, Ni = 0.1) makes a motor inertia follow a reference speed through a spring-damper to a load under 10 N·m; decelerating, it is held at its limit and its anti-windup decides how it comes off. The drive is written in the text format, the Driver's command times yMax its torque, started in the steady state the example asks for | the reference result Dymola 2020x computed for the library's regression tests (DASSL, tolerance 1e-6, a row every 0.8 ms) at its six comparison signals, each relative to its largest value; the instants the command reaches and leaves its limit against the reference's two state events there; OpenModelica's run of the original example beside it (`score --beside`) | Modelica Standard Library 4.1.0 and MAP-LIB_ReferenceResults (Modelica Association, BSD-3-Clause): `Modelica/Blocks/Examples/PID_Controller`, its `PID_Controller.csv`, `comparisonSignals.txt` and `creation.txt`, in the cache; the model is written here from the example's statement |
| `driver.py` | the library Driver on a car at every mass on every driving cycle: whether each run ends, and whether its driven trace follows the cycle as a test-lab driver must | 40 CFR 1066.425(b): the speed within 2.0 mi/h of the schedule's highest and lowest points within 1.0 s, excursions of up to 2.0 s allowed, a shortfall at maximum available power (the accelerator fully down; 1066.415(e)(5)) not counted; a trace outside only where the car was too fast with the brake pedal fully down is told apart (the car's brakes, not the driver); the same cases on today's engine beside the new one | the US Code of Federal Regulations, title 40, §§ 1066.425 and 1066.415, the annual edition of 1 July 2024 (govinfo.gov, public domain), in the cache; the cars and cycles are the repository's own |
| `sundials` | the engine's patched IDAS (`lsim-sundials-sys/sundials/lsim.patch`) on SUNDIALS' own examples with quadratures under error control, against upstream 7.1.1 (the release the engine vendors) and 7.9.0 | the examples' shipped `.out` files (LLNL's runs) and G computed independently: the slider-crank in its minimal coordinate at 34 digits, the Akzo Nobel problem by Radau IIA at rtol 1e-13 (its y1(180) is 9e-15 off the test set's reference) | SUNDIALS (LLNL, BSD-3-Clause): `examples/idas/serial/idasSlCrank_dns.c`, `idasAkzoNob_dns.c` |
| `fast.py` | fast mode: each example project's drive-cycle case, and the battery electric cars on every bundled cycle, against its full dynamic run and against itself on the cycle changed at round-off level; EPA's reference cars against FASTSim | the full dynamic run of the same case (charge used within 1 %); FASTSim 3.1.0 given the same cars (the reference suite's inputs) | the example projects (the repository's); `backend/validation` (EPA's test data, public); FASTSim (U.S. National Laboratory of the Rockies, Apache-2.0): for checking only, never shipped, run in its own virtualenv |
| `omc` | a second tool on the same models: OpenModelica with its dynamic state selection and with static dummy derivatives | the references above | OpenModelica (AGPL-3.0 or OSMC-PL): for checking only, never shipped or linked; the conda-forge build of `omc` with the runtime libraries it needs |

Each model is written by `judge.py` from the problem's statement, so the
repository holds no third party's data; the reference values are read from
the cached files or computed when the cases are written. A wrong
transcription would not go unnoticed: each test set model's error falls
with the tolerance onto the published reference (Andrews' to 9.0
significant digits at rtol 1e-10, the car axis' to 5.8, Akzo Nobel's to
7.4; OpenModelica's alike), and the four-bar's text, which neither the
engine nor OpenModelica runs, is checked against its mechanics when the
cases are written: the accelerations and bar forces at the start, solved
from the bars' mass matrices, weights and length constraints, zero every
one of its equations (to 1e-14; one weight changed shows as 4.9).

## Measures

* **kinks**: for the tables read at a state, the largest error of each
  recorded channel over the grid relative to the channel's largest value
  in the reference, the worst of them; right within 100 rtol, as the
  closed-form cases. The decision the cases were written for (the solver
  work's A3) compared each with its control, the error's slope over the
  seven tolerances, its growth in rtol from the loose to the tight ones,
  and its ratio to the control's: stepped over, the jumps read through
  index reduction were up to 178 rtol off (`kink-gear-linear`, the
  control within 3), their error per tolerance growing as it tightens;
  many kinks of a value read were up to 23 times the control's error at
  one tolerance (`kink-spring-many` at 1e-6; 61 in a sweep of release
  points with the method held to Adams, 1 278 under `auto`, which ran the
  two forms on different methods), 0.8 … 5.1 times as geometric means per
  tolerance, but no more out of proportion to the tolerance than the
  control (the slope and growth tests). The dense tables
  (`kink-dense-…`, `kink-pass-…`) weigh the remedy against stepping
  over: each crossing held is a restart of the integrator, which costs
  about 0.1 to 4 rtol, so a value read of 41 to 401 points held is 7 to
  1 270 times as far off as stepped over at rtol 1e-4 … 1e-6 (0.13 to 68
  times at 1e-7 … 1e-10), while a jump read stepped over is 0.04 to 99
  times as far off as held: better at rtol 1e-4 and 1e-5 (on 401 points
  by up to 27 times, and at 1e-6), worse from 1e-6 or 1e-7 on, its error
  growing 30-fold as the tolerance tightens where the held one's does
  not. The swings and the bead weigh the jumps the same way: the gear
  swung through 41 or 101 points held through every crossing is 517 and
  1 238 rtol off at 1e-4 where stepped over it is 55 and 55 (the located
  runs the held ones to the digits printed); the bead held 429 … 737 at
  1e-4 … 1e-10 where stepped over it is 10 … 187; the clamped cubic
  swung through its edges held 104 at 1e-4 where stepped over it is 34.
  Stepped over, a linear table's slope is up to 178 rtol off on the gear
  (`kink-gear-linear-over`; held 1.0 … 2.8) and, in the A3 checks' probes
  where the speed turns just past a point or the states are chosen
  again, thousands of times the held run's error; on the implicit form
  every read that jumps stops the run (`kink-gear-linear-imp-over`,
  `kink-dense-gear-41-imp-over`: at the first breakpoint, every
  tolerance), where held it is 0.92 … 4.95 rtol (`kink-gear-linear-imp`)
  and 3.1 … 11.7 (`kink-dense-gear-41-imp`), as located. So the default
  holds a linear table's slope on the explicit form (with the releases)
  and every read that jumps on the implicit form (without them), and
  steps over the rest (rule v4, DESIGN.md §8.2).
* **error, scd**: for the test set's problems, the largest relative error
  at the end time over the components the test set counts, and
  scd = −log10 of it; mescd = −log10(max |error| / (atol/rtol + |reference|))
  over all components. For the pendulums and the four-bar, the largest
  error of the positions (m; the angles, rad) over the output grid, and
  scd = −log10 of it (the length is 1 m); for MSL's planar four-bar, the
  largest error of revolute1's angle (rad) over the grid, MSL's reference
  result's beside it. For Stair, the largest
  difference of the counter from `Stair_out.csv` at the CSV's times (the
  value after an event at a whole second); for BouncingBall, the largest
  error of a bounce's time; for the counts, the largest distance of a
  firing from its crossing, infinite where the `when` fired too often or
  too seldom.
* **right or wrong**: a run of a case with a closed form (the pendulums,
  the four-bars) is right when its error is within 100 rtol of the
  reference over the whole grid: 1e-8 at rtol 1e-10, which the brief
  asks of dynamic state selection, and 1 cm on a 1 m rod at rtol 1e-4; a
  count is right when its `when` fired once per crossing (where it
  fired, its error, depends on how steeply the condition crosses: near
  an oscillator's peak, 70 times the error in its state). Otherwise,
  without a message, it is a wrong answer, marked "wrong, no message". The criterion is fixed here, before the runs are
  scored; `score` applies it to every run, and with the release-angle
  sweep (`cases --sweep`) tabulates the runs right, wrong and stopped per
  tolerance, the angle up to which every run is right, and the largest
  error per 0.1 rad in units of rtol, the angle form's beside them (an
  ODE: what the integrator's error alone makes of the same criterion).
* **by time, not by row**: a run's points are matched to the reference's
  grid by their times (to 1e-12, relative): a row a tool writes off the
  grid (OpenModelica's rows at an event 1e-10 s after a whole second) is
  no grid point, two rows at one grid time are an event there (the later
  is the value after it), and a grid time a tool wrote no row at is
  missing, counted and left out. Stair is scored only where the run has
  every one of the CSV's times. The brief named csv-compare's tubes for
  OpenModelica's and the CSVs' series; they are not used: a tube forgives
  an event shifted within its width, and here the shift is measured
  instead (the event-time error beside the error). OpenModelica's Stair,
  its events 1e-10 s late, shows the counter's full step at the whole
  seconds as its error, its event times as 1e-10 s off.
* **the library's reference result** (`msl-pid-controller`): each
  comparison signal's largest difference from the reference over the run,
  at the reference's own rows (every 0.8 ms, the run's output grid; after
  an event, its later row), relative to the signal's largest value, and
  the worst of the six; the instants the controller's command reaches and
  leaves its limit (where the limiter's input crosses it, between the
  output points) against the reference's two state events there. The
  reference is Dymola's at tolerance 1e-6: a run tighter than that is
  judged by its distance from OpenModelica's run of the original example
  at the same tolerance (`--beside`), which falls with the tolerance where
  both are right.
* **the speed-trace rule** (`driver.py`): each run of the map is a cell:
  ✓ inside 40 CFR 1066.425(b) with its allowances; e inside, but only by
  the allowance for a car at its maximum available power, the car having
  stood still at full throttle for more than 10 s while the schedule
  moved (a battery that emptied: a test would end there); b outside only
  on occasions where the car was too fast with the brake pedal fully down
  throughout; ✗ outside on an occasion the Driver could have helped; S, F,
  P or E where the run stopped (an event storm, a bare SUNDIALS flag, no
  progress, another stop). An occasion is a run of output points outside
  the band (every 0.1 s; 1 s on the 83 042 s `long-haul` cycle), counted
  when longer than 2.0 s; a point below the band with the accelerator
  fully down is excused. The target is checked against the cycle's
  published points.
* **steps, ETF, events**: the integrator's steps, error-test failures and
  the events the run handled.
* **wall time**: the run's alone (initialisation and integration, not
  preparation or compilation), the median of three, release build.

Every run either matches its reference or stops with the engine's own
message, which the table quotes; a run that returns a wrong answer without
a message is the worst result and is marked by its error.
