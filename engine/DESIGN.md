# LightSim's simulation engine — Stage 1 design

Status: Stage 1 (design and toolchain spike), October 2026. The workspace in
this folder compiles, its tests pass (`./check.sh`), and the end-to-end
spike runs a model described in the engine's IR through every stage of the
pipeline to an answer that matches the exact one to 1e-10.

Contents

1. [Summary](#1-summary)
2. [Goals and targets](#2-goals-and-targets)
3. [Choosing the solver: evidence](#3-choosing-the-solver-evidence)
4. [Architecture](#4-architecture)
5. [The IR](#5-the-ir)
6. [Preparation pipeline](#6-preparation-pipeline)
7. [Code generation and the cache](#7-code-generation-and-the-cache)
8. [Solver layer](#8-solver-layer)
9. [Causal blocks, Script blocks and FMUs in an acausal network](#9-causal-blocks-script-blocks-and-fmus-in-an-acausal-network)
10. [Fast mode: the inverse model](#10-fast-mode-the-inverse-model)
11. [Outputs, energy books and accuracy reports](#11-outputs-energy-books-and-accuracy-reports)
12. [Today's projects and library in the new engine](#12-todays-projects-and-library-in-the-new-engine)
13. [Python API](#13-python-api)
14. [Error messages](#14-error-messages)
15. [Testing strategy](#15-testing-strategy)
16. [Work breakdown](#16-work-breakdown)
17. [Risks](#17-risks)
18. [Licences](#18-licences)
19. [The Stage 1 spike](#19-the-stage-1-spike)

---

## 1. Summary

* **Equation-based core in Rust.** Components declare ports (across/through
  pairs), parameters, variables and equations; connecting ports generates
  the conservation equations. Models are flattened, aliases removed,
  equations matched, sorted, torn and index-reduced into a semi-explicit
  index-1 DAE, compiled to machine code with **Cranelift** in milliseconds (0.7 ms
  for the spike, 23–32 ms of one thread for the example cars), and
  integrated by **SUNDIALS CVODE/IDA**.
* **Solver: SUNDIALS, with diffsol as the pure-Rust second backend.** On a
  stiff battery–motor–brake DAE with an exact answer both reach
  tolerance-proportional accuracy at the same speed; CVODE re-factorised
  its Newton matrix 2–14× less often than diffsol (what dominates on large
  models; on a 199-state stiff chain CVODE with a structure-exploiting LU
  was 1.4–1.9× faster than diffsol's sparse LU), never failed across rtol
  1e-4 to 1e-12, and is the reference the field trusts. diffsol is
  pure Rust and builds anywhere, but its DAE path failed at rtol ≤ 1e-10
  with default settings. SUNDIALS is the production integrator; diffsol
  sits behind the same `Integrator` trait as an independent cross-check
  (section 3).
* **Parameters are runtime inputs**: changing one never recompiles. The
  prepared model is cached on disk by a key of the model's inputs.
* **Two modes per case**: full dynamic (forward from the driver) and fast
  (the same equations as an inverse model on the prescribed speed trace,
  fixed 1 s steps, limits flagged rather than enforced).
* **Exact events** (zero crossings located on the dense output, consistent
  re-initialisation), **exact Jacobians** (forward-mode differentiation of
  the generated code), **dense output** with per-interval min/max/mean,
  **energy books** from port powers, a **solver report** and a one-click
  10× tighter re-run.
* **Six work packages** (section 16) with fixed crate ownership and the
  interfaces already written down as Rust types in `lsim-ir`.

## 2. Goals and targets

| Goal | Target | How it is checked |
|---|---|---|
| Full dynamic speed | faster than the average of comparable tools on the same drive cycles (the owner's decision, 15 October 2026: LightSim need not be the fastest, but it must count among the fast ones; the earlier fixed target, 1000× real time, is dropped) | a benchmark of the example cars against published and measured times of comparable tools, kept with its sources |
| Fast mode speed | faster than the average of comparable quasi-static tools (same decision; the earlier ≈ 10⁶× real time is dropped) | same |
| Accuracy | every block within its tolerance of an exact answer; global error tracks rtol | the exact-answer suite in `benchmarks/` (built by another agent), tolerance-convergence tests |
| Energy | the energy balance from port powers closes to ≤ 1e-6 of the energy throughput | per-run check, reported |
| Events | located to the integrator's root-finding precision (7e-10 s at rtol 1e-10 in the spike) | event-time tests against closed forms |
| Units | every equation dimensionally consistent at build time | `lsim-prep` unit check |
| Licences | only permissive licences (MIT, Apache-2.0, BSD, ISC, Zlib …) | `scripts/licences.py`, cargo-deny |
| Platforms | Windows x86-64 only for now (the owner's decision, 10 October 2026); macOS and Linux later | Windows CI (WP6); development and most tests also run on Linux |

## 3. Choosing the solver: evidence

### 3.1 The test

`engine/experiments/solver-eval` (outside the workspace; `cargo run
--release` there) solves a stiff index-1 DAE with an exact answer:

* a battery — constant OCV 400 V, R0 = 0.05 Ω, an RC pair R1 = 0.01 Ω,
  C1 = 0.01 F — drives an ideal DC machine (k = 1 N·m/A) spinning an
  inertia (J = 20 kg·m², viscous loss 0.05 N·m·s/rad);
* when the speed first reaches 300 rad/s a brake clamps on with 2000 N·m
  (a state event that changes the dynamics);
* unknowns: RC voltage v1 and speed w (states) and current i (algebraic);
  poles at −12 000 and −0.84 1/s (stiffness ratio ≈ 1.4·10⁴);
* exact answer: x(t) = x_eq + exp(At)(x0 − x_eq) on each side of the event,
  the event time by Newton on that closed form (t_e = 1.669400463631983 s).

Each backend gets the exact Jacobian and its own root finding; results at
400 output times come from each one's dense output. Errors are relative to
each variable's largest magnitude. Times are medians of repeated solves on
a 4-CPU machine shared with other jobs (load 1–3), so they carry ±20 %
noise; counts are exact.

### 3.2 Accuracy and speed (2 states + 1 algebraic, one event)

| backend | rtol | worst error | event-time error | steps | f evals | LU setups | µs per solve |
|---|---|---|---|---|---|---|---|
| SUNDIALS CVODE (ODE form) | 1e-6 | 2.3e-6 | 6.2e-6 s | 179 | 229 | 4 | 94 |
| SUNDIALS IDA (DAE form) | 1e-6 | 1.5e-6 | 2.4e-6 s | 206 | 291 | 55 | 127 |
| diffsol BDF, ODE form | 1e-6 | 1.0e-6 | 3.8e-6 s | 203 | 256 | 56 | 98 |
| diffsol BDF, DAE (mass matrix) | 1e-6 | 1.6e-6 | 5.7e-6 s | 225 | 289 | 62 | 128 |
| CVODE | 1e-8 | 5.5e-8 | 6.6e-9 s | 310 | 390 | 6 | 150 |
| IDA | 1e-8 | 1.4e-8 | 5.1e-8 s | 380 | 492 | 66 | 204 |
| diffsol ODE | 1e-8 | 1.4e-8 | 3.5e-8 s | 350 | 417 | 62 | 145 |
| diffsol DAE | 1e-8 | 2.4e-8 | 8.6e-8 s | 357 | 443 | 79 | 179 |
| CVODE | 1e-10 | 5.0e-10 | 1.8e-9 s | 561 | 648 | 11 | 253 |
| IDA | 1e-10 | 1.8e-10 | 1.7e-10 s | 762 | 924 | 84 | 374 |
| diffsol ODE | 1e-10 | 5.5e-10 | 2.0e-9 s | 630 | 720 | 68 | 231 |
| diffsol DAE | 1e-10 | **failed**: step size too small at t = 0 | | | | | |
| diffsol DAE, smallest step lowered to 1e-20 | 1e-10 | 9.5e-11 | 3.3e-10 s | 670 | 782 | 97 | 468 |
| CVODE | 1e-12 | 2.0e-11 | 7.2e-11 s | 1131 | 1256 | 21 | 441 |
| IDA | 1e-12 | 3.3e-12 | 1.2e-11 s | 1398 | 1580 | 98 | 588 |
| diffsol ODE (step lowered) | 1e-12 | 1.0e-11 | 1.8e-11 s | 1186 | 1327 | 75 | 422 |
| diffsol DAE, faer LU (step lowered) | 1e-12 | **failed**: consistent initialisation (line search) | | | | | |

Rows for rtol 1e-4 and the faer-LU variants are in the experiment's output;
faer's dense LU is 2–4× slower than nalgebra's on a 3×3 system.

Findings: accuracy is the same class for both libraries (error ≈ rtol, event
times to a few rtol); speed on a small problem is the same within noise.
diffsol's mass-matrix DAE path fails at t = 0 for rtol ≤ 1e-10: its
consistent initialisation leaves the algebraic variables' derivatives at
zero, so the first steps' error estimates fail until the step is below its
fixed absolute minimum of 1e-13 s (lowering `min_timestep` fixes it; the
faer variant then fails in initialisation at 1e-12). SUNDIALS ran every case.

### 3.3 Scaling: a stiff torsional chain

N inertias joined by stiff spring-dampers (√(k/J) = 1000 rad/s), driven at
one end: 9, 49 and 199 states, no events, compared with CVODE at 1e-12.
Best of repeated solves:

| backend | states | rtol | steps | LU setups | ms per solve |
|---|---|---|---|---|---|
| CVODE, dense LU | 9 | 1e-8 | 894 | 94 | 0.53 |
| diffsol, nalgebra dense LU | 9 | 1e-8 | 1396 | 338 | 1.08 |
| CVODE, dense LU | 49 | 1e-8 | 2923 | 220 | 8.4 |
| CVODE, band LU | 49 | 1e-8 | 3912 | 278 | 8.5 |
| diffsol, dense LU | 49 | 1e-8 | 3099 | 600 | 17.4 |
| diffsol, faer sparse LU | 49 | 1e-8 | 3854 | 890 | 16.5 |
| CVODE, dense LU | 199 | 1e-6 | 3906 | 243 | 182 |
| CVODE, band LU (structure-exploiting) | 199 | 1e-6 | 4111 | 258 | 36 |
| diffsol, dense LU | 199 | 1e-6 | 4005 | 920 | 882 |
| diffsol, faer sparse LU | 199 | 1e-6 | 4015 | 875 | 67 |
| CVODE, band LU | 199 | 1e-8 | 8086 | 471 | 71 |
| diffsol, faer sparse LU | 199 | 1e-8 | 6987 | 988 | 97 |

Findings: the two take similar numbers of steps, but diffsol
re-factorised the Newton matrix 1.7–4.4× as often as CVODE on every chain
case except the smallest at 1e-6 (equal), and 3.6–14× as often on the DAE
test of 3.2. Both libraries' update rules read the same on paper (a new
factorisation after a 30 % step-size change or 20 steps); the difference
comes from diffsol's more frequent step-size changes and error-test
failures (1.2–5.5× CVODE's in 3.2), each of which forces one. IDA, for the
fully implicit form, factorises about as often as diffsol. Once the model
is large enough for the linear solve to dominate — a vehicle with its
driveline, thermal network and controllers — that is the cost that
matters: with a structure-exploiting factorisation CVODE was 1.4–1.9×
faster than diffsol's sparse LU on the 199-state chain.

### 3.4 Build friction

| | SUNDIALS via `sundials-sys` 0.6.2 | diffsol 0.17.1 |
|---|---|---|
| Source | SUNDIALS 7.1.1 C sources vendored in the crate (`build_libraries`); no download | pure Rust |
| Build needs | CMake ≥ 3.18, a C compiler, libclang (bindgen runs on every build) | Rust only |
| Cold build here (`-j 2`) | 45 s (the whole engine workspace, release, PyO3 and Cranelift included: 3 min 09 s) | 2 min 28 s (nalgebra + faer) |
| Windows | MSVC (already required by Rust's MSVC toolchain), CMake, LLVM for libclang | nothing extra |
| macOS | Xcode command-line tools (already required to link), CMake, libclang ships with them | nothing extra |
| Linux | gcc/clang, CMake, libclang | nothing extra |
| Sparse direct solver | KLU is LGPL (banned): needs our own `SUNLinearSolver` (faer sparse LU, MIT) | faer sparse LU built in |

The friction is real but one-off: work package 4 replaces `sundials-sys`
with an in-tree `lsim-sundials-sys` that compiles the vendored C sources
with the `cc` crate and ships pre-generated bindings — no CMake, no
libclang, only the C compiler every Rust target already needs.

**Done (work package 4).** `crates/lsim-sundials-sys` holds the SUNDIALS
7.1.1 sources of `sundials-sys` 0.6.2 that are built (CVODES, IDAS,
KINSOL, the serial vector, dense/band/sparse matrices, dense and band LU,
the Newton and fixed-point solvers; LICENSE and NOTICE kept), one
hand-written `sundials_config.h` for every platform (compiler-specific
settings chosen by the preprocessor), and the committed bindings, made by
`tools/bindgen` (the only thing that needs libclang, run when SUNDIALS is
upgraded; the bindings are portable across 64-bit targets: no layout
tests, C `long` kept as `c_long`, fixed-width integers spelled as Rust's,
`FILE` opaque). The C code is compiled at -O2 with floating-point
contraction off in every Cargo profile, so results do not depend on the
build profile or on fused multiply-add hardware. Measured on Linux: the C
part builds in about 20 s at `-j 2`; with CMake, clang, LLVM and bindgen
hidden from `PATH` and `LIBCLANG_PATH` unset, a rebuild and the spike's
tests pass, and bindgen, clang-sys, cmake and 20 other build-only crates
left the lockfile. The Windows build (MSVC, `/fp:precise`) is proven by
CI's "Engine (Rust) on Windows" job, which builds SUNDIALS and runs every
engine test; macOS is not built (Windows only for now, section 2). Beside the
SUNDIALS sources, `csrc/` holds our four small C helpers (BSD-3, like the
code they read): the dense output of CVODES and IDAS for selected
components only, with the same coefficients and summation order as
`CVodeGetDky`/`IDAGetDky` (a test checks they give the full dense
output's exact bits inside every step), so a sampled block's tick
interpolates only what it reads; and the polynomial that dense output
is over the last step, for selected components, as CVODES and IDAS hold
it (no arithmetic), which the mixed conditions' check encloses.

### 3.5 Decision

**SUNDIALS (CVODE for models without iteration variables, IDA for index-1
DAEs) is the production integrator; diffsol is a second backend behind the
same trait.**

* Robustness decides: SUNDIALS never failed, including at the tolerances
  of the one-click 10× tighter re-run; diffsol's DAE path did at default
  settings.
* Speed on real models decides: fewer Newton-matrix factorisations, the
  cost that grows with the model.
* "Proven" decides: CVODE/IDA are used by OpenModelica,
  PyBaMM (IDAKLU), CasADi and many FMU exporters; results comparable
  with other tools matter to users.
* The Rust-side cost is small: the run loop, event handling, consistent
  initialisation and output are ours (lsim-solve), not the backend's, so a
  backend is ~500 lines behind `Integrator` (the Stage 1 SUNDIALS one is
  `crates/lsim-solve/src/sundials.rs`).
* diffsol stays useful: an independent implementation for the accuracy
  suite (two integrators agreeing to tolerance), a fallback where a C
  compiler is unavailable (e.g. WebAssembly), and its forward/adjoint
  sensitivities for future calibration work. It is pinned (`=0.17.1`)
  because its API changes between minor versions.

## 4. Architecture

```text
 project JSON ──► lsim-project ──┐                       text format / Base Modelica
 (today's files)   (mapping)     │                         │  lsim-lang (parser, printer)
                                 ▼                         ▼
                      ComponentDef tree (lsim-ir) ◄── lsim-lib (connectors, physics,
                                 │                            vehicle blocks)
                                 ▼
       lsim-prep:  flatten → units → aliases → index reduction → matching → BLT
                   → tearing → symbolic solve → events/modes → sparsity → key
                                 │  PreparedModel (lsim-ir)          ▲
                                 ▼                                   │ cache hit
       lsim-codegen: Cranelift JIT ──► JitModel: ModelFunctions ─────┘ (lsim-engine)
                                 │
              ┌──────────────────┴────────────────────┐
              ▼                                       ▼
   lsim-solve (full dynamic)                lsim-fast (inverse model)
   SUNDIALS CVODE/IDA (+ diffsol)           Rosenbrock-W fixed steps
   events, init, sampled blocks,            limit flags
   recorder, energy, sweeps                          │
              └──────────────────┬────────────────────┘
                                 ▼
                    lsim-engine (build, run, sweep, cache)
                                 │
                    lsim-py ──► Python: lightsim package, app server, Script
                                blocks (sandbox), FMUs (FMPy), AI tools
```

### Crates

| crate | responsibility | owner (WP) |
|---|---|---|
| `lsim-ir` | units, expressions, component definitions, flat system, prepared model, compiled-model interface, diagnostics, reference interpreter — the contracts | WP1 (changes by agreement) |
| `lsim-lang` | the text format: parser, printer, Base Modelica import | WP1 |
| `lsim-prep` | flattening, unit check, alias elimination, matching, BLT, tearing, index reduction, symbolic solving, events/modes, init system, inverse-model preparation, structural diagnostics, structure key | WP2 |
| `lsim-codegen` | Cranelift code generation: residuals, Jacobians (forward mode, sparse/coloured), roots, outputs, event actions; runtime math and table library | WP3 |
| `lsim-solve` | `Integrator` trait, SUNDIALS backend (+ in-tree `lsim-sundials-sys`), diffsol backend, run loop, events, initialisation and homotopy, sampled blocks, recorder, energy quadratures, solver report, sweeps | WP4 |
| `lsim-lib` | connectors, physical primitives, signal blocks, the vehicle blocks | WP5 |
| `lsim-project` | today's project JSON → IR, block mappings, channel map, golden-comparison harness | WP5 |
| `lsim-fast` | fast mode: the inverse-model stepper and limit flags | WP6 |
| `lsim-engine` | the front door: build (with cache), set parameters, simulate, fast mode, sweep | WP6 |
| `lsim-py` | the Python module `lightsim_engine` (PyO3, maturin) | WP6 |
| `lsim-math` | the engine's mathematical library: CORE-MATH's correctly rounded functions, vendored and built for CPUs with and without FMA; the only transcendental functions the engine calls (section 7.1) | WP3 |
| `lsim-script` | the native runner of Script blocks: a strict subset of Python, its results the app's runner's bit for bit or left to Python (section 9) | WP6 |
| `experiments/` | one-off measurements outside the workspace (`solver-eval`, `libm-eval`) | – |

## 5. The IR

All types are in `crates/lsim-ir`; this section explains them, the code is
the contract.

### 5.1 Units (`units.rs`)

* A `Dim` holds the exponents of m, kg, s, A, K, mol, cd; a `Unit` is a
  dimension, a scale to SI and (°C only) an offset.
* Unit text follows Modelica's syntax (`N.m`, `kg.m2`, `m/s2`, `s-1`) and
  also accepts what people type (`N·m`, `kg·m²`, `m/s^2`, `km/h`, `°C`).
* **Numbers in the IR are SI.** A declared `unit` must be a coherent SI
  unit (scale 1, no offset) — as Base Modelica's `unit` attribute is in
  practice; `km/h`, `kW`, `rev/min`, `%`, `°C` are `display_unit`s,
  converted only where values enter or leave the engine.
* Angles are dimensionless (`rad` = 1); revolutions are explicit
  (`rev/min` = 2π/60 rad/s). Today's app shows rotational speed as `1/min`
  meaning rev/min, and acceleration in `g` meaning standard gravity: the
  importer maps them (`app_unit` in lsim-project), since read literally
  they would be off by 2π and confused with the gram.
* Every equation is checked for dimensional consistency when a model is
  prepared; a bare number takes the dimension its context needs in sums
  and comparisons and is dimensionless as a factor.

### 5.2 Expressions (`expr.rs`)

One tree for two scopes: component scope (names are text: `"p.v"`, `"R"`)
and flat scope (resolved: `Var(VarId)`, `Param(ParamId)`, `Der(VarId)`,
`Pre(VarId)`). Operators: `+ − × ÷ ^`, comparisons, `and/or/not`,
`if-then-else`, `noEvent`, and the built-ins `sin cos tan asin acos atan
atan2 sinh cosh tanh exp log sqrt abs sign min max`, all of Base
Modelica's scalar set, plus two engine built-ins:

* `limit(x, lo, hi)` — a saturation; full dynamic mode clamps, fast mode
  passes `x` through and flags the moments it is outside the band;
* table interpolation `Table { table, args }` — 1-D and 2-D tables held as
  runtime data (monotone cubic by default, so the result is C¹ and the
  solver needs no events at breakpoints; linear and the outside-data rules
  of today's `tableOutside` as options). In component scope `args[0]` is
  the table parameter's name and the rest are the abscissae
  (`expr::table("ocv", vec![soc])`, printed `ocv(soc)`); in flat scope
  `table` indexes `FlatSystem::tables` and `args` are the abscissae. Its
  `order` (`[0, 0]` for a value) reads a partial derivative of the
  interpolant instead, which only index reduction makes (section 6, step
  5; printed `table3'[1, 0](x, y)`).

The text format writes `a == b` and `a <> b`; the IR holds them as
`a >= b and a <= b` and `a < b or a > b` (no new comparison operators), and
`true`/`false` as 1 and 0.

**Tables** (`table.rs`): one `TableData` serves the language, preparation
and the code generator: breakpoints `x` and `y` (empty for 1-D), values
row-major (`values[i * y.len() + j]` at `(x[i], y[j])`), `interpolation`
(`MonotoneCubic` by default, or `Linear`), `outside` per axis (`Clamp` by
default, `Linear`, `Error`: today's `tableOutside`) and `axis_units` (the
axes' coherent SI unit texts). `TableData::check` says in plain words what
is malformed. The interpolant built from it (`interp.rs`, `interp::Table`:
Steffen's 1-D cubics, the monotone bicubic, linear; moved here from
`lsim-codegen`, which re-exports it) is the one every reader evaluates,
with its partial derivatives of any order (`eval_order`) and how many
times it is continuously differentiable (`continuity`); the interpreter
reads tables through `Env::table(k, order, args)` (`eval::interpolate`
over the interpolants).

### 5.3 Components (`component.rs`)

* `ConnectorDef`: an across quantity (equal at a connection: voltage `v`,
  speed `w`, velocity `v`, temperature `T`) and a through quantity
  (summing to zero: current `i`, torque `tau`, force `f`, heat flow `Q`),
  positive *into* the component, and how its power is formed (across ×
  through, or the through quantity itself for heat). Stage 1 defines
  `Pin`, `Flange` (rotational, speed-based so long runs carry no growing
  angle), `TFlange` (translational) and `HeatPort`. WP5 adds `FuelPort`
  (specific energy × mass flow) for tanks, engines and fuel cells.
* `PortDecl`: physical (of a connector type), or a causal signal `Input`/
  `Output` with a unit.
* `ParamDecl`: unit, display unit, default, min/max, `structural`
  (today's `variability: fixed`: may change the equations, part of the
  cache key; every other parameter is a runtime input). The default is a
  `ParamValue`: `Real(expr)` (a number or an expression of the same
  scope's parameters), `Bool` (structural), `Enum("Mode.Auto")` (an option
  of an enumeration type, structural; in equations it is its position
  counting from 1), or a table: `Table1D`, `Table2D` (default rules) or
  `Table(TableData)` (its own rules); `ParamValue::table()` gives any of
  them as `TableData`.
* `EnumType` (a name and its options) in `ComponentDef::types`, or in
  `Library::types` for types several components share; a name is looked
  up in the declaring component first (`Library::enum_ordinal`).
* `VarDecl`: unit, continuous or discrete, start value, `fixed`, nominal.
* `SubDecl` + `Connect`: composition. A composite (the battery made of a
  source, R0 and an RC pair; a whole vehicle) and a primitive are the same
  type; a project's diagram is one top-level `ComponentDef`.
* `EquationDecl`: `lhs = rhs`, `when cond then …` (assignments to
  discrete variables and `reinit` of states), `assert` (an error or a
  warning), each with a plain-words label that fault messages quote.
* `EnergyDecl`: stored energy and loss power, for the energy books.

### 5.4 The text format (`lsim-lang`)

A strict subset of Base Modelica (the flat Modelica of MCP-0031), so every
component maps one-to-one and Base Modelica import is the same parser with
more of the grammar. The printer exists now; it fixes the format by
example (this is `to_text` of the library's battery and brake):

```modelica
model Battery.OcvR0Rc "Equivalent-circuit battery: constant open-circuit voltage, …"
  connector p: Pin "positive terminal";
  connector n: Pin "negative terminal";
  parameter Real ocv(unit = "V") = 400 "open-circuit voltage";
  parameter Real r0(unit = "Ohm") = 0.05 "series resistance";
  parameter Real r1(unit = "Ohm") = 0.01 "RC pair resistance";
  parameter Real c1(unit = "F") = 0.01 "RC pair capacitance";
  Electrical.ConstantVoltage source(V = ocv);
  Electrical.Resistor r0(R = r0);
  Electrical.Resistor r1(R = r1);
  Electrical.Capacitor c1(C = c1);
equation
  connect(n, source.n);
  connect(source.p, r0.p);
  connect(r0.n, r1.p);
  connect(r0.n, c1.p);
  connect(r1.n, c1.n);
  connect(r1.n, p);
end Battery.OcvR0Rc;

model Rotational.ThresholdBrake "A brake that clamps on, for good, …"
  connector flange: Flange "the shaft";
  parameter Real tau_max(unit = "N.m") = 0 "torque once engaged";
  parameter Real w_on(unit = "rad/s") = 0 "the speed at which it engages";
  discrete Real engaged(unit = "1", start = 0, fixed = true) "1 once engaged";
equation
  flange.tau = tau_max * engaged "it takes its torque once engaged";
  when flange.w >= w_on then
    engaged = 1;
  end when "it engages when the speed reaches w_on";
  annotation(__LightSim_energy(loss = tau_max * engaged * flange.w));
end Rotational.ThresholdBrake;
```

(A sub-component and a parameter sharing a name, as `r0` does, is legal in
the IR's scoping but not in Modelica; WP1's parser rejects it and WP5
renames such parameters, as Stage 1 already did for the source.)

Energy books use the vendor annotation `__LightSim_energy`, which Base
Modelica tools ignore. Labels are the strings after equations. The
format's user documentation, with every declaration and error message,
is [`docs/text-format.md`](docs/text-format.md).

Users build their own components from the basic ones in this format (the
owner's decision), so it must say everything the library says. Tests
hold that: every library component, every vehicle block in every
combination of its on/off switches and in fast mode's inverse variants
(376 builds, 248 distinct definitions; the other settings at their
defaults), every definition
the example projects import, a component with every IR feature and random
expressions over every function print and read back unchanged; every
example on the documentation page parses, prepares, runs and gives the
numbers the page states (its Rust example is a doctest). `reinit` names a
state of the component, the across quantity of one of its ports
(`reinit(b.w, …)`: the state the port is joined to) or a variable of one
of its parts, as preparation resolves them; a through quantity, a
parameter or a discrete value is refused at parse time. The gear change
of `mech_gear_change` is written in the format
(`crates/lsim-lib/text/gear_change.mo`): the library's shifting gear as
text is the same definition as `lossy_gear` and runs bit for bit as the
model built in Rust; the same shift written with `reinit` matches it to
round-off, its loss counted as the event's, not the gearbox's.

### 5.5 The flat system (`flat.rs`)

Every variable, parameter and equation with its `Origin`: the instance
(path, definition, the diagram label and the app's element id) and the rule
that made it (the n-th equation of a definition, a connection set's across
or through equation, an unconnected port, a signal link). Origins are what
let every later stage speak about the user's parts. Parameters keep their
bindings (`r0.R = r0`) so changing a parent's value updates its children
without a rebuild (implemented in `Model::set_param`); they are laid out in
binding order, so one pass in order re-evaluates them. Also: `tables`
(one `FlatTable` per table parameter — name, instance, the parameter
(whose value is the table's index), the unit of its values and its
`TableData`; a part handed its parent's table shares it) and `asserts`
(`FlatAssert`: condition, message, error or warning, origin).

### 5.6 The prepared model (`prepared.rs`)

A **semi-explicit index-1 DAE** in `y = [x; z]`:

```text
x' = f(t, x, z, p, d, u)        one per state
0  = g(t, x, z, p, d, u)        one per iteration variable z
```

where every other unknown is computed, in order, by an explicit
`Assignment`. Explicitly solvable models have no `z` and are ODEs (CVODE);
an algebraic loop that could not be solved symbolically keeps only its
**tearing variables** in `z`, so the integrator's own Newton iteration
solves them with the step — no nested Newton inside the right-hand side,
and the exact Jacobian covers the loop. A state derivative that is itself
part of an implicit block (mass-matrix-like coupling, e.g. two inertias
through an ideal gear) becomes a `z` with `x' = z`, so the form is
universal. Also: the alias table (eliminated variables are still recorded
under their names), zero-crossing functions and `when` clauses, sampled
external blocks (`ExternalBlock`: a part whose definition's name begins
with `External.`, with a `period` parameter; its signal outputs are
discrete variables the host sets at each tick), and the structure key.

Since the work packages joined (WP2, WP3), the prepared model also holds:

| field | what | made by | used by |
|---|---|---|---|
| `jac_pattern: SparsityPattern` | the structure of `∂[x'; g]/∂y` through the assignments (CSC, `y` order; `n` = 0: not computed) | WP2 | WP3 (colouring; it checks the pattern covers its own), WP4 (sparse LU) |
| `modes: Vec<Mode>` | the `if` relations held as discrete Booleans (below) | WP2 | WP3, WP4 |
| `init: InitSystem` | the initialisation system (below) | WP2 | WP3 (`InitFunctions`), WP4 |
| `limits: Vec<LimitSite>` | inverse models: every `limit` passed through, with its bounds and origin | WP2 | WP6 (fast-mode flags) |
| `guards: Vec<ParamGuard>` | parameter expressions explicit solutions divide by: a parameter change that makes one zero needs a new preparation | WP2 | WP6 (`set_param`, sweeps) |
| `warnings: Vec<Diagnostic>` | what preparation noticed that does not stop a run | WP2 | WP6 (build report, the app) |
| `state_sets: Vec<StateSet>` | the choices index reduction made among states: each block's constraints (origins, words, parts), its candidates, their derivatives by each candidate in flat scope, the pivots, the columns left out (section 6, step 5) | WP2 | WP4 (the run's watch), WP6 (`set_param`, the cache) |

`inputs` (u): in an inverse model, for each prescribed variable in the
`InverseSpec`'s order, the variable and then its time derivatives as deep
as the model needs them (`body.v`, `der(body.v)`, `der(der(body.v))` where
index reduction differentiated twice); `PreparedModel::input_names()`
gives their names.

**Modes** (`Mode { var, relation, crossing, origin }`): a relation of the
equations outside `noEvent` (an `if` condition, the sign test of `abs`
and `sign`) becomes a discrete variable `var` (1 true, 0 false, one of
`discretes`) that the equations read instead, so the integrator never sees
a discontinuity. The contract:

* `zero_crossings[crossing]` is positive where the relation holds and
  negative where it does not (`lhs - rhs` for `>`, `>=`; `rhs - lhs` for
  `<`, `<=`): away from zero, `var = 1` exactly when the crossing is
  positive;
* at the start and after every event the run loop sets every mode from its
  relation (`ModelFunctions::modes`), which also decides the value at
  zero, and iterates events until nothing changes;
* preparation also adds two `when` clauses per mode — rising on `crossing`
  sets 1, falling on its copy at `crossing + 1` sets 0 — so a run loop
  that knows only `when` clauses keeps every mode right between events
  while the inputs are continuous. Where an input jumps (a prescribed
  speed's slope at a sample of its trace, so its derivative; a sampled
  block's output at its tick), a crossing that reads it jumps between two
  steps, where no root is to be found, and a crossing that is exactly 0
  as it starts to move shows no sign change at all: the run loop must
  then enter event mode at the jump (fire the clauses whose conditions
  became true across it, set the modes, iterate) and check every mode
  against its crossing after each step, as the forward run loop does and
  fast mode does (section 10).

**Initialisation** (`InitSystem { unknowns, guesses, assignments,
residuals, discrete_starts }`): the equations at the start time (the
model's, those index reduction differentiated, the initial equations and
the start values that must hold), sorted like the model: Newton iterates
on `unknowns` (first guesses: `guesses`, expressions of the parameters)
until `residuals` vanish, with `assignments` explicit in between; after a
solve every entry of `y` and every state derivative has a value.
`discrete_starts` are the discrete variables' start values (a mode's is its
relation at the solution). `is_empty()`: the start values hold as they
are. Preparation also writes the solved start values into the flat
variables' `start`, so a model without an initialisation solver starts
right too.

Contracts already written for the parallel work: `SparsityPattern`
(Jacobian structure, WP2 → WP3/WP4), `DiscreteBlock` (sampled blocks,
WP4 ↔ WP6), `InverseSpec` (fast mode, WP2 ↔ WP6), and the modes and
initialisation above (WP2 → WP3 → WP4).

### 5.7 The compiled model (`runtime.rs`)

```rust
pub trait ModelFunctions: Send + Sync {
    fn layout(&self) -> &Layout;   // n_x, n_z, n_p, n_d, n_u, n_roots, n_whens, n_vars, n_work
    fn residual(&self, inp: &EvalInput, work: &mut [f64], out: &mut [f64]);            // [x'; g]
    fn jvp(&self, inp: &EvalInput, v: &[f64], work: &mut [f64], out: &mut [f64]);      // (∂[x';g]/∂y)·v
    fn roots(&self, inp: &EvalInput, work: &mut [f64], out: &mut [f64]);
    fn vars(&self, inp: &EvalInput, work: &mut [f64], out: &mut [f64]);                // every channel
    fn when(&self, inp: &EvalInput, fired: &[f64], work: &mut [f64], d_out: &mut [f64]);
    fn start(&self, p: &[f64], y0: &mut [f64], d0: &mut [f64]);
    fn jacobian_dense(&self, inp: &EvalInput, work: &mut [f64], out: &mut [f64]) { /* n jvp's */ }
    // with defaults, so hand-written models need not implement them:
    fn sparsity(&self) -> Option<&SparsityPattern> { None }           // the CSC pattern jacobian_sparse fills
    fn jacobian_sparse(&self, inp: &EvalInput, work: &mut [f64], values: &mut [f64]) { /* from dense */ }
    fn modes(&self, inp: &EvalInput, work: &mut [f64], d_out: &mut [f64]) {}   // every mode from its relation
    fn init(&self) -> Option<&dyn InitFunctions> { None }              // the compiled InitSystem
    fn table_guard_list(&self) -> &[TableGuard] { &[] }                // the table axes the run loop watches
    fn table_guards(&self, inp: &EvalInput, work: &mut [f64], out: &mut [f64]) {}  // > 0 inside the data
    fn eval_table(&self, k: u32, args: [f64; 2]) -> Option<(f64, [f64; 2])> { None } // value, partials
    fn eval_table_order(&self, k: u32, order: [u8; 2], args: [f64; 2]) -> Option<(f64, [f64; 2])>
        { /* order [0, 0] by eval_table, else None */ }               // a partial derivative index reduction reads
    fn table_axes(&self, k: u32) -> Option<[Vec<f64>; 2]> { None }   // the breakpoints it interpolates
    fn condition_kernels(&self) -> Option<&dyn ConditionKernels> { None }  // compiled condition checks
    fn books_kernel(&self, spec: &BooksSpec) -> Option<Arc<dyn BooksKernel>> { None }  // compiled books
}

pub trait InitFunctions: Send + Sync {   // Newton on w, then y0
    fn n_w(&self) -> usize;
    fn guess(&self, p: &[f64], w0: &mut [f64]);
    fn residual(&self, inp: &EvalInput, work: &mut [f64], out: &mut [f64]);
    fn jvp(&self, inp: &EvalInput, v: &[f64], work: &mut [f64], out: &mut [f64]);
    fn sparsity(&self) -> &SparsityPattern;
    fn jacobian_sparse(&self, inp: &EvalInput, work: &mut [f64], values: &mut [f64]);
    fn finish(&self, inp: &EvalInput, work: &mut [f64], y0: &mut [f64]);
}
```

`EvalInput { t, y, p, d, u }`. The functions are pure; the caller owns the
buffers, so one compiled model serves any number of simultaneous runs.
`TableGuard { table, axis, outside }` names one watched table axis: its
guard is `min(a - lo, hi - a)` of the axis argument, positive inside the
data; an `Error` axis stops the run where its guard falls through zero,
the others are booked as time spent outside.

### 5.8 Shared types: what each package must do

The shared types above were unified on the branch `engine/ir-unify`
(work packages 1, 2 and 3 merged; every addition relative to Stage 1 is
additive). What the other packages change to use them:

**WP4 (lsim-solve)**

* `RunInfo::from_prepared`: fill `modes` from `PreparedModel::modes`
  (`ModeInfo { crossing: m.crossing, discrete: position of m.var in
  discretes, label }`); the rule `d = roots[crossing] > 0` holds by the
  contract above. The mode's two `when` clauses (on `crossing` and
  `crossing + 1`) set the same values, so handling both is harmless.
* At the start and after each event, call `ModelFunctions::modes` before
  event iteration (it decides the value exactly at zero).
* Initialise with `ModelFunctions::init()` when it is `Some`: damped
  Newton on `w` (`guess`, `residual`, `jacobian_sparse` on `sparsity`),
  then `finish` gives `y0`; keep homotopy and `IDACalcIC` after it.
* Use `PreparedModel::jac_pattern` (when `n > 0`) or
  `ModelFunctions::sparsity()` for the sparse LU, and
  `jacobian_sparse` for its values.
* Watch `table_guard_list()`/`table_guards()` as extra root functions:
  stop with the table's name at an `Error` axis, book time outside the
  others.
* Check `FlatSystem::asserts` at accepted steps (evaluate with
  `lsim_ir::eval` over the `vars` output until a compiled function
  exists): stop on an error, warn once on a warning.
* A `when` clause whose function stays exactly zero after its event (a
  held voltage) makes IDA fail with "root found at and very near t";
  deactivate such a root after the event as CVODE does.

**WP5 (lsim-lib, lsim-project)**

* Tables: make table parameters with `ParamValue::Table1D`/`Table2D`, or
  `ParamValue::Table(TableData { … })` to set `interpolation: Linear` (as
  today's app) or `outside` per axis (`tableOutside`); read them with
  `lsim_ir::expr::table("loss_map", vec![w, tau])`. A `TableData` is a
  rectangular grid: resample today's table2d sheets onto the union of
  their inner points (exact for linear interpolation). This replaces the
  stand-in expansion in `lsim-lib/src/table.rs`.
* Enumerations: `EnumType` in `ComponentDef::types` (or
  `Library::types`), parameters `ParamValue::Enum("Gearbox.Mode.Auto")`.
* Rename `Battery.OcvR0Rc`'s parameters `r0`, `r1`, `c1`, which share
  names with its parts (the text format rejects the clash).
* Sampled blocks: a definition named `External.…` with signal ports and a
  `period` parameter.
* Any exhaustive `match` on `ParamValue` gains the `Table2D` and `Table`
  arms (or uses `ParamValue::table()`).

**WP6 (lsim-fast, lsim-engine, lsim-py)**

* Fill an inverse model's `u` in `PreparedModel::input_names()` order:
  `InverseSpec::input_names()` (value, then `der()`) misses a second
  derivative where index reduction needs one.
* Take fast mode's limit sites from `PreparedModel::limits` (their bounds
  and origins) instead of searching the equations for `limit`.
* `Model::set_param`: a table parameter's value is its table's index
  (change table data with `JitModel::with_tables`, no recompile); when a
  parameter change makes a `ParamGuard` zero, or makes the choice of
  states fail at the start (`state_sets`), prepare again.
* Show `PreparedModel::warnings` in the build report and the Python
  `model.report`.
* Done: the `Started` wrapper in lsim-engine forwards every method of
  `ModelFunctions` (WP6, and `eval_table_order` and `books_kernel` at the
  merge); `started.rs`'s test fails if the trait gains one it does not
  name.

**WP3 (lsim-codegen): the order of the zero crossings.** The compiled
`roots` evaluates `PreparedModel::zero_crossings` in their order, one
output each (`Layout::n_roots` is their number), the table guards after
them in `table_guard_list()` order. The run loop indexes every
per-crossing table by that order and nothing else: `RunInfo::time_crossings`
(a crossing it schedules as an exact time event instead of watching it),
the root directions and sides, the modes' and the `when` clauses'
`crossing`, the root mask it hands the integrator. A code generator that
reorders, merges or drops crossings breaks all of them without a
compile error; `compiled_roots_follow_the_zero_crossings_order` in
`lsim-solve/tests/events.rs` checks the contract on a compiled model
(time crossings at distinct times around a state crossing and a mode).
`PreparedWhen::strict` needs nothing from the code generator: the run loop
reads it.

**WP3 (lsim-codegen): `ModelFunctions` in full.** The run loop calls every
method of the trait, and a compiled model implements every one: `layout`,
`residual`, `jvp`, `roots`, `vars`, `when`, `start`, `modes` (the modes
from their relations), `init` (the compiled initialisation), `sparsity`
and `jacobian_sparse` (the coloured Jacobian; `jacobian_dense` from them
or from `jvp`), `table_guard_list` and `table_guards` (the watched table
axes), `eval_table` (a table as the compiled code interpolates it, value
and partial derivatives: the energy books, the conditions on functions of
time, the asserts and the impulse projection evaluate flat expressions
outside the compiled code and read the tables through it;
`eval_table_order` the same for a partial derivative, which index
reduction's equations read: the default gives values only), and
`table_axes` (the breakpoints of the data it interpolates, which the run
loop's stops and enclosures follow when a model's tables are swapped after
preparation). The defaults are those of a model without such things: no
modes, no compiled initialisation, a dense Jacobian from `jvp`, no guards,
no tables (`None`). A wrapper around a compiled model (a model started at
other values, a sweep's set) must forward every method: left at a default,
modes stop switching, table guards and the compiled initialisation vanish
without an error, a run whose books, conditions, time events or impulse
projection read a table does not start ("the model does not give its
tables": every expression the run evaluates outside the compiled code is
checked at the start), and a model given other table data is run on the
prepared breakpoints (its stops and enclosures in the wrong places; the
run warns, once per table it stops at or encloses, when the model gives a
table's values, `eval_table`, but not its breakpoints, `table_axes`). The
plan for the long term: one accessor, `table(k) -> Option<&dyn TableView>`
(the values, the partial derivatives and the breakpoints together), so
that a wrapper passes a table on whole or not at all; it replaces
`eval_table` and `table_axes` when WP6 reworks `Started`, with a WP6 test
that a started model gives its tables' values and breakpoints.

**WP3 (lsim-codegen): compiled condition kernels.** The run loop checks
every step of a condition that reads time and continuous variables
(section 8.2) by interpreting it from the IR: `lsim_ir::eval` for a point,
`interval::enclose` for an enclosure over an interval of time or a box
of states (value, rate and second rate). On the BEV's WLTC that check
takes 3.3 % of the run's instructions (4.8 % of its time), and its share
grows as the compiled model gets faster. WP3 compiles it, under this
contract (proposed by the reviews of work package 4's seventh to ninth
rounds), before it starts on it:

* **The interface**, in `lsim-ir/src/runtime.rs` (added, additively),
  with the interpreter as the default:

  ```rust
  /// [lo, hi] of a value and of its first two rates over an interval,
  /// rounded outwards (lsim-solve's `J2`)
  pub struct Enclosure { pub v: [f64; 2], pub d: [f64; 2], pub dd: [f64; 2] }

  pub trait ConditionKernels: Send + Sync {
      /// whether zero-crossing function k is compiled (one the run loop
      /// checks along steps: `RunInfo::time_functions[k]` is `Mixed`)
      fn covers(&self, k: usize) -> bool;
      /// the scratch `point` and `enclose` need: (values, enclosures)
      fn scratch(&self) -> (usize, usize);
      /// condition k (its chain of assignments, then the condition) at
      /// a point: `inp` as for `roots`
      fn point(&self, k: usize, inp: &EvalInput, work: &mut [f64]) -> f64;
      /// condition k over the times `t`, each entry of y it reads given
      /// by its enclosure there (`y[i]`, one per entry of y; the others
      /// are not read), at the discrete values, parameters and inputs
      fn enclose(&self, k: usize, t: [f64; 2], y: &[Enclosure], d: &[f64], p: &[f64],
                 u: &[f64], work: &mut [Enclosure]) -> Enclosure;
  }

  // ModelFunctions gains, with the default None (the interpreter):
  fn condition_kernels(&self) -> Option<&dyn ConditionKernels> { None }
  ```

  Conditions are indexed by their zero-crossing function (the order of
  the zero crossings, above). The run loop gives the leaves as it does
  the interpreter's: along a step a state's dense-output polynomial
  (value and rates), in a certificate's box the box with its rates
  unknown; the caller owns the scratch, so one compiled model serves
  any number of runs.
* **Bitwise the interpreter.** `point` is bitwise the interpreter's value
  of the chain and the condition at the same inputs, and the model's
  compiled `roots` output for that crossing is bitwise the same: the
  IR's order of operations, no fused multiply-add contraction, the same
  library functions as the interpreter calls (`lsim_ir::math`, the
  engine's own: correctly rounded, so that the interval code's widening of
  library results by one ulp is a proof; section 7.1). `enclose` keeps the
  IR's order too, with `interval.rs`'s rounding rules (each operation
  rounded outwards as there, library results widened by its one ulp): its
  enclosure is bitwise the interpreter's, so it holds the point values
  and is exactly as wide.
* **Shared subexpressions** across the conditions (a motor's limits read
  45 and 49 assignments, most of them shared): computed once, which
  changes no result.
* **Tables read at run time.** The kernels read a table's values and its
  breakpoints (the pieces of a 1-D table's enclosure, a 2-D table's
  cells) from the model's table runtime when they run, never baked into
  the code: a model's tables may be swapped after compilation
  (`JitModel::with_tables`). `eval_table` and `table_axes` are
  implemented.
* **Declining.** A condition a kernel cannot handle is not covered
  (`covers(k)` false) and is interpreted, with the same warnings as
  today: what cannot be searched is still decided from the IR by the run
  loop, not by the kernels.
* **The zero-crossing order** is kept (above).
* **A differential test**: at sampled points, intervals and boxes, on
  every condition of the golden models and on random expressions,
  `point` and `enclose` are bitwise the interpreter's, the enclosures
  contain the point values, and `roots` is bitwise `point`.
* **Acceptance**: the golden comparison byte-identical with the kernels
  on and off (a switch in `SolverOptions`; off is the interpreter), with
  identical `mixed_certified`, `mixed_scanned` and `pulses_found`; and,
  after WP3's speed-up of the model itself, the check at most 3 % of the
  BEV WLTC run's instructions under callgrind (`scan_mixed`'s inclusive
  share), with its share of the run's time from lsim-project's
  `scan_share` example (`SolverOptions::time_mixed_checks`, the median of
  five runs) reported beside it.

**WP3 (lsim-codegen): compiled energy books.** The books' integrands
(lsim-solve's `energy::Integrand`: each part's power in and its absolute
value, its loss, its stored energy's rate along `(1, y')`) are compiled
under the contract of the condition kernels:

* **The interface**, in `lsim-ir/src/runtime.rs` (additive): the run
  describes its integrands as a `BooksSpec` (the parts' expressions,
  where each integrand goes, the chain of assignments the rates go
  through, each variable's `RateSource`) and asks
  `ModelFunctions::books_kernel(spec)` for a `BooksKernel` (default
  `None`: interpreted). `BooksKernel::cover(j)` says what it computes of
  part `j`; the run interprets the rest. `SolverOptions::books_kernels`
  (on by default) switches it; `LSIM_BOOKS_KERNELS=0` in the golden and
  `scan_share` runners.
* **Bitwise the interpreter.** `lsim-codegen/src/books.rs` lowers the
  values by `lsim_ir::eval`'s rules and the rates by `ad::dual`'s (which
  differ where they must: `min`/`max`/`limit` pick by `<` and `>`, a zero
  direction has a zero rate, a power whose exponent does not move takes
  the constant-exponent rule, a table's rate is `Iterator::sum` of its
  partial derivatives times the arguments' rates), in the IR's order, the
  engine's library, the model's own tables; rates known to be constant
  (zero, time's one) are folded with the same IEEE operations. A NaN is a
  NaN on both sides (its sign and payload are not defined: the optimiser
  may drop the negations around a product). An integrand that reads what
  the interpreter's dual numbers give up on (a derivative, a previous
  value, a name, a variable with no rate, a table read with other than
  one or two arguments, a read of a table's partial derivative (order
  other than `[0, 0]`, as the condition kernels decline it), a call with
  other than its arguments) is declined
  and interpreted, wherever in it that sits.
* **Tiers.** The model compiles a spec when a run asks, keeps it (by
  spec, compared to the bit) for the next run, and returns it on a tape
  at once; another thread compiles its machine code, to which it switches
  (bitwise the same; tapes for good where executable memory is refused).
  The default floating-point environment while compiling and running.
* **Tests** (`lsim-codegen/tests/books.rs`): every example project's
  books at the operating points of a 120 s drive, 80 random models with
  random books (3 200 points of values and rates with zeros of both
  signs, infinities and NaNs), directed edge cases (constant rates,
  signed zeros, powers at zero, `atan2` with one rate constant, ties and
  NaNs in `min`/`max`/`limit`, a 2-D table read with one argument, exact
  rates declined beside compiled ones) and the kernels kept per spec to
  the bit; on the tape and as machine code against the interpreter. Of the
  example cars' books every power and every stored energy's exact rate
  compiles (the test holds them to it).
* **Acceptance.** The golden comparison byte-identical with the books
  compiled and interpreted, and with the code before them (all 14 cases:
  199 of 201 figures, 984 of 1038 channels, closures unchanged, 1.5e-9 at
  worst). The BEV's WLTC, release, one machine, five runs each: 4.2–4.8 s
  before, 3.3–3.8 s after (medians 4.4 and 3.4 s; about 410× and 525×
  real time); the hybrid's cycles 11–16 % shorter in the golden runs.

**WP3** keeps: `InitFunctions::guess` uses the flat start values rather
than `InitSystem::guesses`, and asserts have no compiled function yet.
Its tapes are wired in: the initialisation, the point kernels and tiered
models run on them (section 7).

**WP3 (lsim-codegen): what a run's model functions cost, and what does
not come from them.** On the BEV's WLTC (215 669 steps), the compiled
model's functions take 28 % of the run's instructions (residual 9.9 %,
channels 8.8 %, zero crossings 6.8 %, tables 9.4 % within them). The
energy books' integrands took 22 %: `lsim-solve` interpreted their flat
expressions (`lsim_ir::eval`, dual numbers for the rates) at every
quadrature call. They are now compiled (below, *compiled energy books*):
the run takes 18.76 billion instructions instead of 22.41 (−16 %), the
books 7.5 % of it, of which the channels they read (`vars`) 5.3 % and the
compiled integrands 0.9 %. SUNDIALS (Newton iterations, dense LU, vector
operations, root checks) takes most of the rest, the condition check
3.6 %. Before the native Script runner (WP6), the hybrid's UDDS (about 92 s
here) spent about two fifths of its time waiting on its Script
controller's Python host (stack samples), and makes 3.0 million steps
and 118 000 events: the controller's outputs change 114 000 times, each
change restarting the integrator.

## 6. Preparation pipeline

`lsim_prep::prepare(lib, top, opts) -> Result<PreparedModel, Vec<Diagnostic>>`.
Stage 1 implements steps 1–4, 6, 7 (simplified) and 11, and step 8 for
`when` clauses; WP2 the rest.

1. **Flatten.** Instantiate the tree; give each parameter, port variable
   and variable a flat record; resolve names; evaluate parameter values in
   SI and keep bindings. Connections form connection sets by union–find
   over (port, inside/outside) nodes: across quantities equal, through
   quantities summed with Modelica's sign rule (a composite's own port,
   seen from inside, enters negated). A physical port with nothing
   connected carries no flow; a signal input takes its single driving
   output (none or two is an error naming the part).
2. **Units.** Every equation and event assignment balances its dimensions.
3. **Aliases.** `a = ±b`, `a + b = 0` and `a = c` remove a variable and an
   equation each, repeated to a fixed point (constants create new aliases).
   States are kept as representatives; two states are never merged (that
   is a constraint for index reduction) and a state never becomes a
   constant. In the spike, 32 of 49 variables go. A kept variable takes
   its start value (a guess) from the variables made one with it, its own
   included, with their sign (a battery's voltage guess reaches the bus
   its node's port carries): a fixed one first, then the guess farthest
   from zero, then the first in the model's order, so the choice depends
   neither on the order the parts are listed in nor on which variable is
   kept; guesses that disagree are told (`START-ALIAS-CONFLICT`).
4. **Matching.** Unknowns are the non-state variables and the states'
   derivatives. Stage 1: Kuhn's augmenting paths (iterative) with a greedy
   start. WP2: Hopcroft–Karp (O(E√V)) for 10⁵-equation models.
5. **Index reduction** (WP2). When the matching fails only because states
   are constrained (two inertias rigidly coupled, a speed prescribed in fast
   mode, a capacitor across a voltage source), Pantelides' algorithm
   differentiates the minimal structurally singular subsets; dummy
   derivatives (Mattsson–Söderlind) choose which states to demote, by
   pivoting on the Jacobian at the start (in each row, among the columns
   within a factor 10 of its largest entry, the most preferred), and again
   at a point of a run where that choice has degraded (dynamic state
   selection, *the choice made again* below; risk R3).
   Symbolic time differentiation of equations is in `symbolic.rs`'s rule
   set, with `limit` and tables.

   **`stateSelect`** (Modelica's, read from the text format and Base
   Modelica; before, read and ignored) is the preference's first key:
   `never` and `avoid` are demoted before the default, `prefer` is kept
   before it, and `always` (like the continuous part of a state a
   `reinit` restarts) is left out of the choice unless no regular choice
   does without it; then a fixed start value, then declared variables
   before derivatives, then the model's order. A mark on x carries to its
   speed where an equation makes one the other's derivative (`der(x) =
   v`), and through aliases (marks that pull both ways are told:
   `STATE-SELECT-CONFLICT`). A marked column competes within a factor 10
   only with the columns marked as it is or weaker, and within a factor
   50 with every column, and the choice the marks (or a `reinit`) ask for
   is taken only where, the entries moving with the solution, no candidate
   it decides moves more than 50 times as much as a state (a parameter's
   scale is no degeneracy): else the choice by pivoting alone. That is
   half the run's limit (below; `lsim_prep::index::MARGIN`), as pivoting's
   own window of 10 is a tenth of it: a choice that starts at the run's
   limit itself stops at its first wobble at one tolerance and not at
   another (a pendulum's turning point, where the factor is largest,
   reached a little further on by the integrator's error). So a marked
   choice may carry up to 50 times a state's error in what it decides,
   where pivoting's carries up to 10: the pendulum with x marked `prefer`
   at 1.55 rad (48 at the release) ends up to 22 times the angle form's
   error over five periods (section 15), at atan 45 290 to 2 040 rtol
   off (`state_choice.rs`). Before the margin (the marks' window at the
   run's limit), at atan 99 the run stopped at its far turning point at
   rtol 1e-5 and ended 297 to 3 664 rtol off without a word at the six
   other tolerances (3 to 18 times the angle form's); now the mark is
   refused there and the choice by pivoting stops near the bottom of the
   swing at every tolerance. Where only the margin kept a marked
   variable from being a state, that is told (`STATE-SELECT-PREFER`, or
   `STATE-SELECT-ALWAYS` with that reason; for a restarted variable,
   `REINIT-NOT-STATE`); `always` that no regular choice keeps, and `never`
   that has to be a state, are told (`STATE-SELECT-ALWAYS`,
   `STATE-SELECT-NEVER`). The judge's pendulum released at 1.5 rad keeps
   y and vy by pivoting (y would move 14 times as much as x there), which
   degenerate at the bottom of the swing; with x marked `prefer` it keeps
   x and vx, which hold for the whole swing.

   **The choice, watched** (`lsim_solve::states`; the measure in
   `lsim_ir::choice`). The choice goes with the prepared model
   (`PreparedModel::state_sets`: each block's constraints with their
   words and parts, its candidates, their derivatives by each candidate
   in flat scope, the pivots), so a cached model carries it. With `M` a
   block's constraints differentiated by its candidates and `S` the
   chosen columns, `B = M_S⁻¹ M` holds how much each decided candidate
   moves per unit of each state candidate while the constraints hold
   (`|dx/dy|` for a pendulum whose rod decides x from y); its largest
   entry outside `S` is the factor by which exchanging that pair would
   enlarge the block's determinant (a basis exchange), whatever the order
   or the form the constraints are written in. Where the run has no other
   choice to go on with (its model was given none, or none works there:
   *the choice made again*, below), it stops, naming the constraint, the
   variables, the parts and what to do (`SolveError::StateSelection`,
   `RUN-STATE-SELECT` in Python), where that factor passes 100 (ten times the factor 10 the choice allows at the
   start: the decided variable then carries a hundred times a state's
   error, and a little further on the choice decides nothing), or where
   the block's determinant changes sign between two step ends with no
   event between them (it passed through zero inside the step). The first
   point inside the step where either happens is found by bisection on
   the step's dense output, so the run stops at the same point whatever
   its tolerance, up to the solution's own error there: the pendulum
   released level, where |x| first falls to |y| / 100 (2.26 ms before the
   bottom), to 1.8e-4 s at rtol 1e-4 and 5.9e-10 s at 1e-10 of the closed
   form; the looping one, where the rod nears the horizontal, to 8.2e-6
   and 1.6e-10 s (`state_choice.rs`). An event ends the stretch the signs
   are compared within, and the point right after it starts the next: the
   run gives the watch the point before the event (a step's end, or a
   block's tick inside a step) and the point after it, each checked as a
   step's end, so the step after an event is watched as any other, its
   sign compared with the point after the event and its dense output
   searched from there; a tick whose outputs reach nothing the integrator
   integrates (the step stands) changes nothing the choice's entries read,
   and the stretch goes on across it. Before (the watch's first version), the step
   after any event, and every step with a tick inside, went unwatched: the
   pendulum released at 2.8 rad with a `when` that changed nothing 2.3 ms
   before its states degenerate crossed the horizontal in that step and
   ended 2.45e-2 m off without a word (rtol 1e-4), as with a display block
   ticking every 3 to 10 ms, and at 1e-5 with ticks every 5 ms it stopped
   1.6e-3 s late. Now such runs stop where the run without the event
   stops, to the last bit where the integration goes on across it; one
   whose event restarts the integrator there stops on the same branch,
   before the rod is level, where its own solution degenerates (restarted
   that close to the point, at order 1, the solution is a few hundred
   rtol off at 1e-10: near the point the error test on x sees the
   decided y's error divided by the factor; `state_choice.rs`). A
   full check at a step's end (the factor, the sign) is skipped where a
   bound on the entries' change since the last one, carried through the
   block's inverse there, shows the verdict cannot have changed; the
   entries that move are compiled with the energy books' kernel, which
   compiles any expression of the channels bitwise as the interpreter
   evaluates it. The factor is in the candidates' own units, as the
   pivoting is: a block whose entries move only at events (parameters,
   tables, discrete values: a gear ratio of 1:150 is a scale, no
   degeneracy) is judged only by whether it is singular, at the start and
   after the events that change what it reads. At the start of every run
   (a parameter set on a built model, a model from the cache) the choice
   is checked at the solved start, where the constraints' independence is
   checked too (their smallest pivot, columns and rows scaled, below 1e-2:
   a singular position, bars in line), and `Model::set_param` and
   `params_with` refuse values at whose start it does not work, as they
   refuse a broken `ParamGuard` (the merge review's pendulum built at
   0.4 rad and set to release level: before, IDA's flag −4 or, from the
   cache, a pendulum that never fell). An integrator that fails where the
   factor is beyond the choice's own window (10), or the constraints are
   near dependent (below 0.1), is told as the choice's failure, its words
   beside it; any other failure of SUNDIALS anywhere in the run is told
   with what the run sees where the step started (the iteration
   variables' Jacobian not a number, or singular, naming the equation and
   the variable; else the variable with the largest local error and the
   equations that decide it, and what can cause it), never by its flag
   alone. The watch is the full dynamic run's: fast mode steps its inverse
   model with its own Newton solver, which names the variables a singular
   Jacobian leaves free (section 10), and the example cars' inverse
   blocks read only ratios. Its cost, counted in instructions against the run before it: +0.04 % on the BEV's WLTC (its blocks read only ratios: the point each step starts from is all it keeps), +0.45 % on the hybrid's mixed cycle (its gearbox's block of 9 constraints, read at every step), +8.5 % on Andrews' mechanism and +14 % on MSL's PlanarFourbar (blocks of 13 to 22 constraints whose entries are sines and cosines of the angles, evaluated at every step: most of it those functions). The points on either side of each event (the channels after it, and a check at each) add 0.47 % on the hybrid's mixed cycle (its 56 218 events), about 7 200 instructions an event; the BEV's WLTC takes 0.14 % fewer (a step's start with no event before it is no longer looked at); the whole golden comparison's CPU time the same (91.85 and 91.60 s before, 92.63 and 91.70 s after, back to back).

   **The choice made again** (dynamic state selection, `lsim_solve::choice`;
   as Dymola and OpenModelica choose the states again at an event, after
   Mattsson, Olsson and Elmqvist, *Dynamic selection of states in Dymola*,
   Modelica 2000). Where the watch finds that a set's factor (an exchange
   with a state the choice may take; one the choice keeps above all is
   no exchange it may make) has passed 3 (`SolverOptions::state_switch`)
   times its factor where the entries that do not move were last read
   (the start, the last switch, an event that changed a discrete value
   they read), where that is above one, or that the block's determinant
   changed sign, the run chooses that set's states again there and goes
   on with them, every other set keeping its choice. Measured from where
   the choice started, a scale is no
   degeneracy: the hybrid car's clutch set starts at 3.9 (its gears'
   ratios move a decided speed that many times as much as a state), and a
   plain threshold of 3 had it switch at t = 0 and changed the golden
   comparison; now no golden case switches. While a choice keeps a column
   the model marks to be kept (`prefer`, `always`, a state a `reinit`
   restarts), the marks lead it and it is made again only at 50, the
   margin preparation takes a marked choice within (`MARKED`); a choice
   the marks could not have (x marked `prefer` at atan 99, refused at the
   start) is made again at 3 like any other. The point is the step's end,
   where the integrator's corrector holds the solution, unless the step
   went past it (its block's sign changed in it, or its factor there is
   beyond the choice's own window, 10): then the first point where the
   factor passes the threshold, by bisection on the step's dense output,
   the step cut back there before its outputs and ticks are taken (cut
   back every time, the run went on from interpolated iteration variables
   and the judge's pendulums came out further off: section 15); a tick
   before the point that restarts the run leaves the switch to the point
   after it, where the watch looks again (`state_choice.rs`).
   The choice there is preparation's (`lsim_prep::prepare_choosing`: the
   same flattening, aliases and differentiation, the dummy derivatives
   pivoted at the run's point by the same rule and the same `stateSelect`
   classes as at the start, `never` and `always` respected; a value the
   point does not give reads 0) for the set that degraded and the sets of
   its constraints at index reduction's other levels (their candidates
   follow from its choice), their window that set's switch threshold so
   the choice the run leaves is not taken again; every other set keeps
   the choice the run has (`ChoicePoint::keep`, `again`: a set is known
   across preparations by index reduction's own numbering of its
   equations and candidates, `StateSet::eqs`, `nodes`, and the equations
   they were differentiated from, `roots`), where that choice is regular
   at the point. The channels the run records are kept as variables of
   their own (a derivative channel that is a state's derivative in the new
   choice is set from it); each choice is compiled once and kept with the
   model (`Rechooser`, the 64 used last; sweeps share it; one compiled by
   two runs at once is kept once; one the run holds a model for, the one
   it started with or one it switched to, is not compiled again: before,
   a run of more than 64 choices compiled again those it held, 4 to 8 ms
   each). The run goes on with it where it is
   another choice, has the same events, discrete values, inputs, tables
   and energy parts, holds every recorded channel, finds every one of its
   states and iteration variables in the point, and holds there: each
   moving set regular and its constraints apart; each set chosen again
   under its base threshold, as a choice that starts there (3, or 50
   while it keeps a column the model marks; held instead to the threshold
   it had, up to 3 times a start's factor of 10, three runs of the sweep
   took choices with factors of 10 to 27, at 1.68, 1.76 and 1.86 rad, left
   them again within 0.03 s, and ended 47, 764 and 107 rtol off against
   181, 281 and 28); each set whose choice was kept inside the run's limit,
   its threshold carried over (the switch is no reading of its fixed
   entries). Then a new integrator on the new
   states from the solution at the point, its iteration variables solved
   again with the new states held (as at any event: the constraints and
   their derivatives met), the energy integrals carried over and the
   stored energy's change booked as an event's (the books close as
   before: the pendulum's heights within 22 rtol across 17 switches, its
   jumps 0.1 rtol, `state_choice.rs`), the recorded channels mapped by
   name, the sampled blocks' clocks, the conditions on time and the watch
   set up again, and an event recorded (`EventKind::States`, "the states
   chosen again for the model (the rod keeps its length): x, vx in place
   of y, vy"). Otherwise it goes on with the states it has, the threshold
   of the set to blame (the one chosen again, or a kept set past the run's
   limit or whose constraints are not apart there) raised to twice its
   own factor there, back to its threshold where
   its factor falls under that again (hysteresis), and the watch stops it
   where they degrade past the run's limit, naming the constraint, as
   without the model's choices (bit for bit, `state_choice.rs`). A switch
   is readied, the integrator started on the new states and their holding
   checked, before the step is cut back to its point, so a refused one
   leaves the run as it was: the step not cut, its end checked as any
   step's end, the run stopping where the run without the model's choices
   stops, to the bit, where no switch holds (`state_choice.rs`; before, a
   refusal in a step cut back to its point restarted the integrator there
   from the step's dense output, 1 370 rtol off that stop at rtol 1e-10
   near the pendulum's degenerate point, and a refusal at a step's end left
   that point's holds, kink and stale-mode checks undone); a tick at the
   point that changes the discrete values readies it again with them, and
   an event there leaves it to the point after the event. One path does
   not leave the run as it was: a switch readied in a step cut back to its
   point, a block's tick at exactly that point setting a discrete value,
   and the switch, readied again with it, refused. The integrator is past
   the point and the run's results are recorded to it, so it restarts
   there from the step's dense output, and the run says so in its notes.
   Measured on the pendulum released at 1.50 rad, with the switch
   threshold at 9.5 so that its first switch cuts its step back: 1 209,
   5 076 and 11 712 rtol off the closed form at rtol 1e-4, 1e-5 and 1e-6.
   The same switch refused at the step's look, with no cut and no restart,
   is 388, 267 and 92 rtol off, and the run that switches there 362, 314
   and 37: the restart from interpolated values on a choice that has
   degraded costs that much. The path needs a tick to fall exactly on the
   point by coincidence (`a_switch_refused_after_a_tick_at_its_cut_point_…`).
   A cut itself is not rare: across the judge 215 of its 20 581 switches
   (1.0 %, in 31 cases: every run released at 1.68 to 1.86 rad cuts one,
   the pendulum with x marked `prefer` at atan 99 eight a run) cut their
   step back, none of them at a tick, as the judge has no sampled blocks
   (the 426 switches of the pendulums released at 1.50, 2.8 and 3.0 rad
   and level, at seven tolerances and the default threshold, cut none). A run keeps
   every model it went on with for its length, up to 1 024 (in pieces
   added as needed; before, 32: a model with six or more sets switching
   on their own, 64 choices, refused every switch after its 32nd). Before
   any set kept its choice, a model with two moving sets could not switch:
   the re-choice pivoted every set with the window of the one that
   degraded (a second pendulum, not degrading, onto a choice it did not
   need), held every set to the base threshold, and raised the threshold
   of the set that asked, not the one that failed: two pendulums released
   at 1.50 and 1.80 rad stopped at 0.2135 s naming rod 1 (1.50 + 1.60 at
   2.2945 s, 1.70 + 2.00 at 0.3027 s, five at 1.50 … 1.58 rad at 2.310 s,
   six at 1.5 … 3.05 rad at 2.91 and 4.05 s at rtol 1e-6 and 1e-10). Now
   each rod chooses its states again on its own, as often as it does alone
   (17 and 17 times in 10 s; the six 13 to 17 times each, as alone, taking
   43 of their 64 choices at rtol 1e-8), but each ends about twice as far
   from its reference as alone: over the 119 rod-runs of the judge's
   held-out multi-pendulum cases, an x-y rod's error is 1.92 times the
   same pendulum's alone (geometric mean; median 2.02, p90 9.4, at most
   20); 27 and 405 rtol at 1e-6, 102 and 333 at 1e-10, alone 132 and 203,
   320 and 81; the five at 1e-10 137 … 926 rtol, alone 320 … 453; the six
   at 1e-8 100 … 18 549 rtol, alone 46 … 9 106. That is the cost of the
   integrator restarts the other sets' switches force on every rod, not
   of the switch or its carry-over: the same models in their angles (ODEs,
   nothing to choose) given events that change nothing at exactly the x-y
   runs' switch instants are 2.25 times as far off as without them
   (geometric mean; median 2.06, p90 11.7), right within 100 rtol in 9 of
   the 35 runs instead of 17, and against them the x-y rods are 1.44 times
   as far off (median 1.33, p90 6.0), about what the single pendulum's x-y
   form is to its angle form (1.21). (The angle forms without such events,
   whose rods share the steps the most demanding one needs and never
   restart, are 0.52 times their rods' errors alone.) A run of
   one pendulum is bitwise what it was but where a switch was refused
   (8 of the judge's runs: section 15, the sixth verdict;
   `state_choice.rs`: `independent_pendulums_…`, `six_pendulums_…`). A double pendulum in x
   and y (its rods' constraints one set at each level, coupled), released
   at 1.2 and 1.6 rad, chooses its states again 9 times in 5 s and is 649,
   100 and 52 rtol off its angles integrated independently at rtol 1e-6,
   1e-8 and 1e-10, as before (the same double pendulum in its angles, an
   ODE: 22, 12, 11): its error is 5.4e-5 m before the first switch and
   grows smoothly through them, the x-y formulation's own (the review's
   measurement). A pendulum near its top is that sensitive to any restart
   of the integrator: the one released at 3.05 rad, 30 842 rtol off at
   1e-10 alone, is 89 751 rtol off beside one at 1.50 rad (whose 17
   switches restart the integrator for both), and 61 348 alone with 17
   events at those times that change nothing it reads (22 692 against
   42 120 at 1e-6, 97 991 against 9 106 at 1e-8): the restarts, not the
   switches' carry-over. The
   engine's builders give every model whose choice moves with its
   solution its choices (`lsim_engine`, `lsim_project::Model`, the judge's
   runner); a run on diffsol (the cross-check) and a model whose tables
   were swapped after it was built keep the detection; fast mode's
   inverse model (its motion prescribed) is given no choices and is never
   run by the run loop (`Model::simulate` refuses it; fast mode steps it
   with its own solver, section 10). Where the run stops so, it says why
   it did not choose the states again: the model was given no other
   choice, the run is on diffsol, dynamic selection is off
   (`state_switch` 0), or what the last try for that set ran into (each
   tested: `state_choice.rs`, `lsim-engine/tests/testcar.rs`; before,
   the run on diffsol and with the switch off said that choosing again
   "gave none that works better", though it never tried). The models with
   the states chosen again are compiled from the data the model was prepared with, so a run
   switches only where each table the model reads is the same
   interpolant, to the bit, in the model it is on and the one it would
   go on with (`ModelFunctions::table_interp`, `Table::same`: the
   breakpoints, the values, the interpolation and the rule outside the
   data; a model that does not give its tables does not switch). Before,
   the guard compared the tables' values at their breakpoints only: the
   pendulum at 1.50 rad beside `der(z) = rate(time)`, its linear table
   held at its edges and given the same table continued along its slope,
   went through 17 switches reading one table between them and the other
   after them, z(10 s) = 9.0963 where either table gives 7.5 or 10, with
   no word; now it stops where its states degrade, naming the rod
   (`state_choice.rs`). A run that does not switch is the run
   without dynamic selection to the last bit: every golden case, every
   judge run below 1.48 rad and every other judge case but the pendulums
   that stopped before. The pendulum released exactly level (x = L, y = 0
   to the bit, x and vx fixed) was refused at preparation: its start's vy
   came out 0 / 0 (`x·vx + y·vy = 0` with y = 0 and vx = 0 says nothing of
   vy), every choice's pivots read NaN; now an explicit solution of the
   initialisation whose divisor and remainder are both exactly zero keeps
   the initialisation's guess for the variable: a variable's start value,
   a derivative's 0 (the initialisation guesses every derivative at 0; a
   start value given to a variable that is a derivative after aliasing,
   as `vy` is `der(y)`, is not read, so the level pendulum's vy starts at
   0 whatever start it is given). Where the start at preparation leaves
   such a solution undecided, preparation warns (`INIT-UNDECIDED`, at the
   values it is prepared at, as `INIT-START-IGNORED` is told), naming the
   equation (as differentiated: the derivative of the rod's length), the
   variable and the value it starts at; where the divisor is not zero the
   solution is as before, to the bit (the golden comparison, every judge
   model but the level pendulum, and `state_choice.rs`'s pendulum released
   at 1 rad moving, vy on the rod's constraint to round-off). And the
   initialisation's Newton takes a point that solves its equations exactly
   at a singular Jacobian (`x² + y² = L²` at y = 0, a double root): it
   prepares with y and vy, and runs through its switches. Such a point is
   told: the run's warnings name the equations whose derivatives are
   dependent there and the unknowns they do not decide near it (found by
   elimination with full pivoting on the Jacobian there,
   `init::dependence`): a multiple root, as here, or one of a continuum
   of roots (dependent equations, an over-constrained mechanism at
   rest). Where the integrator then cannot start from it (`x² + y = 0`
   beside `k x² + k y = 0` at x = y = 0: IDA's consistent initialisation
   fails), the run stops with the start's failure naming them, its
   words beside them (`state_choice.rs`; before, IDA's flag −14 alone,
   and before the Newton took such points, the initialisation's failure
   to converge). Its cost, counted
   in instructions against the same build with the switch off
   (`state_switch` 0) on runs that do not switch: none on the BEV's WLTC
   (no set moves: nothing is looked at; the two counts differ only by when
   the energy books' machine code came in from its thread); +0.3 % on the
   hybrid's mixed cycle (86.02e9 → 86.30e9 instructions: the look at each
   of its 571 511 step ends, the bound against the threshold; the channels
   it takes at a step's end are the step end's own, and a tick's there);
   about 650 instructions a step on Andrews' mechanism and 400 on MSL's
   PlanarFourbar (1.0 and 0.8 % of their runs, counted from the look's own
   functions: their totals move by ±3 % with that thread's timing). The
   golden comparison's CPU time is the same (100.9 and 109.0 s before, 98.8
   and 101.5 s after, back to back; this machine's noise ±5 %), every
   figure, channel, step and event of it the same to the last digit. A
   switch costs a preparation of the model at the point (about 0.25 ms on
   the pendulum: released at 1.50 rad its run at rtol 1e-6 takes 6.6 ms
   with 17 switches, released at 1.40 rad, with none, 2.4 ms in about as
   many steps) and, the first time the model takes a choice, its
   compilation (4 to 8 ms on the pendulum).

   **Through tables.** A table read differentiates by the chain rule,
   `d/dt T(a, b) = ∂T/∂x · a' + ∂T/∂y · b'`, and `∂T/∂x` is itself a table
   read: `Expr::Table` with an `order` per argument (printed
   `table3'[1, 0](…)`; only index reduction makes such reads, models and
   the text format read values). It is the interpolant's own derivative,
   exact up to rounding: cell by cell, the derivative of the cubic (or
   bicubic) polynomial the value is. One implementation serves every
   reader: the interpolant (`lsim_ir::interp::Table`, moved from
   `lsim-codegen`, which re-exports it) gives any partial derivative
   (`eval_order`), the interpreter reads it through `Env::table`,
   preparation through its node environment, the solver through
   `ModelFunctions::eval_table_order`, and the generated code calls the
   same `lsim_tab*` symbols on the table store's entry for that
   derivative (each derivative the model reads gets one after the flat
   tables: the same coefficients, asking for that derivative). The
   compiled code is the interpreter's, bitwise (random expressions with
   derivative reads up to the second order), a first derivative is
   bitwise the rate the value comes with, and a derivative read's own
   Jacobian entries read the next order, checked against differences.

   *Where a derivative is one-sided.* A monotone cubic is C¹: its second
   derivative jumps at every breakpoint; a linear table's first does; a
   table held at its edge values outside its data (`Clamp`) has a kink at
   each edge. There the engine takes the derivative of the cell the
   argument lies in: at a breakpoint inside the data the cell to its right
   (`x_i ≤ x < x_{i+1}`, as the value is found), at the data's last point
   the last cell, so on the data's edges the side inside them; outside, a
   held axis is constant (every derivative along it 0), an axis continued
   linearly has its edge slope and nothing higher. This is right because
   a table may be differentiated at most once more than it is
   continuously differentiable (preparation checks it, below): then every
   derivative below the one read is continuous, so the lower equations,
   the states and the dummy derivatives they decide are continuous too,
   and the equation that reads the jumping derivative holds on each side
   of the jump; what it decides (a dummy derivative's derivative, a
   constraint force, a capacitor's current) jumps there at an isolated
   instant, where either side is a value the solution takes, and which
   one is taken changes nothing that is integrated. The integrator must
   still not step across such a jump as if it were smooth. A table read
   along time stops the integrator at its breakpoints (section 8.2); when
   the model reads a derivative of it that jumps there (of total order one
   more than the table's continuity), the run stops where the cell before
   the breakpoint still holds (some ulps before its time, more than the
   rounding of the position and of the cell search can move it: the
   position, growing or falling, is then still in the cell it comes from)
   and restarts the integrator there, so that the step that reaches the
   breakpoint reads the cell before it and no step's history spans the
   jump (`TimeTable::kink`; measured on a gear whose ratio is a linear
   table over time: 2·10⁻⁸ off the closed form with a plain stop at the
   breakpoint, 4·10⁻¹¹ with this, and 1·10⁻¹¹ read backwards in time, at
   rtol 10⁻¹⁰). Read at a state, a jump's time is not
   known ahead: the integrator steps over it as over any piecewise-smooth
   right-hand side, its error test shrinking the step there (the same gear
   with the ratio a linear table of the load's speed: 7·10⁻¹¹).

   *Not smooth enough* (`INDEX-TABLE-NOT-SMOOTH`): a table differentiated
   more often than that is refused, naming the table, the equation and
   why (a linear table's kinks at its breakpoints, a held table's at its
   edges, a cubic's second derivative), with what would fix it (a monotone
   cubic, continuing linearly outside the data, a compliance); the motion
   would otherwise need an impulse the model does not have. How smooth a
   table is comes from its interpolant (`Table::continuity`: a linear
   table C⁰, a cubic C¹ with every varying axis continued linearly or
   stopping the run outside, C⁰ with one held, a constant C^∞); it is
   checked again when a compiled model's tables are swapped
   (`with_tables`). A derivative read's unit is the value's per the
   axis's, per order.

   Preparation reads the tables (the start it solves, the pivots of the
   choice of dummy derivatives) only in models whose index reduction
   differentiated a table; every other model is prepared exactly as
   before (there the tables read as NaN, and the run solves the start).
6. **BLT.** Tarjan's algorithm orders the strongly connected blocks,
   dependencies first.
7. **Tearing and solving.** A one-equation block affine in its unknown
   becomes an explicit assignment (symbolic `−b/a`; WP2 adds a check that
   `a` cannot be zero for any allowed parameter value, else it becomes an
   iteration variable). Larger blocks: WP2 detects linear blocks (solved at
   run time by a small dense LU inside the generated code) and tears
   non-linear ones with Cellier's heuristic (tearing variables chosen to
   make the rest explicit; few, well-scaled); the tearing variables become
   `z`, the residual equations `g`. Stage 1 keeps every unknown of a
   non-trivial block in `z` (correct, not minimal).
8. **Events and modes** (WP2/WP3). Relations in `when` conditions become
   zero-crossing functions (`a ≥ b` → `a − b`, with direction). Relations
   inside `if` expressions in equations (friction stick/slip, clutch lock,
   diode, `abs`, `sign`, gear selection) become *modes*: a discrete Boolean
   per relation, held between events, with its zero-crossing function; the
   equations use the held value, so the integrator never sees a
   discontinuity. Relations under `noEvent` are evaluated as they stand.
   Stage 1 supports `when` with one comparison and rejects the rest with a
   clear message. **`reinit(x, v)`** (WP2) restarts a state at an event
   without anything new in the run loop, which applies only discrete
   assignments: after alias elimination `x` (resolved to its alias root)
   is split into a continuous part and its jumps, `x = x.continuous +
   x.jump` with `der(x)` read as `der(x.continuous)` everywhere, and the
   action becomes the discrete assignment `x.jump := v − x.continuous`.
   The integrator's state is `x.continuous` (it inherits `x`'s start and
   `fixed`; dummy derivatives keep it a state before anything else), so
   right after the event `x = v` exactly while nothing the integrator
   sees jumps. As with every assignment of a `when` clause, a discrete
   variable `v` reads is its new value (`pre(i)` for the old one) and
   continuous ones are their values at the event. A target that is not a
   state, or that index reduction cannot keep one (two rigidly coupled
   speeds both restarted), is `REINIT-NOT-STATE`. In fast mode's inverse
   model the motion is prescribed: a restart of a prescribed speed is
   dropped and one of a speed that follows it changes nothing, both told
   as information (`REINIT-PRESCRIBED`). The gear change of
   `mech_gear_change` (a dog clutch keeping `J2 ω2 + i2 J1 ω1`) runs to
   the reference's digits (2·10⁻¹⁴) and its energy books show the exact
   shift loss. A gear needs no `reinit` for that, though: where the
   inertias that meet at a shift are the model's and not the gear's (the
   library's gearbox), the run loop's impulse projection keeps the
   momentum at any change of rigid couplings (section 8.2); a `reinit`
   that already keeps it leaves it nothing to move.
9. **Initialisation system** (WP2). A separate matching with the `fixed`
   start values and `initial equation`s as knowns/equations; its own BLT;
   compiled as its own functions; solved by Newton with line search, then
   homotopy if that fails (section 8.4).
10. **Sparsity** (WP2). The structural pattern of `∂[x'; g]/∂y` through
    the assignments, for colouring and sparse LU.
11. **Structure key.** SHA-256 of everything that shapes the generated
    code (equations, structural parameter values, sizes), excluding
    runtime parameter values, labels and origins.

The inverse model of fast mode (section 10) is the same pipeline with a
different known set: `prepare_inverse(lib, top, &InverseSpec, opts)`.

## 7. Code generation and the cache

**Cranelift** (Apache-2.0 WITH LLVM-exception) compiles each prepared
model into straight-line machine code (an `if` becomes a `select`; there
are no branches to mispredict):

| function | computes |
|---|---|
| `residual` | `[x'; g]` |
| `jvp` | `(∂[x'; g]/∂y)·v` by forward-mode differentiation (dual numbers) of the same code: exact, no finite differences |
| `roots` | zero-crossing functions |
| `vars` | every flat variable, aliases included (the channels) |
| `when` | the discrete variables after fired `when` clauses |

Parameters, discrete variables and inputs are read from memory, so a new
parameter value never recompiles. Transcendental functions call the
engine's own library (`lsim_ir::math`: CORE-MATH's correctly rounded
functions, section 7.1) through registered symbols. Measured in the spike: 0.7 ms
to compile all five functions (1.8 kB of machine code); one residual call
7 ns, one Jacobian-vector product 16 ns.

WP3 built (the numbers on the 4-core development machine, release
builds):

* **One lowering, two backends.** The lowering (`lower.rs`) is generic
  over an emitter: Cranelift IR for machine code, or a tape, a flat list
  of operations that `tape.rs` interprets. Both receive the same
  operations in the same order, so a tape computes bit for bit what its
  machine code computes (tests: `tape.rs`, on the initialisation systems
  and on every function of tiered models); a tape's fused multiply-add is
  the CPU's own instruction (`_mm_fmadd_sd`), as the machine code's is,
  not a library's `fma`. Generated functions take one
  argument, a pointer to their inputs and outputs (`CallCtx`), are one
  block each, and load each array's address once.
* **Exactness.** The functions the run loop compares with the interpreter
  (zero crossings, `when`, modes, table guards, the condition kernels)
  are bitwise `lsim_ir::eval`: the IR's order, no fused multiply-add
  contraction, the interpreter's library functions (`pow` for every
  power), its rule for `min`/`max` at a tie of signed zeros. The residual,
  the Jacobian and the channels multiply out a constant integer power
  `x^n` (3 ≤ |n| ≤ 16) in double-double arithmetic and round once: within
  1 ulp of the exact power, and the correctly rounded power itself (the
  library's `pow`) on all 280 000 powers of `tests/fuzz.rs`. Each product's exact rounding error comes from a fused
  multiply-add where the CPU has one and from Dekker's product where it
  has not, so CPUs with and without FMA compute the same bits (the fuzz
  test's mirror on 560 000 powers, the compiled models both ways, and the
  golden comparison lowered both ways, byte-identical); outside
  [2^-960, 2^990] the plain repeated squaring's product. The machine
  code's results are then the same on every x86-64 CPU, and so are the
  library functions it calls: the engine's own, correctly rounded, not the
  platform's (section 7.1).
* **Sparse coloured Jacobians.** Column colouring of the sparsity pattern
  (greedy, largest-first) and one forward sweep with a tangent per colour,
  filling the CSC values directly: `jacobian_sparse`. The example cars need
  8–9 colours, a network of 10⁴ equations 10–11. `jvp` comes from the
  coloured Jacobian (exact, a sum per row) unless `compile_jvp` asks for
  its own forward-mode code: a run calls it a few dozen times, and its own
  code would double the instructions to compile.
* **Tables** (`tables.rs`), runtime data (`JitModel::with_tables` swaps
  them without recompiling): Steffen's monotone cubic in 1-D (C¹,
  monotone and without overshoot on every interval), bicubic Hermite
  patches in 2-D whose node slopes are reduced until every cell's Bézier
  net is monotone along each axis along which its data are (Carlson and
  Fritsch), linear and bilinear; `Clamp`, `Linear` and `Error` outside per
  axis. The value alone (every call outside the Jacobian) stops before the
  rates wherever they cannot change it; an axis with uneven breakpoints
  starts its interval search at the interval it found last. Tests:
  `tests/tables.rs` (C¹ across every breakpoint, cell edge and continued
  edge, monotone and within the data in every interval or cell whose data
  are, on 2 600 random tables).
* **Large models.** Above 5 000 expression nodes: no Cranelift
  optimisation, the single-pass register allocator, the residual's
  assignments shared with the channels through `work`, functions cut into
  chunks of 12 000 nodes compiled on up to four threads. Above 20 000
  nodes the model is **tiered**: `compile` returns it running on tapes
  (built by the same lowering in a fraction of Cranelift's time) and
  another thread compiles its machine code, to which each function then
  switches (`JitModel::wait_machine_code`); results do not depend on when.
  The compilation stops when the last holder of the model goes away, and
  `JitModel::machine_code` says how the functions run (machine code, on
  tapes while it compiles, on tapes for good and why).
* **Memory.** A model's machine code is given back when its last holder
  goes away (cranelift-jit's memory provider leaks it unless the module's
  `free_memory` is called: `tests/memory.rs`). Where the system refuses
  executable memory (Windows' Arbitrary Code Guard, some security
  policies), a model runs on its tapes, which compute the same, and
  `CompileReport::on_tapes` says so (`tests/fallback.rs`, with the
  refusal simulated in the memory provider).
* **The floating-point environment.** Compiling (on every thread it
  uses) and running a model (lsim-solve's `simulate` and `run_loop`) set
  the default environment, rounding to nearest with subnormals kept, for
  their duration and restore the calling thread's own after
  (`lsim_ir::fenv`): a host library's flush-to-zero or denormals-are-zero
  would otherwise turn subnormals into zeros, in the tables' coefficients
  and in the interval enclosures' rounding (`tests/fenv.rs`). Model
  functions called directly, outside a run, compute in the caller's
  environment.
* **Modes, guards, the initialisation** (`InitFunctions`, on tapes: a run
  calls them a handful of times), and the **condition kernels** of section
  5.8 (point kernels on tapes, interval kernels as programs over
  `lsim_ir::interval`'s rules).
* **The target's calling convention.** Every signature takes the ISA's
  default convention, the one Rust's `extern "C"` means on the target
  (System V on Linux and macOS, the Windows x64 convention on Windows),
  and every compiled function is checked against it; calls go to the
  registered runtime symbols only; frames larger than a page probe their
  pages in order (Windows commits a thread's stack one guard page at a
  time). `tests/conventions.rs` compiles the example projects and random
  models for the other x86-64 platform too (Windows from Linux).
* **Unwind information: none** (`unwind_info=false`). Nothing unwinds
  through generated code (it calls only functions that cannot panic, and
  a panic in an `extern "C"` function aborts), but on Windows a debugger,
  a profiler (ETW, VTune) or a crash dump cannot walk the stack through a
  JIT frame, and a structured exception raised in one (an access
  violation) finds no handler there. Registering the frames would take:
  `unwind_info=true`, so that Cranelift describes each function's
  prologue (`CompiledCode::create_unwind_info` gives the Windows x64
  `UNWIND_INFO`); placing each `UNWIND_INFO` in the module's memory within
  4 GB of the code (the table holds 32-bit offsets from one base);
  building the `RUNTIME_FUNCTION` table (start, end, unwind information
  of each function) and calling `RtlAddFunctionTable(table, count, base)`
  once the code is final, and `RtlDeleteFunctionTable` before
  `free_memory` gives the memory back (or `RtlInstallFunctionTableCallback`
  over the module's whole range). cranelift-jit does none of it (wasmtime
  does, in its own code memory). On Linux the counterparts are
  registering `.eh_frame` with `__register_frame` and a perf map
  (`/tmp/perf-<pid>.map`) for profilers. Not done yet.

**Compile time.** The example cars: 10–17 ms on four threads, 23–32 ms of
one thread (`tests/budget.rs`, budget 50 ms). A network of 10 000
equations (84 500 nodes): ready, on its tapes, in 42–48 ms of one thread
(budget 100 ms), its machine code in by about 180–195 ms on four threads;
compiled at once it takes 136–159 ms on four threads, 350–392 ms on one.
Cranelift costs about a microsecond per IR instruction, almost all of the
time; the Jacobian's sweeps are three fifths of a large model's
instructions.

**Per call** (the hybrid at its start, the example cars alike): residual
513–560 ns, channels 667–680 ns, zero crossings 442–456 ns, sparse
Jacobian 1.01–1.04 µs, `jvp` from it 1.14–1.16 µs (at the start of WP3:
565, 717–734, 499–565 ns, 1.01–1.03 µs and, from its own code,
0.76–0.81 µs). A tiered model's tapes cost ten times its machine code's
residual and five times its Jacobian until the machine code is in.

**Cache.** Keyed by SHA-256 of the model's inputs (top component with its
parameter values, every library definition it reaches, connectors, the
library's shared enumeration types, options, mode; the types were left
out until the second review of dynamic state selection, and a library
whose type listed its options in another order hit the entry of the
first, its option's value the first library's) and of what prepares them (the key's format, the engine
version and a digest of the preparation's code: lsim-prep, lsim-ir,
lsim-math and `Cargo.lock`, taken by `lsim-engine/build.rs`); the value is
the `PreparedModel` as JSON (`lsim-engine/src/cache.rs`). The digest
keeps an engine built from other preparation code from reading this
one's entries: the format string, bumped by hand, was left as it was
when dynamic state selection added fields to each `StateSet`, and an
older entry, read with them empty, brought back a stop they had fixed
(two pendulums in one model, at 0.2135 s); those fields have no default
now, so an entry without them is not read either. A hit skips flattening and structural
analysis; machine code is regenerated. Parameter values are in the key
because preparation chooses by them (index reduction picks the states by
pivoting at the start; the start checks warn at the start values): a
study at other values sets them on one built model instead, which checks
the choice of states at their start and refuses values at which it does
not work (section 6, step 5); the prepared model carries the choice, so a
run from the cache watches it as any other run does.
**Decision (WP3, measured with `lsim-codegen`'s `cache_bench`): machine
code is not cached.** On the example cars, preparing takes 2.7–3.7 ms and
reading the prepared model back 1.7–2.6 ms; compiling it then takes
10–17 ms on four threads (25–43 ms on one), of which Cranelift is
22–42 ms of thread time and placing the code in executable memory with
its calls resolved 0.15–0.27 ms. A machine-code hit would still cost at
least 1.1–1.5 ms (measured: the analysis, reading 78–118 kB, the link
step; reading back the tapes, kernels and layouts would come on top), so
it would save at most 9–16 ms a build on four threads (25–42 ms on one):
under 1 % of a drive
cycle's run (the BEV's WLTC takes about 3 s), and parameter studies
already reuse one compiled model (parameters and tables are runtime
data). For 10 000 equations it would save 138 ms on four threads, where
tiering already has the model running in under 50 ms. Against that, a
cache of executable code needs: an object format and loader per platform
(`cranelift-object` writes COFF on Windows, ELF on Linux, Mach-O on
macOS, each with its own relocations), a key that also covers the
Cranelift version, the target triple and the CPU features the code was
compiled for (the exact powers use fused multiply-add only where the CPU
has it), and authentication of what it loads (a writable cache directory
would otherwise be a way to run arbitrary code, which the IT policy
forbids). Revisit when builds of many structurally different models
dominate a session (a design study's variants), with the key above and
an HMAC per installation.

### 7.1 The mathematical library

**Decision (October 2026): every transcendental function the engine
evaluates comes from CORE-MATH's correctly rounded implementations,
vendored and built in-tree (`crates/lsim-math`, re-exported as
`lsim_ir::math`).** Before, the engine called the platform's through
Rust's `f64` methods: glibc here, the MSVC UCRT on Windows, which differ in
the last place between platforms and, through the FMA variants that
glibc's `ifunc`s and the UCRT (`_set_FMA3_enable`) pick at run time,
between CPUs. A correctly rounded function has exactly one right result
for each input, the exact value rounded to the nearest double, so the
engine's results no longer depend on the platform, the compiler or the
CPU.

What goes through it: the interpreter (`lsim_ir::eval`) and preparation's
constant folding; the runtime symbols generated code calls (`lsim_exp`,
`lsim_pow`, …) and the tapes' library calls; the interval enclosures and
so the condition kernels; lsim-solve's dual numbers (energy books, impulse
projection, the conditions' rates); integer powers (`math::powi`: Rust's
own `f64::powi` multiplies by repeated squaring on Linux but calls the C
runtime's `pow` on Windows, which Windows CI showed); and SUNDIALS' own C
maths: `pow` in CVODE's and IDA's step-size and order selection and IDA's
convergence rate, `exp` in KINSOL, which a header `lsim-sundials-sys`
includes first into every SUNDIALS file maps to lsim-math's C entry
points (every other inexact function of `<math.h>` to a name nothing
defines, so that a new call fails to build). Keeping it so: clippy's
`disallowed-methods` (`engine/clippy.toml`) rejects every `f64` and `f32`
method that calls the C runtime's maths, by type, everywhere in the
workspace (CI and `check.sh` run clippy with warnings as errors), and
`lsim-math/tests/sources.rs` checks that list and reads the sources for
any other way in. Not routed, on purpose: `sqrt`, which IEEE 754 rounds
correctly and every target computes with an instruction, and the exact
operations (`abs`, `floor`, `min`, …).

**The candidates**, measured with `experiments/libm-eval` on 9.6 million
inputs: every hard-to-round case CORE-MATH lists for the 13 functions the
engine uses (8.4 million inputs whose exact value lies closest to a
rounding boundary, where a library that is not correctly rounded errs),
2.2 million random inputs over wide and typical ranges, and the special
values, each against its correctly rounded result from MPFR (an IEEE
binary64 context; mpmath at 4000 bits agrees wherever both apply). A cell
gives the results that are not the correctly rounded one and, in
brackets, the worst error in ulps of the exact value.

| function | inputs (hard / random / special) | glibc 2.39 | pxfm 0.1.30 | libm crate 0.2.16 | CORE-MATH |
|---|---|---:|---:|---:|---:|
| sin | 1 975 927 / 170 000 / 42 | 467 681 (1.03e5) | 214 707 (0.5) | 474 139 (0.756) | 0 |
| cos | 1 090 161 / 170 000 / 42 | 268 174 (1.37e5) | 160 234 (0.5) | 272 131 (0.752) | 0 |
| tan | 1 077 363 / 170 000 / 42 | 268 250 (14.4) | 9 161 (0.5) | 274 144 (0.751) | 0 |
| asin | 249 134 / 170 000 / 42 | 7 117 (0.512) | 0 | 12 759 (0.811) | 0 |
| acos | 264 099 / 170 000 / 42 | 16 909 (0.522) | 0 | 40 475 (0.866) | 0 |
| atan | 55 770 / 170 000 / 42 | 8 616 (0.517) | 3 165 (0.5) | 13 817 (0.813) | 0 |
| atan2 | 28 055 / 170 000 / 2 184 | 5 758 (0.514) | 5 769 (0.5, sign of zero) | 29 542 (1.31) | 0 |
| sinh | 290 545 / 170 000 / 42 | 51 259 (1.71) | 3 (0.5) | 51 936 (1.71) | 0 |
| cosh | 36 692 / 170 000 / 42 | 32 990 (1.00) | 3 (0.5) | 34 938 (1.29) | 0 |
| tanh | 31 450 / 170 000 / 42 | 25 397 (2.00) | 4 (0.5) | 23 894 (2.00) | 0 |
| exp | 1 129 432 / 170 000 / 42 | 254 342 (0.510) | 17 535 (4.5e15) | 282 123 (0.870) | 0 |
| log | 134 957 / 170 000 / 42 | 33 058 (0.512) | 386 (0.5) | 36 867 (0.784) | 0 |
| pow | 1 003 111 / 190 000 / 2 184 | 81 128 (0.505) | 12 088 (special values) | 97 271 (0.836) | 0 |

* **glibc 2.39** (x86-64, the FMA variants this CPU gets): 1.5 million
  results rounded the wrong way, and beyond the last place for `sinh` (up
  to 1.71 ulps), `tanh` (1.96 ulps on random inputs between −20 and 20)
  and `sin` and `cos` of huge arguments whose result is near zero (up to
  1.4e5 ulps; checked again with mpmath). The interval code widened
  library results by 2 ulps on the assumption that the platform is within
  1 ulp, which `lsim-solve/tests/libm.rs` checked for `sin`, `cos`, `exp`
  and `ln` only: it did not hold for `sinh`, `tanh`, `sin` and `cos` on
  glibc itself. (The UCRT passed that test's 543 points on Windows CI; its
  other functions, its FMA3 choice and its agreement with glibc were never
  measured.)
* **pxfm 0.1.30** (pure Rust, BSD-3-Clause OR Apache-2.0; ports of
  CORE-MATH and LLVM's libc, used by the `image` crate): correctly rounded
  on `asin` and `acos` and almost on the hyperbolic functions, but not on
  `sin` (one hard case in nine), `cos`, `tan`, `atan`, `atan2`, `exp`,
  `log` and `pow`; `exp` is wrong by up to 4.5e15 ulps where its result is
  subnormal, `atan2` loses the sign of a zero result, `pow(1, NaN)` is NaN.
* **The libm crate 0.2.16** (pure Rust, MIT; musl's): up to 2 ulps.
* **CORE-MATH** (MIT, C, INRIA): every result correctly rounded, in each
  of the three builds measured (with `-mfma`; the x86-64 baseline with the
  C runtime's `fma`; the baseline with the exact software `fma` the engine
  ships). The Rust binding on crates.io (`core-math` 1.4 over
  `core-math-sys`) builds all of CORE-MATH with `-march=native` by default
  and needs the same C compiler on Windows: it brings nothing over
  vendoring the 13 files.

CORE-MATH is the only candidate that rounds correctly; the pure-Rust ones
would build anywhere but are not. It is vendored at revision e78d268 of 13
October 2026: the next revision, 37384b5 of 14 October, rounds three hard
cases of `exp` the wrong way (one ulp, each within 1.6e-16 ulp of a
rounding boundary), so the revision is pinned and an update is checked on
all of CORE-MATH's hard cases first (`crates/lsim-math/core-math/README.md`);
those three inputs are among the crate's test references.

**Two builds, one result.** CORE-MATH's fused multiply-adds need the FMA
instruction to be fast; without `-mfma` they become calls of the C
runtime's `fma`. lsim-math compiles the 13 files twice: with `-mfma`, and
for every x86-64 CPU with each fused multiply-add an exact software one
(musl's, MIT, `csrc/lsim_math_fma.c`); the first call picks the build the
CPU runs (FMA and AVX with the operating system's support, as Rust's
`is_x86_feature_detected!` sees it). A fused multiply-add is exact either
way, so the two builds compute the very same numbers at every step, not
only the same results: tested bit for bit on 6.4 million random inputs
over every magnitude in every test run (`lsim-math/tests/builds.rs`, from
the review; 64 million with `--ignored`), and the golden comparison with
`LSIM_MATH_NO_FMA=1` (which forces the software build) writes the same
bytes for all 14 cases. Contraction of `a × b + c` is off in both
(`-ffp-contract=off`); the C runtime keeps only exact operations
(`floor`, `round`, `fmax`, `fmin`) and the floating-point environment.

**Its own options only.** Correct rounding depends on the C compiler's
options, and the cc crate adds the environment's C flags after the
build's own: with `CFLAGS=-ffast-math`, 2 270 of the 4 672 references
were misrounded in each build and the build succeeded (the review of
80c8ab0 found it). Measured with gcc 13 on the references: `-ffast-math`
or `-Ofast` 2 270 misrounded per build, `-funsafe-math-optimizations`
2 132, `-mfpmath=387` 948, `-ffinite-math-only` 292, `-fno-signed-zeros`
10; `-ffp-contract=fast`, `-freciprocal-math`, `-O0`, `-march=native` and
`-frounding-math` none; clang refuses `-ffast-math` on these files. Three
guards (`lsim-math/c_flags.rs`, SUNDIALS' build too): the build scripts
remove the environment's C flags (`CFLAGS` in all its forms,
`CRATE_CC_NO_DEFAULTS`, `CL`, `_CL_`, `CCC_OVERRIDE_OPTIONS`) before the
cc crate reads them, naming each in a warning; an option that relaxes
IEEE semantics, contracts, flushes subnormals or computes in x87
precision on the command line the cc crate will run (`CC` can carry
options) stops the build; and the first call checks the build it picks
on 27 hard cases (`Build::self_check`, about 15 µs), chosen so that each
of the options above misrounds at least one case of every function it
affects, and panics with the case otherwise. Each guard was seen at work
on each of those options (the later ones with the earlier disabled by
hand).

**The engine's two patches to CORE-MATH** (`lsim-math/core-math/README.md`,
recorded as one diff, `lsim.patch`). Where its last phase still cannot
prove the rounding, a case no input is known to reach, upstream's `pow`
and `atan2` print the input and call `exit(1)`, which would end the
desktop app or the Python process: the engine's copy counts the case
(`lsim_math::unexpected_cases`) and returns the rounding of the
approximation it has. And `long`, 32 bits on Windows and 64 on Linux,
becomes the 64-bit type it is on Linux wherever upstream uses it
(`~0ul>>12` was a 20-bit mask on Windows in cosh, sinh and tanh, a shift
`1l << (ex + 1073)` in cos and tan, `long dn` in asin…), so that Windows
compiles the code Linux does. `core-math/SHA256SUMS` records every
vendored file, which `lsim-math/tests/vendored.rs` checks, with no call
of `exit` or `abort` and no `long` left (it flags every site of
upstream's files). `core-math/upstream-report-draft.md` reports the exp
regression of 37384b5 to CORE-MATH's authors: written, not sent. With the
patches and the guards, the golden comparison writes the same bytes for
all 14 cases as before them (1c02af5 against 1bb2173, Linux).

**Windows.** CORE-MATH uses `__int128` and GCC builtins, which MSVC's `cl`
lacks. lsim-math's build takes clang-cl (the cc crate's
`prefer_clang_cl_over_msvc`, which finds the "C++ Clang tools for Windows"
of Visual Studio, on GitHub's Windows image) and stops with an explanation
where there is none; SUNDIALS stays on `cl`. There upstream takes its
`_MSC_VER` branches (`roundeven` by the `vroundsd` instruction or in
software, not `__builtin_roundeven`: the same values), and before the
second patch its `long`s were 32 bits. All 9.6 million references of
`experiments/libm-eval` (CORE-MATH's hard cases for the 13 functions,
random inputs, special values) were run compiled as on Windows:
`experiments/libm-eval/clang-cl/run.sh` takes the cc crate's clang-cl
command line, emits LLVM IR for `x86_64-pc-windows-msvc` against
stand-ins for the UCRT's headers (so with clang-cl's semantics: 32-bit
`long`, `_MSC_VER`, MS `volatile`), retargets the IR to Linux and runs
it: every one of the 19.2 million results (both builds) correctly
rounded, before the patch and after (October 2026, clang 18). What that
leaves to Windows itself, its code generation and C runtime, Windows CI
runs: every test passed there with the patches and the guards (run
38602668807 on `wp3/codegen`), among them lsim-math's references, the two
builds' bit-for-bit agreement on 6.4 million random inputs, the vendored
files' sums, SUNDIALS' `pow` on hard cases, and every evaluation path
against the library (`lsim-codegen/tests/library.rs`). The job now prints
the clang-cl it finds: Visual Studio 18's, clang 22.1.3, which the cc
crate takes (LLVM 20.1.8's is also on `PATH`); the clang-cl-semantics
run above used clang 18, so a full run of the references on Windows
itself (the README's update steps say how) remains to be done.

**Speed.** Per call, ns (`experiments/libm-eval`, the best of 15 passes
over 65 536 inputs in the ranges models use, one thread of this 2.8 GHz
Xeon; glibc's calls go through the PLT, CORE-MATH's through Rust's FFI,
lsim-math's through the engine's entry points, which pick the build):

| ns per call | sin | cos | tan | asin | acos | atan | atan2 | sinh | cosh | tanh | exp | log | pow |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| glibc 2.39 | 20.0 | 18.8 | 19.8 | 18.7 | 19.4 | 12.4 | 27.8 | 21.4 | 8.2 | 19.3 | 5.3 | 5.3 | 16.5 |
| lsim-math, FMA build | 12.6 | 30.4 | 35.1 | 14.3 | 13.7 | 12.2 | 16.9 | 13.1 | 12.8 | 17.0 | 8.1 | 8.9 | 24.9 |
| lsim-math, software FMA | 65.8 | 330.9 | 445.0 | 55.3 | 56.6 | 49.1 | 50.4 | 63.9 | 63.3 | 83.4 | 34.6 | 119.9 | 444.2 |
| pxfm 0.1.30 | 25.6 | 24.3 | 34.7 | 117.6 | 31.9 | 10.7 | 23.2 | 31.9 | 22.2 | 38.8 | 7.7 | 9.4 | 162.2 |
| libm crate 0.2.16 | 22.3 | 22.5 | 24.0 | 11.5 | 11.5 | 11.8 | 29.2 | 19.0 | 14.1 | 21.4 | 10.8 | 9.4 | 72.3 |

The FMA build, which mainstream x86-64 CPUs since 2013 run (Intel
Haswell, AMD Piledriver; some low-end Atom-based Celerons and Pentiums
do not), is faster than glibc for `sin`, `asin`, `acos`, `atan2`, `sinh`
and `tanh`, as fast for `atan`, and 1.5–1.8× slower for `cos`, `tan`,
`cosh`, `exp`, `log` and `pow`; choosing the build costs
nothing measurable (the engine's entry points against the FMA build
called directly). The software build, for CPUs without FMA, is 3–28×
slower than glibc.

On the golden cases the library is about 2 % of a run's instructions
(callgrind: the BEV's WLTC 1.9 % with glibc and 1.9 % with lsim-math; the
hybrid's mixed cycle 1.6 % and 2.4 %, `log` and `pow` dearer and `atan`
cheaper), and each run's instructions fell by 0.4–0.5 % with its steps
(the BEV's WLTC 214 672 steps against 215 669, the mixed cycle 567 850
against 572 709). Run times, the two builds alternating on a quiet
machine, medians: the BEV's WLTC 3.06 → 2.88 s (6 pairs, single runs
from 2.7 to 4.0 s), the hybrid's mixed cycle 24.6 → 24.5 s (3 pairs), its
UDDS 93.1 → 98.3 s (5 pairs: +0.3 % and +1.1 % with the new build run
first, +4 % to +8 % with it run second): within this machine's noise, but
for perhaps a few per cent on the hybrid. The software build costs the
BEV's WLTC 18 % and the aero BEV's 15 % (3 pairs each).

**The interval code's widening is a proof now.** A correctly rounded `r`
of `f(x)` is the double nearest the exact value, so the exact value lies
between `r`'s neighbours: `next_down(r) ≤ f(x) ≤ next_up(r)`, at overflow
and underflow too. `lsim_ir::interval` widens library results by
`math::LIBRARY_ULPS` = 1 ulp on each side (2 before, on an assumption),
and the kernels with it; `lsim-codegen/tests/library.rs` checks that the
enclosure of a monotone function at a point is exactly that wide. It moved
no result of the golden comparison (the enclosures only decide where a
condition must be searched along a step).

**Not routed: diffsol and the Script blocks.** diffsol, the cross-check
backend, still calls the C runtime's `pow` and `cbrt`: its BDF's step
size and order (the PI controller, `pi_controller.rs`: `error.pow(-k)`),
its first step (`state.rs`: `.pow(1 / (order + 1))`) and its Newton
convergence rate (diffsol-nl's `convergence.rs`: `cbrt`, `pow`), all
through `num-traits`' `Pow<f64> for f64`, which is `f64::powf`, and
`f64::cbrt`. So **diffsol's runs remain dependent on the CPU and the
platform**: glibc's `pow` is not correctly rounded (81 128 of the inputs
above) and glibc picks its FMA variant by CPU, the UCRT's `pow`
need not agree with it, a different last place changes a step size, and
the step sequence then differs (diffsol's own comment in
`pi_controller.rs`: stiff problems "diverge between libm
implementations, which broke the snapshot tests on Windows while passing
on Linux"). SUNDIALS, the product's integrator, does not: its `pow` is
lsim-math's. Routing diffsol would take `[patch.crates-io]` copies of
diffsol 0.17.1, diffsol-nl 0.2.0 and diffsol-la 0.2.0 (MIT, vendored
like CORE-MATH) with about a dozen call sites changed (the PI
controller's four powers, the first step's two, the convergence rate's
`cbrt` and `pow`, and `Scalar for f64`'s `cbrt`, `exp`, `sin`, `cos`),
each to lsim-math, with `cbrt` either vendored too (CORE-MATH has one)
or taken as `pow(x, 1/3)`, and kept in step with every diffsol update;
or a change upstream letting the caller supply the scalar's functions.
Not done: the cross-check needs only to agree with SUNDIALS within
tolerance, which it does on every platform. A Script block's Python
computes with Python's `math` module, the C runtime's, so a model with a
Script block can still differ between platforms through its script.
faer's sparse and dense LU and its triangular solves, which the engine
uses, call no transcendental function.

## 8. Solver layer

### 8.1 The integrator interface

```rust
pub trait Integrator {
    fn name(&self) -> &'static str;
    fn step(&mut self, t_stop: f64) -> Result<Step, SolveError>;   // Internal | Stopped | Root(t, dirs)
    fn y(&self) -> &[f64];
    fn interpolate(&mut self, t: f64, out: &mut [f64]) -> Result<(), SolveError>;
    fn discrete_mut(&mut self) -> &mut [f64];
    fn restart(&mut self, t: f64, y: &[f64]) -> Result<(), SolveError>;
    fn stats(&self) -> SolverStats;
}
```

The run loop (`run_loop`) works with any backend. Stage 1: SUNDIALS CVODE
(no `z`) and IDA (with `z`), dense LU with the exact Jacobian. WP4:

* **`lsim-sundials-sys`**: the vendored SUNDIALS 7.x sources compiled with
  `cc`, a committed `sundials_config.h` per platform family, pre-generated
  bindings; only CVODE, IDA, KINSOL (initialisation fallback) and the
  serial vector.
* **Sparse LU** as a custom `SUNLinearSolver` + `SUNMatrix` (CSC) on faer's
  sparse LU (MIT): symbolic analysis once per structure, numeric
  refactorisation at each setup. Dense LU below ~40 unknowns.
* **Method choice.** BDF with Newton is the default (vehicle models are
  stiff: shaft compliances, electrical RC poles, clutch slip). At start and
  after each event a cheap stiffness estimate (power iteration on the
  Jacobian for |λ|max against the expected step) may select CVODE's Adams
  method with functional iteration for non-stiff models; repeated
  convergence failures switch back. Automatic, reported in the run report.
* **diffsol backend** behind a feature, for the cross-check suite.

**As built (work package 4).**

* The integrators are **CVODES and IDAS** (CVODE and IDA with
  quadratures and sensitivities; the same algorithms), for the energy
  quadratures. `Integrator` gained, with defaults: `quadrature` (the
  integrals inside the last step), `local_error` (the error estimate),
  `set_root_sides` (below, 8.2), `method` and `setup_notes` (the report).
* **Linear solver** (`LinearSolver::Auto`): dense LU up to 40 unknowns,
  band LU when the Jacobian's band widths sum to at most 20, faer sparse LU
  otherwise (`faer_ls`: a direct `SUNLinearSolver` on SUNDIALS' CSC
  matrix; symbolic analysis once per structure, numeric factorisation per
  setup, `Par::Seq` so parallel runs do not contend; a non-finite solve is
  a recoverable failure). The Jacobian's values come from the compiled
  model's own coloured sparse Jacobian (`jacobian_sparse`) when its
  structure covers the matrix's, else from coloured Jacobian-vector
  products; the structure is checked against the model at the start (n ≤
  3000) and extended where it misses an entry.
* **Method choice** (`Method::Auto`): at the start, Adams with fixed-point
  iteration when the power-iteration estimate ρ of the Jacobian's spectral
  radius times the run's length is at most 100, else BDF; at each restart
  BDF → Adams when ρ·h < 0.2 for the last step h, Adams → BDF when ρ·h >
  1.5; during a run Adams → BDF on a convergence failure or when
  nonlinear failures exceed 10 % of the steps. The report says which and
  why. A step that got through with too many failures asks for BDF, and
  the switch is made before the next step (or at a restart, if one comes
  first): the step stays as it ended (at a root, at the stop time), with
  its memory and so its dense output, which the mixed conditions' check,
  the output points, the block ticks and the books read inside it (the
  review of the ninth round found the switch made at once, the step left
  with its end state as a constant: an output 6.4e-5 off at rtol 1e-8). A
  step that fails switches at once, from the last step's end, where the
  failed attempt left the memory. The energy integrals carry over both. (The reference problems' RC and RL circuits and the L = 0 motor run
  on Adams; the spike and the motor with inductance on BDF.) Adams' order
  is capped at 7 (CVODE's default is 12), so the dense output a step
  leaves is a polynomial of degree 7 at most: on smooth problems the
  order rarely passes 5 and nothing changes; a slow cosine at rtol 1e-12,
  which reached order 11, takes 272 steps instead of 194, with half the
  error.
* **diffsol** 0.17.1 (pinned, nalgebra dense LU): the same run loop; root
  directions from the root functions' signs at the step's start and at
  the root; the iteration variables made consistent by the same Newton as
  IDA's path; the energy integrals by three-point Gauss–Legendre on its
  dense output; its smallest step lowered to 1e-20 (section 3.2); a stop
  time it reaches a few ulps short counts as reached.
* **A model with no state** (no state and no iteration variable: every
  variable a function of time, the parameters, the inputs and the
  discrete values) has nothing to integrate, and CVODE given none stopped
  at its start ("too much accuracy requested"). It runs on
  `stateless::Stateless`, whichever backend is chosen, an `Integrator`
  that integrates nothing but the energy books, so the run loop handles
  it as any model (outputs on the grid, events and `when`s on time and
  on algebraic conditions, sampled blocks, the books) and the result has
  the same shape. Its steps go to the stop time, halved until they follow
  the channels as an integrator's steps follow its states: the
  polynomial of degree 4 through every channel at a step's ends and
  quarter points gives the channel at an eighth of the step from either
  end to the run's tolerance (the recorder's min, max and mean read the
  steps' ends, as for any model; the outputs are the model's own values
  at their times). The zero crossings the run loop does not schedule
  itself are checked at each step's end and a sign change is located by
  bisection on the model's own functions, exact at any time (there is no
  interpolant), to SUNDIALS' tolerance, `100 ε (|t| + |h|)`, the step
  ending just past it, as there. The books are integrated by adaptive
  Gauss–Kronrod (7 and 15 points; each piece until the two rules agree to
  `rtol` of its integrand's magnitude and its share of `atol`), or one
  15-point rule per step without their error control. Against exact
  answers (`lsim-solve/tests/stateless.rs`: a sine source across a
  resistor whose resistance steps, a level sensor on the source, a phase
  counter only root finding can find, a sampled block holding a current
  as a second source's voltage), on both backends: the outputs exact (0
  difference), the step's `when` at its time, the level's within an ulp
  (searched ahead, section 8.2), the phase counter's 1.4e-15 and 1.1e-14
  s after the exact times, the books 1e-16 and 1e-15 of themselves off,
  78 steps over 2 s at rtol 1e-8 (it failed at its start before). A
  model with no state but an equation solved by iteration (`x³ + x =
  time`) is not this one: it runs on IDA, whose only entries of y are
  its iteration variables, and its outputs carry the integration's error
  (x 1.8e-5 off the exact root at rtol 1e-4, 2.3e-11 at 1e-10: the
  outside judge's `count-implicit-nostate-1.0`); solving them by Newton
  at every point instead, on `Stateless`, would make them exact, and is
  not done.
* **IDA's order with the energy books under error control.** SUNDIALS
  7.1.1's IDAS decides an order decrease on the states' errors alone
  (`IDATestError`), and its quadratures' test (`IDAQuadTestError`) did
  not revisit a decrease decided there, although its error test and its
  order increase take the states' and the quadratures' errors together.
  On states exactly polynomial in time (a constant torque spinning a
  load up through the library's final drive, a DAE) the states' errors
  at orders k and k−1 are both zero, `0 ≤ ½ · 0` lowered the order at
  every order 2, and the books' quadratures (integrals growing as t²)
  then set the step at order 1: 6 406, 63 715 and 636 288 steps at rtol
  1e-6, 1e-8 and 1e-10 for a 2 s spin-up, growing as 1/√rtol, the gear's
  loss 0.26·√rtol off. Neither the run loop's restarts nor its options
  had a part in it (the run has no event). The vendored IDAS is patched
  (`lsim-sundials-sys/sundials/lsim.patch`): whenever the quadratures'
  errors are the larger, the decrease is decided on the two together.
  Now 96, 116 and 184 steps, most of them in the first nanosecond (where
  the gear's efficiency blends through zero power), the loss 2.4e-10,
  1.1e-12 and 2.2e-14 of itself off (`lsim-solve/tests/ida_order.rs`,
  which IDAS without the patch fails; the final drive's exact-answer test
  holds its books to 1e-10 now, from 1e-5). CVODES decides on both
  already (the same spin-up without the gear's inertias, an ODE: 8, 9
  and 11 steps). Every golden case is a DAE with books, and each takes a
  slightly different sequence of steps: the figures and channels move by
  about the tolerance (the largest, 0.16 × band, the hybrid's clutch
  losses on the HWFET: 6.9e-10 → 5.3e-10 kW against a band of 1e-9;
  every other below 0.003 × band), each on the side of its band it was
  (199 of 201 figures, 984 of 1038 channels), the hybrid's shifts and
  their losses the same (to 1e-12 kWh), its controller's output changes
  within 9 of 10⁵, the books closed to 6.6e-10 or better (1.5e-9 before);
  the steps from 2 % fewer to 1.2 % more per case (0.06 % fewer in all),
  the run times within one run's noise of each other (all 14: 130.4 s
  before, 121.4 s after, one run each on the same machine).
  Judged from outside (section 15, `judge.py sundials`) on SUNDIALS' own
  IDAS examples with quadratures under error control, `idasSlCrank_dns`
  and `idasAkzoNob_dns`: built from the 7.9.0 release, IDAS reproduces
  both of LLNL's shipped 7.9.0 outputs to the last digit (steps,
  residual and Jacobian evaluations, error-test and convergence
  failures, the quadrature G); built from 7.1.1, it reproduces its 7.1.1
  outputs to the last digit but for the slider-crank's G in its 12th
  digit (3.3366160662909388 against the `.out`'s 3.3366160662934146,
  2.5e-12 apart; the build prints the 7.9.0 `.out`'s G, every count the
  same). The engine's copy is the 7.1.1 release with the patch, file for
  file. With the patch IDAS does not
  reproduce them: the patch changes the order decision wherever the
  quadratures' errors are the larger, not only where the states' are
  zero. At the shipped tolerances the slider-crank takes 312 steps
  instead of 251 and the Akzo Nobel problem 473 instead of 500, and G
  comes out closer to G computed independently (the slider-crank in its
  one minimal coordinate at 34 digits; the Akzo Nobel problem by Radau
  IIA at rtol 1e-13, whose y1(180) is 9e-15 from the test set's
  reference): 7.8e-7 off instead of 2.8e-6, 1.0e-10 instead of 2.5e-9.
  Over rtol 1e-4 … 1e-10 it changes the slider-crank at 1e-6 alone, and
  the Akzo Nobel problem at every tolerance: G closer at six of seven
  (at 1e-9, 2.6e-10 off instead of 9.9e-11), 22 % fewer to 45 % more
  steps, and as accurate for the same work or less (1.0e-10 in 473 steps
  where upstream needs 740 for 9.9e-11). A patch confined to the zero
  case (a decrease decided on the states' errors that are all zero
  decided again on the quadratures') leaves LLNL's examples bit for bit
  but does not mend the engine's: the spin-up then takes 136, 220 and
  303 steps, its book 3.3e-9, 6.3e-10 and 1.9e-11 off, and the gear ramp
  of `error_test.rs` 206 170 steps at rtol 1e-10, where the states'
  errors are round-off rather than zero; so the patch stays as it is.
  The slider-crank fails at rtol 1e-8 and tighter on every IDAS, patched
  or not (its corrector, at t = 4e-9 s).

* **Where the error test cannot pass.** Ten failures of the error test
  at one step (the step cut by four from the second on) stop SUNDIALS.
  Where that is a kink the integration's history cannot cross, the run
  now restarts the integrator at the last step's end, once at each
  point, and goes on: a fresh start, its rates from the model, crosses
  it (the report's notes say at how many points). It is met at a
  breakpoint of a table read along time, which the run stops at without
  a restart (section 8.2), where an iteration variable that is a
  derivative (`der(fd.w_out)`, from index reduction) changes its slope
  after a long steady stretch: a gear driven by a torque that starts a
  ramp at 1 s stopped there ("IDA failed (SUNDIALS flag -3)", h = 3.7e-6
  s) at rtol 1e-8 and 1e-10, and now runs to 1e-15 of the exact answer
  with one restart (`lsim-solve/tests/error_test.rs`). The Battery
  Electric Car's city cycle stopped the same way at its breakpoint at
  120 s, at rtol 1e-10 before the patch above and at 1e-9 with it
  (`lsim-project/tests/golden.rs`); one restart there now. Where no step
  passes at all, at a jump the model declares no event for (in a
  `noEvent` relation, a function that is not smooth), the integrators
  took steps shorter than the time's resolution, which leave it where it
  is, without end (CVODE warns of the first ten and goes on, IDA says
  nothing); ten in a row now stop the run there, naming the variable with
  the largest
  local error: the same gear with its torque jumping under `noEvent`
  stops at 1 s naming `der(fd.w_out)`, and the city cycle at 1e-10
  stops at 300.125 s, where no step passes the error test on
  `el_hvbus.t3.i` (not investigated further). Neither path is taken in
  the golden comparison, which is the same to the last digit. No step
  passes either where the model turns singular, and the outside judge
  (section 15) met that far more often than a jump: the index-3 pendulum
  whose states index reduction chose at the start (y and its speed,
  released level with its pivot) stopped at the bottom of its swing at
  rtol 1e-7 and tighter (before, "IDA failed (SUNDIALS flag -9)",
  repeated recoverable residual errors, or no answer at all), so the
  message names both causes, a jump with no event or a singular model,
  and an equation that loses its solution. Where the cause is the states
  index reduction chose, the run now stops before, where they degrade,
  naming the constraint (section 6, step 5, *the choice, watched*). And
  where SUNDIALS itself fails (its flag), the run says what it sees where
  the step started, SUNDIALS' words beside it: an iteration variable
  whose equation loses its solution (`z² = 1 − t` past t = 1 s, where
  before the run ended on "IDA failed (SUNDIALS flag -4) … the corrector
  convergence failed" alone) is named with that equation
  (`error_test.rs`).

### 8.2 Events

* Zero crossings are located by SUNDIALS' root finding (Illinois method on
  the dense interpolant) with each crossing's direction; the spike locates
  its brake event to 7e-10 s at rtol 1e-10.
* At an event: `when` actions run (`ModelFunctions::when`), mode Booleans
  flip (a mode's crossing is positive where its relation holds;
  `ModelFunctions::modes` sets every mode from its relation: section 5.6),
  then **event iteration**: re-evaluate the conditions with the new
  discrete values until nothing changes (bounded, with a diagnostic naming
  the chattering parts if it does not settle), then **re-initialise**:
  CVODE restarts from y; IDA recomputes consistent `z` and `x'`
  (`IDACalcIC`). Both sides of the event are recorded at the same time.
* **Time events** (sample ticks, prescribed-input breakpoints, case end)
  are reached exactly with the integrator's stop time, never by root
  finding.
* **Chattering** (a friction mode flipping every step) is prevented in the
  component models by hysteresis-free stick/slip formulations with mode
  conditions on the *forces* (stick while |τ| ≤ τ_static), the standard
  Modelica approach; the run loop reports more than N events in a time
  window with the parts involved.

**As built (work package 4).** Event iteration runs `when` actions, then
`ModelFunctions::modes`, then re-evaluates every condition until nothing
changes (at most `max_event_iterations`, 50); the integrator restarts only
if a discrete value changed. After every event each root function is
given the side an exact zero counts as (the side it is on, else its mode's
value, else the direction it crossed in): a function that rests at zero
after its crossing — the spike's speed is exactly 300.0 for the first
steps after the brake engages — no longer fires again (IDA reported the
brake five times without this), and one that the iteration variables,
solved again at the event, put back a little short of zero is held on
the side it crossed to (below, *Conditions on iteration variables*).
After every accepted step the modes are
checked against their relations, catching a crossing that started exactly
at zero right after a restart, where root finding cannot see it. Table
guards (`table_guard_list`) are watched as extra root functions: an
`Error` axis stops the run naming the table; the others are booked as time
outside, reported as warnings. The model's asserts are checked at every
accepted step. An event storm is more than `storm_events` (100) state
events (crossings and modes; sample ticks and time events do not count)
within `storm_window` (1e-3) of the run's length, at least 1 µs; the
error names the conditions with their counts (the relay test: "101
events within 1.1e-12 s, from 'Relay': on (101×)").
A mode whose two sides both drive the state back onto its boundary is
such a storm: the trajectory slides along the boundary (Filippov
sliding), and the run loop, which knows only the two sides, switches at
every instant. It does not slide; it stops and names the mode. The
library's Driver did this until 20 October: its integral, frozen while
the command was held at a limit it pushed against, made a heavy car that
fell behind and caught up slide along full throttle (the Battery Electric
Car at 6000 kg on the WLTC, stopped at 961.83 s). Its anti-windup is now
`LimPID`'s back-calculation, which has no mode (section 12.2,
`blocks/vehicle.rs`). One such storm is left on that car's map (section
15): its battery at its minimum SOC, whose delivering mode switched 93
times in 2.1 s as the car, coasting at its empty battery, began to
recuperate (the long-haul cycle at 11 t).

**Run-loop fixes after the golden comparison (work package 4, second
round).**

* *`when` semantics.* A `when` fires when its condition changes from false
  to true at an instant, compared with its value just before that instant:
  with the discrete values before a sample tick set its outputs, not after
  (a condition a tick's outputs made true never fired). Nothing fires at
  the start, as in Modelica (`pre(c) = c` after initialisation; sampled
  blocks' initial outputs are start values too): a condition already true
  at the start fires once it has been false; one exactly at its threshold
  counts as it is written: `x >= 0` holds at zero, `x > 0` does not and
  fires as x leaves zero (`PreparedWhen::strict` keeps the difference,
  which the shared zero crossing loses; an exact zero of a `when`'s
  crossing counts as the side its condition holds on, at the start and
  after events). What must hold from the start belongs in the start
  values (the IR has no `initial()`).
* *Iteration variables in event iteration.* Whenever a discrete value
  changes during event iteration, the iteration variables are solved again
  (states held, `Integrator::consistent_z`) before the `when` values, the
  modes' relations and the conditions read them (`RunInfo::events_read_z`
  says whether any does). The trait's default fails the run: an
  integrator of DAEs that does not implement it cannot go on silently
  with iteration variables that no longer hold (an integrator of ODEs
  never sees it).
* *Conditions on iteration variables* (`Loop::update_sides`). The
  integrator locates a crossing on its interpolant of everything it
  integrates, the iteration variables too, which agree with their
  equations only to its tolerance; where the event solves them again
  from the states, a condition on one of them (`x > 0.5` with `x³ + x =
  z`) can come out a little short of zero, on the side it crossed from
  (2.5e-8 at rtol 1e-7, the crossing located 4e-8 s early). The
  integrator, restarted there, saw it cross again a moment later: a
  `when` fired twice for one crossing, three times for `x³ + x = 2.3 time`
  with no state at rtol 1e-10 (43 of 140 runs over seven tolerances and
  ten rates, with and without the state; so already before the eleventh
  round), and a mode whose relation set it back, where then nothing else
  changed, stayed off until the integrator's next step end (its integral
  1.6e-2 short at rtol 1e-5). OpenModelica 1.27.1 counts one crossing at
  every tolerance: it evaluates a condition with its iteration variables
  solved, and holds a relation on its side by 1e-4 rtol of its operands'
  size (`relationhysteresis`). Here such a function is held on the side
  the integrator saw it on until it reaches that side itself (checked at
  step ends) or goes back past zero by more than its hold: four times
  what the iteration variables' tolerances allow in it, its change as
  each is moved by its tolerance, summed. The inconsistency measured up
  to 0.56 of that; an event that moves the condition itself (a `reinit`,
  a discrete value the iteration variable is solved with) moved it 5e5
  times that and more, and is not held, nor is anything where the event
  moved a state (an engagement) or where the new discrete values alone
  move the function at the point the integrator had. The backends map a
  held value to its side as they map an exact zero
  (`Integrator::set_root_sides` takes the holds). A mode is not held: its
  relation decides at the event, and where that sets it back, the
  integrator restarts there and sees the crossing again (`Loop::undone`).
  Every one of the 140 runs counts one crossing now, a falling `when` and
  an oscillation read through `x³ + x = z` too, and the mode's integral
  is within 0.13 rtol of its exact 1.375 at rtol 1e-4 … 1e-10
  (`lsim-solve/tests/implicit_conditions.rs`, which failed before on all
  four). On the golden models nothing is held; one mode set back (the
  formula student car's accumulator held at its output power limit) is
  now met again 1.4e-14 s after its first crossing instead of at the
  integrator's next step end 1.7e-7 s later, the case's figures moved by
  at most 6e-4 of their bands.
* *Scheduled events are no storms.* What a sample tick or a time event
  changes at its instant (a controller switching an engine's throttle from
  one tick to the next) does not count towards `storm_events`.
* *One instant.* A stop time within 16 ulps of the current time (a tick
  that rounds just before the end) is that instant: the run loop does not
  step (SUNDIALS refuses such an interval) and handles what is due there.
  The output grid ends at `t_end` exactly. The ticks of all the blocks
  due at one instant (inside a step or at its end, a few ulps apart
  included) are one event: each block reads its inputs with what the
  blocks before it set, and an output point at that instant shows the
  values after all of them (its left limit those before them all). A
  block that reads an iteration variable, after a block before it changed
  a discrete value the iteration variables depend on
  (`RunInfo::z_discretes`, through the assignments), reads it solved
  again for the new values (`Integrator::consistent_z`, counted in
  `SolverReport::z_solves`); a computed channel it reads is evaluated
  with them too. (Work package 4's seventh round: B reading z = 2a after
  A set a read z for A's old output, for a whole period: b = 0.8 against
  1.0 at 0.5 s. A computed input read through its chain at a tick where
  the integrator stopped read the channels as last sampled: b = 1.8
  against 2.0.)
* *Exact time events.* A zero-crossing function that depends on time only
  between events (`c·time + b`, `b` of parameters and discrete values:
  `when time >= t_shift`, a mode of `if time > t_on`) is taken out of root
  finding (a root mask in both backends) and reached exactly as a stop
  time, fired in its direction there, rescheduled after every discrete
  change (`RunInfo::time_crossings`). Its modes take their values just
  after the instant while its time, with the discrete values the event
  iteration has set, is still the instant (a timer that re-arms itself at
  its own instant leaves its relation false; `t_last := time` makes `time
  > t_last` true right after). At every scheduled event, as at a root,
  the ticks due at that instant join the event; a root located within a
  few ulps of a scheduled time (SUNDIALS may report it instead of the stop
  time) is that instant, and what is scheduled there joins its event.
* *Tables read along time are stop times.* A table read at a position
  that moves with time alone (`c · time + b`, `b` of parameters and
  discrete values, found through the assignments: a driving cycle's
  target speed) has its breakpoints as stop times
  (`RunInfo::time_tables`): no step spans one, so its kinks fall on step
  ends and a condition it drives is checked at least at every
  breakpoint. Root finding sees a condition only by its sign at step
  ends, and when nothing the integrator integrates moves (a car at rest)
  its steps grow to many seconds: the golden comparison's Battery
  Electric Car in winter once stood 21 s at a start because a step from
  507.8 to 532.6 s spanned its motor's switch-on and switch-off (both
  ends off). A stop is no restart: the integration goes on with its
  history; except where the model reads a derivative of the table
  (index reduction through it, section 6 step 5), whose jumps at the
  breakpoints are kinks of what is integrated: there the run stops where
  the cell before the breakpoint still holds and restarts
  (`TimeTable::kink`, `SolverReport::kink_restarts`). The breakpoints are those of the data the model interpolates:
  a compiled model's tables may be swapped after preparation
  (`JitModel::with_tables`), so the run takes them from the model
  (`ModelFunctions::table_axes`) where they differ from the prepared
  ones, for these stops and for the enclosures of the conditions on
  functions of time (`RunInfo::with_model_tables`).
* *Conditions on explicit functions of time.* A zero-crossing function
  that reads time beyond `c · time + b` (`sin(2π time / T) > 0.95`: a
  heater, a PWM, a load switched by a sine) is classified by preparation
  (`RunInfo::time_functions`), through the assignments: the computed
  variables that read time are replaced by their definitions. One that
  then reads only time, parameters and discrete values is taken out of
  root finding, as a time crossing is, and its sign changes are found
  ahead without integrating: interval arithmetic rounded outwards
  encloses it, its rate and its second rate over a time interval (a 1-D
  table by the compiled interpolant's cubic pieces); the search skips an
  interval whose enclosure keeps the sign, or where it is monotone with
  the same sign at both ends, otherwise advances by what the bound on
  its rate allows (a function of value g and rate at most L cannot reach
  zero within |g| / L), and bisects a bracketed change to adjacent
  floats. The integrator stops there exactly and the crossing fires in
  the direction the sign changes; it is searched again after it fired,
  and when a value it reads changed. A grazing touch without a sign
  change is passed. A function that also reads continuous variables
  (`x > sin(ω time)`; a driver's command from a driving cycle's target
  against the car's speed; a motor's limits from its maps at that
  command) stays with root finding, which sees a sign change only between
  a step's ends, and every step is checked for two inside it, a pulse:
  the driving cycle's conditions too, whose target the breakpoint stops
  keep linear within a step while the speed it is compared with curves.
  The function is evaluated through the chain of the assignments it reads
  (a variable several of them share once; the steps that do not move
  along a step enclosed once while the discrete values keep theirs), not
  expanded into one expression: a motor's limits read 45 and 49 of them,
  which expanded ran to 160,000 and 250,000 characters. Both checks read
  the integrator's dense output over the step as the polynomial it
  interpolates with (`Integrator::dense_output`: CVODE's Nordsieck array
  and IDA's divided differences as they hold them, through two more C
  helpers; diffsol's interpolant through six Chebyshev points, exact for
  its order up to a round-off carried as an error), so no pulse can hide
  between samples. On SUNDIALS the checks are rigorous; on diffsol that
  carried round-off, 4096 ε of the largest sample, is an estimate, not a
  proof (diffsol 0.17.1 keeps its differences and order private, so the
  round-off of its own interpolation cannot be bounded from outside).
  Two checks, the cheap one first. A certificate: a time
  window and a box of the states it reads over which its enclosure
  excludes zero, made around the state at a step's end (the box's
  half-width the width of the state's range over the last step for each
  step of the window, and one more; the window doubles while certificates
  hold, and an attempt that fails waits a few steps, up to 16, before the
  next). A step inside the window over which each state stays inside the
  box needs no more: first by how far the state may stray from its
  polynomial's value at the step's end (the sizes of the basis functions
  over the step: a few products per state, for all such functions), then,
  where that is not enough, by its range (each basis function's range by
  interval products); both rounded outwards. Where none holds, the step
  along the dense output: the states and iteration variables the function
  reads as that polynomial, in powers of the time with interval
  coefficients rounded outwards (the value widened by the round-off of
  evaluating it in floating point; diffsol's error carried, by Markov's
  inequality for the rates); the whole step's enclosure first, then the
  same search for the first sign change. One root finding did not report
  ends the step there, at the first time the model's own root function on
  the dense output is on the new side, as a root does
  (`SolverReport::pulses_found`); the crossing root finding located at
  the step's end is not one (it locates a root to 100 ε (|t| + h), a
  little after the change found here, which is then the only change up
  to the step's end). A 2-D table is enclosed cell by cell: each region of
  its grid holds a polynomial (bilinear or bicubic inside the data, of
  degree 1 along an axis past it), fitted on first use to the model's own
  interpolant at a tensor grid of points inside the region, exactly up to
  round-off, and checked at one more; a box across cells takes their
  hull. A comparison whose sides' difference is strictly monotone over an
  interval flips once at most, one way: the search treats it, and a
  branch it selects between, as monotone. A function whose value moves
  only where a `noEvent` comparison flips (built of values constant
  between events, of such comparisons and functions of these: a motor's
  "running" flag, a command other than exactly 0) is left to root
  finding, as `noEvent` asks: no event needs locating where such a
  comparison flips, and the comparisons a model makes events of are
  modes, constant along a step. On the golden models the certificates
  clear all but 6,386 of 862,704 condition-steps of the BEV's WLTC and
  all but 677 of 1,145,426 of the hybrid's mixed cycle; the checks took
  3.3 % of the BEV run's instructions (four conditions, two of them the
  motor's chains, on steps a tenth as costly as the hybrid's) and 0.64 %
  of the hybrid's; lsim-project's `scan_share` example records the
  check's share of a run's time (the BEV's WLTC: 4.8 %, the median of
  five runs) with those counts, so that a regression shows. After WP3's
  compiled books and one-pass check the checks take 3.6 % of the BEV
  run's instructions (674 M of 18.76 G) and 5.5 % of its time (section
  16, WP3). What cannot
  be searched
  (`atan2` or a derivative that moves with time, a table whose points are
  not known; a function not defined where its search starts; a search
  that makes no headway) is named in a warning and left to root finding:
  never silently. **A known limit:** a condition that reads continuous
  variables but not time (a speed against a shift threshold, a clutch's
  slip, `x > 0.5` with x a function of the states) is not checked along
  the step at all, and without a warning: root finding sees it by its
  sign at a step's ends, and two crossings inside one step go unseen, as
  with every integrator that locates events by root finding. The outside
  judge shows it (`count-explicit-pulse`: `x = z − z³/4` with z = t is
  above 0.5 from 0.539 to 1.675 s, inside one of the 8 to 11 steps the
  integrator takes, and the `when` on it is missed at four of the seven
  tolerances; a harmonic oscillator's nine peaks above 0.9999 over 62 s
  were counted 9, 0, 1, 1, 6, 4 and 4 times at rtol 1e-4 … 1e-10,
  OpenModelica 1.27.1's 0, 0, 2, 8, 4, 9 and 9). Checking such conditions
  as the mixed ones are checked was tried (the same certificate and scan
  for every condition that reads continuous variables): it finds the
  pulse and the nine peaks at every tolerance, but on the golden models
  it is no switch to throw. The hybrid's clutch stick conditions, so
  checked, meet sign changes root finding steps over and its HWFET run
  stops as an event storm at 14.6 s (101 events within 0.74 s); its
  mixed cycle fires 19 more events and moves 81 of its figures and
  channels; and with these conditions interpreted (the compiled condition
  kernels cover only conditions that read time) its runs take 12 to 13
  times as long (the UDDS 566 s against 44 s). It waits for the clutch's
  switching to be looked at (the Driver's anti-windup, the solver work's
  item D, is no mode since 20 October; the storm is the clutch's) and for
  kernels for these conditions; until then the limit stands, recorded
  in STATUS.md. (Work package 4's sixth round: the review's `sin(2π
  time / 10) > 0.95` while nothing integrated moved was stepped over
  entirely, x(100) = 0 against 10.108, without a warning; now
  10.108262410426, exact to round-off, in 34 steps. The seventh round:
  the extremum stops then used for the mixed ones missed a pulse inside a
  monotone stretch of the time term, `sin(time) > x` with x' = 0.3,
  0.29 s long on the sine's rising flank: 0 of 1 fired in 5 steps,
  without a warning; the scan along the dense output fires it, in 5
  steps. The eighth round scans the driving cycle's conditions too: a
  linear table of time against a state that curves, above it for 0.3 s
  inside one step, fired 0 times with root finding and the breakpoint
  stops alone, and fires once now, through a 1-D table and through a 2-D
  one. The ninth round reads the integrator's polynomial instead of
  fitting one: a state that swings by k √3 / 36 inside one step, back at
  its value at the step's ends and middle (k s (s − ½)(s − 1)), passed
  the check that sampled those three points with a box of half its
  swing; its bound over the step does not.)
* *Both sides of an event at an output time.* An output time that falls
  exactly on an event records both sides, as Modelica tools write two
  rows at that time to their result files: `SimResult::values` holds the
  value just after the event (as every consumer has read it),
  `SimResult::left_limits` the index and the values just before it of
  the channels the event changed (the channels as they stood before the
  first event of that instant changed anything), in order;
  `SimResult::before(c, k)` gives either side's value of any channel.
  The run's first output time is the exception: its point (and the
  trace's first point, and the first interval's extremes, which the
  run's integrals hold) is the initial state, before the events of
  t = 0 (the Vehicle at rest starts sliding, with the acceleration of
  its rolling resistance, and sticks within microseconds, so a load
  transfer that follows its acceleration is off by a few N for no
  time). A known limit of the recorder: the readers of the evidence
  allow for it (the app's lift check starts at the second traced point).
  The golden comparison compares today's engine, which records such a
  point before its step, with the left limit. (Work package 4's sixth
  round kept every channel at first: on the hybrid's UDDS, where a
  controller tick falls on 1092 of its 1370 output points, that was
  4.18 MB beside 5.20 MB of values. Of the 474 channels, an event changes
  97 on average: kept as `u32` channel numbers with their values, the
  left limits take 1.33 MB.)
* *Restarts.* A restart hands IDA `y'` in full (`x'` from the model, `z'`
  from `0 = g_x x' + g_z z' + g_t`), skips `IDACalcIC` (the point is
  consistent) and the Newton solve when event iteration just did it, and
  sizes the first step as CVODE sizes its own (`h0 = ½ √(2 / ‖x''‖)`,
  capped by the step the integrator had planned; for CVODE too). On a
  sample-and-hold DAE the steps per changing tick fell from 11.4 to 2.2
  (IDA) and 2.6 to 1.9 (CVODE). The remaining steps on the hybrid resolve
  the fast transient each torque command excites (the tyres' slip settles
  in about 1e-4 s), which the error test on the iteration variables
  demands. A tick whose changed outputs reach nothing the integrator
  integrates or watches (`RunInfo::dynamic_discretes`: no state
  derivative, residual, energy integrand, zero crossing or table argument
  reads them) leaves the solution exactly as it is: the step stands,
  without a restart (`SolverReport::inert_ticks` counts them;
  `light_restarts` counts only the opt-in kind below).
* *Light restarts are opt-in* (`SolverOptions::light_restarts`). Going
  on with the integration's history after a slight tick (only its
  outputs changed, no condition changed side, the jumps of `x'` over the
  planned step and of `z` within a tenth of the error test's budget)
  passes each step's error test, but the history carries the kink the
  tick put into `x'` into the next steps, and for a steadily moving
  command that error has the same sign at every tick: it accumulates to
  about half a tick times the command's whole change. On a 10 000-tick
  ramp at rtol 1e-6 that is 57 tolerance units against 1.6e-11 with
  restarts; a sine command, 5.1e-6 against 3e-15 (the review's tests,
  now in `lsim-solve/tests/run_loop.rs`). A rigorous bound would have to
  carry the kink through the variable-order history, so the default
  restarts.
* *`suppress_algebraic_error` stays off.* Leaving the iteration variables
  out of the error test takes the hybrid's first 100 s from 230 234 to
  62 458 steps and the BEV's first 50 s of WLTC from 7 076 to 2 307; the
  exact-answer suite still passes, but at the same tolerance the DAE
  path's errors grow up to 8× (elec_rc_step's current 6.6e-9 → 5.2e-8,
  motor_dc_spinup's 6.0e-10 → 4.7e-9 and its event 1.4e-10 → 1.3e-9 s).
  Accuracy comes first: it stays an option, off by default.
* *Rigid engagements: the impulse projection* (`lsim-solve/src/run/
  impulse.rs`). Only a part's declared engagement starts one
  (`EngagementDecl { changes }`: the library's gearbox and a gear whose
  ratio is a signal declare their ratio): when `changes` takes a new value
  at an event, the speeds the coupling ties together jump as an
  instantaneous, rigid engagement makes them. A stored energy that merely
  depends on a discrete value starts nothing (the review found the first
  version undoing a model's own `reinit`: a bouncing ball fell through
  the floor), and the states a `reinit` set at the event
  (`FlatSystem::restarts`, the jump that changed) stay where it put them.
  Two stages, as the physics has them in the rigid limit:
  1. *The rigid engagement.* The states coupled to the variables the
     engagement moved (with the states held), through the assignments,
     move so that the momentum the stored energies weigh is kept:
     `Bᵀ (∇E(U(x)) − ∇E(U⁻)) = 0`, `U` the variables the stored energies
     read, `B = ∂U/∂x`. For kinetic energies that is the perfectly
     inelastic engagement (`w_out⁺ = (J_out w_out⁻ + r J_in w_in⁻) /
     (J_out + r² J_in)`), whichever speeds are states. Its loss, `E⁻ − E_a
     ≥ 0`, is the engaging part's.
  2. *Stiff, unbounded links relax.* A part whose forces are bounded
     passes no impulse in zero time, and a tyre is one: its force is at
     most μ N, inside its grip as at it. After stage 1 its slip relaxes
     through its own law, inside its grip over its relaxation time (on
     the hybrid 0.2 to 7 ms, on a 300 kg two-axle car at 20 m/s about
     25 ms) or sliding at its grip, and the integrator follows that
     exactly; its loss is its own slip loss, booked as it happens. So the
     library's wheel declares no link, nor does a slipping clutch. A link
     (`ImpulseDecl { keep, active }`) stands for a coupling a model
     treats as stiff and unbounded: its `active` is judged at the state
     stage 1 leaves; it relaxes back to its relative velocity before the
     event (`keep = κ⁻`, its impulse λ), the same balance over the states
     the active links reach; a link that comes into its range where the
     others' relaxation leaves the states joins them and the stage is
     solved again (the set only grows). One link books what the stage
     loses; several share it as their stiffnesses say, which they do not
     declare, so the event books it as a whole (and the run warns once).
  The end state is the one projection keeping every active link would
  give (stage 1's change is orthogonal, in the masses' metric, to what
  stage 2 can move), so the total loss is the same. Against fully
  resolved runs:
  * a stiff, unbounded link (a linear tyre law with no grip limit) and no
    projection through it: the tyre's dissipation approaches its share
    within 1.2e-2, 1.2e-3 and 1.2e-4 as its stiffness grows from 2e4 to
    2e6 N per m/s (`the_tyres_share_is_what_a_stiff_tyre_dissipates`);
  * a tyre with a grip limit (the review's: F = clamp(k κ, ±4000 N), a
    7 → 12 downshift): it slides after stage 1, and the run is the one
    with its slip integrated at every stiffness (it was 0.196 m/s apart
    50 ms after the shift, the motor at 737 against 611 rad/s, when the
    grip was judged before the event and the slip relaxed at once;
    `a_tyre_past_its_grip_after_the_rigid_stage_passes_no_impulse`);
  * a car with a motor on each axle and the library's tyre law, against
    the car whose gear mesh is a stiff damper and nothing is projected:
    as the mesh stiffens tenfold the speeds over the whole transient
    come tenfold closer (4.7e-4 m/s at c_g = 1000 N·m·s/rad), and so do
    each tyre's and the gear's losses; relaxing the tyres at once while
    inside their grip, as the third round did, stays 0.67 m/s and 22 J
    apart (`a_two_axle_shift_matches_the_fully_resolved_car`).
  Today's engine relaxes the slip of every tyre that gripped before the
  shift at once, and books the impulse times the slip *before* the event
  to the tyre (negative on a downshift while driving). The losses are the
  coupled parts' stored energies before and after each stage
  (`Engagement`), nothing else that jumps at the same instant. Derivatives
  are exact: `B` and the links' gradients by forward-mode differentiation
  through the assignments (`lsim-solve/src/ad.rs`; the iteration
  variables' `∂z/∂x = −g_z⁻¹ g_x` from the compiled Jacobian), the stored
  energies' gradients and Hessians by second-order forward
  differentiation, and Newton's method solves the balance (one step for
  quadratic energies and linear kinematics; a stiffening energy `½ J w² +
  ¼ c w⁴` is kept to 1e-13). Event iteration goes on from the moved
  states: a condition the jump crosses fires at the event, a mode it
  crosses flips there, and an engagement it makes is projected in turn;
  a cascade of more than `SolverOptions::max_event_iterations`
  engagements at one instant stops the run with an event storm naming
  the engagement and the conditions (raise the limit for one that is
  meant: the review's 131 upshifts at one instant run exactly with it at
  200).
  `mech_gear_change` runs to 5e-16 with its loss exact;
  `SolverOptions::impulses` turns the projection off.

### 8.3 Initialisation

1. Start values: `fixed` ones are conditions, others guesses.
2. Solve the initialisation system (`PreparedModel::init`, compiled as
   `ModelFunctions::init()`: section 5.6–5.7) by damped Newton with the
   exact sparse Jacobian; `finish` gives the start vector.
3. If Newton fails: **homotopy** from a simplified problem
   (Modelica's `homotopy(actual, simplified)` operator, available to
   component writers, e.g. a battery with zero RC current or a clutch
   locked) with a natural-parameter continuation in λ ∈ [0, 1], step
   control by Newton iteration count.
4. If that fails: a diagnostic naming the block that did not converge, its
   parts, and the residuals' worst equations.
5. IDA then refines with `IDACalcIC` for the integrator's own consistency.

**As built (work package 4)** (`lsim-solve/src/init.rs`): one damped
Newton serves the initialisation system (`ModelFunctions::init`, then
`finish`, then every mode from its relation, solved again while a mode
changes) and the iteration variables at the start and after every event.
Dense LU up to 100 unknowns, sparse above; backtracking line search on the
row-equilibrated residual; it stops when the update's weighted norm is
below 1e-3 of the tolerance, or when the residual is at round-off after
full Newton steps with the update inside the tolerance (at rtol 1e-10 the
first test alone asks for less than round-off). The homotopy is the
Newton homotopy `F(w) - (1 - λ) F(w0)` until the IR has Modelica's
`homotopy()` operator for simplified models; it cannot pass a fold of the
solution path. A failure names the three equations with the largest
residuals and what they solve for.

### 8.4 Results, quadratures, sweeps

* Output on the case's grid from the dense output (the grid never
  constrains steps). Each interval's min, max and mean over every internal
  step and both sides of events (`Recorder`, working in Stage 1; the spike
  checks a mean against Simpson's rule on the closed form).
* **Energy quadratures** (WP4): each primitive's port powers, losses and
  stored energy integrated as extra quadrature states under error control
  (CVODE's `CVodeQuad*`), section 11.
* **Parallel sweeps** (WP4/WP6): `rayon` over parameter sets, one
  integrator and buffer set per worker, one shared compiled model, the
  Python GIL released for the whole sweep.
* **The run's evidence** (step 2, for the app's verdict and reports):
  the recorder also integrates each channel's square and magnitude over
  the whole run, as integrals of the line through every point the
  integrator visited (its steps, both sides of every event), the means'
  own trapezoid: `SimResult::square_integral` and `magnitude_integral`
  (a duty's RMS, a flow's two directions; exact for a value held between
  events, O(h²) on a smooth one over the integrator's own steps, not
  under its error control as the energy books are). A trace
  (`SolverOptions::trace`: channels and a spacing) samples a few channels
  from the dense output every spacing and at the run's end, interleaved
  with the grid points and the blocks' ticks as they are, and changes
  nothing the grid records (a test checks the values, means and integrals
  to the bit with and without it): `SimResult::trace`. A window
  (`SolverOptions::windows`: a channel and a span of time) keeps the
  channel's highest moving average over that span, of the same line
  through every point visited, read at each, with nothing before the
  start (an energy meter's log; 0 s: the highest value), and when the span
  ended: `SimResult::windows` (a battery check's averaged peak, which the
  output step does not change). The run tells each
  table axis it read past its data with numbers beside its words
  (`SolverReport::table_outside`: the table, the axis, when it first left,
  for how long, the argument furthest past the data at the steps' ends
  and the edge it was past, the axis's first and last point), each guard's
  argument taken from `RunInfo::table_args` in code generation's order (a
  test holds the order to the compiled model's guard list on every example
  case), and each warning `assert` that did not hold
  (`SolverReport::asserts`). A guard counts a read wherever its expression
  is evaluated, also in a branch of an `if` its result does not take (the
  E-Motor's drag table is read while the motor is powered), where today's
  engine counts only the operating point it used: the app's verdict can
  then name a table today's would not (a test holds today's verdict on
  such a case, marked as expected to fail until the guard counts only the
  branch taken).
* **A run's control** (step 2, `SolverOptions::control`, a `RunControl`):
  one hook serves a run's progress, a live run's stream, a study's Stop
  and a live edit. The run loop calls it on the run's thread at the end of
  every step it takes (and once at its end): `reached` is told the point
  reached and the output points recorded since, the values the result
  holds; `stop` ends the run there, at the end of the step taken, with a
  last output point at that time (the grid's times not reached dropped,
  the books and the trace closed there too, `SolverReport::stopped`); and
  `edits` gives parameter changes (`ParamEdit`: the whole vector), which
  are events. A change is taken at the end of the first step at or after
  its `from` (a live one: the next step) and made at the first output
  time after that point (or at its own `at`): the integrator stops there,
  takes the new values (`Integrator::set_params`), solves the iteration
  variables again with them, settles the conditions and restarts, the
  books booking the stored energy's change as at any event; the output
  point there takes the new values and keeps the old ones as its left
  limit; what else reads the parameters (the blocks' input tapes, the time
  functions and crossings) is set up again at the next step, as after a
  switch of the states. `SolverReport::edits` lists each change, when
  taken, when made. Given again with `from` the time it was taken, a
  change is taken at the same step's end and made at the same time, and
  the run is the same bit for bit: every step before it is (a change
  known from the start would not do: CVODE sizes its first step by the
  next stop time, and the values after the change moved by an ulp in the
  test that found it). The parameter sets a run's changes go on with are
  kept for its length beside the models a switch of the states goes on
  with, up to 1024 of each; past that a change is not made and the run
  warns.

**As built (work package 4).** The energy integrals are under the
integrator's error control by default (`energy_error_control`): without
it they ride on the states' steps, and a fast-decaying loss came out 100×
less accurate than the tolerance (the RC step's resistor loss at rtol
1e-10: 1.3e-8 of the energy scale; with it 2.5e-11). Where the states
are exact on long steps the integrals can be off by tens of percent (a
constant torque on an inertia from rest: 15.55 J supplied against 9 J,
on 1 s steps), while the closure, which compares the integrals with each
other, stays zero: only the drift shows it. Books computed without error
control say so (`EnergyBooks::error_controlled`, a warning, the summary)
so that nobody reads them as exact. The stored energy's
change is integrated too, its rate taken along the solution in the
direction (1, y') exactly, by forward-mode differentiation of the
declared stored energy (section 11), so the books close to round-off
when every part's books agree with its equations. The integrator's own error
estimate goes into the report: the largest local error of any step as a
share of the tolerance, and per variable the local errors summed over the
run (an upper bound of the global error that ignores damping).

Sweeps run one set per rayon task on a pool of the asked size (each run
single-threaded inside), sharing the compiled model. Measured
(`lsim-solve/examples/sweep_scaling.rs`, release build, the WLTC-length
ladder drive, 4 vCPUs, each measurement started once other processes had
used under 0.3 cores for 5 s, and their use during it read from
/proc/stat: 0.02–0.18 cores):

| sweep | 1 thread | 4 threads | speed-up | cores used | CPU per run, 1 → 4 threads |
|---|---|---|---|---|---|
| 16 sets, 21 states (100 ms runs), best of 5 in one process | 1542 ms | 448 ms | 3.44× | 3.75 | 96.3 → 105.0 ms |
| the same, a fresh process per sweep, best of 5 (median) | 1725 (1759) ms | 485 (525) ms | 3.56× (3.35×) | 3.36–3.69 | 106–111 → 112–120 ms |
| 64 sets, 21 states, best of 2 | 6644 ms | 1625 ms | 4.09× | 3.90 | 102.8 → 99.1 ms |
| 16 sets, 101 states (600 ms runs), best of 2 | 9446 ms | 2554 ms | 3.70× | 3.72 | 583.8 → 593.8 ms |

Measured again after the recorder's fix (below: each channel's storage
now allocated once at the grid's length; `vec![v; n]` had given it to
one channel only, the others grew by reallocation in every run), the
work's starting code and the new one alternately, 12 rounds of a fresh
process each, 16 sets: 3.32× → 3.54× at best, 3.23× → 3.22× as medians
(the 4-thread sweep 490 → 458 ms at best); in one process the 4-thread
sweep's page faults fell from 3 640 to 2 037 a run (16 sets) and from
3 554 to 782 (64 sets), its system time from 15 to 6 ms and from 12 to
4 ms a run. This VM's timings vary by ±15 % between repeats (the same
64-set sequential sweep took 6.5–10.1 s), more than the effect, so the
speed-up is resolved no better than that.

So ≥ 3.5× holds once a sweep has many runs or long ones; 16 runs of
100 ms lose about 6 % to the last runs' tail (3.75 of 4 cores used) and a
few per cent to the kernel: a run's results (271 channels × 1801 points
× value, min, max and mean: 15.6 MB) are new memory, about 4 900 page
faults and 10 ms of system time a run in a fresh process, and the
allocator's per-thread arenas reuse freed memory less than the main
thread's. Writing the results into one buffer backed by huge pages would
save most of those 10 ms in every run, sequential or not (a change to
`SimResult`'s layout, left for WP6 with the Python API).

## 9. Causal blocks, Script blocks and FMUs in an acausal network

* **Continuous causal blocks** (driver PI, PID, gains, sums, limiters,
  lookup tables, the driving task's speed trace, road profile) are ordinary
  equation components with `input`/`output` ports. A signal link is the
  equation `input = output`; they are sorted with everything else, and an
  algebraic loop through a controller is torn like any other.
* **Physical outputs as signals**: today's `sig_*` ports (battery SOC,
  motor torque…) are output ports whose equations read the physical
  variables (`soc_out = soc`).
* **Sampled blocks** run outside the equations through the `DiscreteBlock`
  trait (already in `lsim-ir`): Script blocks (sandboxed Python, as
  today), FMUs for co-simulation (FMPy in its sandbox, as today), digital
  controllers. Each has a period; its outputs are discrete variables held
  between ticks. The run loop reads the inputs from the solution at each
  tick, calls `tick`, and **restarts the integrator only if an output
  changed** (a restart costs a few small steps; most ticks of a slow
  controller change nothing). As built (work package 4) the steps are not
  cut at ticks: a tick inside a step is evaluated on the dense output
  (an input that is a state or an iteration variable costs one
  interpolation, any other one evaluation of the channels); only when an
  output changed is the step cut back to the tick and the integrator
  restarted there, and the block's next tick then becomes a stop time
  until a tick changes nothing again. So a block that changes nothing
  leaves the steps and the solution exactly as without it (tested: equal
  results and step counts). A tick that changes nothing is made cheap
  (`SolverOptions::tick_read_ahead`, on by default; off gives the same
  results and the same inputs at every tick to the bit, tested on three
  blocks with coinciding ticks, restarts and inert changes, ODE and DAE,
  both backends):
  - a computed input's chain of assignments runs as a flat tape
    (`lsim-solve/src/tape.rs`): the interpreter's operations, operands
    and order, so its values (tested bit for bit on random expressions;
    which NaN comes out of an operation on NaNs and the sign of `min`/
    `max` of ±0, which Rust leaves open, are this toolchain's in both);
    a chain the tape cannot lower keeps the interpreter;
  - the inputs of a block's ticks inside one step are read together
    (`Integrator::interpolate_select_many`): SUNDIALS' own dense-output
    formula (CVODE's powers of s and its sum from the highest power down,
    IDA's coefficients c_j; the same fuzz check and clamp) evaluated
    across the ticks, on AVX where the CPU has it (no fused
    multiply-adds), the tapes too; tested bit for bit against reading
    one by one on Adams up to order 7, BDF and IDA. A later tick that
    cannot be read ahead leaves the tick at hand to be read alone, so an
    error comes where reading one by one raises it;
  - a block due alone ticks in a row while nothing else (a grid point,
    another clock, the step's end) comes before its next tick, without
    allocating; a tick that changes an output goes through the event
    handling as before.

  Measured (`lsim-solve/examples/r1_blocks.rs`, release build, the run
  thread's CPU time; the 21-state ladder drive over a WLTC's 1800 s,
  100 ms a run, 7221 steps; a 10 ms block, 180 001 ticks). Instructions
  (callgrind, exact), the run with the block against the same build
  without one, and before → after this work:

  | the block reads | 21 states | 101 states |
  |---|---|---|
  | a discrete value | +19.0 % → +4.4 % | +4.2 % → +1.0 % |
  | a state | +26.5 % → +6.4 % | +5.8 % → +1.4 % |
  | a computed channel (3 assignments) | +42.8 % → +7.6 % | +9.3 % → +1.7 % |

  Time (median of 8 rounds on a quiet 4-core VM, best of 5 runs a
  round, the period shortened to 1 ms so the ticks stand above the
  machine's noise and their cost scaled back to 180 001 ticks): a tick
  that changes nothing costs 9.5 / 20 / 21 ns (from 66 / 92 / 130 ns),
  so the 10 ms block 2.0 / 3.9 / 4.4 % of the 21-state run (from 13.0 /
  16.9 / 25.0 %). Run at the 10 ms period itself (fewer ticks to a
  batch, and the difference between two 100 ms runs is near the noise)
  the same medians are 4.4 / 5.1 / 4.9 %. On the 101-state drive (one
  round): 0.5 / 2.1 / 2.8 %. So the < 5 % criterion holds for the
  101-state drive and every costlier model, and the smallest model is
  at it: under it in the precise measurement, at 5 % ± its noise run as
  is. What is left is per tick: about 160 instructions of bookkeeping
  and the block call, and for a computed input about 120 more (the
  dense output across the ticks and the tape). The block's own work
  comes on top: a sandboxed Python tick costs microseconds.
  Today's Script blocks run every solver step (≤ 10 ms); they keep that
  rate by default (a `period` parameter, default 10 ms) so results match.
* **Script blocks without Python** (`lsim-script`, WP6). A Script's code
  runs on an interpreter of a strict subset of Python whose every
  operation has CPython's semantics to the bit: 64-bit integers (a
  result beyond them is outside the subset, never wrapped), IEEE floats
  with CPython's own floor division and modulo, exact comparisons
  between integers and floats, dicts (`1 == 1.0 == True` one key), lists,
  tuples, `if`/`for`/`while`, functions with defaults and `global`, the
  builtins scripts use, `clamp`, `interp` exactly as app/solver/maps.py
  computes it, and the math module's exact functions (`sqrt`, `floor`,
  `fabs`, `copysign`, … and the constants). The example cars'
  controllers (the P2 hybrid's Hybrid Control Unit) are in the subset.
  **The reference is the app's own Script runner** as the app calls it:
  the locked-down worker's call of `step()` (`sandbox_worker._run_step`,
  CPython 3.11), the parameters typed by the project model (schemas.py's
  `ParamValue`: a table's values are floats, whatever the file writes),
  a fresh shallow copy of them each call, and an input of -0.0 passed as
  +0.0 (sandbox.py's `float(x or 0.0)`); its protocol too: the worker's
  frames give a call's number of inputs and outputs and each output key's
  length in UTF-8 bytes in 16 bits (`_pack_step`, `_pack_outputs`), and
  past 65 535 the worker stops and the app's run fails, so such a call is
  left to Python. What it does not run is left to
  Python, so a result never differs from the app's: a script that uses
  anything else (other imports, the transcendental math functions and
  powers of floats, which CPython takes from the C runtime and so differ
  in their last bit between platforms, string formatting, classes,
  exceptions, comprehensions, slices, unknown names) is refused when it
  is read and runs on Python, and so is one whose parameters the app
  reads otherwise or refuses (a null, a list, a table holding other than
  numbers) or JSON cannot tell apart (`-0`, Python's integer 0, from
  `-0.0`); an operation met while running that Python supports and the
  subset does not (an integer beyond 64 bits, a string method, a search
  for a NaN in a container, which Python matches by identity, copying
  the sign of a NaN, which CPython's own specialised and plain additions
  give differently, a builtin subscripted, which is a generic alias in
  Python, `int()` of more than 640 digits, Python's lowest digit limit)
  stops the run, which is made again on Python. A NaN result is a NaN;
  its sign and payload are not part of the promise (IEEE 754 leaves them
  open). An exception Python raises is raised with CPython 3.11's
  message, quoting values whole as Python's repr shows them; a message
  whose words are not known here is left to Python: one that would quote
  an object's address (a function, a bound method, an iterator object,
  the scripts' `clamp` and `interp`), the parameters in the project's
  order (the importer keeps them sorted), a character beyond the common
  scripts and symbols whose printing Python 3.11 decides by Unicode 14,
  or a value nested or long beyond what is quoted here. So is what the
  parameters' order decides otherwise (outputs returned in it, the
  parameters or a copy of them or of a table among them; which of a
  table parameter's keys 0 and -0 maps.py names the duplicate), and
  what CPython's compiler or C code decides rather than the language:
  an unhashable key in a dict display of more than 15 pairs (3.11
  builds up to 15 from all their keys and values evaluated first, as
  here, and longer ones in runs of pair by pair), `x in range(n)` of an
  x that is no integer over more than a million items in a call (CPython
  compares it with each, in C, past any time limit), what a call prints
  beyond 1024 writes or 64 KiB, a few milliseconds of CPython's time
  (nothing is printed here, while CPython writes each argument, the
  separator before it and the end of the line, in C, past any time
  limit: the review's 400 000 lines of 512 KiB returned here in 0.05 s
  and were stopped at the limit there) or other than numbers and strings
  of ASCII characters (Python writes text in its standard output's
  encoding, on Windows the ANSI code page, which may not have them), and
  an if/elif chain of more than about 100 branches: each `elif` is a
  level of nesting, as in Python's tree, where CPython refuses some 990
  in a row. What CPython
  refuses to compile is refused here when the script is read (a
  module-level `global` of a name already assigned or used among them).
  **Limits**, those of the app's worker in size: a call stops
  after 2 s, timed from its inputs and its copy of the parameters made
  to its outputs read (as the worker is from the request to its reply):
  the clock is read every 1024 statements, loop rounds, calls and
  operations of expressions, and as every operation whose work grows
  with the size of what it handles goes (a call's arguments, keywords
  and frame, displays, unpacking, concatenation, copies, comparisons and
  searches of containers, an item of a `zip`, indexing and converting
  text, reading the outputs), so no call's work goes unmetered: a single
  statement over large containers, a call of 20 000 keywords or one
  expression of 2¹⁷ operations stops at 2.0 s too. Reading a script
  costs about its length (a MiB in about 0.2 s, whatever its shape). A
  runner may hold 512 MB: every heap block its values make counts as the
  heap keeps it, from before it is made (strings, lists, tuples, dicts
  and their indexes, iterator objects and what they keep of their
  sources, bound methods, functions and their defaults, `interp`'s
  parsed tables, a call's frame while it runs), and the syntax tree 128
  bytes for each byte of code (it holds at most about 100); a script of
  more than 1 MiB is not read. `tests/memory.rs` checks
  it with an allocator that counts every block: each kind of value counts
  exactly what it holds, and scripts that outgrow the cap in each kind of
  value stop before the process holds about 530 MB. Past either limit the
  script is left to Python, whose own limits decide. They are
  not measured the same way: Python is slower and its values larger, so
  a call close to either limit can be stopped by the app's worker and
  return here. On top, outside the subset: a million items a container,
  calls 50 deep, and nesting deeper than half a MiB of the caller's stack
  allows. That is measured where the interpreter nests (its expressions,
  blocks and calls, the parser, the walks through nested data), so no
  script overflows a caller's thread of 1 MiB in any build: tested on
  1 MiB and 2 MiB threads in the test, unoptimised and release profiles
  (`lsim-script/tests/stack.rs`; an unoptimised build's frames are larger,
  so it leaves more deeply nested scripts to Python). It reaches nothing
  outside the script. Evidence: `lsim-script/tests/parity.rs` runs the
  app's runner (`tests/parity.py`, needing pydantic as the backend does)
  and the native one on the same calls (the Hybrid Control Unit on four
  drives of 6000 calls, fifteen scripts that exercise each feature, 300
  random expressions over signed zeros, subnormals, the largest floats,
  infinities and NaN): 35 681 calls bitwise equal, 2725 failing on both
  with the same message, 121 random calls handed back to Python, none
  different; its edge cases add 129 errors scripts meet, each failing
  with CPython's message or left to Python, and 22 scripts CPython
  refuses to compile, each refused here too; `tests/app_runner.rs` checks
  the cases the reviews found and the limits; and the hybrid's four cases give bitwise the same results
  on both runners (`lsim-project/examples/script_parity`).
  A Hybrid Control Unit call costs 6.5–7.1 µs natively (release build,
  `lsim-script/examples/hcu_speed`, on a 4-core VM where the runner
  before the limits took 7.3–7.9 µs) against about 25 µs in-process in
  Python and far more across a process boundary. Counting every
  operation and argument on the clock changed nothing measurable: 12
  runs alternated with the build before it on a noisier VM gave 6.8–10.0
  against 7.0–10.3 µs, both medians 7.5 µs.
* **Numbers in JSON** are parsed correctly rounded (serde_json's
  `float_roundtrip`), as Python reads them: without it about one decimal
  in eight with many digits came back one bit off (measured: 239 004 of
  2 000 000 shortest representations), which moved the golden
  comparison's Python Script host's outputs and any project parameter
  written with many digits.
* **FMUs for model exchange** (WP4, later): their states join `x`, their
  derivatives and event indicators are called from the generated code's
  slots through a C-ABI shim; Jacobians by directional derivatives where
  the FMU provides them, else finite differences on that FMU's block only.
* Sampled blocks are not allowed inside a fast-mode run's algebraic path
  (the inverse model must be evaluable at any time); a Script block in a
  fast-mode case runs at the fast-mode step and is flagged when its output
  would feed back into the prescribed motion.

## 10. Fast mode: the inverse model

* The case's speed trace is **prescribed**: the vehicle body's speed and
  its derivative become known inputs (`InverseSpec.prescribed`); the
  driver block is removed and its commands become unknowns
  (`InverseSpec.freed`). The same equations, prepared again with that
  known set (`prepare_inverse`): index reduction differentiates what the
  prescription constrains (rigid driveline speeds follow the wheels'), the
  sorted model computes the required torques, currents and powers
  explicitly backwards from the wheels to the sources.
* Remaining states (state of charge, RC voltages, temperatures, fuel mass,
  compliant shaft twists) are stepped at the trace's spacing (1 s on WLTC)
  by **ROS34PW2**, a Rosenbrock-W method: L-stable for the stiff electrical
  states, linearly implicit (one LU, four solves per step, no Newton
  iterations) and tolerant of a Jacobian kept over many steps. With the
  speed trace linear between samples, each step sees the constant
  acceleration the standard backward-facing (quasi-static) method uses.
* **Limits flag, they do not act**: every `limit(x, lo, hi)` in the model
  (motor torque at speed, battery power, brake capacity, engine full load)
  passes its value through and records each stretch of time it is outside
  its band (`LimitFlag`: start, end, which part, worst excess). Fast mode
  never falls back to forward simulation; it answers "what would it take"
  and says where the vehicle could not have followed.
* Cost: a few residual calls a step (tens of ns each for a vehicle) plus a
  small dense LU refreshed only when the Jacobian drifts; measured, 1800
  steps of the test car in about 6.0 ms (3.0·10⁵× real time, STATUS WP6).
  The speed goal is section 2's: faster than the average of comparable
  tools, measured against them.
* **Input events.** A trace is a function of time: its breakpoints are the
  samples where its slope changes (a sample on the line through its
  neighbours, to four units of round-off, is none), steps are split only
  there, and each piece is read as the line through its ends, so samples
  added on a trace's lines change nothing beyond round-off (a property
  test, `lsim-fast/tests/input_events.rs`). At a breakpoint the inputs
  that are derivatives (`der(body.v)`) jump, and at a block's tick its
  outputs may: an input event (FMI 3.0: an input with a discontinuity).
  The stepper enters event mode there: with the roots just before the
  jump (the last step's end) and just after it, it fires each `when`
  whose condition became true (Modelica's semantics, as the forward run
  loop has them: `x >= 0` holds at zero, a strict `x > 0` does not, so a
  strict `when` fires as its crossing leaves zero upwards and not as it
  reaches zero from below; `WhenInfo::strict`), runs the actions, sets
  every mode from its relation and repeats, each round's roots against
  the last, until nothing changes (a located event's actions are followed
  by the same iteration). After each step it checks the conditions whose
  crossings were exactly 0 at the step's start and left it within the
  step, which no crossing test sees: a mode whose relation that changes,
  a strict `when` that becomes true. They change at the step's start:
  event mode there, those crossings taken on the side they leave to (as
  just after the instant; the mode held there, not set again from its
  relation, which at zero says the opposite), so the modes and clauses
  that depend on them follow at once; then the step again. A mode wrong
  for another reason is set at the step's end. Before this, a mode on
  the acceleration kept its value across every sample where the
  acceleration changed sign (the review's probe, x' = d with d =
  [der(v) > 0] on 0 → 1 → 0 m/s: x(4) = 4 instead of 2), and the inverse
  body's rolling resistance stayed off after the first start from rest:
  the example cars' charge used came out 25 to 48 % short on 7 of their
  9 cycles, without a flag; a strict `when der(v) > 0` counted the stops
  of a trace, not its starts (OpenModelica: 2 starts, at 2 and 8 s; fast
  mode 1, at the stop at 6 s); and a mode on a mode set where its
  crossing left zero followed one step late (d0 = [v > 0], d1 =
  [d0 > 0.5], x' = d1, a car that stands until 2 s and drives off:
  x(4) = 1; OpenModelica gives 2 to 4e-10, fast mode now 2 to round-off).
  A property test drives traces that stand at exactly 0, start, stop and
  start again at random, and checks the time a mode on the speed holds,
  the same for a mode on that mode, and a strict counter of the starts
  against their exact values, with samples added on the lines too
  (`lsim-fast/tests/input_events.rs`).
* **Verified against forward runs and against FASTSim** (`engine/judge/
  fast.py`, 20 October 2026): where no flag is raised, fast mode's charge
  used must be within 1 % of the full dynamic run's (rtol 1e-6) on the
  same cycle (the band, fixed before scoring), and the cycle changed at
  round-off level (every speed 1e-9 of itself up or down by turns) must
  give the same flags and a charge used within a millionth of it. Inside,
  all 61: the two battery-electric example cars' 9 drive-cycle cases
  (−0.20 … −0.04 %) and the same two cars on each of the 27 bundled
  cycles but the long-haul (52: −0.76 … +0.02 %, New York's city cycle
  the largest, 2.1 to 4.1 significant digits); on the speed the full
  dynamic run drove, every 0.1 s, which takes the driver's tracking out,
  −0.05 … +0.07 %; perturbed, at most 1.5e-7 apart, no flag either way.
  The long-haul cycle (805 km) empties both batteries: fast mode flags
  them from the second they reach their minimum SOC, and the full
  dynamic runs stop about 200 s later, where the battery is empty
  (14 881 s on the aerodynamic BEV, 14 541 s on the BEV), naming the
  E-Motor's equation that cuts its torque to what its supply gives (the
  forward run's, outside fast mode; before the solver work's state
  selection round, on a bare SUNDIALS flag −4). On EPA's four reference cars (UDDS,
  HWFET), fast mode's energy at the wall is within 0.02 … 0.17 % of the
  full dynamic run's and within 1.3 % of FASTSim 3.1.0's given the same
  cars (mean 0.92 %; today's engine is within 1.3 % too,
  `benchmarks/fastsim`); before the input events, 25 … 47 % short of
  both. Before the fixes below (the same judge): of the 54 standard
  cycles, 9 stopped on a singular Jacobian that named nothing (7 of them
  where the full dynamic run finishes), 2 raised flags the car never
  reached, and New York's city cycle was 1.43 % over (1.80 % on the
  driven speed), outside the band without a flag; on the driven speed
  the 9 example cases were −0.02 … +0.40 %, the inverter's no-load loss
  charged while standing. Fast mode takes 10 … 260 ms a cycle on these
  cars on a shared, loaded machine (the 805 km cycle 2.0 … 2.8 s; their
  full dynamic runs 0.06 … 7.0 s), and 1 or 2 solves a run are tried
  again (up to 2 472 before). Counted by callgrind, a fast run of the
  aero BEV's UDDS takes 7 % fewer instructions than before (fewer solves
  tried again), the test car's 2.2 % more for the same work per step
  (12.88 residuals, 11.50 Newton iterations: the checks of every solve
  against its reference point).
* **The example projects' cars** (`lsim-project`, `ImportOptions::inverse`):
  a drive-cycle case is imported as its inverse model. The vehicle body's
  speed is prescribed from the cycle its Driver follows (the Driving
  Task's profile, scaled as it scales it; `ImportReport::inverse` gives
  the trace and the `InverseSpec`), and the parts whose forward models
  cannot be run backwards take their inverse variants (lsim-lib, the
  `prescribed`/`inverse` switch of their configuration; the forward
  import is unchanged, so are the golden figures): the **Driver** keeps
  only its pedal map, its pedal command an input the inverse model solves
  for (`limit`ed to full brake … full throttle, so a cycle the car cannot
  follow is flagged), its PI controller and the command's source removed,
  and beyond full brake its brakes keep following the pedal (the friction
  brakes past their capacity, recuperation where there are none);
  the **vehicle body**'s rolling resistance acts against the prescribed
  motion, or the acceleration that starts it forwards from rest (no
  stiction: at rest nothing pushes, and a pedal that holds a car at rest
  is not determined by its motion); the **wheels** turn with the
  prescribed speed, their slip the force over slip stiffness × load (at
  most the slip at the tyre's grip: so the torque balance `F·(1 + slip)`
  grows with the force and has one root, not a second one near a slip of
  −1) and its loss the extra torque `F·r·slip` (the same power, to first
  order in the slip, above 0.5 m/s): a slip that follows the cycle's steps
  of acceleration would need an impulse, which no inverse model of a
  compliant tyre avoids, and without the slip's loss fast mode is 2.6 %
  short on the aero BEV's UDDS (its tyres' slip losses); the force is
  flagged beyond the tyre's grip; the **brakes** act against forward
  rotation (a wheel's slip turns it back a little at the last moment of a
  stop, which must not turn a brake into a drive) and follow a command
  beyond 1; the **e-motor**'s torque follows its command everywhere, so
  the command the inverse model solves for is always determined: the
  inverter runs at a command of 0 too (an instant in a forward run, the
  state of a standing car in fast mode) but draws nothing while its shaft
  stands and its command is 0 to round-off (a car standing with nothing
  to drive, whose full dynamic run holds it with the friction brakes, its
  command 0 and the inverter off: charging the loss map's 77 W there put
  fast mode 1.4 % over the full dynamic run on New York's city cycle,
  which stands a third of its time, without a flag; a car stands only
  on whole pieces of a cycle, so the switch falls on a step's ends); the
  command is not held to −1 … 1 (it leaves that range only where the
  pedal is beyond full throttle, which is flagged); and a supply window
  does not cut the torque: beyond what the supply gives the electrical
  power is flagged, and of recuperation beyond
  what it takes the supply takes what it can and the rest is lost in the
  inverter (in a full dynamic run the driver brakes harder and the
  friction brakes take it: the battery's energy is the same, the pedal
  smaller; no bundled cycle reaches it); the **battery**'s state of
  charge is flagged below its minimum SOC and above full (a car that
  drives its battery empty could not have driven on: the 805 km
  long-haul cycle takes both cars' states of charge on down to −1.17 and
  −1.26, what it would take, and each battery is flagged from the second
  it reaches its minimum SOC; the e-motor's flag of its supply window
  sees only what the motor draws, not a standing car's other loads). A
  saturation anywhere the pedal drives would leave the pedal free where
  Newton's method strays into it (the Jacobian singular), and a second
  root lets it settle there: 7 of
  the 54 standard cycles of the two cars stopped on a singular Jacobian,
  named by nothing, and two (the FTP-75, WLTC class 3's extra high
  phase) flagged the pedal and two tyres' grip at points inside a step
  where the pedal stood at 3.7·10¹⁵ and the tyres on their second root;
  the FTP-75's first 1369 s, which differ from the UDDS by 61 samples
  within 8·10⁻⁴ km/h of its lines, raised them, the UDDS not. Preparation
  reads an inverse model's tables when it chooses the states (a battery's
  open-circuit voltage decides its start; without it the start is not
  solved and the spread of values used instead puts the tyres at their
  grip, where no choice of states is regular). Refused, with the reason:
  an acceleration test (`FAST-NO-CYCLE`: its speed is what it measures),
  a profile along distance, repeated or with steps, and a Driver whose
  pedal a sampled block reads (`FAST-SAMPLED-CONTROL`: the hybrid's
  supervisory controller, a Script, acts at its ticks on what it read
  then, so no pedal worked out back from the motion moves the car now).
  The stepper keeps every solve to the root that continues from its
  consistent reference point: a prediction farther from it than the
  iteration variables' size so far (their largest magnitudes at
  consistent points) is dropped for the point itself (its derivatives are
  stale, taken where the equations behaved otherwise: with the inverse
  variants regular but without this, Newton's method went from such
  predictions to the battery's other current, its voltage near 0, where
  the torque does not depend on the pedal, and 6 of the 54 cycles still
  stopped there, the BEV's HWFET at 136 s among them), a solution that
  far is checked by a solve from the point and the root nearer it kept,
  a failed solve is tried again from the point with fresh derivatives,
  a solution off by ten orders of magnitude is no root (a step small
  beside a value of 10¹⁵ passed the relative test), a singular
  `∂g/∂z` names the iteration variables it leaves free (`the equations
  do not determine el_driver.cmd there`) and a solve that does not
  converge the slowest one; an event located at a step's end ends the
  step there (a crossing that reaches exactly 0 at a cycle's stop was
  lost, and a step of no length after it stopped the run).
  `engine/judge/fast.py` is the outside judge of these runs (its README:
  the full dynamic run of each case, the battery-electric cars on every
  bundled cycle, and FASTSim on EPA's reference cars).

## 11. Outputs, energy books and accuracy reports

* **Channels**: every flat variable, named by path (`battery.c1.v`); the
  project importer's channel map gives today's names
  (`el-battery:sig_soc`), so the app and the Python package read results
  as today.
* **Summary rows, duty, limits, references**: the figures from the
  channels (`golden::figures_with`, the app's KPIs keep their keys); the
  verdict, the energy report, part energies, duty, limits, references and
  hand calculations by today's Python code from the run's evidence
  (`backend/app/solver/evidence.py`, section 12.3: no Rust copy of them);
  moved into Rust only where speed requires (WP6).
* **Energy books**: for every primitive, `energyIn − energyOut − losses −
  stored = 0` from its port powers (across × through), its loss expression
  and its stored-energy expression — today's `partEnergy`. A part's books
  close when its declared loss and stored energy agree with its equations,
  which WP5 tests for every library component; between parts, connection
  equations conserve power exactly at every node (equal across, through
  summing to zero). With all of them integrated as quadratures under error
  control, the run checks the global closure Σ(sources) − Σ(losses) −
  ΔΣ(stored) − Σ(boundaries) against the energy throughput and reports it
  (target ≤ 1e-6).

  As built (work package 4): every primitive with physical ports gets ∫
  power in, ∫ |power in|, ∫ loss and ∫ d(stored)/dt; parts that declare
  neither loss nor storage are the boundaries (sources, grounds, lossless
  converters) whose net intake is the energy supplied. The **closure** is
  supplied − lost − ∫ d(stored)/dt, relative to the throughput (half the
  sum of every part's ∫ |power in|): it is round-off when the books are
  right, and the run warns above 1e-6
  and can be made to fail (`energy_tolerance`), naming the parts whose
  books close worst. Jumps of the stored energy at events, from the states
  before and after, are booked as a separate entry (energy lost at
  events). Each stored energy's rate `dE/dt` is exact: forward-mode
  differentiation of the declared stored energy along `(1, y')` through
  the assignments that compute the variables it reads
  (`RunInfo::stored_rates`), with `y'` the model's own `x' = f(t, x, z)`
  for the states on every backend (on IDA its `y'` would differ by the
  residual its Newton iteration leaves) and the integrator's rate for the
  iteration variables. The time's rate is one; a table is the model's own
  interpolant (`ModelFunctions::eval_table`: the books' powers, losses
  and stored energies read the tables the compiled code reads), its
  derivatives exact (a C¹ monotone cubic); along a zero direction a rate
  is zero, even where the function's own derivative is infinite (`sqrt`
  at zero: no `inf · 0`). Only a stored energy that reaches a derivative
  or a previous value through its assignments, or whose rate is not
  finite at a point (`sqrt` at zero while moving), takes a fourth-order
  central difference, its step moving no entry of `y` by more than 1e-3
  of its size or of its nominal scale. (Work package 4's sixth round
  found the books without tables: a declared stored energy or loss that
  read one made them NaN, and the default run, its books under error
  control, stopped at t = 0.) (Work package 4's fourth round found the
  difference's step collapsing whenever an entry of `y` passed zero while
  moving: its round-off grows as `|E| / step`, and a full fuel tank stores
  some 1e9 J, so the hybrid's books closed to only 1.6e-6 on the UDDS and
  5.4e-7 on the HWFET, by an amount that any change of the step sequence
  reshuffled: 10.5, 1.6, −0.8 and 1.9 J on the HWFET at rtol 1, 0.999,
  1.001 and 0.99 × 1e-6. Exact, the same runs close to 4.3e-12, 8.1e-14,
  5.1e-12 and 2.4e-12.) The **drift** — stored energy from the states at
  the end minus the books' — is the integration error of the energies, of
  the order of rtol; it is reported, with a warning when it exceeds
  100·rtol. The integrands are compiled by the model where it can
  (section 5.8, *compiled energy books*), bitwise what the run would
  interpret; what it cannot compile stays interpreted.
* **Solver report** on every run: backend, method, tolerances, steps,
  evaluations, Jacobians, error-test and Newton failures, events (with
  times and parts), restarts, initialisation path, energy closure.
* **One-click accuracy check**: re-run with tolerances 10× tighter
  (`SolverOptions::tighter`) and report each channel's largest difference
  and each KPI's change — an estimate of the global error the user can
  trust. The spike's test checks that tightening reduces the error as
  expected.

## 12. Today's projects and library in the new engine

### 12.1 Project JSON → IR

`lsim_project::import(project, registry)` (walk, ids, units and errors are
in Stage 1; mappings in WP5):

| project JSON | IR |
|---|---|
| part (`componentDefId`, `label`, `id`) | `SubDecl` of the mapped definition; name = id made an identifier (`el-battery` → `el_battery`), `label`, `ui_id` = id |
| `parameterOverrides` (display units) | SI modifiers through the block's mapping (`app_unit`: `1/min` → `rev/min`, `g` → `gn`) |
| `variability: fixed` parameter | structural modifier (part of the cache key) |
| table1d / table2d | table parameters (runtime data) |
| wire (`connections`) | `connect` of the two physical ports |
| signal link (`dataBusConnections`) | `connect` of output to input |
| sub-system (`isSubSystem`) | a nested composite (WP5) |
| implicit links of today's engine (wheels to the vehicle body, engine to its tank, fuel cell to the hydrogen tank, ambient to thermal parts) | explicit connects the importer adds, so the physics is visible |
| case `parameterOverrides` | runtime parameter values (no rebuild), structural ones re-prepare |
| case kind, duration, timeStep, outputEvery, endDistance, laps | run settings: grid, stop conditions as `when` events |
| case mode (new field): `full` / `fast` | which solver runs it |

### 12.2 Component library mapping (WP5)

Each of the 36 blocks becomes a composite of primitives (or one equation
component) with today's port ids mapped; their parameter semantics are
taken from `components.json` and the Python solver, and each is tested
against exact answers from `benchmarks/`.

| today | new composition (sketch) |
|---|---|
| `battery.generic` | OCV(SOC, T) table source + R0(SOC, T) + up to two RC pairs + SOC integrator (∫i dt / capacity) + cell-limit outputs; thermal port |
| `motor.emotor` | ideal machine (torque = demand × available torque at speed and voltage, `limit` on full-load curve) + loss map table → electrical power = mechanical + loss; with its bus's supply window, the torque cut to the share of the command whose power fits, the inverter off where not even the spin losses fit (its torque the drag; the share is then not solved: at an empty battery it would have none, and IDA stopped with flag -4) |
| `engine.combustion` | torque source with full-load and fuel maps (tables), fuel port, idle/on logic as modes |
| `fuelcell.stack`, `fuel.h2_tank`, `fuel.tank` | polarisation table source; tanks as fuel-port reservoirs (mass state) |
| `controller.dcdc` | average-model converter with efficiency, power conserved minus loss |
| `electric.voltage_source`, `electric.constant_drive`, `electric.climate` | constant voltage; power consumers (i = P/v with a guard); climate load model |
| `electric.node`, `mech.node` | composite whose own ports are all connected together (one connection set: equal across, through sums to zero) |
| `boundary.ground` | ground with three pins |
| `mech.shaft`, `mech.final_drive`, `mech.gearbox`, `mech.differential`, `mech.transfer_case` | inertia, ideal gear with efficiency on the torque the teeth carry, after the inertias before them (loss in the power direction, blended within `p_dir` of no power), selected ratio (gearbox: discrete gear from the signal, shift as a rigid engagement), open differential (torque split, speed sum) |
| `mech.clutch` | stick/slip friction with engagement signal: modes and events, no chattering |
| `mech.brake` | friction torque against rotation, holding at standstill (stick mode) |
| `propulsion.wheel` | rolling radius, slip-based tyre force (today's model), rolling resistance; translational port to the body |
| `propulsion.propeller` | torque ∝ ω² load |
| `vehicle.body` | mass, road load (Cd·A or A/B/C), grade, load transfer (exact, no longer one step behind), downforce; translational port |
| `driver.driver` | continuous PI speed controller with `LimPID`'s back-calculation anti-windup (no mode), accelerator/brake split, recuperation blending |
| `signal.driving_task`, `signal.road_profile`, `signal.lookup`, `signal.constant`, `control.pid`, `control.traction` | causal signal blocks (tables as monotone cubic or linear as today) |
| `signal.script`, `signal.fmu` | sampled `DiscreteBlock`s, section 9 |
| `signal.monitor`, `container.system` | no physics: channel selection; sub-system composite |
| `boundary.ambient` | ambient temperature source on heat ports; air density parameter |
| `track.lap` | stays in today's lap-mode solver until a later stage (quasi-steady-state, not a time simulation) |

### 12.3 Keeping results comparable

* **Same names**: the channel map; summary keys computed by today's code.
* **Golden comparisons** (WP5): every example project and case runs on both
  engines. Today's engine integrates with semi-implicit Euler at ≤ 10 ms
  and uses some one-step-behind couplings (load transfer, source limits);
  the new engine solves those exactly. So each KPI's acceptance band is
  measured, not guessed: run today's engine at its normal step and at a
  10× smaller step; the band is the larger of 2× that difference and a
  floor (0.1 % for energies, 0.5 % for times); the new engine must fall
  within the band of today's fine-step result. Each difference outside is
  investigated and classed: the new engine wrong (fixed), today's engine
  wrong (shown by an exact answer, a fully resolved model, a converged
  reference or a conservation law, and documented; today's engine is not
  changed for it), or the band wrong for that channel (the comparison
  changed only for a documented, sound reason, never widened to pass).
  The October 2026 triage found every remaining difference in the second
  class, with one cause: today's engine applies a gear's efficiency to,
  and reports as the gear's, the torque of the sources above it, the
  inertias between them not taken out; the library's gears lose their
  efficiency on, and report, the torque their teeth carry, after the
  inertias before the mesh (`lsim-project/golden/README.md`).
* **Same result schema**: the Python layer converts `SimResult` into
  today's whole `SimResult` JSON, so the app needs no change to show new
  runs (step 2's evidence part, `backend/app/solver/new_engine.py`): the
  channels with min/max/mean and the summary figures, and from the run's
  evidence (`evidence.Evidence`, the one object today's engine also fills
  from its `RunContext`) today's own code makes the rest: the run verdict
  (`verdict.judge` on the Driver's trace from the dense output, every
  0.1 s whatever the output step, 0.01 s in an acceleration test; the
  battery checks, the averaged peak from the run's window
  (`SimResult::windows`) and the row showing the number its flag is
  judged on, an enforced cap the engine holds to its tolerance (within
  100 times the run's rtol) at the cap, not over it; each E-Motor's and
  Engine's time above its maximum speed, by more than 1e-6 of it, on the
  line through the trace, and its highest speed from the integrals; the
  rows a failed check makes `notValid`; the status;
  a value that is not a number judged before the result shows it as a
  gap), the part energies and the energy report (each part's losses and
  stored energy's change from the engine's books, `ImportReport::instances`
  mapping its path to the element; what went in and out each way from an
  E-Motor's channels' integrals and a mechanical part's ports' powers on
  the trace, what came out being what went in less what it lost and
  stored, so each part's books close exactly; the Vehicle's named terms
  from its road-load channels), the duty (mean and RMS from the run's
  integrals, extremes from the output intervals'), what held each
  driveline back (today's rule, `reports.lane_state`, on the trace, a
  change at the last event of the run inside the 0.1 s before it), the
  references and hand calculations, a table read past its data
  (`SolverReport::table_outside`, today's summary rows, note and verdict,
  for a table today's engine reads as a part's data; a record of another,
  a Driving Task's schedule, is a note with the code
  `NEW-TABLE-NOT-JUDGED`, not judged). Today's warnings that a supply
  held the car back (a battery at its maximum-power point, full or at its
  Max Charge Power; a fuel cell at its maximum power or taking nothing
  back; a bus with no source; a consumer cut back) come from the trace
  of each motor's time held and recuperation lost, each consumer's share
  served and each battery's window, each once in today's words (a fuel
  cell whose Hydrogen Tank the run drains holds the motors back at its
  maximum power in today's words, as today's, which holds it to the
  hydrogen left; one empty from the start says the tank is empty); a
  battery reaching its minimum SOC is said at the run's event; a Script
  output with no port is today's warning too. A run that reads a value
  outside its parameter's limits (a study's point: Data Checks check only
  the study's base project) is not run (`NEW-LIMITS`, in Data Checks'
  words): today's engine reads many such values clamped, the new engine
  as typed. Today's checks of the values a run uses are today's code's
  own words: what its `RunContext` says while it is built (a CG height
  with every wheel on one axle, wheels off the road at the start, a
  driveline over-constrained), which the path's never-stepped
  `RunContext` says too, and what a step of today's run says once it
  sees it, which the path asks of the run (the Ambient's air outside the
  usual, at the run's start, with the air density the run's Vehicle
  used; wheels lifting off the road, only with a CG height or a Downforce
  Area as today's checks it, an axle's load at 0 in the integrals, which
  and when from the trace at the Vehicle's own axle-load event, from the
  trace's second point on, or else at such an event after the first
  traced stretch, because the first point and the integrals' lowest value
  hold the state before the events of t = 0, in which a Vehicle at rest
  moves a few N off a light axle for no time; an
  E-Motor's supply at 1 V or less at any point the integrator visited).
  The check that the car has
  driven by 60 s (`verdict.not_driven`, which today's engine asks at the
  end of each output step) is asked at every traced point, so the path's
  verdict does not depend on the output step; an acceleration test with
  no Vehicle is judged as standing still, as today's trace of it has it.
  Today's warnings about its
  own fixed-step solver (a part too stiff for its smallest step, a
  last-resort clamp on a bus) are not the new engine's and are not given;
  the engine's books' own closure and drift are. A judged battery check
  the run kept no record of (its power or voltage, or its power's moving
  average) fails the path with `NEW-ERROR`, never passes on no number.
  The harness counts the notes' codes by code (its `[note_codes]`).
  The engine's own warnings on its run reach the user; its preparation's
  notes about the start values (`START-ALIAS-CONFLICT`, `START-NOT-SOLVED`,
  `INIT-START-ASSUMED`, `INIT-UNDECIDED`, `INIT-START-IGNORED`: review
  finding 36) are notes, never the run's warnings. Not the same: the
  engine books each inertia with its part (today's books have a
  driveline's spinning parts as one *Rotating parts* row), a battery's
  duty *Losses* are all its internal losses (today's: R0's alone), and a
  wheel's or a gear's flows each way come from a 0.1 s trace.
* **Live runs, a study's Stop, live edits** (step 2's item 4; review
  finding 19). The app's every run is live (it streams), and a study's
  point and an assistant's run have a control: on the path such a run
  runs the engine in a thread of its own with one `RunControl` (section
  13), the one stop and progress hook, while the path's thread passes the
  output points on as today's `step` messages, the result's values bit for
  bit (the channels the result keeps, a value that is not a number a gap,
  as there), at the case's real-time pace (the engine waits on the
  control's queue, 2 points then, 256 otherwise), and reads the run's
  control every 20 ms: a cancel stops the run where it is, judged as
  today's judges a stopped run (*cancelled*, today's words, its figures
  per distance not valid at the time it stopped); a `set_param` is a live
  edit. A parameter the library marks structural takes effect on the next
  run, as today's says; for any other the project with the edits so far is
  built again (a build, no run: 15 ms for the BEV car) and its parameters
  compared with the run's: those that moved go to the run, an event at its
  next output time, and a model the engine builds otherwise for the value
  (the vehicle's mass, a gear ratio: a structure of their own), or a value
  it refuses, takes effect on the next run too, in the engine's words.
  Those notes go to the live run's log, not into the result, which says
  what the run did: the result names its engine, its version and its
  solver options (`engine`, `engineVersion`, `solverOptions`; today's
  engine fills them too) and lists the live edits it made, each at the
  time the engine took it (`liveEdits`); the app's RunSnapshot keeps them
  (`RunSnapshot.of`, the app's snapshot of a run), and `app.solver.replay`
  makes the run again from the snapshot on the engine that ran it, with
  its options and its live edits, the new engine taking each where the run
  took it: the same result, bit for bit (the harness's STREAM S3). Today's
  engine applies a live edit at the output step at which it polled it,
  which nothing records: a replay of its run with live edits fails,
  saying so. A replay runs to the case's end: a stopped run's is the
  whole run.
* **The app's path, both engines** (step 2's judge,
  `python -m benchmarks.back_to_back`): every example case and eight
  probes run through the app's own `simulate`, on today's engine at 10 ms
  and 1 ms and on the new engine's path in strict mode (nothing falls back);
  every field of the two results is compared against the bands above (a
  channel over every point of today's 1 ms run, its first point a figure of
  its own; one only the new engine records against the port the data bus
  wires to it, where today records that port), over the union of what the
  three runs give, with a ledger per case that fails it where anything is
  left out or taken twice (what today's 10 ms run alone flags, a warning, a
  run not valid or a check not passed, is a difference too), and
  each difference classed as above (`benchmarks/back_to_back/classes.toml`,
  each rule with its date, reason and check), beside the planted-fault
  verdict matrix, the result schema, speed traces with exact answers from
  the regulations' text, streaming, stop and replay. What it judges and
  the pass marks were registered before its first run
  (`benchmarks/back_to_back/acceptance.toml`); where the path stands,
  measured before the wiring and after its evidence part, is
  `benchmarks/back_to_back/baseline/README.md`.

## 13. Python API

The module `lightsim_engine` (lsim-py, built by maturin into an abi3 wheel
for Python 3.11 and later) is what the app's backend and the `lightsim`
package call. What exists now is below; `engine/crates/lsim-py/README.md`
says how to build and test it, and `tests/test_api.py` is its contract
test. The Rust of the conversions is in `lsim-py/src/lib.rs`, the part that
knows nothing of Python (import, build, run, the golden path) in
`lsim-py/src/core.rs`.

```python
import lightsim_engine as le

model = le.build(project_json, case_id, cache_dir=None)   # import + prepare (cached) + compile
model.report              # sizes, prepare/compile seconds, cache hit, structure key, warnings, case
model.param_names()       # the engine's parameter names
model.params()            # current values, SI
model.set_params({"el_vehicle.cd": 0.30})                 # runtime, no rebuild
run = model.simulate(rtol=1e-6, atol=1e-8, output_step=None, t_end=None, scripts=None,
                     native_scripts=True, trace=None, trace_step=0.1, windows=None,
                     control=None, edits=None)
run.times                 # NumPy float64
run.channels["el-battery:sig_soc"]   # today's names and display units, NumPy arrays
run.engine_channels       # the engine's own names, SI
run.min, run.max, run.mean           # like channels, per output interval
run.events, run.report, run.energy   # energy: the books, or None
run.figures               # today's summary figures {key, label, value, unit}
run.integrals, run.trace, run.windows, run.table_outside   # the evidence for the app's verdict and reports
control = le.RunControl(queue=0)    # stop, points as they are recorded, live changes
control.cancel(); control.points(timeout=0.1); control.set_params({...}); control.t
runs = model.sweep([{...}, {...}], threads=4)   # Runs, or the exception for a failed set
le.diagnose(project_json, case_id)    # structural and unit checks only
le.ModelError             # .diagnostics: [{severity, code, message, parts, hint, detail}]
```

* **Same run as the golden comparison.** `build` imports the case the way
  `lsim_project::golden::run_case` does and builds it through
  `lsim_engine::Engine`; `simulate` uses the case's output grid, the same
  solver options (SUNDIALS IDA, rtol 1e-6, atol 1e-8, energy books on) and
  runs an acceleration test again to the moment it crosses the finish. The
  channels, in today's names and units, and the figures
  (`golden::figures_with`) are bit for bit those of `run_case`; two tests
  prove it (the BEV car's WLTC case, and the hybrid car's mixed cycle).
* **GIL.** `build`, `simulate` and `sweep` release it while the engine
  works; a Script function takes it back for the length of one call. A sweep
  runs its sets on up to `threads` Rust threads over the one compiled model.
* **Parameters** are engine names with SI values. `set_params` is all or
  none; a name that is not a parameter, a value that is not a finite number,
  a structural parameter (build again), a value that breaks what
  preparation divided by, or one at whose start the states index
  reduction chose do not work raises a plain `ValueError` with the
  engine's own message. A sweep's sets sit on top of what `set_params` set.
* **Sampled blocks.** The Traction Control block runs today's rule in Rust.
  A Script part runs the Python function the caller gives for it, whenever
  one is given (`scripts={part id or label: f}`,
  `f(t, dt, inputs) -> dict | None`), called at the start and then every
  period, outputs it leaves out holding their value: a replacement
  controller is never set aside for the project's code, and a key that is
  neither the id nor the label of a Script part of the case is a
  `ValueError` naming it and the parts (in `simulate` and `sweep`), as
  are two keys naming one part (its id and its label). A part given none runs its own code on `lsim-script`'s
  native runner (§9: the app's results to the bit, nothing outside the
  script reached) with `native_scripts=True`, the default, which is the
  caller's consent to it; when that code is outside the subset, read or
  met part-way, the call raises a `ModelError` coded `RUN-SCRIPT` that
  names the part, its hint to give the part a function. With
  `native_scripts=False` a part without a function is a `ValueError`
  naming it. `run.report["scripts"]` says which ran each part
  (`"python"` or `"native"`). `script_host` in `lib.rs` is the single
  place a Script block's host is chosen. FMU parts raise a `ModelError`
  (not supported yet).
* **The run's evidence** (step 2's evidence part): `simulate(...,
  trace=[names], trace_step=0.1)` samples channels (today's `element:port`,
  in their display units) or engine variables (SI) from the dense output
  every `trace_step` s and at the run's end, without changing anything
  else (`run.trace`: NumPy arrays, `times` among them); `run.integrals`
  gives each channel (display units) and each engine variable (SI) over
  the run: `integral` (∫x dt), `square` (∫x² dt), `magnitude` (∫|x| dt,
  NaN for a unit with an offset), `max` and `min`; `simulate(...,
  windows={name: seconds})` keeps each name's highest moving average over
  its span of time (`run.windows`: `window`, `peak` in the name's unit, and
  `t`, when the span ended; 0 s: the highest value); `run.table_outside`
  each table axis read past its data (table, part, axis, first, total,
  t_furthest, and in the unit the engine reads the axis in, SI but for the
  climate demand's outside temperature in °C, furthest, edge, lo, hi, with
  its warning's words);
  `run.report["asserts"]` each warning assert that did not hold (`t`,
  `message`); `model.report["instances"]` each element's instance name
  (the energy books' parts' paths); `model.var_names()` the engine's
  variables and today's channels. What the app reads of them is section
  12.3's.
* **A run's control** (step 2, item 4: one stop and progress callback for
  a study's Stop, a run's progress and a live run). `simulate(control=c)`
  runs with `c`, a `RunControl` (one run each), which other Python threads
  use while the run goes: `c.cancel()` stops it where it is
  (`run.report["stopped"]`, section 8.4); `c.t` and `c.t_end` are its
  progress; `c.points(timeout)` gives the output points recorded since the
  last call, each `(t, values)` in `c.channels`' order (today's channels,
  display units), the result's values bit for bit, through a queue of at
  most `queue` points that the run waits on while it is full (the reader
  paces the run so; `queue=0`, the default, keeps none, for a stop
  alone); `c.set_params({...})` changes parameters from the run's next
  output time on, checked as `set_params` checks them on top of the
  changes before (a value bound to another may be given only as its
  binding makes it). The control's state is atomics, a mutex-guarded
  queue and condition variables in Rust: the run never takes the
  interpreter's lock for it (it goes on while another thread holds the
  lock, a test shows), and `points` waits without the lock. A run told to
  stop, or whose reader called `close()`, waits for no reader. `run.report
  ["edits"]` lists the changes made (`t`, the point the run had reached
  when it took the change; `at`, when it made it; the parameters that
  changed), and `simulate(edits=[(t, {...}), ...])` makes them again: the
  same run, bit for bit. An acceleration test, which runs again to the
  moment it crosses the finish, tells its last run's points only (after
  its first run, when that is the result), its second run making the
  first's changes before the finish at their times and refusing others (a
  change there would move the finish it runs to): `run.report["edits"]`
  then lists the first run's.
* **Output grid.** A grid (from `t_end` and `output_step`, the caller's or
  the case's own) of more than 10⁷ points, or whose results would take
  more than 4 GB (each point holds every recorded variable's value, min,
  max and mean), is a `ValueError` before the run. Below it the grid, the
  recorder and the energy books reserve their room with `try_reserve` and
  fail with `RUN-MEMORY` instead of aborting, and the copies the `Run`
  makes for Python (times, channels, min, max, mean, engine channels, the
  books' series) ask for their room first and raise `MemoryError`; an
  acceleration test's first run lets go of its results before it runs
  again to the finish.
* **The app.** With `LIGHTSIM_ENGINE=new` the backend
  (`backend/app/solver/new_engine.py`) runs drive-cycle and acceleration
  cases on this module, live ones too (section 12.3: streamed, stopped,
  edited live), and everything else on today's engine (FMU parts,
  Formula Student events, hybrid charge-balance cases,
  batteries built from cells and a study's point holding a value outside
  its parameter's limits, which Data Checks refuse for a single run and
  today's engine reads clamped, too); any failure of the new engine runs the
  case on today's engine with a warning. Every fall-back carries a reason
  code (`NEW-LAP`, `NEW-FAILED` with the engine's own codes, ...:
  `new_engine.CODES`), and with `LIGHTSIM_ENGINE_STRICT=1` nothing falls
  back: the case fails with its code (the both-engines diff's strict mode,
  `benchmarks/back_to_back`). The result is today's whole result, made
  by today's own code from the run's evidence (section 12.3). The
  default is today's engine. There the Script blocks run in today's
  locked-down worker (each part is given a function that calls it, and
  `native_scripts=False`) until the owner decides the native runner may
  replace it.
* **Errors.** A project that cannot be imported or prepared, or a run that
  fails, raises `ModelError` (a `ValueError`) with the diagnostics (code,
  message, parts, hint, detail) so the app can highlight the parts; a
  failed run's code says what failed (`RUN-INTEGRATOR`, `RUN-INIT`,
  `RUN-EVENT-STORM`, `RUN-STATE-SELECT` with the constraint's parts,
  `RUN-ENERGY`, `RUN-TABLE`, `RUN-ASSERT`, `RUN-BLOCK`, `RUN-SCRIPT`,
  `RUN-MEMORY`).
  A panic inside the engine becomes a `ModelError` coded `ENGINE-FAULT`;
  bad arguments are `ValueError` or `TypeError`.
* **Not yet:** fast mode for project files (no `mode` argument, so nothing
  silently runs something else), FMU parts, the one-click tighter run as
  its own call (call `simulate` again with a smaller `rtol`).

## 14. Error messages

Every diagnostic has a stable **code**, a **message naming the parts by
their labels**, the **parts** (paths, which carry the app's element ids, so
the canvas highlights them), a **hint** and the technical **detail**
(equations) for a "details" fold. The origins recorded at flattening make
this possible at every stage:

| stage | example (from the Stage 1 tests) |
|---|---|
| flattening | `[UNKNOWN-NAME] In 'Heater' (Bad), equation 5 uses 'Rx', which is not one of its variables, ports or parameters.` |
| units | `[UNIT-MISMATCH] In 'Heater' (Electrical.BadResistor), the equation “v = R / i” does not balance its units: the left side is in V, the right side in m2.kg.s-3.A-3.` |
| structure (over) | `[STRUCT-OVER] 'Bench supply' and 'Charger' set the same quantity more than once: 2 equations ('Bench supply' (the source holds its voltage); 'Charger' (the source holds its voltage)) for 1 unknown (p.v of 'Bench supply'). Two ideal voltage sources connected in parallel fight over one voltage: put a resistance between them, or remove one.` |
| structure (under) | `[STRUCT-UNDER] Nothing determines p.i of 'Bench supply': 1 unknown with no equation between them, in 'Bench supply'. Look for a part that is not connected, a circuit with no Ground, or a shaft with nothing to drive or hold it.` |

Method: a structurally singular system is split by the Dulmage–Mendelsohn
decomposition (alternating paths from unmatched equations: the
over-determined part; from unmatched unknowns: the under-determined part);
each part's equations and unknowns map to instances through their origins.
WP2 grows a **catalogue of recognised patterns** with tailored hints, each
with a test model: two ideal sources in parallel; a circuit with no ground;
a floating thermal network; two speed sources on one rigid shaft; a
gearbox with no ratio input; a signal input left open; an algebraic loop
through a controller with no feed-through break; a part not connected at
all. The catalogue as built (each code's model and expected parts are in
`lsim-prep/tests/faults.rs`, one test per fault): `ELEC-SOURCE-LOOP`,
`ELEC-CURRENT-SOURCES`, `ELEC-NO-GROUND`, `THERM-FLOATING`,
`THERM-TEMP-CONFLICT`, `MECH-SPEED-CONFLICT`, `MECH-FLOATING`,
`PART-UNCONNECTED`, `GEAR-NO-RATIO`, `SIGNAL-UNCONNECTED`, `SIGNAL-SOURCES`,
`CAUSAL-LOOP` (a warning), `SINGULAR-LOOP`, `INIT-OVER`, `DER-NOT-STATE`,
`INDEX-TABLE-NOT-SMOOTH`, `STATE-SELECT-SINGULAR`, `STATE-SELECT-ALWAYS`,
`STATE-SELECT-NEVER`, `STATE-SELECT-PREFER` and `STATE-SELECT-CONFLICT`
(warnings),
`PIVOT-ZERO-AT-START` (a warning), `PARAM-CYCLE`, `EXTERNAL-PERIOD`, `EXTERNAL-LOOP` (a warning),
`WHEN-CONDITION`, `WHEN-CONTINUOUS`, `REINIT-NOT-STATE`, `STRUCT-OVER` and
`STRUCT-UNDER`; `INIT-START-IGNORED` is tested with the index-reduction
models, `INIT-UNDECIDED` (a warning) with the pendulum released exactly
level (`lsim-solve/tests/state_choice.rs`), the `INVERSE-*` codes with fast mode's, and
`INDEX-TABLE-NOT-SMOOTH`'s three reasons (a linear table's kinks, a held
table's edges, a cubic's second derivative) in
`lsim-prep/tests/index_tables.rs`. `INDEX-DIFFERENTIATE` (an equation that
cannot be differentiated) and `INDEX-TOO-HIGH` stay as safety nets: since
tables differentiate, no known model reaches them. A floating network is
told apart two ways: structurally (a node balance that alias elimination
reduces to 0 = 0 is dropped, leaving the potentials undecided) and, when
the structure balances, numerically (a linear block singular at the start
whose null direction moves only potentials, temperatures or speeds, all
together). Run-time failures get the same treatment: a Newton failure names the
block's parts and the equations with the worst residuals; an integrator
failure names the variables with the largest error-test weights; repeated
events name the modes that chatter.

## 15. Testing strategy

* **Unit tests** in every crate (Stage 1: units, interpreter, matching,
  BLT, symbolic solving and differentiation, alias elimination,
  flattening, diagnostics, recorder, printer, importer).
* **Generated code against the interpreter**: every operator and built-in
  compared with `lsim_ir::eval` at several points, and every
  Jacobian-vector product with central differences
  (`lsim-codegen/tests/codegen.rs`); WP3 adds 10 000 random expressions
  against the interpreter (`fuzz.rs`), every Jacobian and its products
  against Richardson-extrapolated differences on random smooth models,
  the example projects (also with every block implicit) and their
  initialisation systems (`jacobian.rs`), the tables' C¹ continuity and
  monotonicity (`tables.rs`), the `when` semantics (`when.rs`), tapes
  against machine code (`tape.rs`), the calling conventions
  (`conventions.rs`) and the compile budgets (`budget.rs`).
* **The mathematical library** (section 7.1): every function correctly
  rounded on 4 672 references (hard-to-round cases, special values, the
  inputs where glibc errs; MPFR and mpmath agree on each), in both builds
  and through the Rust and C entry points (`lsim-math/tests`); every
  evaluation path, from the interpreter to the kernels' enclosures,
  bitwise the library on them (`lsim-codegen/tests/library.rs`);
  SUNDIALS' `pow` correctly rounded where glibc's is not and its sources
  calling no inexact function of the C runtime
  (`lsim-sundials-sys/tests/maths.rs`); no engine code calling the
  platform's maths (clippy's `disallowed-methods`,
  `lsim-math/tests/sources.rs`); the two builds bit for bit the same on
  6.4 million random inputs (`builds.rs`); the vendored files the
  recorded ones, patched (`vendored.rs`); the self-check at the first
  call failing for any one wrong function (`src/lib.rs`).
* **Exact-answer suite**: the reference suite being built in `benchmarks/`
  gives each block's exact answers; `lsim-solve/tests/reference.rs` (and
  `lsim-project/tests/exact.rs` for the library's blocks) runs every case on both backends (SUNDIALS and diffsol) and asserts each
  within its stated tolerance and the two backends within tolerance of
  each other. Event times are checked separately.
* **Convergence tests**: every exact case at three tolerances; the error
  must fall with the tolerance (the spike's `tighter_tolerance_shrinks_the_error`).
* **Energy closure** on every test run and every example car.
* **Golden comparisons** with today's engine (section 12.3), in CI on the
  example projects; a report of every KPI with its band.
* **The outside judge** (`engine/judge`, October 2026): published
  problems with their own reference solutions, so the solver is not
  judged by its own probes. The Bari IVP Test Set's Andrews mechanism
  (index 3), car axis (index 3) and Akzo Nobel problem against the
  test set's reference solutions, scored as the test set scores its
  solvers (significant correct digits, scd); the IFToMM planar pendulum
  and multiple four-bar (5 × 5 loops through singular positions) against
  the pendulum's closed form (Jacobi elliptic functions, mpmath) and the
  grid's minimal coordinates integrated at 40 digits, with the
  benchmarks' own energy-drift limits; the same pendulum released from
  any angle, or looping; the Reference FMUs' Stair (no state) and
  BouncingBall against their `_out.csv` and the exact bounces; SUNDIALS'
  own IDAS examples with quadratures under error control, built against
  the engine's patched IDAS and upstream's (section 8.1); and
  OpenModelica 1.27 on the same models, with its dynamic state selection
  and with static dummy derivatives (for checking only, never shipped).
  `judge.py` downloads the sources into a cache checked by SHA-256 (the
  test set and IFToMM are free for research only: none of their data is
  in the repository), writes the models in the text format, runs them at
  rtol 1e-4 … 1e-10 through `lsim-project`'s `judge` example and scores
  error, scd, steps, error-test failures, events and wall time, beside an
  earlier build's (`--before`). First verdict (16 October 2026, the
  solver work's first round): Stair exact at every tolerance (it failed
  at its start before); BouncingBall's 11 bounces to 5e-11 s at 1e-10,
  the error falling with the tolerance; Andrews 3.6 to 9.0 scd over the
  range, more than the test set's five solvers at the same tolerance (6.6
  at 1e-7 against their 3.5–5.4) in 8 to 20 times their steps; the car
  axis 1.6 to 5.8, Akzo Nobel 1.9 to 7.4; the pendulum released at
  0.4 rad to 2.5e-9 m at 1e-10. Its failures are static state
  selection's (risk R3). A run of a case with a closed form is right
  when its error is within 100 rtol of the reference over the whole run
  (1e-8 at rtol 1e-10, as the brief asks of dynamic state selection; 1
  cm at 1e-4), a criterion `judge.py` fixes before it scores (the first
  verdict's "within 1000 × rtol" was chosen after the runs, and the
  judge did not apply it). Released at every 0.01 rad from 0.01 to 3.14
  rad (`judge.py cases --sweep`), the index-3 pendulum is right at every
  tolerance only up to 0.71 rad, and its error grows with the angle: at
  most 17 to 69 rtol up to 0.70 rad, up to 266 rtol from 0.71 to 1.00
  rad and 360 to 1.30 rad, 786 at 1.31 … 1.40 and 868 at 1.45 rad
  (rtol 1e-10). Up to 1.47 rad no run stops and 103 of 1029 are wrong
  without a message: 35 of the 147 angles at rtol 1e-10 (8.7e-8 m at
  1.45 rad), 1.1e-2 m at 1.40 rad and rtol 1e-4. How much of that is the
  problem's own sensitivity (its period grows with the angle) and how
  much the conditioning of the states chosen at the start, the work on
  state selection (C) is to separate, beside OpenModelica's dynamic
  selection at the same angles; it is not to take the range up to 1.47
  rad as right. From 1.48 rad on (the IFToMM problem, released level
  with its pivot, among them), and looping, no run is right: at rtol
  1e-4 … 1e-6, 326 of the 501 runs end wrong without a message (the
  position 0.3 to 2 m off), the rest stop; at 1e-7 … 1e-10, 665 of 668
  stop, at the bottom of the swing or where the rod passes the
  horizontal. The four-bar does not prepare. The second verdict (work
  package 4's twelfth round: conditions on iteration variables held on
  their side, section 8.2; the judge scoring by time and by its fixed
  criterion): a `when` on a variable solved from an implicit equation fires
  once per crossing (it fired twice or three times in 8 of the 28 runs
  of the four `count-implicit` cases; OpenModelica once in all), and a
  condition on a state alone is still missed where one step holds both
  its crossings (`count-explicit-pulse` at four tolerances of seven,
  `count-oscillator-peaks` at six; section 8.2, a known limit; on the
  oscillator OpenModelica misses at five); every other case is the
  same, steps, error and events, as before the round.
  The third verdict (20 October 2026, the solver work's item D: the
  library Driver's anti-windup, section 12.2): the Modelica Standard
  Library's `Blocks.Examples.PID_Controller` with the Driver as its
  `LimPID` (`msl-pid-controller`), against the library's reference
  result, was 1.6 off (relative to each signal's largest value) with the
  frozen integral at every tolerance, and is 1.2e-4 off with
  back-calculation at rtol 1e-7 … 1e-10, as OpenModelica's run of the
  original example is (the reference's own accuracy, Dymola at 1e-6);
  beside that run the engine's differs by 1.6e-7 at 1e-10 and the
  instants the command reaches and leaves its limit by 2.9e-10 s, both
  falling with the tolerance. The Battery Electric Car at 1 to 25 t on
  the 27 bundled cycles and its own city cycle (`driver.py`, 701 runs,
  each driven trace judged by 40 CFR 1066.425(b)): before, 504 runs
  stopped as event storms of the frozen integral and 5 with a bare
  SUNDIALS flag, 191 were inside the rule; after, 588 are inside, 75
  outside only where the car was too fast with its brakes fully applied
  (on 11 of the 28 columns: from 10 t on the LA92, 15 t on the US06, 18
  to 23 t on the other nine), 37 inside only by the rule's allowance for
  a car at its maximum available power, standing at its empty battery on
  the two long-haul cycles (they stopped with a bare flag -4 until the
  E-Motor's fix, section 12.2), and one stops with an event storm the
  run loop names (the battery's minimum-SOC mode, long-haul at 11 t).
  None is outside where the Driver could have helped. On the heavy cars
  today's engine differs from the new one by up to 2 % in consumption,
  and by 0.13 % or less once its frozen integral is replaced by
  back-calculation at its 1 ms step (`driver.py today
  --back-calculation`, a check only): the difference is the
  anti-windup (STATUS.md).
  The fourth verdict
  (the solver work's state selection, detection: the choice watched,
  section 6 step 5, and `stateSelect`; new cases: each pendulum also in
  its angle, an ODE with no choice of states, as the control; the
  pendulum with x marked `prefer`; MSL's PlanarFourbar). Of the
  pendulum's 2 198 runs over the release angles, the 432 that ended
  wrong without a message and the 840 that stopped on a bare SUNDIALS
  flag (175) or with no step passing (665) are now 103 wrong and 1 169
  stopped, each naming the rod and the coordinate it decides, where that
  coordinate first moves a hundred times as much as the state: just
  before the bottom of the swing where the choice decides x (released
  from 1.48 to 1.67 rad), as the rod passes the horizontal where it
  decides y (from 1.68 rad), at the same point to four or five digits at
  every tolerance. The IFToMM pendulum (3 runs 1.0 to
  1.1 m off before, 4 with no step passing) and the four looping ones (6
  wrong, 22 stopped on a flag or with no step passing) stop so at every
  tolerance. The 103 still wrong are runs whose choice holds throughout,
  released at 0.72 to 1.47 rad (the factor at the release, the swing's
  largest, at most 9.9, inside the choice's own window of 10; from 1.48
  rad the runs stop): the x-y form's own larger error, its decided
  coordinate carrying the factor. In its angle the same pendulum is wrong
  in only 48 of its 1 029 runs up to 1.47 rad (its 1 096 wrong runs of
  the 2 198 are nearly all above), and right in 66 of these 103 at the
  same angle and tolerance; per run the x-y form is up to 28.6 times the
  angle form's error (1.47 rad, rtol 1e-6; 15.9 times at 0.72 rad and
  1e-10, at a factor of 0.88), and at 1e-10 from 1.31 rad up to 6.0 times
  (the worst runs of 1.31 … 1.40: 786 against 245 rtol). With x marked
  `prefer`, released at 1.50 and 1.55 rad, the runs that all stopped or
  ended 1 m off go to their end (3 right, 11 beyond 100 rtol: 8.4e-8 and
  2.7e-7 m at 1e-10, 3 and 22 times the angle form's, the second starting
  at a factor of 48; OpenModelica's dynamic selection 9.8e-8 and 6.6e-8).
  PlanarFourbar (revolute1 marked `always`, met; pivoting alone takes the
  same states): 3.1e-9 rad at rtol 1e-10 and 2.8e-5 at 1e-6 against the
  reference in its one degree of freedom; OpenModelica 1.9e-7 and 5.6e-4
  on the same text, MSL's Dymola result 7.1e-4 (1e-6). No run of the
  judge ends on a bare SUNDIALS flag (182 before), and every run of the
  other cases is bitwise as before (3 339 of the 4 557). The fourth
  verdict (the independent check of the detection, fixed: the step after
  an event watched, a marked choice taken only within half the run's
  limit; new cases `pendulum-event-2.00` and `-2.80`, the pendulum with a
  `when` that changes nothing about 2 ms before its states degenerate,
  and `pendulum-prefer-x-1.5607`, x marked `prefer` at atan 99): every
  run of the 4 557 is bitwise as before (none has an event near a
  degenerate choice, nor a mark between 50 and 100). The event cases,
  which passed the point on the wrong branch at one tolerance each and
  stopped only later (at 1.30 s instead of 0.724 s at rtol 1e-4 released
  at 2.8 rad, 72 100 rtol seconds late at 1e-5 at 2.0 rad; at the check's
  horizon of 1.28 s, 2.45e-2 m off without a word), now stop within 10
  rtol seconds of the closed form's point at every tolerance, as without
  the event. At atan 99, where the marked runs ended 297 to 3 664 rtol off
  without a word or stopped at the far turning point (rtol 1e-5), the
  mark is refused and told and every run stops near the bottom, within 6
  rtol seconds of the closed form's point.
  The fifth verdict (the solver work's state selection, dynamic: the
  choice made again, section 6 step 5; a new case, `pendulum-level`, the
  pendulum released exactly level, and each run's switches in the report):
  every one of the judge's 4 592 runs goes to its end (the four-bar, which
  does not prepare, has none). The 1 169 runs of the release-angle sweep
  that stopped (released at 1.48 rad or more) choose their states again 8
  to 19 times each (y and vy near the bottom of each swing, x and vx as
  the rod nears the horizontal) and end: 146 right by the judge's
  criterion, 1 023 more than 100 rtol off. The same pendulum in its angle
  (an ODE, no choice of states) is right in 121 of the same 1 169 runs,
  and per run the x-y form's error is 1.21 times the angle form's
  (geometric mean; median 1.17; 0.80 at rtol 1e-4, 1.85 at 1e-7); per run
  its tail is p75 2.35, p90 4.8, p95 8.2, p99 19.6, at most 42.7 (45 runs
  above 10: the worst 2.63 rad at 1e-6, 2.89 at 1e-7, 2.61 at 1e-8). The
  geometric mean alone would not carry "not the choice"; what does is
  that the runs that never switch (the 1 029 below 1.48 rad) have the same
  tail against the same angle form: geometric mean 1.17, median 1.20, p90
  4.9, p95 7.4, p99 13.7, at most 28.6 (23 above 10; from 1.00 rad 1.56,
  1.61, 5.5, 7.4, 12.0, 28.6), slightly lighter at its far end only (p99
  13.7 against 19.6), and that the carry-over measured at the switches
  themselves is at round-off (the review's: the stored energy's change at
  the switch instants 2e-16 to 9e-13 J at rtol 1e-6 … 1e-10, the
  constraints just after a switch within 4.5e-11): what is left is the
  problem's own sensitivity over its five periods, not the choice
  (released at 3.13 and 3.14 rad, next to its unstable top, both forms are
  0.2 to 2 m off at rtol 1e-4 … 1e-6). The target, every run
  right within 100 rtol, is not met, by either form. Per tolerance, rtol
  1e-4 … 1e-10, the sweep's runs right / wrong / stopped were 146/1/167,
  143/4/167, 137/10/167, 138/9/167, 130/17/167, 120/27/167, 112/35/167 and
  are 176/138/0, 168/146/0, 165/149/0, 163/151/0, 149/165/0, 131/183/0,
  120/194/0 (the angle form's right / wrong: 165/149, 176/138, 192/122,
  160/154, 148/166, 141/173, 120/194); below 1.48 rad every run is bitwise
  as before. The threshold and the point were chosen on this sweep (the
  x-y form's error over the angle form's from 1.11 rad, geometric mean,
  and its runs right of the 2 198): 3 times the factor where the choice
  was last read, at the step's end, 1.27 and 1 072; 3 itself, 1.22 and
  1 092, but it switched the hybrid car's clutch set at its start and
  changed the golden comparison (section 6 step 5); 1.5, 2 and 4: 1.15,
  1.22 and 1.41, 1 025, 1 062 and 1 001; 1.125: 2.02 and 946; the step cut
  back every time to where the factor first passes the threshold, by
  bisection: 1.40 at 2 and 1.64 at 4. At rtol 1e-10, with OpenModelica
  1.27's dynamic selection from the first verdict's runs beside each
  (`omc` was not run again): released at 0.40 rad 2.5e-9 m, as before
  (1.3e-9), at 1.50 rad 3.2e-8 (9.8e-8), at 1.55 rad 6.2e-9 (6.6e-8), the
  IFToMM pendulum 3.6e-8 (8.6e-8), at 3.00 rad 5.2e-7 (1.3e-5), at 3.13
  rad 1.9e-5 (6.4e-4), looping 1.8e-7 … 6.7e-6 (6.4e-7 … 2.0e-5); released
  exactly level, refused at preparation before, 2.6e-8 m. Over the seven
  tolerances the runs released at 1.48 … 1.57 rad, the IFToMM pendulum and
  the level one are 9 to 723 rtol off (84 runs), at 3.00 rad 4 569 to
  23 654, looping 230 to 225 151. The marked cases: x `prefer` at 1.50 and
  1.55 rad keeps its choice and is bitwise as before; at atan 99, the mark
  refused at the start, the run that stopped near the bottom now goes on
  (19 to 939 rtol, 5.3e-8 m at 1e-10). The event cases go on through their
  event and their switches (2.00 rad: 83 to 667 rtol; 2.80 rad: 1 142 to
  9 518, the angle form there 1.8e-7 m at 1e-10 against 9.5e-7). Andrews'
  mechanism, the car axis, Akzo Nobel, PlanarFourbar and every other case:
  bitwise as before (3 360 runs; every run that differs is one that
  stopped, 1 225, or the level one, 7). The IFToMM four-bar does not
  prepare, as before (`SINGULAR-LOOP`: its accelerations and rod forces at
  the start form a linear block the values given leave singular); dynamic
  selection acts only on a run that has started, and the four-bar was not
  looked into further.

  The sixth verdict (the review of the fifth, fixed; section 6 step 5:
  only the set that degraded is chosen again, and a refused switch leaves
  the step as it was): the judge's 4 592 runs are bitwise the fifth
  verdict's but 8 of the sweep, each with one switch refused (a choice
  that would start at a factor of 10 to 27, mixing x with vy or y with vx,
  in a step cut back to its point: the step is no longer cut back for a
  refused switch, the integrator no longer restarted there): released at
  1.68, 1.70 (twice), 1.76 (twice), 1.79, 1.86 and 1.95 rad, 181 → 248,
  91 → 16, 33 → 25, 338 → 215, 281 → 233, 110 → 59, 28 → 237 and 145 → 52
  rtol (geometric mean of the ratios 0.79), so the sweep's runs right at
  rtol 1e-4 are 177 (176), the others' counts as before, and per run the
  x-y form's error over the angle form's is 1.212 (geometric mean; p75
  2.33; median, p90, p95, p99 and the largest as above). Twelve cases are
  new, held out (added
  after the rule was chosen, not used to choose it: `pendulums-…`, several
  pendulums in one model, each rod a set of its own; `double-pendulum-…`,
  one coupled set; each beside its angle form). Before, every run of a
  model with two or more pendulums stopped (35 runs, at 0.21 to 4.05 s,
  naming a rod): the run chose every set again with the window of the
  one that degraded and held every set to the base threshold. Now each
  goes to its end, each rod switching as often as alone, and is right in
  none of the 35 by the judge's criterion (the angle forms in 17): rtol
  1e-4 … 1e-10, the x-y form's worst rod over its angle form's, 128/21 …
  340/31 rtol at 1.50 + 1.60 rad, 104/120 … 333/56 at 1.50 + 1.80,
  322/334 … 265/208 at 1.70 + 2.00 (631/97 at 1e-6, 610 before the
  refused switch's change), 281/93 … 926/30 for the five, 10 567 /
  16 909 … 428 915 / 19 148 for the six (4.3e-5 m at 1e-10, its rod at
  3.05 rad). That is more than a pendulum alone takes against its angle
  form, because each rod's switches restart the integrator for every
  rod, and a pendulum near its top is that sensitive to any restart (the
  one at 3.05 rad alone 30 842 rtol at 1e-10, beside one at 1.50 rad
  89 751, alone with 17 events at those switches' times that change
  nothing it reads 61 348: section 6 step 5). Measured over the 119
  rod-runs (the second review's control, rerun): an x-y rod is 1.92 times
  as far off as the same pendulum alone (geometric mean); the angle forms
  given events that change nothing at the x-y runs' switch instants are
  2.25 times as far off as without them (right in 9 of the 35 runs
  instead of 17), and the x-y rods 1.44 times as far off as those. The double pendulum chooses
  its states again 9 times in 5 s, as before, and is right in 5 of its 7
  runs (the angle form in 7): 10, 53, 649, 70, 100, 71 and 52 rtol, the
  angle form 12 … 11, its error 5.4e-5 m before its first switch (the x-y
  form's own).
* **Performance gates**: not built yet. When they are, they compare build
  time (prepare + compile) and run time for each example car in both modes
  with the base commit (as the backend's performance job does) and with
  the comparable-tools benchmark of section 2; a regression of more than
  10 % fails.
* **Structural-fault corpus**: each pattern of section 14 has a model and
  an expected code and named parts (like today's broken-models corpus).
* **Python tests** (pytest) of the module once WP6 adds the API.
* `./check.sh` runs rustfmt, clippy (warnings are errors), all tests and
  the licence check; CI runs its steps on Windows (the target); Linux is
  the development platform and not in CI.

## 16. Work breakdown

Six packages that separate agents can build in parallel. Each owns crates
and files outright; `lsim-ir` is shared: additions are agreed between the
owners who use them and land as separate small commits; nothing in it is
changed incompatibly without all owners' agreement. Each package develops
against the Stage 1 implementations of the others' crates (which already
work end to end) or against hand-written test doubles of the interfaces.

### WP1 — Language, units and the IR's stewardship

* **Owns**: `crates/lsim-lang`, `crates/lsim-ir` (custodian), docs of the
  text format.
* **Builds**: the parser for the text format (section 5.4) with spans and
  plain-words errors; unit checking at parse time; `structural parameter`,
  enumerations, Boolean parameters, tables (`ParamValue::Table1D/2D`,
  `Expr::Table`) and `assert` end to end in the IR; Base Modelica import
  (the flat subset MCP-0031 defines: classes, records of scalars, `der`,
  `when`, `if`, functions of scalars); `to_text` round trip.
* **Interface**:
  ```rust
  pub fn parse(text: &str) -> Result<Vec<ComponentDef>, Vec<LangError>>;
  pub fn to_text(def: &ComponentDef) -> String;
  pub mod basemodelica { pub fn import(text: &str) -> Result<ComponentDef, Vec<LangError>>; }
  ```
* **Accepted when**: every `lsim-lib` component round-trips (`to_text` →
  `parse` → equal), the vehicle blocks in every configuration too; 50
  malformed inputs each give a span and a plain message (70 today); every
  example of the format's documentation runs; 10 hand-written Base Modelica models import and simulate to
  their known answers; unit errors are caught at parse time with the
  equation quoted.

### WP2 — Model preparation

* **Owns**: `crates/lsim-prep`.
* **Builds**: Hopcroft–Karp; tearing (Cellier) and linear-block detection;
  Pantelides + dummy derivatives; `if`-relation modes and their zero
  crossings; clocked partitions for `ExternalBlock`s; the initialisation
  system (`prepare_init`); the inverse model (`prepare_inverse` from an
  `InverseSpec`); parameter-binding order; start values as expressions of
  parameters (re-evaluated at run time); the Jacobian `SparsityPattern`;
  the structural-diagnostics catalogue (section 14); pivot checks on
  symbolically solved equations.
* **Interface**:
  ```rust
  pub fn prepare(lib: &Library, top: &ComponentDef, o: &PrepOptions) -> Result<PreparedModel, Vec<Diagnostic>>;
  pub fn prepare_inverse(lib: &Library, top: &ComponentDef, spec: &InverseSpec, o: &PrepOptions)
      -> Result<PreparedModel, Vec<Diagnostic>>;
  // PreparedModel gains (agreed with WP3): jac_pattern: SparsityPattern, init: InitSystem, modes: Vec<Mode>
  ```
* **Accepted when**: index-2 and index-3 test models (rigidly coupled
  inertias, a capacitor across a source, a pendulum in Cartesian
  coordinates) prepare and simulate to their exact answers; tearing leaves
  at most the expected tearing variables on a benchmark set of algebraic
  loops; 10⁵-equation synthetic networks prepare in under 1 s; every
  catalogue fault gives its code and parts; the inverse model of the
  example cars prepares with only the drive-cycle inputs known.

### WP3 — Code generation

* **Owns**: `crates/lsim-codegen`, `crates/lsim-math`.
* **Builds**: coloured sparse Jacobians (`jacobian_sparse`); the table
  runtime (monotone cubic 1-D, 2-D) with values and derivatives; modes;
  chunking of large models; the initialisation functions; a compile-time
  budget and an `opt_level` policy; a measured decision on machine-code
  caching (`cranelift-object`).
* **Interface**:
  ```rust
  pub fn compile(m: &PreparedModel, o: &CodegenOptions) -> Result<JitModel, CodegenError>;
  impl ModelFunctions for JitModel { … }
  impl JitModel { pub fn jacobian_sparse(&self, inp: &EvalInput, work: &mut [f64], values: &mut [f64]); }
  ```
* **Accepted when**: 10 000 random expressions agree with the interpreter
  (bit-exact for arithmetic, 1 ulp for library calls); every `jvp` matches
  central differences; sparse and dense Jacobians agree; tables are C¹ and
  monotone where their data are; a 10⁴-equation model compiles in under
  100 ms; the example cars compile in under 50 ms.
* **Compiled condition kernels** (from work package 4's ninth round):
  the conditions that mix time and states compiled into a point and an
  interval kernel from the same IR (`ConditionKernels` in lsim-ir, the
  interpreter its default), bitwise the interpreter's and the compiled
  `roots` bitwise the point kernel, sharing subexpressions, tables read at
  run time, a condition declined falling back to the interpreter;
  accepted on the differential test, the golden comparison byte-identical
  with the kernels on and off, and the check at most 3 % of the BEV WLTC
  run's instructions after WP3's speed-up (section 5.8).
* **Done** (branch `wp3/codegen`; section 7): 11 100 random expressions
  agree with the interpreter, the functions it compares bitwise, the
  residual's group bitwise but for its exact integer powers (within 1 ulp
  of the exact power; `fuzz.rs`); every column of the sparse Jacobian and
  every product matches Richardson-extrapolated central differences to
  their own error estimate (median relative error 1e-13; `jacobian.rs`),
  the dense Jacobian is the sparse one, the forward-mode code's columns
  the coloured Jacobian's bit for bit; the tables are C¹ and monotone
  where their data are (`tables.rs`); the `when` semantics are pinned
  (`when.rs`); the example cars compile in 23–32 ms of one thread and
  10 000 equations are ready in 42–48 ms, tiered (`budget.rs`; their
  machine code in about 190 ms on four threads); machine code is not
  cached (measured, section 7). The condition kernels: bitwise the
  interpreter (`kernels.rs`), the golden comparison byte-identical with
  them on and off with the same counters (BEV WLTC: 856 318 certified,
  6 386 scanned, no pulses); the check took 3.5 % of the BEV WLTC run's
  instructions (3.39 % at the start, the rest of the run having become
  cheaper), not 3 %: the kernels' enclosures are 0.95 % (217 million
  instructions against the interpreter's 240 million: the interval
  arithmetic itself, which must stay bitwise the interpreter's), the rest
  of the check, 2.5 %, is the run loop's (dense output, bases,
  certificates). Golden comparison byte-identical throughout. After the
  review: a dropped model gives its machine code back; a tiered model's
  compilation stops when it is dropped and reports how it ends; a system
  that refuses executable memory gets tapes; compiling and running set
  the default floating-point environment; the exact powers are the same
  bits with and without FMA (section 7). Then one mathematical library for
  the whole engine, SUNDIALS' C code included: CORE-MATH's correctly
  rounded functions, the same bits on every platform and CPU, the
  interval widening a proof (section 7.1). Then the energy books
  compiled (section 5.8, *compiled energy books*): the BEV WLTC run 16 %
  fewer instructions, 22 % less time. The condition check after it,
  measured with callgrind on the BEV WLTC (`scan_share`, its
  `scan_mixed`): 674 million instructions of 18.76 billion, **3.6 %**
  (it was 773 million of 22.41 billion, 3.45 %: the run around it got
  cheaper). A step on which every condition's certificate holds (852 430
  of 858 716 condition-steps) is now cleared in one pass over the states
  they read, against the intersection of their boxes, exactly when the
  check of each in turn would clear each (the same counters, the golden
  comparison byte-identical): 99 million instructions fewer. **3 % is not
  reached, and not without changing what the check does.** What is left
  is its substance: the integrator's polynomial over each step and the
  bound on how far each state strays over it (1.2 %, every step, the only
  rigorous bound the certificate can use), the one-pass test (0.36 %), the
  compiled interval kernels' enclosures (1.1 %, of which 2-D and 1-D table
  enclosures 0.76 %: the interval arithmetic that must stay bitwise the
  interpreter's) and the polynomials of the steps searched (0.5 %). The
  floor of this check is therefore about 3.5 %; lower would need fewer
  enclosures or a bound over several steps at once, which changes the
  decisions (counters, windows) and needs its own review. Share of the
  run's time (`scan_share`, five runs): 5.5 % median (0.19 s of 3.4 s;
  the timer's own cost included).

### WP4 — Solver runtime

* **Owns**: `crates/lsim-solve`, new `crates/lsim-sundials-sys`.
* **Builds**: the in-tree SUNDIALS build (no CMake, no libclang); the
  faer sparse `SUNLinearSolver`; the diffsol backend (feature); automatic
  method choice; event iteration, modes and time events; the
  `DiscreteBlock` clock scheduler (restart only on change); initialisation
  with Newton and homotopy; energy quadratures and the closure check; the
  solver report and the 10× tighter re-run; parallel sweeps.
* **Interface**:
  ```rust
  pub fn simulate(m: &dyn ModelFunctions, info: &RunInfo, o: &SolverOptions, grid: OutputGrid,
                  blocks: &mut [Box<dyn DiscreteBlock>]) -> Result<SimResult, SolveError>;
  pub fn sweep(m: &(dyn ModelFunctions + Sync), info: &RunInfo, sets: &[Vec<f64>], o: &SolverOptions,
               grid: OutputGrid, threads: usize) -> Vec<Result<SimResult, SolveError>>;
  ```
* **Accepted when**: the exact-answer suite passes on both backends; the
  backends agree within tolerance; event times within 10·rtol of exact
  ones; energy closure ≤ 1e-6 on every example; a 10 ms Script block that
  changes nothing costs < 5 % run time; sweeps scale ≥ 3.5× on 4 cores;
  SUNDIALS builds with only a C compiler on Windows (MSVC; lsim-math's
  CORE-MATH needs clang-cl as well, section 7.1); macOS and Linux later.
* **Second round (from the golden comparison)**: `when` conditions made
  true by a tick fire; what a clock schedules is no event storm; `when`
  semantics at the start decided as Modelica's; event iteration with the
  iteration variables solved again; ticks a few ulps before the end;
  cheap restarts (11.4 → 2.2 steps a tick on a DAE) and light ones; exact
  time events; the impulse projection that keeps the momentum at a gear
  shift (section 8.2); `suppress_algebraic_error` measured and left off.
* **Third round (from the review of the second)**: the projection starts
  only at a declared rigid engagement and leaves what a `reinit` set; its
  loss splits in two physical stages (the engaging part's, each link's,
  all ≥ 0) checked against a resolved stiff tyre; exact derivatives
  (forward-mode through the assignments, the compiled Jacobian for the
  iteration variables) and Newton's method instead of finite
  differences; event iteration goes on after it; light restarts opt-in
  (an exact pass-through stays for ticks that reach nothing integrated);
  time crossings' right limit only at their own time, and joining a root
  at the same instant; strict `when` conditions (`PreparedWhen::strict`);
  alias start conflicts told and decided independently of the order; the
  zero crossings' order a stated, tested contract (section 5.8).
* **Fourth round (from the review of the third)**: a tyre passes no
  impulse (its force is bounded by its grip): after a shift's rigid
  engagement its slip relaxes in time, integrated, which a fully resolved
  two-axle car confirms and relaxing at once does not; links are judged
  after the rigid stage, join as others relax, and several share their
  loss as the event's; a cascade of engagements at one instant is
  projected to its end or stops the run naming it; `consistent_z` fails
  by default; inert ticks are counted apart from light restarts.
* **Fifth round (from the review of the fourth)**: the energy books'
  stored-energy rates exact (forward-mode through the assignments, the
  model's x' on IDA), the closure back to round-off (1.5e-9 or better on
  every golden case, from 1.6e-6); tables read along time stop the
  integrator at their breakpoints; both sides of an event at an output
  time recorded (`SimResult::left_limits`).
* **Sixth round (from the final review)**: every sampled block due at an
  instant ticks in one event (an output point there no longer shows the
  state between two ticks); the energy books read the model's tables
  (`ModelFunctions::eval_table`), the rates through them and through
  the time exact; conditions on explicit functions of time found ahead
  without integrating and reached exactly (interval enclosures,
  `RunInfo::time_functions`), those that mix time and states stopped at
  the extrema of their time terms, the rest named in a warning; a zero
  direction has a zero rate; left limits of the changed channels only;
  moving tables' breakpoints recomputed. The golden comparison is the
  same to the last digit (none of its models has such a condition).
* **Seventh round (from the review of the sixth)**: a block ticking at an
  instant reads the iteration variables solved again for what the blocks
  before it set (`RunInfo::z_discretes`, `SolverReport::z_solves`), and a
  computed input read through its chain at a stop is evaluated there;
  mixed conditions are checked along every step's dense output
  (`SolverReport::pulses_found`) instead of stopping at their time terms'
  extrema; integer powers enclosed with a proven bound and the platform's
  libm tested; a run whose books or conditions read a table the model
  does not give does not start (the full `ModelFunctions` contract, §5.8);
  books without error control flagged. The golden comparison is the same
  to the last digit again.
* **Tenth round (the open criteria)**: an idle sampled block's tick made
  cheap: its inputs read ahead across the step (the dense output and a
  computed input's chain as a tape, bit for bit), its idle ticks run in
  a row, nothing allocated (section 9; from 19.5 % to under 5 % of the
  smallest model's run); the recorder's channels each given their
  grid-length storage (`vec![v; n]` had given it to one), so the runs
  of a sweep reallocate less (section 8.4). The golden comparison is the
  same to the last digit.
* **Eleventh round (solver fundamentals, 16 October 2026)**: a model with
  no state runs, on an integrator of nothing but its books
  (`stateless`, section 8.1); IDAS decides an order decrease on the
  states' and the books' errors together (the vendored copy patched,
  section 8.1: a spin-up whose states are exactly linear stayed at order
  1, its books setting the step); where the error test fails ten times
  at a kink the history cannot cross, the integrator restarts there and
  goes on, and steps that no longer move the time stop the run, naming
  the variable (section 8.1; no golden run takes either path). With the
  order the golden comparison moves by about the tolerance, every figure
  and channel on the side of its band it was (section 8.1).
* **Twelfth round (the outside judge's checks, 16 October 2026)**: a
  condition on a variable solved from an implicit equation is held on
  the side it crossed to where solving the iteration variables again at
  its event put it back a little short of zero, so its `when` fires
  once per crossing, and a mode that its relation set back there
  restarts the integrator, which meets the crossing again (section 8.2:
  before, two or three firings, a mode off until the next step end).
  The judge scores by time, not by row, with a criterion fixed before
  scoring (section 15), and counts crossings with exact answers. A
  condition on the states alone, not on time, is still left to root
  finding: a known limit (section 8.2). The golden comparison: one mode
  of the formula student car met 1.7e-7 s earlier, its figures moving by
  at most 6e-4 of their bands; the rest the same to the last digit.
* **Status (as built)**: the exact-answer suite passes on both backends,
  ODE and DAE paths (`lsim-solve/tests/reference.rs`); the backends agree
  within 4.2·rtol; events within 2.6·rtol on SUNDIALS at every tolerance,
  on diffsol within 7.2·rtol at 1e-6 and 1e-8 but up to 34·rtol at 1e-10
  (its step error control, not root finding: its root finder stops
  within about 100 ulps of t, and at 1e-10 its solution itself is 19
  rtol off on the RC step against SUNDIALS' 2.8; closing it would mean
  changing diffsol's error control, and diffsol is the cross-check only:
  it is held to 50·rtol);
  energy closure ≤ 1.1e-7 everywhere tested before the example projects
  imported, ≤ 1.5e-9 on every golden case since the stored-energy rates
  are exact, ≤ 6.6e-10 since IDAS decides its order on the books' errors
  too (section 8.1); the idle 10 ms block and the sweeps as measured in sections
  9 and 8.4 (the block 2.0–4.4 % of the 21-state run, at most 2.8 % of
  the 101-state one, from 19.5 % at worst in the earlier measurement (25.0 %
  in section 9's); sweeps 3.2–3.5× for 16 short
  runs, ≥ 3.5× for many or long ones); SUNDIALS builds with only a C
  compiler on Windows: CI's "Engine (Rust) on Windows" job builds it with
  MSVC (and lsim-math with clang-cl) and runs every engine test there
  (green on the work's starting commit, run 38620416352, and on the merged
  tree from dd8feb0, run 38840409543).

### WP5 — Component library and project import

* **Owns**: `crates/lsim-lib`, `crates/lsim-project`.
* **Builds**: the physical library (electrical, rotational, translational,
  thermal, fuel; signal blocks) and the 36 blocks of section 12.2 with
  today's parameters, ports and channels; `standard_registry()`; the
  importer's sub-systems and implicit links; the golden-comparison harness
  and its report.
* **Interface**:
  ```rust
  pub fn library() -> Library;
  pub fn standard_registry() -> Registry;          // lsim-project
  pub fn import(project: &Value, registry: &Registry) -> Result<(ComponentDef, ImportReport), Vec<Diagnostic>>;
  ```
* **Accepted when**: every block passes its exact-answer cases from
  `benchmarks/`; every example project imports and builds; every KPI of
  every example case is inside its golden band (section 12.3) or has a
  documented intended difference; every block's energy books close.

### WP6 — Fast mode, the front door, Python and integration

* **Owns**: `crates/lsim-fast`, `crates/lsim-engine`, `crates/lsim-py`,
  `crates/lsim-script`,
  `engine/check.sh`, CI, notes for `backend/`.
* **Builds**: the Rosenbrock-W inverse stepper with limit flags; the
  facade (build, simulate, fast, sweep, a cache keyed on every input:
  preparation chooses by the parameter values, section 7); the Python API of section 13 with NumPy arrays and GIL release;
  the `DiscreteBlock` implementations for Script blocks and FMUs over
  today's sandboxes; the conversion to today's result JSON; performance
  gates (not built yet, section 15); a Windows wheel (macOS and Linux later).
* **Interface**:
  ```rust
  impl FastSolver for RosenbrockW { fn run(&self, m: &dyn ModelFunctions, p: &[f64], inputs: &[Trace], o: &FastOptions) -> Result<FastResult, FastError>; }
  impl Model { pub fn fast(&self, traces: &[Trace], o: &FastOptions) -> Result<FastResult, FastError>; }
  ```
* **Accepted when**: fast mode runs WLTC on every example car faster
  than the average of comparable quasi-static tools and its flags
  coincide with the forward run's limit hits; full dynamic runs each
  example car faster than the average of comparable tools (section 2;
  the fixed 10⁶× and 1000× targets were dropped on 15 October 2026); pytest covers
  build/run/sweep/errors; a backend feature flag lets the app run a case
  on the new engine and show it like any run.

### Order and parallelism

All six start now. Dependencies are soft because each develops against
Stage 1's working pieces: WP2's inverse model gates the end-to-end fast
mode (WP6 starts its stepper on hand-written inverse models); WP3's sparse
Jacobian gates WP4's sparse LU for large models (WP4 starts with dense
and band); WP5's blocks need WP1's tables and WP2's modes for the clutch,
brake and gearbox (WP5 starts with the electrical and thermal parts and
the vehicle body). A short integration checkpoint each week runs
`check.sh`, the exact-answer suite and the spike.

## 17. Risks

| # | risk | mitigation |
|---|---|---|
| R1 | Sampled Script blocks at 10 ms force 180 000 integrator restarts on WLTC | restart only when an output changes; recommend continuous-time controllers; measure (WP4 acceptance). **WP4:** done — no restart and no extra step for a tick that changes nothing; its inputs read ahead across the step, bit for bit: 9.5–21 ns a tick, 2.0–4.4 % of the cheapest test model's run (20 000× real time), < 3 % of the 101-state one (section 9) |
| R2 | SUNDIALS build on Windows/macOS | in-tree `cc` build with committed config header and bindings; CI on all three from the start (WP4/WP6). **WP4:** built and tested on Linux with only a C compiler; the MSVC build is proven by CI's "Engine (Rust) on Windows" job (run 38620416352 on a619ab4; the merged tree from dd8feb0); macOS is not built (Windows only for now) |
| R3 | Index reduction needs dynamic state selection for some models | static dummy derivatives with an initial pivoting check cover vehicle drivelines; report the rare case clearly; dynamic selection only if a real model needs it. **The outside judge (16 October 2026, section 15):** the rare case is neither rare nor reported clearly. The index-3 pendulum released at 1.48 rad or more (the IFToMM planar pendulum among them), or looping, keeps the states chosen at the start through the point where they degenerate: at rtol 1e-4 … 1e-6 two thirds of the runs end without a message and wrong (the position 0.3 to 2 m off on a 1 m rod, the energy up to 29 J), the rest stop there (SUNDIALS flag −4, or no step passes), and at 1e-7 … 1e-10 nearly all stop; the IFToMM multiple four-bar does not prepare (SINGULAR-LOOP at its start values); released exactly level (y = 0) the pendulum is refused (STATE-SELECT-SINGULAR), 6e-17 m below it prepares. Below 1.48 rad the runs end, but 103 of the 1029 up to 1.47 rad without a message more than 100 rtol off the closed form (section 15). OpenModelica 1.27 (DASSL) with its dynamic state selection runs every pendulum (at rtol 1e-10 to 1.3e-9 m released at 0.40 rad, 9.8e-8 and 6.6e-8 m at 1.50 and 1.55 rad, 8.6e-8 m on the IFToMM problem, 6.4e-7 … 2.0e-5 m looping, 1.3e-5 and 6.4e-4 m at 3.00 and 3.13 rad); with static dummy derivatives it fails as the engine does, mostly worse (results of NaN reported as a success); neither of its selections runs the four-bar (it fails at its start, or does not build). **Detection (21 October 2026, section 6 step 5):** the choice is watched along every run, at its start, on `set_param` and on either side of every event; where it degrades the run stops there, naming the constraint, the variables and the parts: none of the judge's pendulum runs ends on a bare SUNDIALS flag or without a message for a degenerate choice any more (1 169 of the release-angle sweep's runs stop so, where 432 ended wrong and 840 stopped on a flag or with no step passing), nor where an event comes just before the point (`pendulum-event-…`); `stateSelect` leads the choice where it starts within half the run's limit (with x marked `prefer` the pendulum at 1.50 and 1.55 rad runs to its end; at atan 99 the mark is refused and told). **Dynamic selection (23 October 2026, section 6 step 5, *the choice made again*):** where the watched choice degrades past a threshold (3 times its factor where its fixed entries were last read; 50 while it keeps a column the model marks to be kept), the run prepares the model again with the states pivoted at that point, restarts the integrator on them from the solution there and goes on, the energy integrals carried and the channels kept by name. Every pendulum run of the judge now goes to its end (the sweep's 1 169 runs from 1.48 rad stopped before; released exactly level the pendulum prepares and runs), at rtol 1e-10 3.2e-8 m at 1.50 rad, 3.6e-8 m on the IFToMM problem, 1.8e-7 … 6.7e-6 m looping (OpenModelica's figures above: 9.8e-8, 8.6e-8, 6.4e-7 … 2.0e-5); about as accurate as the same pendulum in its angle (1.21 times its error, geometric mean), and like it more than 100 rtol off in most runs from 1.48 rad (1 023 of the 1 169; the angle form 1 048): the problem's own sensitivity, not the choice. A run that does not switch is bitwise as before (every golden case, every other judge run). Where the run cannot switch (diffsol, a model given other tables after it was built; fast mode's inverse model is given no choices and never run by the run loop), or where no other choice works, the run stops naming the constraint and why, as before. **Fix round (23 October 2026, the review of the above):** a model with two or more moving sets could not switch (two pendulums in one model stopped where the first degraded); now only the set that degraded is chosen again, every other set keeps its choice and threshold, and every held-out multi-pendulum run goes to its end, none within 100 rtol (their angle forms in 17 of 35: one part's switches restart the integrator for all, section 15, the sixth verdict); tables are compared as interpolants before a switch. Open: the IFToMM four-bar does not prepare (`SINGULAR-LOOP` at its start values: not a choice of states) |
| R4 | Friction and clutch modes chatter | force-based stick/slip conditions in the components; event-storm detection naming the parts. **WP4:** storm detection done (over 100 state events within 1e-3 of the run, the error names the modes' parts; tested on a chattering mode); the force-based conditions are WP5's |
| R5 | Golden comparisons show differences that are today's numerical error | measured bands from today's own fine-step runs; differences triaged and documented |
| R6 | Fast-mode speed (now: faster than the average of comparable tools, section 2) | Rosenbrock-W with Jacobian reuse; explicit backward evaluation of the sorted inverse model; measured early in WP6 |
| R7 | diffsol API churn | pinned exact version; it is the second backend, not the product's |
| R8 | Cranelift code quality on very large models | chunking; measure; LLVM is not an option (licence-clean but heavy to ship); our models are straight-line arithmetic where Cranelift does well. **WP3:** compile time, not code quality, was the limit (about a microsecond per IR instruction): models above 20 000 expression nodes run on tapes while their machine code compiles (10⁴ equations ready in under 50 ms) |
| R9 | KLU is LGPL and banned | faer sparse LU (MIT) as our SUNDIALS linear solver (WP4). **WP4:** done; dense, band and sparse LU give the same runs on every reference model (test) |
| R10 | Parallel agents change `lsim-ir` incompatibly | additive changes only; owners agree; check.sh in every package's CI |
| R11 | Base Modelica is a moving specification (MCP-0031) | the text format is a strict subset; import tracks the published version |
| R12 | Windows x64 is the target and only Linux is tested here | **WP3:** generated code takes the ISA's default calling convention everywhere, checked per function and by compiling every example for `x86_64-pc-windows-msvc` in the tests; stack probes for large frames; tapes where executable memory is refused; the default floating-point environment for compiling and running; the same bits on CPUs with and without FMA. Only Windows CI can run the code there: the convention end to end, the stack probes, the refusal path on a real Arbitrary Code Guard process (Windows CI now runs every engine test, green). **The mathematical library (October 2026):** the engine ships its own and calls it everywhere, SUNDIALS' C code included (`lsim-math`: CORE-MATH's correctly rounded functions, section 7.1), so results no longer depend on the C runtime or the CPU and the interval widening is a proof; built on Windows with clang-cl, proved by Windows CI; after the review, CORE-MATH patched not to end the process and not to depend on the width of `long` (all references correctly rounded compiled with clang-cl's semantics), the compiler held to the build's own options and the library checked at its first call. Still the platform's: diffsol's step control (the cross-check backend: its runs remain CPU- and platform-dependent) and a Script block's Python |

## 18. Licences

The engine's mathematical library (`crates/lsim-math`, October 2026)
vendors 13 of CORE-MATH's C files (MIT, INRIA and contributors;
`core-math/LICENSE` and every file's header) and musl's `fma` (MIT,
`csrc/LICENSE-musl`): no crate added (licences.py and cargo-deny pass
unchanged).

Since the merge of the six work packages the workspace builds with 234
third-party crates (the count `scripts/licences.py` prints),
all permissive (licences.py and cargo-deny: "bans ok, licenses ok"):
sundials-sys, bindgen, clang-sys, libloading and cmake are gone; in came
`cc` with `jobserver` and `getrandom` (MIT OR Apache-2.0; `r-efi` taken
under MIT), faer 0.24.4 (MIT), rayon (MIT OR Apache-2.0), diffsol 0.17.1
(MIT) with nalgebra (Apache-2.0) and their dependencies, and, for tests
only, toml (MIT OR Apache-2.0). The vendored SUNDIALS is BSD-3-Clause and
contains no KLU and no GPL code.

`scripts/licences.py` (run by `check.sh`) reads `cargo metadata` and
requires every third-party crate's SPDX expression to be satisfiable with
MIT, MIT-0, Apache-2.0 (also WITH LLVM-exception), BSD-2/3-Clause, ISC,
Zlib, 0BSD, BSL-1.0, CC0-1.0 or Unicode-3.0; GPL, LGPL, AGPL, EUPL and SSPL
never pass. cargo-deny 0.20.2 (`deny.toml`, same policy) agrees: "bans ok,
licenses ok". Stage 1 (historical) built with 113 third-party crates, all
permissive: the main ones are Cranelift 0.134.4 and regalloc2 (Apache-2.0
WITH LLVM-exception), sundials-sys 0.6.2 and the vendored SUNDIALS 7.1.1
(BSD-3-Clause; no KLU, no GPL code), bindgen (BSD-3-Clause, build only),
PyO3 0.29.3, serde, sha2, thiserror (MIT OR Apache-2.0), libloading (ISC),
foldhash (Zlib), unicode-ident ((MIT OR Apache-2.0) AND Unicode-3.0);
crates offered as "Unlicense OR MIT" are taken under MIT. The solver
experiment adds diffsol 0.17.1 (MIT), nalgebra (Apache-2.0), faer (MIT)
and their dependencies: 196 crates, all permissive (`r-efi` is offered as
MIT OR Apache-2.0 OR LGPL and taken under MIT).

## 19. The Stage 1 spike

What runs (`crates/lsim-engine/tests/spike.rs`, `examples/spike.rs`): the
model of section 3.1 built from library components — the battery is a
composite of a source, R0 and an RC pair; a DC machine, an inertia, a
damper, the threshold brake and a ground — is flattened (49 variables, 48
equations plus one `when`), checked for units, reduced by alias elimination
(32 aliases), matched and sorted (2 states, 16 explicit assignments),
compiled by Cranelift (0.7 ms, 1.8 kB), and solved by CVODE with the
exact Jacobian; the brake event is located and the integrator restarted.
The same model with every block kept implicit runs through IDA with 16
iteration variables, exercising the DAE path and consistent
re-initialisation.

Measured (`cargo run --release -p lsim-engine --example spike`, best of 20
on the shared 4-CPU machine, load ≈ 3; 4 s simulated, output every 10 ms):

| path | rtol | solve | × real time | steps | worst error vs exact | event-time error |
|---|---|---|---|---|---|---|
| ODE (CVODE) | 1e-6 | 0.45 ms | 9 000 | 183 | 3.8e-6 | 1.4e-5 s |
| ODE (CVODE) | 1e-8 | 0.58 ms | 6 900 | 363 | 2.9e-8 | 6.4e-8 s |
| ODE (CVODE) | 1e-10 | 0.68 ms | 5 900 | 621 | 2.7e-10 | 6.9e-10 s |
| DAE (IDA, 16 iteration variables) | 1e-6 | 0.99 ms | 4 000 | 392 | 3.3e-7 | 5.1e-7 s |
| DAE (IDA) | 1e-8 | 1.54 ms | 2 600 | 782 | 4.2e-9 | 1.5e-8 s |
| DAE (IDA) | 1e-10 | 4.34 ms | 920 | 2534 | 2.9e-10 | 7.7e-10 s |

Preparation 0.29 ms; compilation 0.71 ms; a residual call 7.3 ns; a
Jacobian-vector product 16 ns. The tests assert errors below 1e-8 and
event times within 1e-8 s at rtol 1e-10 on both paths; that tightening the
tolerance 10× twice shrinks the error each time; that a parameter change
(battery OCV 400 → 450 V, through its binding to the source inside the
composite) needs no recompilation and matches the new exact answer; that
the interval min/max/mean are right (the mean of the speed over [3, 4] s
against Simpson's rule on the closed form); and that a cached build skips
preparation and gives identical results. From Python, the module built by
PyO3 runs the same spike (`lightsim_engine.spike(1e-8)`: error 2.9e-8).

The DAE path's extra steps at 1e-10 are IDA's error control on 16
algebraic variables that the ODE path computes exactly; production keeps
only tearing variables in `z` (WP2) and may exclude algebraic variables
from the error test (`IDASetSuppressAlg`) after measuring its effect on
accuracy. The ODE path's run time is about 2.5× the hand-written CVODE of
the solver experiment because the recorder evaluates every channel at
every internal step and at 400 output times; WP4 restricts that to the
channels the case records.

How to reproduce:

```sh
export CARGO_TARGET_DIR=…            # one shared target dir
engine/check.sh                       # fmt, clippy, tests, licences
cargo run --release -p lsim-engine --example spike --manifest-path engine/Cargo.toml
(cd engine/experiments/solver-eval && cargo run --release)              # section 3.2
(cd engine/experiments/solver-eval && CHAIN=1 cargo run --release)      # section 3.3
(cd engine/experiments/solver-eval && HMIN=1e-20 RTOLS=1e-10,1e-12 cargo run --release)
```

Python integration of the app (`backend/`) is unchanged in Stage 1; WP6
adds the engine behind a process-wide switch (`LIGHTSIM_ENGINE=new`) once
the example cars run.
