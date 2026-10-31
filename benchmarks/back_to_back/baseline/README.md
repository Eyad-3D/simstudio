# Step 2's baseline: where the new engine's path stands

Measured on 30 October 2026 by one command, `python -m benchmarks.back_to_back
all OUT` (with `LSIM_SCRATCH`, `LSIM_JUDGE_CACHE` and, for the ENERGY line's
E3, `LSIM_FAST_BIN`, fast mode's runner built from this tree:
`cargo build --release -p lsim-project --example fast`), every run and probe
at commit `dc8eac2` with a clean tree: the tenth measurement, after the
four fix rounds of step 2's evidence part (`aab11d9`, `f7889c0`, `5e5e885`;
`7e8ea6c`, `923c2b9`; `d4a6215`, `61779c8`; `c8c83e3`, `dc8eac2`). Every
raw record and probe result carries that stamp (commit, the module's
version and the SHA-256 of its compiled file, fast mode's runner's SHA-256,
the files' SHA-256), and [`report.md`](report.md), the harness's own report
of this run, lists it. The lines, cases, measures and pass marks are those
of [`../acceptance.toml`](../acceptance.toml), registered in `5c82fb0`
before the first measured run (eighteen dated changes since, at its end;
the last two written before this measurement). The raw results (691 runs)
are outside the repository, in the lane's cache
(`judge-cache/step2/baseline-10`), each with its SHA-256 in `SHA256SUMS`.

Nine measurements came before: six of the path before the wiring (the
app's code unchanged since `2b94af6`), the seventh (`cbf7f39`,
`judge-cache/step2/baseline-7`) of the path wired to the run's evidence,
before its fix rounds, the eighth (`923c2b9`, `judge-cache/step2/baseline-8`)
after the first two and the ninth (`61779c8`,
`judge-cache/step2/baseline-9`) after the third. The ninth is this one's
reference below; the sections after it say how the earlier ones moved.

## After the tenth: the runs measured again for the fifth fix round

The fourth re-check found two non-blocking defects, both in the path.
bev-car with the rear wheels at 0.1 % of the Vehicle's weight each (the
fronts 49.9 %) and a CG height of 0.5 m passes Data Checks and succeeds on
today's engine, and the path said the rear wheels lift off the road at
t = 0.00 s (with 0.01 % each today lifts at 30.75 s and the path said
0.00 s as well): the engine's first recorded point, which the trace and the
integrals' lowest value hold, is the state before the events of t = 0, in
which the Vehicle at rest still slides and moves a few N of load for no
time. And `value_problems`, which keeps a run from the path when it reads a
value outside its limits, had its branch for a part's own value (47 of
FAULTS' 53 refusals) covered by no test. `70c276c` tells a lift from the
trace's second point on, or at a Vehicle axle-load event after the first
traced stretch (a lift that begins and ends within the first step goes
unsaid; KNOWN-LIMITS and DESIGN say so), and tests the part's own value.

The tenth measurement stands as the baseline. Its runs were measured again
at `70c276c` on a clean tree, into `judge-cache/step2/baseline-11` (the 691
runs of items, faults, META and ENERGY's fast mode; the stop-latency,
replay, mutation and no-skip probes were not run again, since nothing this
round changed can move them). Against the tenth, record by record, the
whole result but the code's stamp and the run's wall time: the 315 `new`,
315 `today-normal`, 26 `today-fine` and 27 META records are identical, and
the 8 fast-mode records too but for the runner's own timing (`run_s`). No
line moves. The reason is that the path can only lose a lift warning by the
change, and no `new` record of the tenth measurement says a lift: the four
that do are today's engine on four faults the path does not run (a Vehicle
mass of 0 and -280 kg: `NEW-LIMITS`; a Wheelbase at a hundredth and a CG
height ×100 as case values: the engine's `RUN-EVENT-STORM`). The
comparison is lane-step2/fix10/raw1011.py.

## The tenth measurement: the evidence part's fourth fix round

The evidence part's fourth review found one blocking defect and two
others. A study's point can hold a value Data Checks refuse for a single
run (they check only a study's base project): today's engine reads many
such values clamped, the new engine read them as typed, so at -50 kPa the
new path ran a negative drag under today's warning, which gave today's
clamped density of 0, and at -300 °C it lost today's "Cycle not followed"
and four rows not valid; a CG height of -0.3 m ran too. A study point with
the rear wheels' load share at 0 % warned of a lift where today's engine,
which checks a lift only with a CG height or a Downforce Area, says
nothing; and two branches of the lift's time had no test. `c8c83e3` refuses
a run that reads a value outside its parameter's limits (`NEW-LIMITS`, in
Data Checks' words; it runs on today's engine, or in strict mode fails with
the code), gives the air density the run used in the Ambient's warning,
checks a lift only where today's does, and tests both branches. `dc8eac2`
lists the code and lets FAULTS' unsaid hold the rows today's run flags (a
row not valid, a check not passed), which the -300 °C point had lost with
the same status and words (two dated changes, before this measurement).
Against the ninth measurement, line by line:

* **B2B**: as before, to the difference: 22 of 26 items as expected, the
  same 1 498 differences in the same fields and classes (0 a, 117 b, 529
  c, 852 unclassified). No item holds a value outside its limits, so none
  is refused for it. Every ledger balances.
* **STRICT**: 119 refusals (76): the 53 planted faults whose value is
  outside its parameter's limits are `NEW-LIMITS` now; 10 of them the new
  engine refused before (`NEW-FAILED` 48 → 38), 43 it ran. The same four
  items refused that should run; nothing bad, no code unlisted.
* **SCHEMA**: 0 errors in 683 results.
* **FAULTS**: 288 faults, none hidden, the same one violation (bev-car's
  Driver with no P gain, a value inside its limits: the two Drivers' laws
  differ). The one flip is gone: bev-car's E-Motor at 10 000 % (its limit
  is 1 000 %) is refused now; the Driver difference it showed stays
  documented (KNOWN-LIMITS: the new Driver can ask for more recuperation
  than the supply takes). The
  53 refused: 29 of them today's engine fails or warns on, 24 it passes;
  by the line's registered words a refusal is not success. New path: 124
  success (146), 29 warning (41), 135 failed (101). Unsaid: 5 on 1 fault
  (3 on 3): the two negative masses are refused; the Driver with no P gain
  now counts its 4 rows not valid beside its warning. Every other raw
  record, today's engine's 343 and the new path's other runs, is the same
  as the ninth measurement's in status, warnings, errors, flags and codes.
  mutmut as before: 1 126 mutants, 344 survive, none with a written
  reason.
* **META passes**, its nine as before (none of its values is outside its
  limits).
* **ANALYTIC passes** (2.7e-14 s), **ENERGY vacuous** (E1 14 pass, E2 as
  before, E3 met no limit), **SCRIPTS** 1 of 3, **FMU, TEXT, QUICK** and
  **STATUS** (5 of 11): as before.
* **STREAM**: as before; S2 on a shared machine (another lane ran its tests
  on the other CPUs), today's p95 0.19 s live (0.17) and 0.50 s for the
  study (0.37).
* **NOSKIP passes**: 161 tests (160: the harness's new one), none skipped
  but the two Windows-only ones; with the module hidden 6 fail, none
  skips.

The machine, tools, module and fast mode's runner are the ninth
measurement's; no engine crate changed.

## The ninth measurement: the evidence part's third fix round

The evidence part's third review found two blocking defects and one
other: the new path gave none of today's run-time checks of a case's
values (the Ambient's air outside the usual, a Wheelbase of 0 with a CG
height), nor today's check that the car has driven by 60 s, so it said
success where today's warns; and two pieces of a guard had no test.
`d4a6215` fixes them and the same class where the scan of today's
run-time warnings found it (a CG height on one axle, wheels lifting off
the road, a motor with no live supply, an acceleration test with no
Vehicle); `61779c8` lets the judge see the class: FAULTS plants 20 faults
as a case's own value (`case_values`) and lists each fault's *unsaid*,
today's warnings the new path's run does not give (a dated change, before
this measurement). Against the eighth measurement, line by line:

* **B2B**: as before, to the difference: 22 of 26 items as expected, the
  same 1 498 differences in the same fields and classes (0 a, 117 b, 529
  c, 852 unclassified); no item's run says anything new (none of them sets
  off a check the round added). Every ledger balances.
* **STRICT**: 76 refusals (74): the two new case-value faults the new
  engine refuses, fs-electric's Wheelbase at a hundredth and its CG height
  100 times (`NEW-FAILED`, an event storm of the axle load where the front
  wheels lift); the same four items refused that should run; nothing bad.
* **SCHEMA**: 0 errors in 683 results (643; the 40 runs of the case-value
  faults).
* **FAULTS**: 288 faults (268). None hidden; the one violation and the one
  flip of before. The 20 case-value faults: today's engine and the new
  path give the same status on 18 (the Ambient at 0, minus, 100 times and
  a hundredth of its pressure and at 2 000 °C, the Wheelbase at 0 and
  minus: warning on both, in today's words; the other eleven success on
  both), and the new engine refuses the two above, which today's warns
  on ("the front wheels lift off the road at t = 0.01 s"). Before
  `d4a6215` six of them were violations (the four pressures and the two
  Wheelbases), which the matrix of before could not plant. Of the 268 of
  before, 42 new-path runs moved, each in its words only, its status the
  same: 40 that do not drive now say "No distance covered after 60 s"
  as today's do (bev-car's 20 and the hybrid's 20; 39 fail on both
  engines, bev-car with a negative mass fails on the new path and warns
  on today's, as before); fs-electric with its Vehicle deleted now says the line was not
  reached and gives today's acceleration rows (its battery's mean power
  1.76 kW against today's 1.81 kW); bev-car and fs-electric with a
  negative mass say all the wheels lift off the road at 0.00 s, which is
  the new engine's own model (a negative weight carries nothing) where
  today's clamps the mass to 1 kg. Unsaid: 3 warnings on 3 faults, each a
  difference of the two models, not a missing check: the negative masses
  (today's 1 kg car drives a little and says the cycle was not followed,
  or that its rear wheels lift) and the Driver with no P gain (the
  violation). New path: 146 success, 41 warning, 101 failed. Today's
  engine's records are identical, field for field, to the eighth
  measurement's (321 runs): the checks `d4a6215` shares with it moved
  without changing it. mutmut: 1 126 mutants of `verdict.py` (1 097; its
  two new functions), 344 survive (348), none with a written reason.
* **META passes**, its nine as before; the two failing bev-car faults now
  also say "No distance covered after 60 s", at every setting.
* **ANALYTIC passes**, **ENERGY vacuous** (E1 14 pass, E2 as before, E3
  met no limit), **SCRIPTS** 1 of 3, **FMU, TEXT, QUICK** and **STATUS**
  (5 of 11): as before.
* **STREAM**: as before; S2 on a shared machine, today's p95 0.17 s live
  (0.18) and 0.37 s for the study (0.41).
* **NOSKIP passes**: 160 tests (158: the harness's two new ones), none
  skipped but the two Windows-only ones; with the module hidden 6 fail,
  none skips.

The machine, tools, module and fast mode's runner are the eighth
measurement's; no engine crate changed.

## The eighth measurement: the evidence part's two fix rounds

The evidence part's first review found five blocking and four other
defects, its second one blocking and two others; the five commits above
fix them (the lane's notes, `lanes/step2-notes.md`, say which and with
what test). Against the seventh measurement, line by line:

* **B2B**: 22 of 26 items as expected (20 of 24): the two items the first
  fix round added, bev-car's city cycle with its battery's Output Power
  Limit only checked (23 kW over 0.5 s, `case-city-checked`, and 23.6 kW
  over none, `-w0`), run on the new path and warn on both engines, as
  expected. Their 196 differences (b 11, c 32, unclassified 55 each) are
  every difference that moved: the twelve items of before differ exactly
  as before, each difference in the same field and class and by the same
  number of bands (to 1e-6). An added item differs as bev-car's city
  cycle does but for two summary rows: its battery's time over the
  checked limit (1.510 s against today's 1.534 s over 0.5 s; 0.0040 s
  against 0.028 s over none: today holds each step's power over the
  whole step, so a step whose power is over the limit counts whole and
  its peak is higher, 23.61 against 23.602 kW), unclassified; and its
  recuperated energy, 1.0037 bands off as on the city cycle (1.0039),
  where the rule that classes it b (`bev-city-recuperation`) names the
  city cycle's items only, so it is unclassified here. In all 1 498: 0 a
  (0), 117 b (95), 529 c (465), 852 unclassified (742). Every ledger
  balances. No run has a note of what the verdict does not judge
  (`[note_codes]`, new: the count is 0).
* **STRICT**: as before, 74 refusals with listed codes, the same four
  items refused that should run, no NEW-ERROR; no message of any new-path
  run carries a code that no section lists (checked in every message
  now, not only in a refusal's).
* **SCHEMA**: 0 errors in 643 results (631; the two added items' runs).
* **FAULTS**: no fault hidden, and the one violation of before (bev-car's
  Driver with no P gain: the two Drivers' laws differ). Three faults moved
  on the new path, each by the first fix round: fs-electric with no or a
  negative internal resistance, whose enforced 80 kW cap the engine holds
  to its tolerance, now success as today's (before: warning, "broke its
  Output Power Limit" at 80.0 kW); bev-car with its E-Motor scaled 100
  times, now a warning where today's passes, the one flip: the new
  engine's Driver asks the battery for up to 536 kW of recuperation where
  it takes 120 kW and the run says its charging was held back; today's
  Driver never asks more than the supply takes, a difference of the
  Driver laws (`docs/KNOWN-LIMITS.md`). The new path: 135 success, 34
  warning, 99 failed (134, 35, 99). Five more moved in their words only,
  their status the same: the consumer cut back now said where the
  battery runs empty (bev-car's battery at 0, negative and a hundredth,
  the hybrid's final drive negative), the charging held back said on
  bev-car at a hundredth of its mass (the same Driver difference), and
  the minimum SOC's time said at the run's event (the second fix round):
  bev-car's battery at a hundredth, 214 s as today's, where the output
  grid's next point said 215 s; the hybrid with its final drive negative,
  385 s (386 s; today's car, which drives otherwise, says 1 336 s, both
  fail). Refused 59 as before; no note of what the verdict does not
  judge. mutmut as before (1 097 mutants, 348 survive, none with a
  written reason; `verdict.py` is unchanged).
* **META passes**, its set nine now: the two added items warn at all four
  settings (their averaged peaks over the limit at every output step and
  at rtol 1e-8), the seven of before as before.
* **ANALYTIC passes**, as before (2.7e-14 s, the allowance labelled
  LightSim's own).
* **ENERGY is vacuous** (it passed): by the first fix round's judge E3 is
  vacuous when no item meets a limit on either side, which none does; E1
  passes on all 14 items (12), E2 as before.
* **STREAM**: as before; S2 on a shared machine, today's p95 0.18 s live
  (0.17) and 0.41 s for the study (0.59).
* **NOSKIP passes**: 158 tests (155: the harness's three new ones), none
  skipped but the two Windows-only ones; with the module hidden 6 fail,
  none skips.
* **FMU, TEXT, QUICK, SCRIPTS** (1 of 3) and **STATUS** (5 of 11): as
  before.

The machine and tools are the seventh measurement's (below); the module
and fast mode's runner were built again from this tree, whose engine
crates the fix rounds' last two commits do not change.

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

## Where each line stands (the tenth measurement)

| line | state | what was measured |
|---|---|---|
| B2B | fails | 22 of 26 items as expected: the two live cases (NEW-LIVE), the performance probe (NEW-CASE-KIND) and the FMU probe (NEW-FMU) are refused where they should run. Of 1 498 differences on the 14 items both engines ran: 0 class a, 117 b, 529 c, 852 unclassified. Every item's ledger balances. No note of what the verdict does not judge. |
| STRICT | fails | only for the four items above. 119 refusals, every one with a listed code: NEW-LIMITS 53 (planted faults outside their parameters' limits), NEW-FAILED 38, NEW-BUILD 16, NEW-BALANCE 3, NEW-LAP 2, NEW-LIVE 2, NEW-CASE-KIND, NEW-CELLS, NEW-FMU, NEW-FS-EVENT and NEW-MISSING one each; no NEW-ERROR, no process that died; no message with a code no section lists. |
| SCHEMA | passes | 0 errors in 683 results. |
| FAULTS | fails | Of the 288 planted faults (20 of them a case's own value) today's engine fails or warns on 138; 53 hold a value outside its parameter's limits and are refused (NEW-LIMITS: not success); none is hidden by a note; one comes out "success" on the new path (bev-car's Driver with no P gain, which the new engine's Driver follows the cycle without: the two Drivers' laws differ); no flip; unsaid: that Driver's warning and its 4 rows not valid. mutmut: 1 126 mutants of verdict.py, 344 survive, none with a written reason yet. |
| META | passes | the same verdict (status, notValid rows, passed flags) at every output step (0.1, 1, 5 times the case's) and at rtol 1e-8 for all 9 runs; 3 fail and 3 warn. |
| ANALYTIC | passes | the verdict's time outside both bands within 2.7e-14 s of the exact answers on all five traces; the "cycle not followed" message labels its 1 %/2 s allowance LightSim's own. Both sources' copies are the pinned ones. |
| ENERGY | vacuous | E1: the part energies' losses and stored energies against the engine's books within 5e-16 of the throughput, all 14 items. E2: the held sine's battery power RMS within 3e-16 of the closed form. E3 vacuous: fast mode's flags and the lanes' stretches at a limit the same on the 8 BEV cycle items, none of which meets a limit on either side. |
| STREAM | fails | S1: live runs refused (today's engine streams all 601 points of both live cases, equal to its result). S2: the live run is refused, and the study's stops reach no running point on the new path (its points are done in about 1.3 s): not measured; today's p95 0.19 s live, 0.50 s for the study (a shared machine). S3: a replay gives the same SHA-256 on both engines, but RunSnapshot records no engine, version or solver options. |
| FMU | fails | refused (NEW-FMU): nothing to compare with fmusim or FMPy. |
| TEXT | not available | the app reads no text components. |
| QUICK | not available | a case has no quick-mode option. |
| SCRIPTS | fails | one of three: on the new path today's worker serves every step of the hybrid's Script part in all three single runs; no decision record, no page for the owner. |
| NOSKIP | passes here | with the module: 161 tests, none skipped but the two Windows-only stop-latency tests; with it hidden: 6 fail, none skips; CI's Windows engine job runs them with LIGHTSIM_REQUIRE_NEW_ENGINE=1 (to be confirmed on Windows CI after the merge). |
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
