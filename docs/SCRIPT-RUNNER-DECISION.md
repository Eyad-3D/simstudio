# Which runs Script blocks: the decision record

A Script block holds a short piece of Python, `step(t, dt, inputs, state,
params)`, that LightSim calls every 10 ms of a run to turn its inputs into
outputs (the P2 hybrid's Hybrid Control Unit is one). Two programs can run
that code:

- **today's worker**: a separate Python process, locked down as far as each
  system allows without administrator rights
  (`backend/app/solver/sandbox_worker.py`, `scripting.py`);
- **the engine's own runner** (`engine/crates/lsim-script`): an interpreter,
  written in Rust, of a strict subset of Python that gives the worker's
  results to the bit.

This record holds what the owner needs to choose between them: the threat
model, the differential fuzzing against CPython 3.11, the count of unsafe
code the runner pulls in, and the gap that lets today's worker read files and
use the network on Windows (PLT-35). It makes **no switch**: every Script
block still runs in today's worker, in today's engine and on the new
engine's path (`new_engine.py` passes `native_scripts=False`; quick mode
refuses a project with a Script, `QUICK-SAMPLED`). The owner's plain-language
page is kept with the owner's decisions (`decisions/script-runner-explained.md`
in the owner's workspace). Measured on 6 November 2026 on work/step2.

## What each runner is

**Today's worker.** Three layers, each a backup for the one before:

1. *A check of the code before it runs* (`scripting.check_script`, also all
   that Data Checks do): imports other than `math`, names that start with two
   underscores, attributes that start with one or lead to the interpreter's
   frames, classes, and builtins such as `open`, `eval` and `getattr` are
   refused. At run time the code sees only an allow-listed set of builtins.
   Its own words: "This is NOT a security sandbox": Python cannot be fenced
   in from inside itself.
2. *A process of its own*, one per run, which drops what it does not need
   before it runs any code: on Linux 5.13 or newer Landlock forbids every
   file and, from 6.7, TCP; resource limits cap memory at 512 MB and file
   writes at 0 bytes. On Windows a job object caps memory, allows one
   process (no programs started), ends it with the engine, and the worker
   lowers itself to the low integrity level (it cannot change the user's
   files); its environment is cleared. On macOS Apple's sandbox stops
   writes, programs and the network.
3. *The engine's clock*: a call that has not answered within 2 s is stopped
   by killing the process, which also stops loops inside C code.

Projects from another computer run their scripts only after the person has
seen the code and chosen **Run scripts** (`script_trust.py`, by a SHA-256 of
each script's text).

**The engine's own runner.** It reads the script into its own syntax tree
and evaluates it: numbers, strings, lists, tuples, dicts, `if`, `for`,
`while`, functions, the builtins and math functions whose results are exact
or correctly rounded everywhere, `clamp` and `interp`. There is nothing else
in its language: no imports but `math`, no files, programs, network,
environment, string formatting, classes or exceptions; a name it does not
know is refused when the script is read. A script outside the subset, or a
call that meets something outside it (an integer beyond 64 bits, say), is
left to Python, so a result never differs from the worker's. It holds the
worker's limits from inside: 2 s per call (its clock read as the work goes),
512 MB of values counted as they are made, and limits on nesting and the
stack. It runs inside the engine's process, with no process of its own.

**What fits the subset.** Every Script we have: the hybrid's controller and
the eleven examples of the help's Script pages (`reference/script-api.md`,
`reference/script-cookbook.md`), 12 of 12 (`lsim_script::check`). Scripts
that use `math.sin`, `math.exp` or a power of a float (whose last bit
differs between C libraries), `sum`, `sorted`, string formatting or
exceptions go to Python.

## The STRIDE threat model

STRIDE is a checklist of six kinds of attack. The asset at risk is the
person's computer: their files, their network and other machines on it, the
machine staying usable, and the numbers a run gives. The attacker writes a
script and gets it run: a project shared by someone else, an example from
the web, or an AI agent working on a project with planted instructions. The
doors in are the script's text, its parameters, its inputs during the run,
and (for the worker) the channel between the engine and the worker. The
trust boundaries: the project file and the engine; the engine and the worker
process; the worker and the operating system.

| | Threat | Today's worker | The engine's own runner |
| --- | --- | --- | --- |
| **S**poofing | Another program poses as the worker, or a script as an approved one | On Windows the engine and worker talk over a pair of local sockets made by Python's `socketpair`, which in the newest Python 3.11 the app is built with checks that the other end is its own (the fix of CVE-2024-3219; read in 3.11.17's `socket.py`). Approval is by the exact text's SHA-256 | No channel: it runs in the engine. Approval as today |
| **T**ampering | The script changes the person's files, the engine's memory or other runs | Files: Linux no writes (Landlock, size limit 0), Windows low integrity level, macOS sandbox. The engine's memory: a separate process; it sends only framed numbers back | Has no way to name a file. Runs in the engine's memory, but safe Rust reaches only its own values: one `unsafe` line of its own (`unsafe impl Send for Runner`, its reason written beside it); the libraries it uses are counted below |
| **R**epudiation | Nobody can tell what a script did | Neither runner logs what a script does; the run's messages name a failing script and time. An AI app's calls, runs among them, are logged in `.ai/audit.jsonl` | The same |
| **I**nformation disclosure | The script reads the person's files or secrets and sends them away | Linux 5.13+: no files, and from 6.7 no TCP (UDP and local sockets stay open). **Windows: it can read every file the person can and use the network** once past the first layer (PLT-35, below). macOS: it can read files. This record found the first layer let a `.format` field walk into the worker's memory as text (`'{0.__globals__[sys].path}'.format(clamp)`: the module paths and the person's user name in them, the worker's arguments; no calls, and its environment is cleared); closed in this step (`scripting._format_problem`) | Nothing to read but its own inputs and parameters: no file, network or environment function exists in its language, and no attribute but the listed methods |
| **D**enial of service | The script hangs or fills the machine | 2 s per call (the process killed), 512 MB (resource limits or job object), no programs. A crash of the worker fails the run: the fuzzing found one (an output too large to be a number), fixed in this step | 2 s per call and 512 MB, counted inside. Because it runs in the engine, a flaw in those limits would hang or fill the engine itself, with no operating system cap behind it; the fuzzing found none (below) |
| **E**levation of privilege | The script gets more power than it was given | Past the first layer it is a full Python: on Windows it then reads files and uses the network; it cannot start programs or write (job object, low integrity). In-process Python sandboxes have a long history of escapes, which is why the second layer exists | There is no path from a script to the engine's own code: the runner evaluates its tree, with no `eval`, no calls into C and no imports. An escape would need a memory-safety flaw in the unsafe code counted below, or in the compiler |

**What a malicious author does.** A script's author chooses its code, so a
harmful script would be written outside the subset, and on today's design a
script outside goes to the worker. The engine's own runner therefore makes
the scripts it runs safe (and identical), but on its own it does not stop a
harmful script: what stops that is what happens to the scripts it cannot
run. That is the choice the owner makes (below).

## Differential fuzzing against CPython 3.11

`engine/crates/lsim-script/tests/fuzz.rs` writes random whole scripts
(module constants, 1-D and 2-D tables, a module list, helper functions with
defaults, keywords and `global`, `if`/`elif`/`else`, `for` over ranges,
lists, `enumerate`, `zip` and dicts, bounded `while` loops, `break` and
`continue`, early returns, list and dict methods, the block's `state` across
calls, every builtin and math function of the subset, containers compared
and built, values of the wrong type, and large floats with a fraction) and
runs each, 12 calls of inputs drawn from the special values (signed zeros,
subnormals, the largest floats, infinities, NaN), on the native runner and
on the app's worker as the app calls it (`tests/parity.py`: the worker's
`_compile` and `_run_step` on CPython 3.11.17, parameters typed by the
app's project model, Python stopped past the worker's 2 s). A divergence is
anything but the same 64 bits of every output under the same names in the
same order, the same message where Python raises, both refusing, or the
native runner leaving the script to Python.

- hours: 24.0, eight runs of 3 h side by side on CPUs 12–23 (seeds 101–108,
  at a024f5e, after the fix below), plus 0.3 h of shorter runs before them
  (seeds 1–3, 79 500 scripts), which found the one divergence
- inputs: 5 375 500 scripts, 54 915 038 calls (the eight long runs)
- divergences: 1 found, 1 fixed, 0 open; the long runs found none

| In the eight long runs | Count |
| --- | --- |
| calls with the same bits | 20 062 915 |
| calls failing on both with the same message | 33 623 303 |
| scripts left to Python when read | 33 539 |
| calls left to Python | 1 228 820 |
| scripts Python did not finish within 2 s (the app's run fails on them) | 2 573 |
| … of which the native runner left to Python | 2 572 |
| … of which the native runner finished every call (listed below) | 1 |
| native runner crashes | 0 |

**Found and fixed.**

1. *A float exactly halfway between its two shortest readings was quoted
   wrong.* `186323534390030.125` appeared in a message as `…030.13`; CPython
   (David Gay's conversion) takes the reading whose last digit is even,
   `…030.12`. About 7 in 100 doubles from 2^40 to 2^53 are such ties.
   Outputs were never affected (they are numbers, not text). Fixed in
   `value::float_repr`, with four such values in the parity test (31dacc3).
2. *Today's worker stopped on an output too large to be a number*
   (`{'y': 10 ** 400}`), and the run blamed memory; in-process it raised an
   unnamed `OverflowError`. Not a difference between the runners (the
   native runner leaves integers beyond 64 bits to Python), but a defect of
   the worker. Both now say "output 'y' is too large to be a number"
   (ffe0fca).

**Listed, not a difference in the language.** Each runner holds its own
2 s per call, read on its own clock, and the native runner is faster, so a
call near the limit can fail the run on the worker and finish on the native
runner: one script of the 5.4 million (`fuzz 106:447036`, nested helper
loops) took CPython more than 2 s in one call, while the native runner (a
debug build, with the eight runs sharing twelve CPUs) finished all twelve
calls in 2.6 s. A switch would keep this: a few scripts the worker stops
for time would finish on the native runner. The other way round cannot
fail a run: a call the native runner does not finish within 2 s is left to
Python (62 calls in the long runs).

**What it does not cover.** The fuzzing judges the subset against the
worker. It does not try scripts built to escape the worker (the threat model
covers that), nor the hosts around the runner (a run's own tests cover
those). The leaves (why scripts and calls went to Python) are counted in
each run's summary; the largest are integers beyond 64 bits and NaNs in
containers or as dict keys (where CPython matches by identity), which the
runner leaves to Python on purpose. Raw outputs: `judge-cache/step2/scripts`
in the lane's scratch folder (not in the repository).

## cargo-geiger count

cargo-geiger 0.13.0 (`cargo geiger` in `engine/crates/lsim-script`, Rust
1.94.1) counts the `unsafe` code in everything the runner is built from.
"Used" is unsafe code compiled into the build; the crates marked build-time
run only while it is compiled and are not in the shipped program.

- cargo-geiger, unsafe expressions in the runner's own code: 0
- cargo-geiger, unsafe items in the runner's own code: 1 (`unsafe impl Send for Runner`)
- cargo-geiger, unsafe expressions used by the run-time crates: 2881

| Crate | Unsafe functions | Expressions | Impls | Methods | What it is |
| --- | --- | --- | --- | --- | --- |
| lsim-script | 0 | 0 | 1 | 0 | the runner |
| lsim-ir | 1 | 2 | 0 | 0 | the engine's block interface (its floating-point control register code) |
| lsim-math | 0 | 42 | 0 | 0 | the engine's correctly rounded maths (calls into CORE-MATH's C) |
| serde, serde_core, thiserror | 0 | 5 | 0 | 0 | data types, error types |
| serde_json | 0 | 75 | 0 | 0 | reading the parameters as JSON |
| itoa | 1 | 145 | 0 | 0 | writing integers as text |
| memchr | 34 | 1992 | 2 | 110 | fast byte search (serde_json's reader) |
| zmij | 3 | 620 | 0 | 6 | writing floats as text |
| **run time** | **39** | **2881** | **3** | **116** | |
| proc-macro2, unicode-ident, syn, quote, serde_derive, thiserror-impl | 0 | 106 | 3 | 5 | build-time only |

For scale: today's worker runs on CPython, written in C, where every line is
outside what such a count can check.

## The PLT-35 gap

On Windows, today's worker can still **read any file the person can read and
open network connections**: its job object and low integrity level stop it
starting programs and writing the person's files, and nothing else. Only the
first layer, the check of the code, stands between a script and the
person's documents and the internet, and that check is a list of what is
refused, not a wall (this record found and closed one hole in it). Closing
the gap needs the worker started as an AppContainer process: a Windows
sandbox with no network and access only to folders granted to it.

**What step 6 changes** (branch work/ai, read on 6 November at dc8231f, whose
sandbox code is the same as at ceff770; not merged): it builds that AppContainer (`backend/app/solver/win_sandbox.py`,
`sandbox.STRICT`). The Script and FMU workers of a run started by an AI agent
through the AI connection, and since 6 November of **every** run of the
command-line tool (`lightsim run`, `lightsim-backend run`, a person's
included), start inside an AppContainer with no capabilities: no network,
reading only what LightSim grants (the runtime it needs, an FMU's folder)
and what Windows lets every app container read (Windows' own folders and
Program Files), writing nothing. The worker checks that it is confined
before it runs any code; where the AppContainer cannot be set up the run is
refused with the code `sandbox_unavailable`, never run less safely;
`LIGHTSIM_STRICT_SANDBOX=0` gives a person on the command line the app's
rules back. Setting one up and clearing it costs a second or two each (about
4 s each from a Python environment of 19 000 files), and runs at once set
theirs up one after another. Its Windows tests passed on CI (run
42175270483, the frozen engine's worker included). **Runs a person starts in
the app keep today's worker**, so for them the gap stays open after step 6.

## The choice

| Option | A harmful script, in a run a person starts on Windows | Numbers | Cost |
| --- | --- | --- | --- |
| A. Today's worker for every script (no change) | Can read files and use the network once past the code check | Today's | None |
| B. The engine's runner where it can, today's worker for the rest | The same as A: its author writes it outside the subset | The same to the bit (fuzzing above) | A switch with a fall-back to the worker; scripts on the runner need no process and are faster |
| C. The engine's runner only; a script outside the subset refused | Cannot read a file or use the network | The same to the bit | Scripts outside the subset (`math.sin`, `sum`, string formatting) stop running |
| D. Step 6's AppContainer for every run's worker, a person's too | Cannot read the person's files or use the network | Today's | 2–4 s more per run with a Script on Windows (set-up and clearing; one run measured 3.8 s in all); needs step 6 merged |
| **B + D**. The engine's runner where it can, the AppContainer worker for the rest | Cannot read the person's files or use the network | The same to the bit | The AppContainer's 2–4 s only on runs with a script outside the subset (none of the 12 we have) |

**Recommendation: B + D**, once step 6's AppContainer is merged; until then,
today's worker as it is (A). The engine's runner cannot reach a file or the
network at all, because nothing in its language can say it, and it gives the
same numbers to the last bit (no divergence in 24 hours and 5.4 million
scripts after the one fixed); the AppContainer closes the Windows gap for the few
scripts that still need Python, and costs its set-up time only on those
runs. B alone would not be enough, because a harmful script would simply be
written outside the subset; D alone would close the gap but charge every run
with a script the AppContainer's set-up, and keep every script in a second
process. What the switch would take, as its own step: the app's runs passing
`native_scripts=True` with a coded fall-back to the worker for a script
outside the subset (at its start, or at the call that leaves it, the run
started again on the worker); step 6's strict worker for that fall-back in a
person's runs too; and step 9's paid security review covering the choice.
