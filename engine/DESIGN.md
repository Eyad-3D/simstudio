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
  index-1 DAE, compiled to machine code with **Cranelift** in about a
  millisecond, and integrated by **SUNDIALS CVODE/IDA**.
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
| Full dynamic speed | ≥ 1000× real time on the example cars (WLTC: 1800 s in ≤ 1.8 s) | criterion benchmarks per example car, CI gate |
| Fast mode speed | ≈ 10⁶× real time on WLTC (≈ 2 ms) | same |
| Accuracy | every block within its tolerance of an exact answer; global error tracks rtol | the exact-answer suite in `benchmarks/` (built by another agent), tolerance-convergence tests |
| Energy | the energy balance from port powers closes to ≤ 1e-6 of the energy throughput | per-run check, reported |
| Events | located to the integrator's root-finding precision (7e-10 s at rtol 1e-10 in the spike) | event-time tests against closed forms |
| Units | every equation dimensionally consistent at build time | `lsim-prep` unit check |
| Licences | only permissive licences (MIT, Apache-2.0, BSD, ISC, Zlib …) | `scripts/licences.py`, cargo-deny |
| Platforms | Windows, macOS, Linux (x86-64, arm64) | CI matrix (WP6) |

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
left the lockfile. CI must still prove the Windows (MSVC, `/fp:precise`)
and macOS (Xcode command-line tools, arm64 and x86-64) builds. Beside the
SUNDIALS sources, `csrc/` holds our two small C helpers (BSD-3, like the
code they read): the dense output of CVODES and IDAS for selected
components only, with the same coefficients and summation order as
`CVodeGetDky`/`IDAGetDky` (a test checks they give the full dense
output's exact bits inside every step), so a sampled block's tick
interpolates only what it reads.

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
| `experiments/` | one-off measurements outside the workspace (the solver evaluation) | – |

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
  `table` indexes `FlatSystem::tables` and `args` are the abscissae.

The text format writes `a == b` and `a <> b`; the IR holds them as
`a >= b and a <= b` and `a < b or a > b` (no new comparison operators), and
`true`/`false` as 1 and 0.

**Tables** (`table.rs`): one `TableData` serves the language, preparation
and the code generator: breakpoints `x` and `y` (empty for 1-D), values
row-major (`values[i * y.len() + j]` at `(x[i], y[j])`), `interpolation`
(`MonotoneCubic` by default, or `Linear`), `outside` per axis (`Clamp` by
default, `Linear`, `Error`: today's `tableOutside`) and `axis_units` (the
axes' coherent SI unit texts). `TableData::check` says in plain words what
is malformed.

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
  that knows only `when` clauses keeps every mode right between events.

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
  parameter change makes a `ParamGuard` zero, prepare again.
* Show `PreparedModel::warnings` in the build report and the Python
  `model.report`.

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

**WP3** keeps: its interpreted tape (`tape.rs`) is not wired in,
`InitFunctions::guess` uses the flat start values rather than
`InitSystem::guesses`, and asserts have no compiled function yet.

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
   static selection with a pivoting check on the Jacobian at
   initialisation; dynamic state selection is out of scope until a model
   needs it (risk R3). Symbolic time differentiation of equations is in
   `symbolic.rs`'s rule set, extended with `der` of tables (the monotone
   cubic's derivative) and of `limit`.
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
parameter value never recompiles. Transcendental functions call Rust's
standard library through registered symbols. Measured in the spike: 0.7 ms
to compile all five functions (1.8 kB of machine code); one residual call
7 ns, one Jacobian-vector product 16 ns.

WP3 adds:

* **Sparse coloured Jacobians.** Column colouring of the sparsity pattern
  (greedy, largest-first) and one `jvp` sweep per colour, filling the
  CSC values directly: `jacobian_sparse(inp, work, values)`. A vehicle
  model's Jacobian needs ~5–15 colours whatever its size.
* **Tables.** The generated code calls the table runtime
  (`lsim-codegen/src/tables.rs`) for value and derivatives; the data live
  in the compiled model's table store built from `FlatSystem::tables`
  (tables are runtime data: `JitModel::with_tables` swaps them without
  recompiling). Monotone cubic by Fritsch–Carlson (C¹, no overshoot),
  bicubic Hermite patches in 2-D; linear and today's `tableOutside` rules
  as options; each read axis gets a table guard for the run loop.
* **Large models.** Assignments split into chunks of bounded size, chained
  through the `work` buffer, so register allocation stays linear; a budget
  test (10⁴ equations compile in under 100 ms).
* **Modes** for `if` relations (`modes`: every mode from its relation),
  and the initialisation functions (`InitFunctions`), as in 5.6–5.7.

**Cache.** Keyed by SHA-256 of the model's inputs (top component, every
library definition it reaches, connectors, options, engine version); the
value is the `PreparedModel` as JSON (`lsim-engine/src/cache.rs`, working
in Stage 1). A hit skips flattening and structural analysis; machine code
is regenerated, since it takes about a millisecond per small model and
should stay under 50 ms for a vehicle. Caching machine code
(`cranelift-object` + a small loader) is deferred until measurements on the
example cars show compilation dominating a cached build. WP6 removes
runtime parameter values from the key and re-applies them on a hit, so a
parameter study shares one entry.

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
  why. (The reference problems' RC and RL circuits and the L = 0 motor run
  on Adams; the spike and the motor with inductance on BDF.)
* **diffsol** 0.17.1 (pinned, nalgebra dense LU): the same run loop; root
  directions from the root functions' signs at the step's start and at
  the root; the iteration variables made consistent by the same Newton as
  IDA's path; the energy integrals by three-point Gauss–Legendre on its
  dense output; its smallest step lowered to 1e-20 (section 3.2); a stop
  time it reaches a few ulps short counts as reached.

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
brake five times without this). After every accepted step the modes are
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
* *Scheduled events are no storms.* What a sample tick or a time event
  changes at its instant (a controller switching an engine's throttle from
  one tick to the next) does not count towards `storm_events`.
* *One instant.* A stop time within 16 ulps of the current time (a tick
  that rounds just before the end) is that instant: the run loop does not
  step (SUNDIALS refuses such an interval) and handles what is due there.
  The output grid ends at `t_end` exactly.
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
* *Both sides of an event at an output time.* An output time that falls
  exactly on an event records both sides, as Modelica tools write two
  rows at that time to their result files: `SimResult::values` holds the
  value just after the event (as every consumer has read it),
  `SimResult::left_limits` the index and every channel's value just
  before it (the channels as they stood before the first event of that
  instant changed anything), in order. The golden comparison compares
  today's engine, which records such a point before its step, with the
  left limit.
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

**As built (work package 4).** The energy integrals are under the
integrator's error control by default (`energy_error_control`): without
it they ride on the states' steps, and a fast-decaying loss came out 100×
less accurate than the tolerance (the RC step's resistor loss at rtol
1e-10: 1.3e-8 of the energy scale; with it 2.5e-11). The stored energy's
change is integrated too, its rate taken along the solution by a
fourth-order central difference of the declared stored energy in the
direction (1, y') (exact for the quadratic energies of capacitors,
inductors and masses), so the books close to round-off when every part's
books agree with its equations (section 11). The integrator's own error
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
  results and step counts). Measured (`lsim-solve/examples/r1_blocks.rs`,
  release build, the run thread's CPU time, the period shortened to
  0.1 ms so 18 million ticks stand well above a shared machine's noise):
  a tick that changes nothing costs 19 ns when the block reads a discrete
  value, 47 ns when it reads a state (one selected-component
  interpolation) and 108 ns when it reads a computed channel (its chain
  of assignments interpreted, here through a sine source). A 10 ms block
  over a WLTC (180 001 ticks) so costs 3.4, 8.5 or 19.4 ms: about 3.4 /
  8.5 / 19.5 % of the 21-state test drive, which runs at 18 000× real
  time (100 ms); 0.6 / 1.4 / 3.2 % of the 101-state one (about 3 000×
  real time, 600 ms); and at most 1.1 % of any model that runs at the
  1000× target.
  So the < 5 % criterion holds for models as costly as the 101-state
  drive or more, not for very cheap ones read through a computed channel
  (where the absolute cost is still under 20 ms a WLTC). The block's own
  work comes on top: a sandboxed Python tick costs microseconds.
  Today's Script blocks run every solver step (≤ 10 ms); they keep that
  rate by default (a `period` parameter, default 10 ms) so results match.
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
* Budget: 1800 steps in ≈ 2 ms means ≈ 1 µs per step: a few residual calls
  (tens of ns each for a vehicle) plus a small dense LU refreshed only when
  the Jacobian drifts. Verified against forward runs: where no flag is
  raised, fast and full dynamic agree on energy within the trace's
  discretisation (a documented band).

## 11. Outputs, energy books and accuracy reports

* **Channels**: every flat variable, named by path (`battery.c1.v`); the
  project importer's channel map gives today's names
  (`el-battery:sig_soc`), so the app and the Python package read results
  as today.
* **Summary rows, duty, limits, references**: computed by today's Python
  code from the channels at first (the app's KPIs keep their keys); moved
  into Rust only where speed requires (WP6).
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
  iteration variables. Only a stored energy that reaches a derivative, the
  time or a table through its assignments takes a fourth-order central
  difference, its step moving no entry of `y` by more than 1e-3 of its
  size or of its nominal scale. (Work package 4's fourth round found the
  difference's step collapsing whenever an entry of `y` passed zero while
  moving: its round-off grows as `|E| / step`, and a full fuel tank stores
  some 1e9 J, so the hybrid's books closed to only 1.6e-6 on the UDDS and
  5.4e-7 on the HWFET, by an amount that any change of the step sequence
  reshuffled: 10.5, 1.6, −0.8 and 1.9 J on the HWFET at rtol 1, 0.999,
  1.001 and 0.99 × 1e-6. Exact, the same runs close to 4.3e-12, 8.1e-14,
  5.1e-12 and 2.4e-12.) The **drift** — stored energy from the states at
  the end minus the books' — is the integration error of the energies, of
  the order of rtol; it is reported, with a warning when it exceeds
  100·rtol.
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
| `motor.emotor` | ideal machine (torque = demand × available torque at speed and voltage, `limit` on full-load curve) + loss map table → electrical power = mechanical + loss |
| `engine.combustion` | torque source with full-load and fuel maps (tables), fuel port, idle/on logic as modes |
| `fuelcell.stack`, `fuel.h2_tank`, `fuel.tank` | polarisation table source; tanks as fuel-port reservoirs (mass state) |
| `controller.dcdc` | average-model converter with efficiency, power conserved minus loss |
| `electric.voltage_source`, `electric.constant_drive`, `electric.climate` | constant voltage; power consumers (i = P/v with a guard); climate load model |
| `electric.node`, `mech.node` | composite whose own ports are all connected together (one connection set: equal across, through sums to zero) |
| `boundary.ground` | ground with three pins |
| `mech.shaft`, `mech.final_drive`, `mech.gearbox`, `mech.differential`, `mech.transfer_case` | inertia, ideal gear with efficiency (loss in the power direction as a mode), selected ratio (gearbox: discrete gear from the signal, shift as an event), open differential (torque split, speed sum) |
| `mech.clutch` | stick/slip friction with engagement signal: modes and events, no chattering |
| `mech.brake` | friction torque against rotation, holding at standstill (stick mode) |
| `propulsion.wheel` | rolling radius, slip-based tyre force (today's model), rolling resistance; translational port to the body |
| `propulsion.propeller` | torque ∝ ω² load |
| `vehicle.body` | mass, road load (Cd·A or A/B/C), grade, load transfer (exact, no longer one step behind), downforce; translational port |
| `driver.driver` | continuous PI speed controller with feed-forward, accelerator/brake split, `limit`s |
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
  investigated and either fixed or documented as an intended correction.
* **Same result schema**: the Python layer converts `SimResult` into
  today's `SimResult` JSON (channels with min/max/mean, summary, energy,
  messages), so the app needs no change to show new runs.

## 13. Python API

The module `lightsim_engine` (lsim-py, built by maturin into an abi3 wheel
for Python ≥ 3.11) is wrapped by the `lightsim` package; the app server
calls the same functions. Stage 1 exposes `version()` and `spike(rtol)`,
which already runs the engine from Python with the GIL released.

```python
import lightsim_engine as le

model = le.build(project_json, case_id, cache_dir=…)      # prepare (or cache) + JIT
model.report                     # sizes, prepare/compile times, structure key, diagnostics
model.set_params({"el-battery.capacity_Ah": 210.0})      # runtime: no rebuild
run = model.simulate(mode="full", rtol=1e-6, output_step=1.0,
                     scripts=sandbox_callbacks, fmus=fmu_blocks,
                     progress=callback, cancel=token)
run.channels["el-battery:sig_soc"]        # NumPy arrays (zero-copy), today's names
run.min, run.max, run.mean, run.events, run.energy, run.report, run.flags
check = model.simulate(..., rtol=1e-7)    # the one-click 10x tighter run
runs = model.sweep([{…}, {…}], threads=4) # parallel in Rust, GIL released
fast = model.simulate(mode="fast")        # inverse model, limit flags in run.flags
le.diagnose(project_json)                 # structural and unit checks only, for Data Checks
```

Errors arrive as `le.ModelError` carrying the diagnostics (code, message,
parts, hint, detail) so the app can highlight the parts.

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
`INDEX-DIFFERENTIATE`, `STATE-SELECT-SINGULAR`, `PIVOT-ZERO-AT-START` (a
warning), `PARAM-CYCLE`, `EXTERNAL-PERIOD`, `EXTERNAL-LOOP` (a warning),
`WHEN-CONDITION`, `WHEN-CONTINUOUS`, `REINIT-NOT-STATE`, `STRUCT-OVER` and
`STRUCT-UNDER`; `INIT-START-IGNORED` is tested with the index-reduction
models and the `INVERSE-*` codes with fast mode's. A floating network is
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
  (`lsim-codegen/tests/codegen.rs`); WP3 extends this to randomised
  expression fuzzing.
* **Exact-answer suite**: the reference suite being built in `benchmarks/`
  gives each block's exact answers; an adapter in `lsim-engine/tests` runs
  every case on both backends (SUNDIALS and diffsol) and asserts each
  within its stated tolerance and the two backends within tolerance of
  each other. Event times are checked separately.
* **Convergence tests**: every exact case at three tolerances; the error
  must fall with the tolerance (the spike's `tighter_tolerance_shrinks_the_error`).
* **Energy closure** on every test run and every example car.
* **Golden comparisons** with today's engine (section 12.3), in CI on the
  example projects; a report of every KPI with its band.
* **Performance gates** (criterion, CI): build time (prepare + compile) and
  run time for each example car in both modes, against the targets of
  section 2 with headroom; a regression of more than 10 % fails.
* **Structural-fault corpus**: each pattern of section 14 has a model and
  an expected code and named parts (like today's broken-models corpus).
* **Python tests** (pytest) of the module once WP6 adds the API.
* `./check.sh` runs rustfmt, clippy (warnings are errors), all tests and
  the licence check; CI runs it on Linux, macOS and Windows.

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
  `parse` → equal); 50 malformed inputs each give a span and a plain
  message; 10 hand-written Base Modelica models import and simulate to
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

* **Owns**: `crates/lsim-codegen`.
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
  the build needs only a C compiler on Windows, macOS and Linux.
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
* **Status (as built)**: the exact-answer suite passes on both backends,
  ODE and DAE paths (`lsim-solve/tests/reference.rs`); the backends agree
  within 4.2·rtol; events within 2.6·rtol on SUNDIALS at every tolerance,
  on diffsol within 7.2·rtol at 1e-6 and 1e-8 but up to 34·rtol at 1e-10
  (its root finding, not ours: the cross-check is held to 50·rtol);
  energy closure ≤ 1.1e-7 everywhere tested (no example project imports
  until WP5); the idle 10 ms block and the sweeps as measured in sections
  9 and 8.4 (< 5 % for models as costly as the 101-state drive, 19.5 % on
  a 100 ms model read through a computed channel; 3.44–4.09×); the build
  proven on Linux only.

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
  `engine/check.sh`, CI, notes for `backend/`.
* **Builds**: the Rosenbrock-W inverse stepper with limit flags; the
  facade (build, simulate, fast, sweep, cache without runtime parameters in
  the key); the Python API of section 13 with NumPy arrays and GIL release;
  the `DiscreteBlock` implementations for Script blocks and FMUs over
  today's sandboxes; the conversion to today's result JSON; criterion
  benchmarks and CI performance gates; wheels for three platforms.
* **Interface**:
  ```rust
  impl FastSolver for RosenbrockW { fn run(&self, m: &dyn ModelFunctions, p: &[f64], inputs: &[Trace], o: &FastOptions) -> Result<FastResult, FastError>; }
  impl Model { pub fn fast(&self, traces: &[Trace], o: &FastOptions) -> Result<FastResult, FastError>; }
  ```
* **Accepted when**: fast mode runs WLTC on every example car at ≥ 10⁶×
  real time and its flags coincide with the forward run's limit hits;
  full dynamic runs each example car at ≥ 1000× real time; pytest covers
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
| R1 | Sampled Script blocks at 10 ms force 180 000 integrator restarts on WLTC | restart only when an output changes; recommend continuous-time controllers; measure (WP4 acceptance). **WP4:** done — no restart and no extra step for a tick that changes nothing; 19–108 ns a tick, < 5 % of the run for models at or slower than ~3 000× real time (section 9) |
| R2 | SUNDIALS build on Windows/macOS | in-tree `cc` build with committed config header and bindings; CI on all three from the start (WP4/WP6). **WP4:** built and tested on Linux with only a C compiler; the MSVC and macOS builds are written for (config header, `/fp:precise`) but still to be proven in CI |
| R3 | Index reduction needs dynamic state selection for some models | static dummy derivatives with an initial pivoting check cover vehicle drivelines; report the rare case clearly; dynamic selection only if a real model needs it |
| R4 | Friction and clutch modes chatter | force-based stick/slip conditions in the components; event-storm detection naming the parts. **WP4:** storm detection done (over 100 state events within 1e-3 of the run, the error names the modes' parts; tested on a chattering mode); the force-based conditions are WP5's |
| R5 | Golden comparisons show differences that are today's numerical error | measured bands from today's own fine-step runs; differences triaged and documented |
| R6 | Fast-mode target of 10⁶× real time | Rosenbrock-W with Jacobian reuse; explicit backward evaluation of the sorted inverse model; measured early in WP6 |
| R7 | diffsol API churn | pinned exact version; it is the second backend, not the product's |
| R8 | Cranelift code quality on very large models | chunking; measure; LLVM is not an option (licence-clean but heavy to ship); our models are straight-line arithmetic where Cranelift does well |
| R9 | KLU is LGPL and banned | faer sparse LU (MIT) as our SUNDIALS linear solver (WP4). **WP4:** done; dense, band and sparse LU give the same runs on every reference model (test) |
| R10 | Parallel agents change `lsim-ir` incompatibly | additive changes only; owners agree; check.sh in every package's CI |
| R11 | Base Modelica is a moving specification (MCP-0031) | the text format is a strict subset; import tracks the published version |

## 18. Licences

Since work package 4 the workspace builds with 231 third-party crates,
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
licenses ok". The engine workspace builds with 113 third-party crates, all
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
adds the engine behind a case-level switch once the example cars run.
