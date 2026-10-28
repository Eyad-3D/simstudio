# Step 2's acceptance lines, measured

Commit 3e9e852d25, Python 3.11.17, lightsim_engine 0.1.0, AMD Ryzen 9 7900X 12-Core Processor.

| line | state |
|---|---|
| B2B | fail |
| STRICT | fail |
| SCHEMA | pass |
| FAULTS | fail |
| META | vacuous |
| ANALYTIC | fail |
| ENERGY | cannot be measured |
| STREAM | fail |
| FMU | fail |
| TEXT | not available |
| QUICK | not available |
| SCRIPTS | fail |
| NOSKIP | pass |
| STATUS | fail |

## B2B

20 of 24 items as expected; 1019 differences: a 419, b 97, c 250, unclassified 253.

| item | expect | got | today 10 ms / 1 ms | new | a | b | c | unclassified |
|---|---|---|---|---|--:|--:|--:|--:|
| aero-bev/case-udds | run | run | success / success | warning | 32 | 10 | 19 | 9 |
| aero-bev/case-hwfet | run | run | success / success | warning | 32 | 10 | 19 | 7 |
| aero-bev/case-wltc | run | run | success / success | warning | 32 | 10 | 19 | 7 |
| bev-car/case-city | run | run | success / success | warning | 34 | 12 | 19 | 9 |
| bev-car/case-wltc | run | run | success / success | warning | 33 | 11 | 19 | 9 |
| bev-car/case-wltc-hvac | run | run | success / success | warning | 33 | 11 | 19 | 9 |
| bev-car/case-wltc-winter | run | run | success / success | warning | 33 | 11 | 19 | 9 |
| bev-car/case-wltc-summer | run | run | success / success | warning | 33 | 11 | 19 | 9 |
| bev-car/case-city-live | run | NEW-LIVE ✗ | success / success | failed |  |  |  |  |
| fs-electric/case-accel-75m | run | run | success / success | warning | 40 | 1 | 20 |  |
| fs-electric/case-autocross | refuse:NEW-LAP | NEW-LAP | success / success | failed |  |  |  |  |
| fs-electric/case-endurance | refuse:NEW-LAP | NEW-LAP | success / success | failed |  |  |  |  |
| hybrid-car/case-udds | refuse:NEW-BALANCE | NEW-BALANCE | success / success | failed |  |  |  |  |
| hybrid-car/case-hwfet | refuse:NEW-BALANCE | NEW-BALANCE | success / success | failed |  |  |  |  |
| hybrid-car/case-mixed | refuse:NEW-BALANCE | NEW-BALANCE | success / success | failed |  |  |  |  |
| hybrid-car/case-mixed-live | run | NEW-LIVE ✗ | success / success | failed |  |  |  |  |
| probe/hybrid-udds-single | run | run | success / success | warning | 39 | 4 | 26 | 71 |
| probe/hybrid-hwfet-single | run | run | success / success | warning | 39 |  | 26 | 61 |
| probe/hybrid-mixed-single | run | run | success / success | warning | 39 | 6 | 26 | 53 |
| probe/bev-performance | run | NEW-CASE-KIND ✗ | success / success | failed |  |  |  |  |
| probe/fs-event-acceleration | refuse:NEW-FS-EVENT | NEW-FS-EVENT | success / success | failed |  |  |  |  |
| probe/bev-cells | refuse:NEW-CELLS | NEW-CELLS | success / success | failed |  |  |  |  |
| probe/bev-fmu | run | NEW-FMU ✗ | success / success | failed |  |  |  |  |
| probe/no-module | refuse:NEW-MISSING | NEW-MISSING | success / success | failed |  |  |  |  |

By rule: a new-path-lacks-messages: 4; a new-path-lacks-reports: 384; a new-path-lacks-verdict: 7; a preparation-note-as-warning: 12; a preparation-note-status: 12; b acceleration-ends-at-the-line: 1; b bev-city-recuperation: 1; b gears-today-rule: 95; c engine-note: 12; c new-records-more: 202; c today-integrator-checks: 24; c today-run-summary: 12; unclassified None: 253

Unclassified, by item and field:

- probe/hybrid-udds-single | channels.max | value: 27
- probe/hybrid-mixed-single | channels.min | value: 23
- probe/hybrid-udds-single | channels.min | value: 21
- probe/hybrid-hwfet-single | channels.min | value: 19
- probe/hybrid-hwfet-single | channels.max | value: 19
- probe/hybrid-mixed-single | channels.max | value: 18
- probe/hybrid-udds-single | channels.values | value: 17
- probe/hybrid-hwfet-single | channels.values | value: 16
- probe/hybrid-mixed-single | channels.mean | value: 12
- probe/hybrid-hwfet-single | channels.mean | value: 7
- probe/hybrid-udds-single | channels.mean | value: 6
- aero-bev/case-udds | channels.min | value: 3
- aero-bev/case-udds | channels.max | value: 3
- aero-bev/case-udds | channels.mean | value: 3
- aero-bev/case-hwfet | channels.min | value: 3
- aero-bev/case-wltc | channels.min | value: 3
- bev-car/case-city | channels.min | value: 3
- bev-car/case-city | channels.max | value: 3
- bev-car/case-city | channels.mean | value: 3
- bev-car/case-wltc | channels.min | value: 3
- bev-car/case-wltc | channels.max | value: 3
- bev-car/case-wltc | channels.mean | value: 3
- bev-car/case-wltc-hvac | channels.min | value: 3
- bev-car/case-wltc-hvac | channels.max | value: 3
- bev-car/case-wltc-hvac | channels.mean | value: 3
- bev-car/case-wltc-winter | channels.min | value: 3
- bev-car/case-wltc-winter | channels.max | value: 3
- bev-car/case-wltc-winter | channels.mean | value: 3
- bev-car/case-wltc-summer | channels.min | value: 3
- bev-car/case-wltc-summer | channels.max | value: 3
- bev-car/case-wltc-summer | channels.mean | value: 3
- aero-bev/case-hwfet | channels.max | value: 2
- aero-bev/case-hwfet | channels.mean | value: 2
- aero-bev/case-wltc | channels.max | value: 2
- aero-bev/case-wltc | channels.mean | value: 2

## STRICT

74 refusals: NEW-BALANCE 3, NEW-BUILD 16, NEW-CASE-KIND 1, NEW-CELLS 1, NEW-FAILED 46, NEW-FMU 1, NEW-FS-EVENT 1, NEW-LAP 2, NEW-LIVE 2, NEW-MISSING 1.

Items not as expected: bev-car/case-city-live (NEW-LIVE, expected run), hybrid-car/case-mixed-live (NEW-LIVE, expected run), probe/bev-performance (NEW-CASE-KIND, expected run), probe/bev-fmu (NEW-FMU, expected run).

Other failures: 0.

## SCHEMA

0 errors in 631 results (jsonschema 4.26.0 (Draft 2020-12)).

## FAULTS

268 of 268 faults measured; today's engine fails or warns on 129; violations (today fails or warns, the new path says success): 1; hidden by the engine's preparation notes: 73.

Today: {'success': 139, 'warning': 61, 'failed': 68}. New path: {'warning': 208, 'failed': 59, 'success': 1}. Refused: {'NEW-BUILD': 16, 'NEW-FAILED': 43}. Engine notes on the new runs: {'INIT-START-ASSUMED': 46, 'START-ALIAS-CONFLICT': 238, 'START-NOT-SOLVED': 243}.

Mutation: {'tool': 'mutmut 3.8.1', 'mutants': 935, 'by_state': {'killed': 608, 'survived': 325, 'timeout': 2}, 'complete': True, 'survived': 325, 'without_reason': 325}.

Violations: fs-electric:el-motor.

Hidden: bev-car:el-vehicle, bev-car:el-driver, bev-car:el-wheel-fl, bev-car:el-motor, bev-car:el-final-drive, bev-car:el-diff, bev-car:el-wheel-fr, bev-car:c-5, bev-car:c-6, bev-car:c-10, bev-car:c-12, bev-car:db-2, fs-electric:el-vehicle, fs-electric:el-driver, fs-electric:el-wheel-rl, fs-electric:el-diff, fs-electric:el-wheel-rr, fs-electric:c-4, fs-electric:c-8, fs-electric:c-10, fs-electric:db-1, hybrid-car:el-vehicle, hybrid-car:el-hcu, hybrid-car:el-tank, hybrid-car:el-gearbox, hybrid-car:el-fd, hybrid-car:el-diff, hybrid-car:el-motor, hybrid-car:el-wheel-l, hybrid-car:el-wheel-r, hybrid-car:c-3, hybrid-car:c-4, hybrid-car:c-5, hybrid-car:c-6, hybrid-car:c-10, hybrid-car:c-12, hybrid-car:db-4, hybrid-car:db-5, bev-car:el-vehicle.mass_kg:negative, bev-car:el-vehicle.mass_kg:x100, bev-car:el-vehicle.mass_kg:div100, bev-car:el-vehicle.cd:x100, bev-car:el-vehicle.frontal_area_m2:x100, bev-car:el-wheel-fl.radius_m:zero, bev-car:el-wheel-fl.radius_m:negative, bev-car:el-wheel-fl.radius_m:div100, bev-car:el-battery.capacity_kWh:zero, bev-car:el-battery.capacity_kWh:negative, bev-car:el-battery.capacity_kWh:div100, bev-car:el-motor.torque_scale_pct:div100, bev-car:el-final-drive.ratio:zero, bev-car:el-final-drive.ratio:negative, bev-car:el-final-drive.ratio:x100, bev-car:el-final-drive.ratio:div100, bev-car:el-driver.driver_kp:zero, fs-electric:el-vehicle.mass_kg:negative, fs-electric:el-vehicle.mass_kg:x100, fs-electric:el-wheel-rl.radius_m:zero, fs-electric:el-wheel-rl.radius_m:negative, fs-electric:el-wheel-rl.radius_m:div100, fs-electric:el-motor.torque_scale_pct:div100, fs-electric:el-final-drive.ratio:negative, fs-electric:el-final-drive.ratio:x100, fs-electric:el-final-drive.ratio:div100, fs-electric:el-wheel-rl.mu:zero, fs-electric:el-wheel-rl.mu:negative, fs-electric:el-wheel-rl.mu:div100, hybrid-car:el-wheel-l.radius_m:zero, hybrid-car:el-wheel-l.radius_m:negative, hybrid-car:el-motor.torque_scale_pct:div100, hybrid-car:el-fd.ratio:negative, hybrid-car:el-fd.ratio:x100, hybrid-car:el-fd.ratio:div100.

## META

7 of 7 the same at every setting; something judged: False.
- bev-car/case-city: new warning; meta-x0.1 warning; meta-x5 warning; meta-rtol1e-8 warning
- bev-car/case-wltc: new warning; meta-x0.1 warning; meta-x5 warning; meta-rtol1e-8 warning
- fs-electric/case-accel-75m: new warning; meta-x0.1 warning; meta-x5 warning; meta-rtol1e-8 warning
- bev-car:el-vehicle.mass_kg:x100: new warning; meta-x0.1 warning; meta-x5 warning; meta-rtol1e-8 warning
- bev-car:el-motor.torque_scale_pct:div100: new warning; meta-x0.1 warning; meta-x5 warning; meta-rtol1e-8 warning
- fs-electric:el-battery.internal_resistance_ohm:x100: new NEW-FAILED; meta-x0.1 NEW-FAILED; meta-x5 NEW-FAILED; meta-rtol1e-8 NEW-FAILED
- fs-electric:el-motor.torque_scale_pct:div100: new warning; meta-x0.1 warning; meta-x5 warning; meta-rtol1e-8 warning

## ANALYTIC

| trace | band | exact, s | verdict, s | error, s |
|---|---|--:|--:|--:|
| plateau excursion | epa | 9.562624 | 9.5 | 0.0626 |
| plateau excursion | wltp | 12 | 11.9 | 0.1 |
| lag on a ramp | epa | 28.781312 | 28.7 | 0.0813 |
| lag on a ramp | wltp | 30 | 29.9 | 0.1 |
| overshoot at a peak | epa | 5.82439665 | 5.8 | 0.0244 |
| overshoot at a peak | wltp | 7.98759305 | 8 | 0.0124 |
| short excursion | epa | 1.7562624 | 1.7 | 0.0563 |
| short excursion | wltp | 2 | 1.9 | 0.1 |
| slow car below a ramp | epa | 16.7125248 | 16.7 | 0.0125 |
| slow car below a ramp | wltp | 17.2 | 17.1 | 0.1 |

Worst error 0.1 s against 1e-06 s; the 1 %/2 s rule labelled as LightSim's own: False. Sources (SHA-256): {'40 CFR 1066.425 (2024-07-01)': '90187363ce0f181d03a23ab79ad566d5d66781bcfe5675549986e2d23473de5a', 'EU 2017/1151 Annex XXI Sub-Annex 6 (legislation.gov.uk)': '1c465cf11633de49b8547dee633ac8dba846a4f8ef2cdd268104d2275e02d032'}.

## ENERGY

E1: Counter({'cannot be measured': 12}). E2: {'exact_kw': 5.431390245600108, 'today_kw': 5.431390245600112, 'new_kw': None, 'new_status': 'warning', 'state': 'cannot be measured'}. E3: {'state': 'cannot be measured', 'why': 'the new path makes no limits report, and fast mode is not reachable from Python (lightsim_engine has no fast-mode call: engine/crates/lsim-py/README.md)'}.

## STREAM

S1 fail: bev-car/case-city-live today-normal: {'points': 601, 'recorded': 601, 'unequal': 0, 'first_unequal': None}; bev-car/case-city-live new: NEW-LIVE; hybrid-car/case-mixed-live today-normal: {'points': 601, 'recorded': 601, 'unequal': 0, 'first_unequal': None}; hybrid-car/case-mixed-live new: NEW-LIVE

S2 fail: {"live": {"today": {"stops": 20, "shown": 20, "p95_s": 0.10056807799992384, "median_s": 0.06978969449983197, "max_s": 0.10291358300037245, "mark_s": 0.25, "state": "pass", "unstopped_s": 15.527686861000802, "unstopped_status": "success", "unstopped_first": "Case 'WLTC Class 3b' solved: 1800 steps \u00d7 1 s (100 sub-steps each), 1801 points recorded, 70 result channels."}, "new": {"stops": 20, "shown": 0, "p95_s": null, "median_s": null, "max_s": null, "mark_s": 0.25, "state": "not measured", "unstopped_s": 2.409899934718851e-05, "unstopped_status": "failed", "unstopped_first": "Not run [NEW-LIVE]: it does not stream a live run yet; with LIGHTSIM_ENGINE_STRICT=1 no case runs on today's engine instead."}}, "study": {"today": {"stops": 20, "shown": 20, "p95_s": 0.35820461899947986, "median_s": 0.15230089499937094, "max_s": 0.42254269099976227, "mark_s": 1.0, "state": "pass", "unstopped_s": 20.37773474400001, "unstopped_statuses": ["success"], "stopped_statuses": [["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run"], ["cancelled before running", "cancelled while running", "not run"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "not run"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"]]}, "new": {"stops": 20, "shown": 0, "p95_s": null, "median_s": null, "max_s": null, "mark_s": 1.0, "state": "not measured", "unstopped_s": 0.9443195979993106, "unstopped_statuses": ["warning"], "stopped_statuses": [["cancelled before running", "not run", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "warning"], ["cancelled before running", "warning"], ["cancelled before running", "not run", "warning"], ["cancelled before running", "warning"]]}}, "machine": "shared (the step 1 lane may be running): an indication, not the judged number"}

S3 fail: {"records_engine": false, "records_engine_version": false, "records_solver_options": false, "runs": [{"engine": "today", "case": "bev-car/case-city", "status": "success", "first": "Case 'City Cycle' solved: 600 steps \u00d7 1 s (100 sub-steps each), 601 points recorded, 70 result channels.", "sha256": "604eb740f1eeee48f30e5cf24cb25f11f10558f34de268ef50f27d1ae4c197a6", "replayed": "604eb740f1eeee48f30e5cf24cb25f11f10558f34de268ef50f27d1ae4c197a6", "same": true}, {"engine": "today", "case": "fs-electric/case-accel-75m", "status": "success", "first": "Case 'Acceleration 75 m' solved: 399 of 2500 steps \u00d7 0.01 s, ended at 75.3 m driven at t = 3.985 s (2 sub-steps each), 400 points recorded, 65 result channels.", "sha256": "7af6d9c7a75778da4fc511a39a48547bfa2093a4a05c4efe6be7f487300176c8", "replayed": "7af6d9c7a75778da4fc511a39a48547bfa2093a4a05c4efe6be7f487300176c8", "same": true}, {"engine": "new", "case": "bev-car/case-city", "status": "warning", "first": "Run on the new engine (LIGHTSIM_ENGINE=new).", "sha256": "a349cbf25e868122f71530c74e458044d7e7bf6f476f0c23d545ab48c6c386aa", "replayed": "a349cbf25e868122f71530c74e458044d7e7bf6f476f0c23d545ab48c6c386aa", "same": true}, {"engine": "new", "case": "fs-electric/case-accel-75m", "status": "warning", "first": "Run on the new engine (LIGHTSIM_ENGINE=new).", "sha256": "ffe24e8d8f35c929cd2415fb013fbe23693970aac5268d3bb6d8acb8ce60bf98", "replayed": "ffe24e8d8f35c929cd2415fb013fbe23693970aac5268d3bb6d8acb8ce60bf98", "same": true}]}

## FMU

{"state": "fail", "probe": "NEW-FMU", "why": "the new path refuses a project with an FMU part (NEW-FMU): nothing to compare with fmusim or FMPy"}

## TEXT

{"state": "not available", "why": "the app's library has no part written as text, and the backend reads no text components"}

## QUICK

{"state": "not available", "why": "a case has no run option for quick mode (SimCase has no 'mode'), and the new path runs the full dynamic engine only"}

## SCRIPTS

{"state": "fail", "of_three": 1, "a_worker_on_new_path": true, "b_decision_record": [], "c_owner_page": false}

## NOSKIP

{"state": "pass", "a_tests": 30, "a_failed": 0, "a_skipped": 0, "a_windows_only": 2, "b_tests": 30, "b_failed": 4, "b_skipped_engine": 0, "c_ci": true, "detail": {"a": {"tests": 30, "failed": [], "skipped": ["stop latency is judged on Windows only", "stop latency is judged on Windows only"]}, "b": {"tests": 30, "failed": ["test_the_module_is_there_where_it_must_be", "test_new_engine_runs_a_cycle_case_like_today", "test_a_failing_new_engine_falls_back_with_a_warning", "test_both_engines_run_the_acceleration_test_and_the_diff_classes_it"], "skipped": ["stop latency is judged on Windows only", "stop latency is judged on Windows only"]}}}

## STATUS

{"state": "fail", "present": 0, "of": 11, "each": {"the energy report": false, "part energies": false, "duty": false, "limits": false, "references": false, "the verdict": false, "live streaming": false, "study stop and progress": false, "FMU parts": false, "text components": false, "quick mode": false}, "documents": "checked by hand at each part's review (not measured here)"}
