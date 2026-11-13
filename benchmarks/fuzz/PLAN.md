# Fuzzing the engine's inputs: the plan

Step 3, item 4(c) of the roadmap. Written and committed before the first
fuzz run; what the runs found is reported against it, not the other way
round.

## The one rule

Every input ends, within the budget below, in a **result** or a **coded
refusal**. Never a panic, a hang or an abort, and never a NaN or an
infinity reported as a success.

## What counts as a failure

An input fails when any of these happens:

1. a Rust panic anywhere (the Python module would turn it into
   `ENGINE-FAULT`, but that is still a fault of the engine);
2. an abort or a signal: stack overflow, a memory error the address
   sanitizer sees (in the engine, SUNDIALS or the maths library), an
   illegal instruction;
3. the input takes longer than its time budget (a hang);
4. the input takes more memory than its memory budget;
5. a run that ends without an error but has a NaN or an infinity in any
   recorded channel (its value, minimum, maximum or mean at any output
   time);
6. a refusal without a code or without a message: every error must carry
   a non-empty code (`LangError::code`, `Diagnostic::code`, or the
   `SolveError` kind the Python module maps to a `RUN-…` code) and a
   non-empty plain message.

A refusal of the engine's own limits (`RUN-TIME-LIMIT`,
`RUN-MEMORY-LIMIT`, the step limit) is a coded refusal and passes; the
reports count how often each happened.

## Budget per input

| | |
|---|---|
| wall time | 30 s (libFuzzer `-timeout=30`) |
| memory | 1536 MB resident (libFuzzer `-rss_limit_mb=1536`, address sanitizer included), no single allocation over 1024 MB (`-malloc_limit_mb=1024`) |
| a run inside the harness | `time_limit` 10 s and `memory_limit` 256 MB (the run limits this step adds); a short run: 1 s of model time for a text model, at most 2 s of a project case |

## The targets

All four are built with cargo-fuzz (libFuzzer, address sanitizer, debug
assertions on) and the `arbitrary` crate, in `engine/fuzz`.

| target | input | what it drives |
|---|---|---|
| `text` | any UTF-8 text | `lsim_lang::parse`, `parse_with` (standard library), `parse_library`; every definition printed back with `to_text` and parsed again; the last definition of a text that parses built (`Engine::build`) and run for a short time |
| `basemodelica` | any UTF-8 text | `lsim_lang::basemodelica::import`; a model it imports built and run for a short time |
| `project` | JSON bytes, or `arbitrary` edits of one of the four example projects (a value at a path replaced, a key removed, an element duplicated) | `serde_json` then `lsim_project::import_case` for one of the project's cases, then `Engine::build` and a short run (the app's path through the Python module, without Python) |
| `params` | `arbitrary`: an example project and case, and parameter values from {NaN, +Inf, −Inf, 0, −0, ±1e300, ±1e−300, 5e−324, f64::MAX, f64::MIN_POSITIVE, any f64} | changed at run time (`params_with`, the model built once per process), or put in the case's `parameterOverrides` and built again; then a short run |

The Base Modelica importer: the brief said it does not exist yet, but
`lsim_lang::basemodelica::import` does (commit b205ed4). It reads Base
Modelica text; the pipeline that makes that text from the Modelica
library (`omc --BaseModelica`) is the other lane's. So it is fuzzed here
as the fourth target.

## Seeds

A small seed corpus per target, made by `engine/fuzz/src/bin/seeds.rs`:
the standard library's components printed as text, the Modelica texts of
the importer's tests and the text format's documentation, the four
example projects (`backend/projects`). libFuzzer seeds: target `text`
3101, `basemodelica` 3201, `project` 3301, `params` 3401 (fork mode adds
the job number).

## CPU time

On CPUs 0-7 of this machine (two per target, at the same time):
`text` 0-1, `basemodelica` 2-3, `project` 4-5, `params` 6-7.

1. A smoke run of at most 30 minutes per target, to clear the shallow
   failures first.
2. After each fix, the regression test from the shrunk input, then the
   target runs again.
3. The main run: at least 4 hours of wall time per target in total
   (8 CPU-hours: two fork jobs). If the coverage count still grew in the
   last hour of a target's run, it runs 2 hours more, up to 12 hours of
   wall time per target.

## What each target's report gives

CPU-hours, executions, final corpus size (after `cargo fuzz cmin`),
failures found and fixed (each with its regression test), and the
rule-of-three bound: with no failure in the last N executions (those
after the last fix), the failure rate per input is probably (95 %) below
3/N.

## Fixes

Each failure is fixed at its root, in the engine, with a regression test
in the crate where the fault was (the shrunk input as the test's text),
not in the harness. The step 1 golden comparison must stay at 0
differences in 11 347 values.
