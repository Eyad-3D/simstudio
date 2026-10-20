# The outside judge

The engine's solver is judged on published problems with their own
reference solutions, not on its own probes. `judge.py` downloads the
problems' sources, writes each problem as a model in the text format,
runs it at rtol 1e-4 … 1e-10 and scores the results the way the problems'
sources score solvers: the error against the reference, significant
correct digits (scd), steps, error-test failures, events and wall time.
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
released at every 0.01 rad from 0.01 to 3.14 rad.

## The judges

| case | what it judges | reference | source and terms |
|---|---|---|---|
| `testset-andrews` | Andrews' squeezing mechanism: 7 bodies, index 3, 27 unknowns, t ≤ 0.03 s | the test set's reference solution at the end (PSIDE, rtol = atol = 1e-14); scd over the 7 angles, mescd over all 27, as the test set counts them | Bari IVP Test Set, problem 13: F. Mazzia, C. Magherini, *Test Set for Initial Value Problem Solvers*, release 2.4, University of Bari, 2008; E. Hairer, G. Wanner, *Solving ODEs II*, 1996, pp. 530–536. Free download, no licence stated: the equations are transcribed here with this citation; its code and reference values are not copied |
| `testset-caraxis` | the car axis: index 3, a time-dependent constraint, t ≤ 3 s | the test set's reference (quadruple-precision GAMD, 1e-24) at the end, all 10 components | Bari IVP Test Set, problem 17 (S. Schneider, PhD thesis, Geneva 1994); as above |
| `testset-chemakzo` | the chemical Akzo Nobel problem: stiff, index 1, t ≤ 180 s | the test set's reference (PSIDE, 1e-19) at the end | Bari IVP Test Set, problem 3; as above. SUNDIALS' `idasAkzoNob_dns` is the same problem |
| `iftomm-pendulum` | the planar simple pendulum, released horizontally, in x and y (index 3), 10 s | the closed form (Jacobi elliptic functions, mpmath at 34 digits); the benchmark's own measure, the energy drift, against its limit of 5e-5 J | IFToMM Library of Computational Benchmark Problems: R. Masoudi, T. Uchida, D. Vilela, A. Luaces, J. Cuadrado, J. McPhee, *A library of computational benchmark problems for the multibody dynamics community*, ECCOMAS Multibody Dynamics 2013. Research use only: its descriptions and the results other groups submitted stay in the cache |
| `iftomm-fourbar` | the multiple four-bar mechanism: 5 × 5 four-bar loops, 55 bars, 60 coordinates, 55 constraints, 5 degrees of freedom, through singular positions (a row's bars in line), 10 s | its minimal coordinates (each row's angle; the parallelograms keep their shape) integrated by Gragg–Bulirsch–Stoer extrapolation at 40 digits to an estimated 1e-28 (energy kept to 4e-27; DOP853 at rtol 1e-13 agrees to 6e-11); the benchmark's energy-drift limit of 1 J | IFToMM (A. Callejo); as above. The two submitted results agree with the reference to 3e-7 (Callejo) and 6e-6 (Burkhardt) rad up to 2 s, and drift apart later with their energy errors (0.75 and 0.06 J) |
| `pendulum-swing-θ0`, `pendulum-loop-m` | the same pendulum released at rest from θ0 (swinging), or given the speed at the bottom that makes m = 4gL/v0² < 1 (looping) | the closed form, as above | mathematics (mpmath, BSD-3-Clause) |
| `fmu-stair` | a model with no continuous state: a counter raised by a time event each second | `Stair_out.csv`, point for point; the events at whole seconds exactly | Reference FMUs v0.0.41 (Modelica Association, BSD-2-Clause), `Stair/model.c` |
| `fmu-bouncingball` | state events and `reinit`: a ball that bounces until it rests | the bounces in closed form (each one's time and speed); `BouncingBall_out.csv` beside it, which fmusim wrote with forward Euler at 1 ms: its h is up to 2.4e-2 m off the exact bounces (median 3.4e-3 m), and it bounces 12 times against the exact 11 | Reference FMUs v0.0.41, `BouncingBall/model.c` |
| `count-implicit-a`, `count-implicit-nostate-a`, `count-explicit-pulse` | a `when` on a variable solved from an implicit equation (`x^3 + x = z`, z = a t, with and without the state), and on an explicit function of a state (`x = z - z^3/4`) above its threshold from 0.539 to 1.675 s, inside one of the integrator's steps: each must fire once, at its crossing | the exact crossing times (`x^3 + x = 0.625`; the root of `t^3 - 4 t + 2`); OpenModelica's counts beside them | none published: the exact answers (an independent check's probes of the solver work's first round) |
| `msl-pid-controller` | the library Driver as `LimPID`, in the Modelica Standard Library's `Blocks.Examples.PID_Controller`: a PI controller (k = 100, Ti = 0.1 s, yMax = 12 N·m, Ni = 0.1) makes a motor inertia follow a reference speed through a spring-damper to a load under 10 N·m; decelerating, it is held at its limit and its anti-windup decides how it comes off. The drive is written in the text format, the Driver's command times yMax its torque, started in the steady state the example asks for | the reference result Dymola 2020x computed for the library's regression tests (DASSL, tolerance 1e-6, a row every 0.8 ms) at its six comparison signals, each relative to its largest value; the instants the command reaches and leaves its limit against the reference's two state events there; OpenModelica's run of the original example beside it (`score --beside`) | Modelica Standard Library 4.1.0 and MAP-LIB_ReferenceResults (Modelica Association, BSD-3-Clause): `Modelica/Blocks/Examples/PID_Controller`, its `PID_Controller.csv`, `comparisonSignals.txt` and `creation.txt`, in the cache; the model is written here from the example's statement |
| `driver.py` | the library Driver on a car at every mass on every driving cycle: whether each run ends, and whether its driven trace follows the cycle as a test-lab driver must | 40 CFR 1066.425(b): the speed within 2.0 mi/h of the schedule's highest and lowest points within 1.0 s, excursions of up to 2.0 s allowed, a shortfall at maximum available power (the accelerator fully down; 1066.415(e)(5)) not counted; a trace outside only where the car was too fast with the brake pedal fully down is told apart (the car's brakes, not the driver); the same cases on today's engine beside the new one | the US Code of Federal Regulations, title 40, §§ 1066.425 and 1066.415, the annual edition of 1 July 2024 (govinfo.gov, public domain), in the cache; the cars and cycles are the repository's own |
| `sundials` | the engine's patched IDAS (`lsim-sundials-sys/sundials/lsim.patch`) on SUNDIALS' own examples with quadratures under error control, against upstream 7.1.1 (the release the engine vendors) and 7.9.0 | the examples' shipped `.out` files (LLNL's runs) and G computed independently: the slider-crank in its minimal coordinate at 34 digits, the Akzo Nobel problem by Radau IIA at rtol 1e-13 (its y1(180) is 9e-15 off the test set's reference) | SUNDIALS (LLNL, BSD-3-Clause): `examples/idas/serial/idasSlCrank_dns.c`, `idasAkzoNob_dns.c` |
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

* **error, scd**: for the test set's problems, the largest relative error
  at the end time over the components the test set counts, and
  scd = −log10 of it; mescd = −log10(max |error| / (atol/rtol + |reference|))
  over all components. For the pendulums and the four-bar, the largest
  error of the positions (m; the angles, rad) over the output grid, and
  scd = −log10 of it (the length is 1 m). For Stair, the largest
  difference of the counter from `Stair_out.csv` at the CSV's times (the
  value after an event at a whole second); for BouncingBall, the largest
  error of a bounce's time; for the counts, the largest distance of a
  firing from its crossing, infinite where the `when` fired too often or
  too seldom.
* **right or wrong**: a run of a case with a closed form (the pendulums,
  the four-bar) is right when its error is within 100 rtol of the
  reference over the whole grid: 1e-8 at rtol 1e-10, which the brief
  asks of dynamic state selection, and 1 cm on a 1 m rod at rtol 1e-4; a
  count is right when its `when` fired once per crossing (where it
  fired, its error, depends on how steeply the condition crosses: near
  an oscillator's peak, 70 times the error in its state). Otherwise,
  without a message, it is a wrong answer, marked "wrong, no message". The criterion is fixed here, before the runs are
  scored; `score` applies it to every run, and with the release-angle
  sweep (`cases --sweep`) tabulates the runs right, wrong and stopped per
  tolerance, the angle up to which every run is right, and the largest
  error per 0.1 rad in units of rtol.
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
