# Step 2's baseline: the new engine's path before the wiring

Measured on 28 October 2026 by `python -m benchmarks.back_to_back` (its
`run`, `replay`, `stop`, `mutation`, `noskip` and `report` parts: what `all`
runs), the runs with the app's code of `2b94af6` (unchanged since: only the
harness moved), the report by the harness of `3e9e852`, with the lines,
cases, measures and pass marks of
[`../acceptance.toml`](../acceptance.toml), registered in `5c82fb0` before
the first measured run (three dated changes since, at its end). It records
where the new engine's path into the app (`LIGHTSIM_ENGINE=new`) stands on
every line before step 2's next parts change it. [`report.md`](report.md) is
the harness's own report of this run; the raw results (631 runs, about
4 GB) are outside the repository, in the lane's cache
(`judge-cache/step2/baseline`), each with its SHA-256 in its `SHA256SUMS`.

The machine: the owner's PC, WSL 2 (Ubuntu 24.04) on an AMD Ryzen 9 7900X,
Python 3.11.17, `lightsim_engine` 0.1.0 built from this tree (release),
jsonschema 4.26.0, mutmut 3.8.1. Another lane's runs shared the machine
throughout: the stop latencies below are an indication only (their pass
marks are judged on Windows CI, `backend/tests/test_stop_latency.py`).

## Where each line stands

| line | state | what was measured |
|---|---|---|
| B2B | fails | 20 of 24 items as expected: the two live cases (NEW-LIVE), the performance probe (NEW-CASE-KIND) and the FMU probe (NEW-FMU) are refused where they should run. Of 1 019 differences on the 12 items both engines ran: 419 class a, 97 b, 250 c, 253 unclassified. |
| STRICT | fails | only for the four items above. 74 refusals, every one with a listed code: NEW-FAILED 46, NEW-BUILD 16, NEW-BALANCE 3, NEW-LAP 2, NEW-LIVE 2, NEW-CASE-KIND, NEW-CELLS, NEW-FMU, NEW-FS-EVENT and NEW-MISSING one each; no NEW-ERROR, no process that died, every pre-run fall-back message with its code. |
| SCHEMA | passes | 0 errors in 631 results (every run of the harness that gave a result). |
| FAULTS | fails | Of the 268 planted faults today's engine fails or warns on 129; 74 of those would come out "success" on the new path: 1 says so outright (fs-electric's E-Motor deleted), 73 say "warning" only because of the engine's preparation notes (finding 36). mutmut: 935 mutants of verdict.py, 325 survive, none with a written reason yet. |
| META | vacuous | the same verdict at every output step and tolerance for all 7 runs, because the new path makes none: no notValid, no passed flag, and every "warning" is a preparation note. |
| ANALYTIC | fails | the verdict's time outside the band is off by up to 0.1 s against the 1e-6 s mark (it adds whole 0.1 s samples), and the "cycle not followed" message does not say the 1 %/2 s allowance is LightSim's own. |
| ENERGY | cannot be measured | no part energies, duty or limits on the new path (E1, E2, E3); fast mode is not reachable from Python (E3). Today's engine gives the held sine's RMS to 8e-16. |
| STREAM | fails | S1: live runs refused (today's engine streams every point equal to its result, both live cases). S2: the live run is refused; the study's stop reaches no running point in any of 20 stops (not measured). S3: a replay gives the same SHA-256 on both engines, but RunSnapshot records no engine, version or solver options. |
| FMU | fails | refused (NEW-FMU): nothing to compare with fmusim or FMPy. |
| TEXT | not available | the app reads no text components. |
| QUICK | not available | a case has no quick-mode option. |
| SCRIPTS | fails | one of three: Script blocks run in today's worker on the new path (the hybrid's single runs); no decision record, no page for the owner. |
| NOSKIP | passes here | with the module: 30 tests, none skipped but the two Windows-only stop-latency tests; with it hidden: 4 fail, none skips; CI's Windows engine job builds the module and runs them with LIGHTSIM_REQUIRE_NEW_ENGINE=1 (to be confirmed on Windows CI after the merge). |
| STATUS | fails | 0 of 11 of the things step 2's goal names are on the new path. |

## B2B, in detail

The 12 items both engines ran (the BEVs' and the aerodynamic BEV's cycles,
the acceleration test, the hybrid's three cycles as single runs) and their
differences by class and rule (`../classes.toml` has each rule's reason and
check):

| class | rule | differences |
|---|---|--:|
| a | new-path-lacks-reports: no part energies, energy report, duty, limits or references | 384 |
| a | preparation-note-as-warning, preparation-note-status: a start-guess note shown as the run's warning, and the status it makes | 24 |
| a | new-path-lacks-verdict: a check row's limit and passed flag | 7 |
| a | new-path-lacks-messages: today's run notes not written (an acceleration test's "estimates" note and its power-limit note, a typed profile's note) | 4 |
| b | gears-today-rule: gear torque, power and losses, reconciled by golden/definitions.py | 95 |
| b | bev-city-recuperation: the BEV's recuperated energy on the city cycle, 1.0039 × band | 1 |
| b | acceleration-ends-at-the-line: today ends at 75.306 m (the end of its step), the new engine at 75.3000007 m | 1 |
| c | new-records-more: channels only the new engine records | 202 |
| c | today-integrator-checks: today's two energy-balance rows | 24 |
| c | engine-note, today-run-summary: which engine ran it; today's fixed-step summary | 24 |
| | unclassified | 253 |

Where the 253 unclassified differences sit (`where` in the report: how many
of a channel's worst points must go for the rest to be inside its band):

* **185 on the hybrid's single runs** (channels' values 33, minima 63,
  maxima 64, means 25: speeds, slips, gear, clutch and wheel torques). 164
  of them sit on at most 5 % of the output points, half on 5 points or
  fewer, at the gear shifts and engine starts. The golden comparison runs
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

## FAULTS, in detail

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
| B2B | 22 of 25 items as expected; 1 167 differences: 462 a, 253 b, 277 c, 175 unclassified | 20 of 24; 1 019: 419 a, 97 b, 250 c, 253 unclassified | Its 25 items are not on record; this run has 24, one of them an FMU probe that step 2's goal says should run, so one more is refused wrongly (NEW-FMU). Fewer b: its rules are lost, and this run classes a statistic of a gear's power only where today's rule can be rebuilt on it, so 68 such stay unclassified on the BEVs. The hybrid's unclassified (185 against its 153) sit as its did, on a few points at the shifts, with the same likely cause. The two b figures it found are found again to the digit: 75.306 m against 75.3000007 m, and 1.0039 × band. |
| STRICT | fails only for the three items wrongly refused; 68 refusals, all with listed codes | fails only for the four wrongly refused; 74 refusals, all with listed codes | the FMU probe; a different parameter-fault set |
| SCHEMA | 0 errors in 540 results | 0 errors in 631 | more runs (the META runs, the sine probe) |
| FAULTS | 68 of the 244 faults today's engine flags would be "success": 1 shows, 67 hidden; mutmut 935 mutants, 325 survive, none with a reason | 74 of 129: 1 shows, 73 hidden; mutmut 935 mutants, 325 survive, none with a reason | Its parameter faults and its count of "fails or warns" are not on record (244 is more than this run's 129 flagged runs; it may have counted Data Checks' warnings too); the shape is the same: one shows, the start-guess note hides the rest. 16 faults come from today's model builder (NEW-BUILD), as then. The same 935 mutants and the same 325 survivors' count, on tests chosen afresh (the first attempt's list is not on record). |
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

* **The WLTP's text** comes from legislation.gov.uk's copy of Regulation
  (EU) 2017/1151 (Annex XXI, Sub-Annex 6, 2.6.8.3.1; SHA-256 in the
  report), the retained UK copy, not EUR-Lex's consolidated text: EUR-Lex
  answered HTTP 202 with an empty page, as it did for the first attempt.
  The roadmap's "1.2.6.6" (not yet confirmed) is 2.6.8.3.1 in this copy;
  UN R154 was not opened.
* **The FMU line** needs Reference FMUs v0.0.41 and fmusim; nothing reaches
  them while the path refuses FMU parts, so neither was fetched for this
  baseline (the backend's CI uses v0.0.39; the first attempt found the
  v0.0.41 asset answering 404).
* **Stop latency on Windows** is for CI after the merge; the numbers here
  (today's engine: p95 0.10 s live, 0.36 s for the study) are from a shared
  Linux machine.
