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
| Events | located to the integrator's root-finding precision (≈ 1e-10 s at rtol 1e-10 in the spike) | event-time tests against closed forms |
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
  of today's `tableOutside` as options).

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
* `ParamDecl`: unit, display unit, default (a number or an expression of
  the same scope's parameters), min/max, `structural` (today's
  `variability: fixed`: may change the equations, part of the cache key;
  every other parameter is a runtime input).
* `VarDecl`: unit, continuous or discrete, start value, `fixed`, nominal.
* `SubDecl` + `Connect`: composition. A composite (the battery made of a
  source, R0 and an RC pair; a whole vehicle) and a primitive are the same
  type; a project's diagram is one top-level `ComponentDef`.
* `EquationDecl`: `lhs = rhs`, `when cond then …`, `assert`, each with a
  plain-words label that fault messages quote.
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
  when flange.w >= w_on then "it engages when the speed reaches w_on"
    engaged = 1;
  end when;
  annotation(__LightSim_energy(loss = tau_max * engaged * flange.w));
end Rotational.ThresholdBrake;
```

(A sub-component and a parameter sharing a name, as `r0` does, is legal in
the IR's scoping but not in Modelica; WP1's parser rejects it and WP5
renames such parameters, as Stage 1 already did for the source.)

Energy books use the vendor annotation `__LightSim_energy`, which Base
Modelica tools ignore. Labels are the strings after equations.

### 5.5 The flat system (`flat.rs`)

Every variable, parameter and equation with its `Origin`: the instance
(path, definition, the diagram label and the app's element id) and the rule
that made it (the n-th equation of a definition, a connection set's across
or through equation, an unconnected port, a signal link). Origins are what
let every later stage speak about the user's parts. Parameters keep their
bindings (`r0.R = r0`) so changing a parent's value updates its children
without a rebuild (implemented in `Model::set_param`).

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
external blocks (`ExternalBlock`), and the structure key.

Contracts already written for the parallel work: `SparsityPattern`
(Jacobian structure, WP2 → WP3/WP4), `DiscreteBlock` (sampled blocks,
WP4 ↔ WP6), `InverseSpec` (fast mode, WP2 ↔ WP6).

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
}
```

`EvalInput { t, y, p, d, u }`. The functions are pure; the caller owns the
buffers, so one compiled model serves any number of simultaneous runs.

## 6. Preparation pipeline

`lsim_prep::prepare(lib, top, opts) -> Result<PreparedModel, Vec<Diagnostic>>`.
Stage 1 implements steps 1–4, 6, 7 (simplified), 9 and 11; WP2 the rest.

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
   constant. In the spike, 32 of 49 variables go.
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
   clear message.
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
* **Tables.** Runtime calls `lsim_table1(handle, x)` / `lsim_table2(handle,
  x, y)` with value and derivative; handles index a table store passed in
  the `EvalInput` (tables are parameters: no recompile). Monotone cubic by
  Fritsch–Carlson (C¹, no overshoot), bilinear-monotone for 2-D; linear and
  today's `tableOutside` rules as options.
* **Large models.** Assignments split into chunks of bounded size, chained
  through the `work` buffer, so register allocation stays linear; a budget
  test (10⁴ equations compile in under 100 ms).
* **Modes** for `if` relations, and the initialisation functions.

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

### 8.2 Events

* Zero crossings are located by SUNDIALS' root finding (Illinois method on
  the dense interpolant) with each crossing's direction; the spike locates
  its brake event to 7e-10 s at rtol 1e-10.
* At an event: `when` actions run (`ModelFunctions::when`), mode Booleans
  flip, then **event iteration**: re-evaluate the conditions with the new
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

### 8.3 Initialisation

1. Start values: `fixed` ones are conditions, others guesses.
2. Solve the initialisation system (prepared separately, section 6) by
   damped Newton with the exact Jacobian.
3. If Newton fails: **homotopy** from a simplified problem
   (Modelica's `homotopy(actual, simplified)` operator, available to
   component writers, e.g. a battery with zero RC current or a clutch
   locked) with a natural-parameter continuation in λ ∈ [0, 1], step
   control by Newton iteration count.
4. If that fails: a diagnostic naming the block that did not converge, its
   parts, and the residuals' worst equations.
5. IDA then refines with `IDACalcIC` for the integrator's own consistency.

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
  between ticks. The run loop stops the integrator exactly at each tick
  (stop time), reads the inputs from the solution, calls `tick`, and
  **restarts the integrator only if an output changed** (a restart costs
  a few small steps; most ticks of a slow controller change nothing).
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
  and its stored-energy expression — today's `partEnergy`, now exact by
  construction because connection equations conserve power at every node
  (equal across, through summing to zero). The run checks the global
  closure Σ(sources) − Σ(losses) − ΔΣ(stored) − Σ(boundaries) against the
  energy throughput and reports it (target ≤ 1e-6).
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
all. Run-time failures get the same treatment: a Newton failure names the
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
| R1 | Sampled Script blocks at 10 ms force 180 000 integrator restarts on WLTC | restart only when an output changes; recommend continuous-time controllers; measure (WP4 acceptance) |
| R2 | SUNDIALS build on Windows/macOS | in-tree `cc` build with committed config header and bindings; CI on all three from the start (WP4/WP6) |
| R3 | Index reduction needs dynamic state selection for some models | static dummy derivatives with an initial pivoting check cover vehicle drivelines; report the rare case clearly; dynamic selection only if a real model needs it |
| R4 | Friction and clutch modes chatter | force-based stick/slip conditions in the components; event-storm detection naming the parts |
| R5 | Golden comparisons show differences that are today's numerical error | measured bands from today's own fine-step runs; differences triaged and documented |
| R6 | Fast-mode target of 10⁶× real time | Rosenbrock-W with Jacobian reuse; explicit backward evaluation of the sorted inverse model; measured early in WP6 |
| R7 | diffsol API churn | pinned exact version; it is the second backend, not the product's |
| R8 | Cranelift code quality on very large models | chunking; measure; LLVM is not an option (licence-clean but heavy to ship); our models are straight-line arithmetic where Cranelift does well |
| R9 | KLU is LGPL and banned | faer sparse LU (MIT) as our SUNDIALS linear solver (WP4) |
| R10 | Parallel agents change `lsim-ir` incompatibly | additive changes only; owners agree; check.sh in every package's CI |
| R11 | Base Modelica is a moving specification (MCP-0031) | the text format is a strict subset; import tracks the published version |

## 18. Licences

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
