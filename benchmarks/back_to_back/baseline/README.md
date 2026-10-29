# Step 2's baseline: where the new engine's path stands

Measured on 29 October 2026 by one command, `python -m benchmarks.back_to_back
all OUT` (with `LSIM_SCRATCH`, `LSIM_JUDGE_CACHE` and, for the ENERGY line's
E3, `LSIM_FAST_BIN`, fast mode's runner built from this tree:
`cargo build --release -p lsim-project --example fast`), every run and probe
at commit `cbf7f39` with a clean tree: the seventh measurement, the first
after step 2's evidence part (items 1 to 3 and review findings 34 to 36:
`bad15c5`, `7b05396`, `6a2b22c`, `cbf7f39`). Every raw record and probe
result carries that stamp (commit, the module's version and the SHA-256 of
its compiled file, fast mode's runner's SHA-256, the files' SHA-256), and
[`report.md`](report.md), the harness's own report of this run, lists it.
The lines, cases, measures and pass marks are those of
[`../acceptance.toml`](../acceptance.toml), registered in `5c82fb0` before
the first measured run (eleven dated changes since, at its end; the last
three written before this measurement). The raw results (639 runs) are
outside the repository, in the lane's cache
(`judge-cache/step2/baseline-7`), each with its SHA-256 in `SHA256SUMS`.

Six measurements came before, all of the path before the wiring (the app's
code unchanged since `2b94af6`); the sixth (`95b945d`,
`judge-cache/step2/baseline-6`) is this one's reference below, and the
lists after it say how each earlier one moved.

## Since the seventh measurement (not measured again yet)

The evidence part's review found five blocking and four other defects;
`aab11d9`, `f7889c0` and `5e5e885` fix them (the lane's notes,
`lanes/step2-notes.md`, say which and with what test). They change how
the lines read, so the seventh measurement's numbers below are of the code
before them: by the judge as it is now, its ENERGY is **vacuous**, not a
pass (E3 met no limit on any item), FAULTS lists one more number, the
flips (today's engine passes, the new path warns or fails), and META's
set has two more items (bev-car's city cycle with a checked battery limit,
over 0.5 s and over none), which the seventh measurement did not run.
Previews at `5e5e885`, not measurements (`lane-step2/fix6`): META the
same verdict at all four settings on all nine of its set, judged;
FAULTS on the seventh measurement's runs of today's engine: no fault
hidden, the one violation of before (bev-car's Driver P gain 0: the two
Driver laws differ) and one flip, bev-car's E-Motor scaled 100 times,
whose new run says its charging was held back (the new engine's Driver
asks it for up to 536 kW of recuperation where the battery takes 120 kW,
131 kJ lost; today's Driver never asks more than the supply takes, a
difference of the Driver laws written in `docs/KNOWN-LIMITS.md`); the
enforced cap read as broken before is no longer a flip.

## The seventh measurement: the path wired to the run's evidence

The new engine's path now returns today's whole result, made by today's own
code from the run's evidence (`backend/app/solver/evidence.py`,
`new_engine.py`; DESIGN.md 12.3): the run verdict, the part energies and
the energy report, the duty, what held each driveline back, the references
and the hand calculations. Against the sixth measurement:

* **B2B**: of 1 302 differences on the 12 items both engines ran, **none is
  class a** (341 before: the reports, the verdict, the notes and the
  messages the path left out), 95 b, 465 c and 742 unclassified (270
  before). The two the path still left out are written now (an
  acceleration test's estimates note and a battery held at its power cap);
  the start-guess notes are notes (`engine-preparation-notes`, c, 24). The
  unclassified grew by the fields now compared: of the 742, 270 are the
  channels' differences of before (68 on the BEVs' gears' statistics, 202
  on the hybrid's single runs, as listed below), and 472 are in the
  reports:
  * 237 part energies and 200 energy-table and Sankey rows: 193 on the
    gears (the BEVs' final drive and differential, the hybrid's gearbox,
    clutch, final drive and differential: today's rule puts the gear's
    efficiency on the torque of the sources above the mesh, the inertias
    not taken out, so the gears' power, losses and flows each way differ
    by 1.0 to 61 bands, as their channels do, which `gears-today-rule`
    explains on channels only), 197 on the wheels and brakes (their flows
    each way from the run's 0.1 s trace of their ports, where today books
    every solver step: 1.0 to 411 bands, the free wheels' small flows the
    largest), 33 the energy report's remainder and balance error (the new
    engine's books close to about 1e-10 of their throughput, today's to 0
    by construction), 3 the gearbox's *gear shifts* term today's books
    name and the new path does not (the shift's loss is in the gearbox's
    losses), and the rest on the batteries, motors and the Vehicle (its
    peak, mean and RMS power from the trace);
  * 25 duty rows: the acceleration test's minima (today's sampling starts
    after its first step, the new path's extremes include the start: its
    battery's power 4.28 kW against 5.30 kW), the hybrid's regeneration
    peaks (1.5 to 5.4 bands), the battery's *Losses* (all its internal
    losses on the new path, R0's alone on today's) and one motor's losses'
    mean (1.55 bands);
  * 9 limit lanes: the hybrid's tyres at their grip for 0.9 s against
    today's 0.012 s on the UDDS, its braking 87.7 s against 87.0 s on the
    HWFET, a lane's first 8 ms, and the order of states where those move;
  * one reference's percentage (the hybrid's fuel consumption against its
    reference: -0.366 % against -0.363 %).
  Every item's ledger balances.
* **FAULTS**: of the 129 planted faults today's engine fails or warns on,
  **none is hidden** (73 before, all by the start-guess note) and one is a
  violation: bev-car with its Driver's P gain at zero, which today's
  engine runs 335 of its 601 points off the band (*cycle not followed*,
  17 km/h off) and the new engine within 1.6 km/h of the target (success):
  its Driver is LimPID's PI with back-calculation anti-windup (DESIGN.md
  12.2), whose integral alone follows the cycle; today's clamps its
  integral. Not a fault the verdict misses: the two Drivers are different
  laws, and the verdict reads each run's own trace. (The one violation of
  before, the Formula Student car with its E-Motor deleted, now warns as
  today's does.)
  The new path refuses 59 faults itself, as before (NEW-FAILED 43,
  NEW-BUILD 16). mutmut on `verdict.py` (which grew by the exact band):
  1 097 mutants, 734 killed, 348 survived, 14 with no test, 1 timed out;
  none of the survivors has a written reason in `../survivors.toml` yet, so
  the line fails on them too.
* **META passes**: the same verdict at the output steps 0.1, 1 and 5 s and
  at rtol 1e-8 for all 7 runs, and not vacuous (3 of them fail and 1
  warns: the verdict judges them).
* **ANALYTIC passes**: the five traces' time outside both bands within
  2.7e-14 s of the exact answers (0.1 s before), and the "cycle not
  followed" message labels its 1 %/2 s allowance as LightSim's own rule.
* **ENERGY passes**: E1, the part energies' losses and stored energies
  against the engine's books, within 5e-16 of the throughput on all 12
  items; E2, the held sine's battery power RMS 5.4313902456001095 kW
  against the exact 5.431390245600108 kW (3e-16); E3, fast mode's flags
  against the lanes' stretches at a limit, on all 8 BEV cycle items, the
  same: neither side meets a limit on any of them (no flag, no limited
  stretch), so E3 shows agreement only on nothing being limited. A probe
  outside the registered cases (bev-car's city cycle with its battery's
  power held to 15 kW, `lane-step2/evidence/cap15-e3.log`) does meet one:
  fast mode flags 24.70-30.00 s and 126.56-150.00 s, the new path's lane
  is at the set limit 24.39-31.60 s and 125.92-170.20 s, its stretches'
  starts within 0.7 s, its ends up to 20 s later, as a car that fell
  behind the cycle catches up at its limit (fast mode follows the cycle by
  construction): E3's pass mark would fail it, for that reason.
* **STATUS**: 5 of 11 (the energy report, part energies, duty, limits,
  references; the verdict counts once FAULTS has no violation).

Every other line is as at the sixth measurement: STRICT's 74 refusals with
the same four items refused that should run, SCHEMA's 0 errors in 631,
STREAM (the live runs refused; S2 on a shared machine: today's p95 0.17 s
live and 0.59 s for the study; S3 the same SHA-256, no engine fields),
FMU, TEXT, QUICK, SCRIPTS 1 of 3 and NOSKIP (155 tests, none skipped
but the two Windows-only ones; with the module hidden 6 fail, none
skips). After the measurement `51e3585` made a battery check with no
averaging window a plain result (its passed flag had been NumPy's bool,
which no result could be written with); no run here has such a battery
(the examples' power limit, fs-electric's, averages over 0.5 s), so these
records stand for that code too.

The machine: the owner's PC, WSL 2 (Ubuntu 24.04) on an AMD Ryzen 9 7900X,
Python 3.11.17, `lightsim_engine` 0.1.0 built from this tree (release),
jsonschema 4.26.0, mutmut 3.8.1. The stop latencies are an indication
only, from a machine another lane shares (their pass marks are judged on
Windows CI, `backend/tests/test_stop_latency.py`).

## Where each line stands

| line | state | what was measured |
|---|---|---|
| B2B | fails | 20 of 24 items as expected: the two live cases (NEW-LIVE), the performance probe (NEW-CASE-KIND) and the FMU probe (NEW-FMU) are refused where they should run. Of 1 302 differences on the 12 items both engines ran: 0 class a, 95 b, 465 c, 742 unclassified. Every item's ledger balances. |
| STRICT | fails | only for the four items above. 74 refusals, every one with a listed code: NEW-FAILED 46, NEW-BUILD 16, NEW-BALANCE 3, NEW-LAP 2, NEW-LIVE 2, NEW-CASE-KIND, NEW-CELLS, NEW-FMU, NEW-FS-EVENT and NEW-MISSING one each; no NEW-ERROR, no process that died. |
| SCHEMA | passes | 0 errors in 631 results. |
| FAULTS | fails | Of the 268 planted faults today's engine fails or warns on 129; none is hidden by a note; one comes out "success" on the new path (bev-car's Driver with no P gain, which the new engine's Driver follows the cycle without: the two Drivers' laws differ). mutmut: 1 097 mutants of verdict.py, 348 survive, none with a written reason yet. |
| META | passes | the same verdict (status, notValid rows, passed flags) at every output step (0.1, 1, 5 times the case's) and at rtol 1e-8 for all 7 runs; 3 fail and 1 warns. |
| ANALYTIC | passes | the verdict's time outside both bands within 2.7e-14 s of the exact answers on all five traces; the "cycle not followed" message labels its 1 %/2 s allowance LightSim's own. Both sources' copies are the pinned ones. |
| ENERGY | passes | E1: the part energies' losses and stored energies against the engine's books within 5e-16 of the throughput, all 12 items. E2: the held sine's battery power RMS within 3e-16 of the closed form. E3: fast mode's flags and the lanes' stretches at a limit the same on the 8 BEV cycle items, none of which meets a limit on either side. |
| STREAM | fails | S1: live runs refused (today's engine streams all 601 points of both live cases, equal to its result). S2: the live run is refused, and the study's stops reach no running point on the new path (its points are done in about 1.3 s): not measured; today's p95 0.17 s live, 0.59 s for the study (a shared machine). S3: a replay gives the same SHA-256 on both engines, but RunSnapshot records no engine, version or solver options. |
| FMU | fails | refused (NEW-FMU): nothing to compare with fmusim or FMPy. |
| TEXT | not available | the app reads no text components. |
| QUICK | not available | a case has no quick-mode option. |
| SCRIPTS | fails | one of three: on the new path today's worker serves every step of the hybrid's Script part in all three single runs; no decision record, no page for the owner. |
| NOSKIP | passes here | with the module: 155 tests, none skipped but the two Windows-only stop-latency tests; with it hidden: 6 fail, none skips; CI's Windows engine job runs them with LIGHTSIM_REQUIRE_NEW_ENGINE=1 (to be confirmed on Windows CI after the merge). |
| STATUS | fails | 5 of 11: the energy report, part energies, duty, limits and references; not yet the verdict (FAULTS has a violation), live streaming, study stop and progress, FMU parts, text components, quick mode. |

## Before the wiring: the six measurements

The rest of this page is the account of the first six measurements, of
the path before its evidence part; the sixth's tables are its own.

The sixth measurement was made on 28 October by the same command (with
`LSIM_SCRATCH` and `LSIM_JUDGE_CACHE` set), every run and probe at commit
`95b945d` with a clean tree: the judge with its checker's ten defects, its
re-check's eight, its third re-check's three, its fourth re-check's three
and its fifth re-check's four fixed, the app's code unchanged since
`2b94af6`, the acceptance file with eight dated changes. Its raw results
(631 runs, about 4 GB) are in the lane's cache
(`judge-cache/step2/baseline-6`); its `report.md` and `SHA256SUMS` are this
folder's at `e0b81e5`.

Five measurements came before. The first (runs at `2b94af6`, report at
`ef13ba0`; `judge-cache/step2/baseline`, records not stamped) and the second
(`8887593`, after the checker's ten defects; `judge-cache/step2/baseline-2`)
agree on every line to the count, the second reading more (every field,
the runs' own Script runners, the message a run writes, the sources'
pinned copies). The third (`f735ea0`, after the re-check's eight;
`judge-cache/step2/baseline-3`), the fourth (`a80a2d6`, after the third
re-check's three; `judge-cache/step2/baseline-4`) and the fifth (`581139f`,
after the fourth re-check's three; `judge-cache/step2/baseline-5`) moved
as the lists below say. Against the fifth, this sixth one moved only here:

* **B2B**: 1 253 differences became 1 147: class c 562 became 441 and the
  unclassified 255 became 270. A channel only the new run records that the
  project's data bus wires to a port today's runs record (a monitor's or a
  driver's input, a brake's or the E-Motor's demand, the hybrid's
  controller inputs: 169 such channels on the 12 items) is now compared
  with that port's channel (`diff._lend`), where the fifth classed it c
  (`new-records-more`) and never compared it; its label is compared with
  the one today's engine gives the port, its unit with the port's. The 169
  "extra" (c) went, and 63 differences came on those channels, each one its
  source's own, the same class and size: 48 first points (c; the BMS
  monitor's current, the E-Motor's demand on the acceleration test, the
  hybrid's gearbox gear input) and 15 values, minima and maxima on the
  hybrid's single runs (unclassified: the gearbox's gear input, the
  controller's motor speed and traction, the clutch's engage and the
  engine's on inputs, the E-Motor's demand; 1.00 to 9.4 bands, as their
  sources' differences are). No label or unit differs. `new-records-more`
  keeps 33: the battery's two current limits on each item and the track's
  nine signals, which no recorded port is wired to. Nothing else moved:
  every other difference of the fifth is here, with its class and size.
  The acceleration test's grid is still b, its last times the distance
  channel's own (4 s and 3.9994 s). Every item's ledger balances, with
  10 045 to 137 080 leaves "by source"; none says "10 ms only" (today's
  10 ms run says no other text or flag than its 1 ms run anywhere both and
  the new run have, and flags nothing they do not), and the gaps are the
  fifth's.
* **NOSKIP**: 153 tests (the harness has 10 more); with the module hidden
  6 fail, none skips.
* **STREAM** S2 (a shared machine): today's p95 0.09 s live and 0.48 s for
  the study, against 0.10 s and 0.45 s.

Every other line is the same to the count, every fault's row included.

Against the fourth, the fifth moved only here:

* **B2B**: 941 differences became 1 253, class c 250 became 562: the 312
  new ones are the first points (values, minima, maxima and means of 78
  channels on the 12 items), now each a figure of its own, and every one
  class c (`today-first-point-before-its-step`): today's 0, or its start
  value, recorded before its first step, and the new engine's first value
  between today's first two values, within the channel's band (16 of them,
  all on the acceleration test, lie outside those two values by more than
  their own band, inside only by the channel band's slack: the E-Motor's
  losses, the front axle's load and both front wheels' normal loads, each
  with its minimum, maximum and mean). Nothing
  else moved: every difference of the fourth is here, with its class and
  its size. The acceleration test's point at the line, which today's 10 ms
  run does not have (its list ends one point sooner) and which the fourth
  never compared, is compared on all its channels now, and no channel of
  that item moves out of its band (its first points aside, the item has
  no value difference). The gear rule keeps its 93 b; its 32 minima and maxima
  lie, at their worst output interval, 0.062 band outside the law's range
  there (widened by the law's error at the interval's ends), against the
  one band now asked (`diff._interval`). Every item's
  ledger balances (163 341 to 766 704 leaves an item; `report.md` has
  each one's table); two have gaps, leaves only today's 10 ms run gives:
  the acceleration test 5 (a third change of state of the E-Motor's limit
  lane and its seconds, and a "Solver step reduced" note) and the hybrid's
  Mixed Cycle 8 (two tyres' slip sinks of the energy report).
* **NOSKIP**: 143 tests (the harness has 46 more); with the module hidden
  6 fail, none skips.
* **STREAM** S2 (a shared machine): today's p95 0.10 s live and 0.45 s for
  the study, against 0.09 s and 0.36 s.

Every other line is the same to the count: STRICT's 74 refusals, SCHEMA's
0 errors in 631, every fault's row and mutmut's 935 mutants (608 killed, 2
timed out, 325 surviving), META, ANALYTIC, ENERGY, S1, S3, FMU, TEXT,
QUICK, SCRIPTS and STATUS.

Against the third, the fourth moved only on B2B, and by two differences:

* **B2B**: b 97 became 95 and the unclassified 253 became 255. The
  gear rule now also reads the new run's own gear channel against the new
  engine's law rebuilt from the same sources (`diff.teeth`), and the
  hybrid's UDDS differential torques' minima (sides a and b, 1.00 × band)
  do not stay within the inertias' term plus one band of today's rule
  (1.08 against 1.04 bands), so they are unclassified now. Every other b
  of the rule passes: the values and means within 0.84 × band of the
  rebuild, the minima and maxima holding their values' ends. The
  acceleration test's last output time is still b: 400 of its 401 times
  are today's, and the new run's last, 3.9994 s, is inside today's last
  step. No list of the new run is shorter or longer than today's. Every
  other difference was the same, difference for difference, and every
  other line the same to the count, but NOSKIP (97 tests, 13 more in
  the harness; 6 fail with the module hidden, none skips) and the stop
  latencies of a shared machine (today's p95 0.09 s live, 0.36 s for the
  study).

Against the second, the third moved here:

* **B2B**: 1 019 differences became 941, class a 419 became 341. A duty
  part the new path leaves out is now one difference, as a part energy or a
  channel is, not one for each of its rows: 105 rows of 27 parts (the
  matcher of duty parts now takes the parts of both runs, so a part with no
  rows is compared). Eight messages keep their counts and classes under new
  keys: a quoted name now keeps its numbers (five of today's "Case 'WLTC
  Class 3b' solved" lines, class c; the hybrid's three start-guess notes
  naming '12 V Loads', class a). b 97, c 250 and the 253 unclassified are
  the same, difference for difference. The new checks find nothing more:
  no point the new path leaves out (the first point included), no row
  repeated, and no number of a message to compare, since the new path
  writes none of today's messages.
* **SCRIPTS** a is the same, now shown by counts: today's worker served
  every step the engine asked of the Hybrid Control Unit (136 901 on the
  UDDS, 76 501 on the HWFET, 60 001 on the Mixed Cycle), in a process other
  than the run's. b reads what a record holds now; there is none.
* **NOSKIP**: 84 tests (the harness has 35 more); with the module hidden 6
  fail (the new Script-runner test needs it), none skips.
* **STREAM** S2 (a shared machine): today's p95 0.09 s live and 0.26 s for
  the study, against 0.08 s and 0.44 s.

STRICT, SCHEMA, FAULTS (every fault's row, and mutmut's 935 mutants, 608
killed, 2 timed out, 325 surviving), META, ANALYTIC, ENERGY, S1, S3, FMU,
TEXT, QUICK and STATUS were as in the second, to the count, and are so in
the fourth, the fifth and the sixth.


## Where each line stood at the sixth measurement

| line | state | what was measured |
|---|---|---|
| B2B | fails | 20 of 24 items as expected: the two live cases (NEW-LIVE), the performance probe (NEW-CASE-KIND) and the FMU probe (NEW-FMU) are refused where they should run. Of 1 147 differences on the 12 items both engines ran: 341 class a, 95 b, 441 c, 270 unclassified. Every item's ledger balances. |
| STRICT | fails | only for the four items above. 74 refusals, every one with a listed code: NEW-FAILED 46, NEW-BUILD 16, NEW-BALANCE 3, NEW-LAP 2, NEW-LIVE 2, NEW-CASE-KIND, NEW-CELLS, NEW-FMU, NEW-FS-EVENT and NEW-MISSING one each; no NEW-ERROR, no process that died, every pre-run fall-back message with its code. |
| SCHEMA | passes | 0 errors in 631 results (every run of the harness that gave a result). |
| FAULTS | fails | Of the 268 planted faults today's engine fails or warns on 129; 74 of those would come out "success" on the new path: 1 says so outright (fs-electric's E-Motor deleted), 73 say "warning" only because of the engine's preparation notes (finding 36). mutmut: 935 mutants of verdict.py, 325 survive, none with a written reason yet. |
| META | vacuous | the same verdict at every output step and tolerance for all 7 runs, because the new path makes none: no notValid, no passed flag, and every "warning" is a preparation note. |
| ANALYTIC | fails | the verdict's time outside the band is off by up to 0.1 s against the 1e-6 s mark (it adds whole 0.1 s samples), and the "cycle not followed" message the verdict writes for the plateau excursion does not say the 1 %/2 s allowance is LightSim's own. Both sources' copies are the pinned ones. |
| ENERGY | cannot be measured | no part energies, duty or limits on the new path (E1, E2, E3); fast mode is not reachable from Python (E3). Today's engine gives the held sine's RMS to 8e-16. |
| STREAM | fails | S1: live runs refused (today's engine streams all 601 points of both live cases, each with every channel, equal to its result). S2: the live run is refused; the study's stop reaches no running point in any of 20 stops (not measured). S3: a replay gives the same SHA-256 on both engines, but RunSnapshot records no engine, version or solver options. |
| FMU | fails | refused (NEW-FMU): nothing to compare with fmusim or FMPy. |
| TEXT | not available | the app reads no text components. |
| QUICK | not available | a case has no quick-mode option. |
| SCRIPTS | fails | one of three: on the new path today's worker serves every step of the hybrid's Script part (the Hybrid Control Unit) in all three single runs, in a process of its own; no decision record, no page for the owner. |
| NOSKIP | passes here | with the module: 153 tests, none skipped but the two Windows-only stop-latency tests; with it hidden: 6 fail, none skips (before `edf809e` the tests of the time skipped 3 and passed the rest); CI's Windows engine job builds the module and runs them with LIGHTSIM_REQUIRE_NEW_ENGINE=1 (to be confirmed on Windows CI after the merge). |
| STATUS | fails | 0 of 11 of the things step 2's goal names are on the new path. |

## B2B at the sixth measurement, in detail

The 12 items both engines ran (the BEVs' and the aerodynamic BEV's cycles,
the acceleration test, the hybrid's three cycles as single runs) and their
differences by class and rule (`../classes.toml` has each rule's reason and
check). Each item's ledger accounts for every leaf of the union of its three
runs (a number, a text, a flag or a missing value, by its path): compared
where today's 1 ms run and the new run both have it (by source, where it is
a leaf of a channel only the new run records, compared with the port the
data bus wires to it; 10 ms only, where today's 10 ms run says another text
or flag there: none), reported where one of them lacks it, a gap where only
today's 10 ms run has it; all 12 balance.

| class | rule | differences |
|---|---|--:|
| a | new-path-lacks-reports: no part energies, energy report, duty, limits or references (a duty part counted once) | 306 |
| a | preparation-note-as-warning, preparation-note-status: a start-guess note shown as the run's warning, and the status it makes | 24 |
| a | new-path-lacks-verdict: a check row's limit and passed flag | 7 |
| a | new-path-lacks-messages: today's run notes not written (an acceleration test's "estimates" note and its power-limit note, a typed profile's note) | 4 |
| b | gears-today-rule: gear torque, power and losses, reconciled by golden/definitions.py (today's rule) and diff.teeth (the new engine's law), both rebuilt from the new run's own sources | 93 |
| b | bev-city-recuperation: the BEV's recuperated energy on the city cycle, 1.0039 × band | 1 |
| b | acceleration-ends-at-the-line: today ends at 75.306 m (the end of its step), the new engine at 75.3000007 m | 1 |
| c | new-records-more: channels only the new engine records, with no port today records wired to them (the battery's current limits, the track's signals) | 33 |
| c | today-integrator-checks: today's two energy-balance rows | 24 |
| c | engine-note, today-run-summary: which engine ran it; today's fixed-step summary | 24 |
| c | today-first-point-before-its-step: a channel's first point, which today records before its first step, the new engine's between today's first two values, within the channel's band (16 outside the two values by more than their own band) | 360 |
| | unclassified | 270 |

Where the 270 unclassified differences sit (`where` in the report: how many
of a channel's worst points must go for the rest to be inside its band):

* **202 on the hybrid's single runs** (channels' values 38, minima 71,
  maxima 68, means 25: speeds, slips, gear, clutch and wheel torques; 15 of
  them on inputs wired to channels among the others, the same differences
  again under the input's key). 164
  of the 185 of the third measurement sit on at most 5 % of the output
  points, half on 5 points or fewer, at the gear shifts and engine starts;
  the two more are the UDDS differential torques' minima at 1.00 × band,
  which the gear rule's bound no longer explains (above). The golden comparison runs
  the same engine bit for bit and finds all 88 of these channels inside on
  the UDDS and the HWFET (`lsim-project/golden/results.md`), comparing an
  output time that falls on an event with the value just before it, as
  today's engine records it; the app's path gets no such value from the
  module (its `Run` gives one value at each output time). That is the
  likely cause, named, not shown: showing it needs the module to export the
  left limits (step 2's evidence part).
* **68 on the battery-electric cars** (the final drive's and the
  differential's power and losses, minimum, maximum and mean over each
  output interval, on 1 167 points in the median): the gears' rule again
  (their values are class b), but today's rule cannot be rebuilt on a
  statistic of a product (power is torque times speed: the rule applied to
  the sources' statistics reproduces today's own statistics only to 6 to 76
  bands), so `gears-today-rule` cannot show it; and five on a brake's
  minimum power and the E-Motor's losses on the UDDS (1.01 to 2.57 bands,
  1 to 87 points).

## FAULTS at the sixth measurement, in detail

| | deletions | parameter faults | all |
|---|--:|--:|--:|
| planted | 160 | 108 | 268 |
| today's engine fails or warns | 76 | 53 | 129 |
| the new path says success | 1 | 0 | 1 |
| ... or only a preparation note's warning | 38 | 35 | 73 |

The new path's runs carry the engine's preparation notes (START-NOT-SOLVED
on 243 of the 268 faults, START-ALIAS-CONFLICT on 238, INIT-START-ASSUMED on
46); the last two are warnings, and the path shows them as the run's, so 208
of the 209 faults it ran end as a "warning", whatever the fault did:
the BEV with its E-Motor deleted, which today's engine fails ("did not drive
the cycle: 0.00 of 7.29 km"), comes out as a warning about the battery's and
the Power Consumer's start voltages. 51 of the 73 hidden faults have a Data
Checks error, so the app would not run them anyway; 22 would reach the run.
The new path refuses 59 faults itself (NEW-FAILED 43 with the engine's own
codes, NEW-BUILD 16 from today's model builder, which the path calls first),
four of them where today's engine says success: three deleted negative wires
(ELEC-NO-GROUND, STRUCT-OVER; today's engine grounds them implicitly) and the
hybrid's engine at 100 times its torque (RUN-EVENT-STORM at the clutch).
mutmut on verdict.py: 935 mutants, 608 killed, 2 timed out, 325 survive:
in `_acceleration` 87, `judge` 51, `distance_trace_metrics` 48,
`terminal_checks` 37, `trace_metrics` 32, `CycleTrace.sample` 24,
`beyond_data` 15, the rest 31. None has a written reason in
`../survivors.toml` yet, so the line fails on them too. The tests:
test_verdict.py without its minute-long bundled-examples test,
test_distance_profile.py, test_map_edges.py, test_acceleration.py,
test_run_warnings.py and test_power_limit.py; each test process is held to
2.5 GB (one of `_RangeMinMax`'s mutants grew a list to 5.5 GB before the
cap was there).

## Beside the recovered baseline

The first attempt at this judge (21 October, on the machine that was lost)
left only its last report (`integ/recovered-judge-report.txt`). Line by line:

| line | then | now | why they differ |
|---|---|---|---|
| B2B | 22 of 25 items as expected; 1 167 differences: 462 a, 253 b, 277 c, 175 unclassified | 20 of 24; 1 147: 341 a, 95 b, 441 c, 270 unclassified | This run compares each channel's first point as a figure of its own (360 differences, class c: today records it before its first step), and a channel only the new engine records with the port wired to it (its "extra" was class c: 202 then, 33 now; 15 of the unclassified are its sources' own differences again), where a comparison over "every point but the first", as the first attempt's most likely was, has none. Its 25 items are not on record, nor how it counted a missing report's rows (this run counts a duty part the new path leaves out once: one per row, it had 1 019 and 419 a); this run has 24, one of them an FMU probe that step 2's goal says should run, so one more is refused wrongly (NEW-FMU). Fewer b: its rules are lost, and this run classes a statistic of a gear's power only where today's rule can be rebuilt on it, so 68 such stay unclassified on the BEVs; and this run's gear rule also reads the new run's own gear channel against the new engine's law. The hybrid's unclassified (202, 187 of them on channels today records, against its 153) sit as its did, on a few points at the shifts, with the same likely cause. The two b figures it found are found again to the digit: 75.306 m against 75.3000007 m, and 1.0039 × band. |
| STRICT | fails only for the three items wrongly refused; 68 refusals, all with listed codes | fails only for the four wrongly refused; 74 refusals, all with listed codes | the FMU probe; a different parameter-fault set |
| SCHEMA | 0 errors in 540 results | 0 errors in 631 | more runs (the META runs, the sine probe) |
| FAULTS | 244 planted (160 deletions and 84 parameter faults, 21 parameters × 4); 68 flagged by today's engine and "success" on the new path: 1 shown, 67 hidden; mutmut 935 mutants, 325 survive, none with a reason | 268 planted (160 deletions and 108 parameter faults, 27 × 4); 129 flagged by today's engine, 74 of them "success" on the new path: 1 shown, 73 hidden; mutmut 935 mutants, 325 survive, none with a reason | Its 244 is most likely the planted faults: its words ("68 of the 244 planted faults that today's engine fails or warns on") read either way, but 160 + 21 × 4 = 244 has this run's shape (160 + 27 × 4 = 268), while 244 flagged faults would need about 340 parameter faults at this run's rates (76 of 160 deletions and 53 of 108 parameter faults flagged). Its 21 parameters and its flagged count are not on record; the planted counts differ by six parameters, and the shape is the same: one shows, the start-guess note hides the rest. 16 faults come from today's model builder (NEW-BUILD), as then. The same 935 mutants and the same 325 survivors' count, on tests chosen afresh (the first attempt's list is not on record). |
| META | vacuous | vacuous | the same: the new path makes no verdict |
| ANALYTIC | off by up to 16.7 s against 1e-6 s; no label | off by up to 0.1 s; no label | Its traces are not on record. 16.7 s is this run's slow car's whole time outside (16.7125 s): its exact answer most likely took the regulations' allowance for a car that cannot keep up at full throttle (40 CFR 1066.425(b)(4)(ii), WLTP 2.6.8.3), under which none of it counts; this run's pass mark judges the time outside the limits themselves ((b)(1)-(2), 2.6.8.3.1), which the verdict reports, and states the allowance beside it |
| ENERGY | cannot be measured | cannot be measured | the same |
| STREAM | live refused; a study's stop does not reach running points (14 of 20 stops); replay same SHA-256, no engine, version or options | live refused; the stop reaches no running point in 20 of 20; replay same SHA-256, no engine, version or options | this run counts a point cancelled at t = 0 (its worker saw the stop before it started) as reaching nothing |
| FMU | refused | refused | the same |
| TEXT, QUICK | not available | not available | the same |
| SCRIPTS | one of three | one of three | the same |
| NOSKIP | passes here; CI to confirm | passes here; CI to confirm | the same |
| STATUS | 0 of 11 | 0 of 11 | the same |

## What could not be done here

At the seventh measurement:

* **E3** is measured on the registered cases only where nothing is
  limited: none of the eight BEV cycle items meets a limit on either side.
  The probe beside it (a 15 kW battery cap) shows the lanes' stretches
  starting with fast mode's but ending later, the forward car catching up
  with a cycle it fell behind: by its pass mark E3 would fail there, for a
  reason of the two modes, not of the report.
* **The duty's RMS** comes from the recorder's integral of the line
  through every point the integrator visited, exact for a value held
  between events, not from quadratures under the integrator's error
  control (the energy books are those); **a wheel's and a gear's flows each
  way** and a clutch's and the Vehicle's peak, mean and RMS power come
  from the run's 0.1 s trace of their ports (the engine's books keep a
  part's net power only).
* **A table guard** of the engine counts a read wherever its expression is
  evaluated, also in a branch the run does not take: the E-Motor's drag
  table, read while the motor is powered, says it ran past its data where
  today's engine counts only the operating point it used (a probe with
  the drag table cut at 6 000 1/min: 548 s outside on the WLTC, today
  none). No example is affected; fixing it is the engine's (its guards on
  the branch taken).
* **mutmut's survivors** (348) still need their written reasons, and
  **Windows CI** still has to run NOSKIP and the stop latency.

Before:

* **The WLTP's text** comes from legislation.gov.uk's copy of Regulation
  (EU) 2017/1151 (Annex XXI, Sub-Annex 6, 2.6.8.3.1; its URL and SHA-256
  pinned in acceptance.toml), the retained UK copy, not EUR-Lex's
  consolidated text: EUR-Lex answered HTTP 202 with an empty page, as it
  did for the first attempt.
  The roadmap's "1.2.6.6" (not yet confirmed) is 2.6.8.3.1 in this copy;
  UN R154 was not opened.
* **The FMU line** needs Reference FMUs v0.0.41 and fmusim; nothing reaches
  them while the path refuses FMU parts, so neither was fetched for this
  baseline (the backend's CI uses v0.0.39; the first attempt found the
  v0.0.41 asset answering 404).
* **Stop latency on Windows** is for CI after the merge; the numbers here
  (today's engine: p95 0.09 s live, 0.48 s for the study; 0.10 s and 0.45 s
  at the fifth measurement, 0.09 s and 0.36 s at the fourth, 0.09 s and 0.26 s at the third, 0.08 s and
  0.44 s at the second, 0.10 s and 0.36 s at the first) are from a shared
  Linux machine.
