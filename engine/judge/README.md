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
python3 engine/judge/judge.py score    CACHE CASES RESULTS [--before RESULTS0] [--out FILE]
python3 engine/judge/judge.py sundials CACHE WORK             # SUNDIALS' own IDAS examples
python3 engine/judge/judge.py omc      CASES RESULTS          # OpenModelica, if omc is on the PATH
```

`fast.py` judges fast mode (DESIGN.md §10) the same way, from outside:

```text
<backend's Python> engine/judge/fast.py cases CASES     # EPA's reference cars as project files
cargo build --release -p lsim-project --example fast    # the runner (engine/)
python3 engine/judge/fast.py run   CASES RESULTS        # every example case and EPA car
python3 engine/judge/fast.py score RESULTS --fastsim FASTSIM_JSON [--before RESULTS0] [--out FILE]
```

`run` runs each drive-cycle case of the example projects, and each of
EPA's four reference cars on the UDDS and HWFET (`backend/validation`'s
suite, written as project files by `cases`), in fast mode, in full
dynamic mode (rtol 1e-6, as the golden comparison) and in fast mode again
on the speed the full dynamic run drove, every 0.1 s (which takes the
driver's tracking out). `score` puts fast mode's charge used beside the
full dynamic run's (inside its band when fast mode raises no flag and the
two agree within 1 %, fixed before scoring) and its energy at the wall,
counted as the suite counts it, beside FASTSim 3.1.0's on the same cars
(FASTSIM_JSON: `benchmarks/fastsim/compare.py fastsim` in FASTSim's own
virtualenv; its runs given LightSim's inputs are the comparison, its own
vehicle files beside them), with steps, events (those at the cycle's
samples counted apart), flags and run times, and `--before` an earlier
build's results beside them.

`test_judge.py` checks the scoring itself on made-up series whose right
scores are known (standard library only; `engine/check.sh` runs it).
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
| `sundials` | the engine's patched IDAS (`lsim-sundials-sys/sundials/lsim.patch`) on SUNDIALS' own examples with quadratures under error control, against upstream 7.1.1 (the release the engine vendors) and 7.9.0 | the examples' shipped `.out` files (LLNL's runs) and G computed independently: the slider-crank in its minimal coordinate at 34 digits, the Akzo Nobel problem by Radau IIA at rtol 1e-13 (its y1(180) is 9e-15 off the test set's reference) | SUNDIALS (LLNL, BSD-3-Clause): `examples/idas/serial/idasSlCrank_dns.c`, `idasAkzoNob_dns.c` |
| `fast.py` | fast mode: each example project's drive-cycle case against its full dynamic run, and EPA's reference cars against FASTSim | the full dynamic run of the same case (charge used within 1 %); FASTSim 3.1.0 given the same cars (the reference suite's inputs) | the example projects (the repository's); `backend/validation` (EPA's test data, public); FASTSim (U.S. National Laboratory of the Rockies, Apache-2.0): for checking only, never shipped, run in its own virtualenv |
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
* **steps, ETF, events**: the integrator's steps, error-test failures and
  the events the run handled.
* **wall time**: the run's alone (initialisation and integration, not
  preparation or compilation), the median of three, release build.

Every run either matches its reference or stops with the engine's own
message, which the table quotes; a run that returns a wrong answer without
a message is the worst result and is marked by its error.
