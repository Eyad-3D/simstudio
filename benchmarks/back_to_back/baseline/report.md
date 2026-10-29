# Step 2's acceptance lines, measured

Report made at commit 923c2b9f89, Python 3.11.17, lightsim_engine 0.1.0, AMD Ryzen 9 7900X 12-Core Processor. What it reads was made by:

- commit 923c2b9f89, lightsim_engine 0.1.0 (9ba13a3233): 651 raw records; stop.json, replay.json, mutation.json, noskip.json

| line | state |
|---|---|
| B2B | fail |
| STRICT | fail |
| SCHEMA | pass |
| FAULTS | fail |
| META | pass |
| ANALYTIC | pass |
| ENERGY | vacuous |
| STREAM | fail |
| FMU | fail |
| TEXT | not available |
| QUICK | not available |
| SCRIPTS | fail |
| NOSKIP | pass |
| STATUS | fail |

## B2B

22 of 26 items as expected; 1498 differences: b 117, c 529, unclassified 852.

| item | expect | got | today 10 ms / 1 ms | new | a | b | c | unclassified |
|---|---|---|---|---|--:|--:|--:|--:|
| aero-bev/case-udds | run | run | success / success | success |  | 10 | 32 | 45 |
| aero-bev/case-hwfet | run | run | success / success | success |  | 10 | 32 | 40 |
| aero-bev/case-wltc | run | run | success / success | success |  | 10 | 32 | 40 |
| bev-car/case-city | run | run | success / success | success |  | 12 | 32 | 53 |
| bev-car/case-wltc | run | run | success / success | success |  | 11 | 32 | 46 |
| bev-car/case-wltc-hvac | run | run | success / success | success |  | 11 | 32 | 46 |
| bev-car/case-wltc-winter | run | run | success / success | success |  | 11 | 44 | 46 |
| bev-car/case-wltc-summer | run | run | success / success | success |  | 11 | 44 | 47 |
| bev-car/case-city-live | run | NEW-LIVE ✗ | success / success | failed |  |  |  |  |
| fs-electric/case-accel-75m | run | run | success / success | success |  | 1 | 77 | 17 |
| fs-electric/case-autocross | refuse:NEW-LAP | NEW-LAP | success / success | failed |  |  |  |  |
| fs-electric/case-endurance | refuse:NEW-LAP | NEW-LAP | success / success | failed |  |  |  |  |
| hybrid-car/case-udds | refuse:NEW-BALANCE | NEW-BALANCE | success / success | failed |  |  |  |  |
| hybrid-car/case-hwfet | refuse:NEW-BALANCE | NEW-BALANCE | success / success | failed |  |  |  |  |
| hybrid-car/case-mixed | refuse:NEW-BALANCE | NEW-BALANCE | success / success | failed |  |  |  |  |
| hybrid-car/case-mixed-live | run | NEW-LIVE ✗ | success / success | failed |  |  |  |  |
| probe/hybrid-udds-single | run | run | success / success | success |  | 2 | 36 | 126 |
| probe/hybrid-hwfet-single | run | run | success / success | success |  |  | 36 | 104 |
| probe/hybrid-mixed-single | run | run | success / success | success |  | 6 | 36 | 132 |
| probe/bev-performance | run | NEW-CASE-KIND ✗ | success / success | failed |  |  |  |  |
| probe/fs-event-acceleration | refuse:NEW-FS-EVENT | NEW-FS-EVENT | success / success | failed |  |  |  |  |
| probe/bev-cells | refuse:NEW-CELLS | NEW-CELLS | success / success | failed |  |  |  |  |
| probe/bev-fmu | run | NEW-FMU ✗ | success / success | failed |  |  |  |  |
| probe/no-module | refuse:NEW-MISSING | NEW-MISSING | success / success | failed |  |  |  |  |
| bev-car/case-city-checked | run | run | warning / warning | warning |  | 11 | 32 | 55 |
| bev-car/case-city-checked-w0 | run | run | warning / warning | warning |  | 11 | 32 | 55 |

Items without today's two reference runs (not judged): none.

Notes of what the verdict does not judge, by code ([note_codes]): none.

Each compared item's ledger: every leaf of the union of today's two runs and the new run, compared (both today's 1 ms run and the new run have it), by source (a leaf of a channel only the new run records, compared with the port the data bus wires to it), 10 ms only (compared, and today's 10 ms run says another text or flag there than its 1 ms run), reported (one of them lacks it) or a gap (only today's 10 ms run has it); it balances when the five make the union.

| item | union | compared | by source | 10 ms only | reported | gaps | balanced |
|---|--:|--:|--:|--:|--:|--:|---|
| aero-bev/case-udds | 562930 | 460098 | 89102 | 0 | 13730 | 0 | yes |
| aero-bev/case-hwfet | 314990 | 257458 | 49842 | 0 | 7690 | 0 | yes |
| aero-bev/case-wltc | 739648 | 604491 | 117117 | 0 | 18040 | 0 | yes |
| bev-car/case-city | 256304 | 211147 | 39117 | 0 | 6040 | 0 | yes |
| bev-car/case-wltc | 766685 | 631528 | 117117 | 0 | 18040 | 0 | yes |
| bev-car/case-wltc-hvac | 766685 | 631528 | 117117 | 0 | 18040 | 0 | yes |
| bev-car/case-wltc-winter | 766692 | 631535 | 117117 | 0 | 18040 | 0 | yes |
| bev-car/case-wltc-summer | 766704 | 631543 | 117117 | 0 | 18044 | 0 | yes |
| fs-electric/case-accel-75m | 163341 | 131168 | 10045 | 2 (messages/info: Battery 'Accumulator' held at its Output Power Limit (# kW at the terminals) from t = # s — the motors get what is left after the other loads. The run summary says for how long./0/text: 10 ms "Battery 'Accumulator' held at its Output Power Limit (80 kW at the terminals) from t = 0.37 s — the motors get what is left after the other loads. The run summary says for how long.", 1 ms "Battery 'Accumulator' held at its Output Power Limit (80 kW at the terminals) from t = 0.21 s — the motors get what is left after the other loads. The run summary says for how long.", new "Battery 'Accumulator' held at its Output Power Limit (80 kW at the terminals) from t = 0.20 s — the motors get what is left after the other loads. The run summary says for how long."; references/Energy from the sources at least the motion and road load (hand calculation)/0/note: 10 ms '½·m·v² change 0.0424 kWh + height 0.0000 kWh + road load 0.0106 kWh, summed over the recorded speed; every other loss adds to it', 1 ms '½·m·v² change 0.0424 kWh + height 0.0000 kWh + road load 0.0105 kWh, summed over the recorded speed; every other loss adds to it', new '½·m·v² change 0.0424 kWh + height 0.0000 kWh + road load 0.0105 kWh, summed over the recorded speed; every other loss adds to it') | 22121 | 5 (limits/lanes/E-Motor/0/changes/2/0; limits/lanes/E-Motor/0/changes/2/1; limits/lanes/E-Motor/0/seconds/machine; messages/info: Solver step reduced to # ms (from # ms) because the tyres' Slip Stiffness (Wheel 'Wheel FL': #) is too stiff for the larger step: the run takes about # times as long./0/level; messages/info: Solver step reduced to # ms (from # ms) because the tyres' Slip Stiffness (Wheel 'Wheel FL': #) is too stiff for the larger step: the run takes about # times as long./0/text) | yes |
| probe/hybrid-udds-single | 755874 | 604433 | 137080 | 0 | 14361 | 0 | yes |
| probe/hybrid-hwfet-single | 422753 | 338222 | 76680 | 0 | 7851 | 0 | yes |
| probe/hybrid-mixed-single | 331673 | 265433 | 60180 | 0 | 6052 | 8 (energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/elementId; energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/group; energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/kWh; energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/label; energy/sinks/el-wheel-rr\|Wheel RR — tyre slip/0/elementId) | yes |
| bev-car/case-city-checked | 256341 | 211183 | 39117 | 1 (messages/warning: Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power, averaged over # s, reached # kW at t = # s against # kW./0/text: 10 ms "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power, averaged over 0.5 s, reached 23.5 kW at t = 150.01 s against 23 kW.", 1 ms "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power, averaged over 0.5 s, reached 23.5 kW at t = 150.00 s against 23 kW.", new "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power, averaged over 0.5 s, reached 23.5 kW at t = 150.00 s against 23 kW.") | 6040 | 0 | yes |
| bev-car/case-city-checked-w0 | 256341 | 211183 | 39117 | 1 (messages/warning: Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power reached # kW at t = # s against # kW./0/text: 10 ms "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power reached 23.6 kW at t = 150.01 s against 23.6 kW.", 1 ms "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power reached 23.6 kW at t = 150.00 s against 23.6 kW.", new "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power reached 23.6 kW at t = 150.00 s against 23.6 kW.") | 6040 | 0 | yes |

By rule: b acceleration-ends-at-the-line: 1; b bev-city-recuperation: 1; b gears-today-rule: 115; c engine-note: 14; c engine-preparation-notes: 28; c new-records-more: 37; c today-first-point-before-its-step: 408; c today-integrator-checks: 28; c today-run-summary: 14; unclassified None: 852

Unclassified, by item and field:

- probe/hybrid-udds-single | channels.max | value: 29
- probe/hybrid-mixed-single | channels.min | value: 25
- probe/hybrid-udds-single | channels.min | value: 24
- probe/hybrid-hwfet-single | channels.min | value: 22
- probe/hybrid-hwfet-single | channels.max | value: 21
- probe/hybrid-udds-single | channels.values | value: 19
- probe/hybrid-hwfet-single | channels.values | value: 19
- probe/hybrid-mixed-single | channels.max | value: 18
- probe/hybrid-mixed-single | channels.mean | value: 12
- probe/hybrid-mixed-single | partEnergy.peakPower | value: 10
- probe/hybrid-mixed-single | partEnergy.energyInReverse | value: 8
- bev-car/case-city | partEnergy.energyIn | value: 7
- bev-car/case-city | energy.parts.inKWh | value: 7
- probe/hybrid-hwfet-single | channels.mean | value: 7
- probe/hybrid-hwfet-single | partEnergy.peakPower | value: 7
- probe/hybrid-mixed-single | partEnergy.energyIn | value: 7
- bev-car/case-city-checked | partEnergy.energyIn | value: 7
- bev-car/case-city-checked | energy.parts.inKWh | value: 7
- bev-car/case-city-checked-w0 | partEnergy.energyIn | value: 7
- bev-car/case-city-checked-w0 | energy.parts.inKWh | value: 7
- aero-bev/case-udds | partEnergy.energyIn | value: 6
- aero-bev/case-udds | partEnergy.energyOut | value: 6
- aero-bev/case-udds | energy.parts.inKWh | value: 6
- aero-bev/case-udds | energy.parts.outKWh | value: 6
- aero-bev/case-hwfet | partEnergy.energyIn | value: 6
- aero-bev/case-hwfet | partEnergy.energyOut | value: 6
- aero-bev/case-hwfet | energy.parts.inKWh | value: 6
- aero-bev/case-hwfet | energy.parts.outKWh | value: 6
- aero-bev/case-wltc | partEnergy.energyIn | value: 6
- aero-bev/case-wltc | partEnergy.energyOut | value: 6
- aero-bev/case-wltc | energy.parts.inKWh | value: 6
- aero-bev/case-wltc | energy.parts.outKWh | value: 6
- bev-car/case-city | partEnergy.energyOut | value: 6
- bev-car/case-city | energy.parts.outKWh | value: 6
- bev-car/case-wltc | partEnergy.energyIn | value: 6
- bev-car/case-wltc | partEnergy.energyOut | value: 6
- bev-car/case-wltc | energy.parts.inKWh | value: 6
- bev-car/case-wltc | energy.parts.outKWh | value: 6
- bev-car/case-wltc-hvac | partEnergy.energyIn | value: 6
- bev-car/case-wltc-hvac | partEnergy.energyOut | value: 6
- bev-car/case-wltc-hvac | energy.parts.inKWh | value: 6
- bev-car/case-wltc-hvac | energy.parts.outKWh | value: 6
- bev-car/case-wltc-winter | partEnergy.energyIn | value: 6
- bev-car/case-wltc-winter | partEnergy.energyOut | value: 6
- bev-car/case-wltc-winter | energy.parts.inKWh | value: 6
- bev-car/case-wltc-winter | energy.parts.outKWh | value: 6
- bev-car/case-wltc-summer | partEnergy.energyIn | value: 6
- bev-car/case-wltc-summer | partEnergy.energyOut | value: 6
- bev-car/case-wltc-summer | energy.parts.inKWh | value: 6
- bev-car/case-wltc-summer | energy.parts.outKWh | value: 6
- fs-electric/case-accel-75m | duty.min | value: 6
- probe/hybrid-udds-single | channels.mean | value: 6
- probe/hybrid-udds-single | partEnergy.energyIn | value: 6
- probe/hybrid-udds-single | partEnergy.energyOut | value: 6
- probe/hybrid-udds-single | energy.parts.inKWh | value: 6
- probe/hybrid-udds-single | energy.parts.outKWh | value: 6
- probe/hybrid-mixed-single | partEnergy.losses | value: 6
- probe/hybrid-mixed-single | duty.min | value: 6
- bev-car/case-city-checked | partEnergy.energyOut | value: 6
- bev-car/case-city-checked | energy.parts.outKWh | value: 6

## STRICT

74 refusals: NEW-BALANCE 3, NEW-BUILD 16, NEW-CASE-KIND 1, NEW-CELLS 1, NEW-FAILED 46, NEW-FMU 1, NEW-FS-EVENT 1, NEW-LAP 2, NEW-LIVE 2, NEW-MISSING 1.

Items not as expected: bev-car/case-city-live (NEW-LIVE, expected run), hybrid-car/case-mixed-live (NEW-LIVE, expected run), probe/bev-performance (NEW-CASE-KIND, expected run), probe/bev-fmu (NEW-FMU, expected run).

Other failures: 0.

## SCHEMA

0 errors in 643 results (jsonschema 4.26.0 (Draft 2020-12)).

## FAULTS

268 of 268 faults measured; today's engine fails or warns on 129; violations (today fails or warns, the new path says success): 1; hidden by the engine's preparation notes: 0.

Today: {'success': 139, 'warning': 61, 'failed': 68}. New path: {'success': 135, 'warning': 34, 'failed': 99}. Refused: {'NEW-BUILD': 16, 'NEW-FAILED': 43}. Engine notes on the new runs: {'INIT-START-ASSUMED': 46, 'START-ALIAS-CONFLICT': 238, 'START-NOT-SOLVED': 243}. Notes of what the verdict does not judge, by code: {}.

Mutation: {'tool': 'mutmut 3.8.1', 'mutants': 1097, 'by_state': {'killed': 734, 'survived': 348, 'no tests': 14, 'timeout': 1}, 'complete': True, 'survived': 348, 'without_reason': 348}.

Violations: bev-car:el-driver.driver_kp:zero.

Hidden: none.

Flips (today's engine passes, the new path warns or fails; counted, not in the pass mark): bev-car:el-motor.torque_scale_pct:x100.

## META

9 of 9 the same at every setting; something judged: True.
- bev-car/case-city: new success; meta-x0.1 success; meta-x5 success; meta-rtol1e-8 success
- bev-car/case-wltc: new success; meta-x0.1 success; meta-x5 success; meta-rtol1e-8 success
- fs-electric/case-accel-75m: new success; meta-x0.1 success; meta-x5 success; meta-rtol1e-8 success
- bev-car/case-city-checked: new warning; meta-x0.1 warning; meta-x5 warning; meta-rtol1e-8 warning
- bev-car/case-city-checked-w0: new warning; meta-x0.1 warning; meta-x5 warning; meta-rtol1e-8 warning
- bev-car:el-vehicle.mass_kg:x100: new failed; meta-x0.1 failed; meta-x5 failed; meta-rtol1e-8 failed
- bev-car:el-motor.torque_scale_pct:div100: new failed; meta-x0.1 failed; meta-x5 failed; meta-rtol1e-8 failed
- fs-electric:el-battery.internal_resistance_ohm:x100: new NEW-FAILED; meta-x0.1 NEW-FAILED; meta-x5 NEW-FAILED; meta-rtol1e-8 NEW-FAILED
- fs-electric:el-motor.torque_scale_pct:div100: new warning; meta-x0.1 warning; meta-x5 warning; meta-rtol1e-8 warning

## ANALYTIC

| trace | band | exact, s | verdict, s | error, s |
|---|---|--:|--:|--:|
| plateau excursion | epa | 9.562624 | 9.562624 | 1.78e-15 |
| plateau excursion | wltp | 12 | 12 | 0 |
| lag on a ramp | epa | 28.781312 | 28.781312 | 0 |
| lag on a ramp | wltp | 30 | 30 | 0 |
| overshoot at a peak | epa | 5.82439665 | 5.82439665 | 1.42e-14 |
| overshoot at a peak | wltp | 7.98759305 | 7.98759305 | 2.66e-14 |
| short excursion | epa | 1.7562624 | 1.7562624 | 1.55e-15 |
| short excursion | wltp | 2 | 2 | 1.33e-15 |
| slow car below a ramp | epa | 16.7125248 | 16.7125248 | 3.55e-15 |
| slow car below a ramp | wltp | 17.2 | 17.2 | 3.55e-15 |

Worst error 2.66e-14 s against 1e-06 s; the 1 %/2 s rule labelled as LightSim's own: True. Sources: 40 CFR 1066.425 (annual edition of 1 July 2024) (cfr/CFR-2024-title40-vol37-sec1066-425.xml, SHA-256 90187363ce0f): same; Regulation (EU) 2017/1151, Annex XXI, Sub-Annex 6 (legislation.gov.uk's copy) (wltp/eur-2017-1151-annex-Sub-6.html, SHA-256 1c465cf11633): same.

## ENERGY

E1: Counter({'pass': 14}). E2: {'exact_kw': 5.431390245600108, 'today_kw': 5.431390245600112, 'new_kw': 5.4313902456001095, 'new_status': 'success', 'state': 'pass'}. E3: {'state': 'vacuous', 'rows': [{'id': 'aero-bev/case-udds', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'aero-bev/case-hwfet', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'aero-bev/case-wltc', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'bev-car/case-city', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'bev-car/case-wltc', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'bev-car/case-wltc-hvac', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'bev-car/case-wltc-winter', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'bev-car/case-wltc-summer', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}], 'met_a_limit': 0}.

## STREAM

S1 fail: bev-car/case-city-live today-normal: {'points': 601, 'recorded': 601, 'unequal': 0, 'lacking_channels': 0, 'first_unequal': None}; bev-car/case-city-live new: NEW-LIVE; hybrid-car/case-mixed-live today-normal: {'points': 601, 'recorded': 601, 'unequal': 0, 'lacking_channels': 0, 'first_unequal': None}; hybrid-car/case-mixed-live new: NEW-LIVE

S2 fail: {"live": {"today": {"stops": 20, "shown": 20, "p95_s": 0.184513895001146, "median_s": 0.13185651950152533, "max_s": 0.18935052300003008, "mark_s": 0.25, "state": "pass", "unstopped_s": 15.12194376199841, "unstopped_status": "success", "unstopped_first": "Case 'WLTC Class 3b' solved: 1800 steps \u00d7 1 s (100 sub-steps each), 1801 points recorded, 70 result channels."}, "new": {"stops": 20, "shown": 0, "p95_s": null, "median_s": null, "max_s": null, "mark_s": 0.25, "state": "not measured", "unstopped_s": 2.813500032061711e-05, "unstopped_status": "failed", "unstopped_first": "Not run [NEW-LIVE]: it does not stream a live run yet; with LIGHTSIM_ENGINE_STRICT=1 no case runs on today's engine instead."}}, "study": {"today": {"stops": 20, "shown": 20, "p95_s": 0.4074043359978532, "median_s": 0.19045397250010865, "max_s": 0.4344261219994223, "mark_s": 1.0, "state": "pass", "unstopped_s": 21.856853527999192, "unstopped_statuses": ["success"], "stopped_statuses": [["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "not run"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"]]}, "new": {"stops": 20, "shown": 0, "p95_s": null, "median_s": null, "max_s": null, "mark_s": 1.0, "state": "not measured", "unstopped_s": 1.2631943020023755, "unstopped_statuses": ["success"], "stopped_statuses": [["cancelled before running", "not run", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "not run", "success"], ["success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "success"]]}}, "machine": "shared (the step 1 lane may be running): an indication, not the judged number"}

S3 fail: {"records_engine": false, "records_engine_version": false, "records_solver_options": false, "runs": [{"engine": "today", "case": "bev-car/case-city", "status": "success", "first": "Case 'City Cycle' solved: 600 steps \u00d7 1 s (100 sub-steps each), 601 points recorded, 70 result channels.", "sha256": "604eb740f1eeee48f30e5cf24cb25f11f10558f34de268ef50f27d1ae4c197a6", "replayed": "604eb740f1eeee48f30e5cf24cb25f11f10558f34de268ef50f27d1ae4c197a6", "same": true}, {"engine": "today", "case": "fs-electric/case-accel-75m", "status": "success", "first": "Case 'Acceleration 75 m' solved: 399 of 2500 steps \u00d7 0.01 s, ended at 75.3 m driven at t = 3.985 s (2 sub-steps each), 400 points recorded, 65 result channels.", "sha256": "7af6d9c7a75778da4fc511a39a48547bfa2093a4a05c4efe6be7f487300176c8", "replayed": "7af6d9c7a75778da4fc511a39a48547bfa2093a4a05c4efe6be7f487300176c8", "same": true}, {"engine": "new", "case": "bev-car/case-city", "status": "success", "first": "Run on the new engine (LIGHTSIM_ENGINE=new).", "sha256": "9d133f135282487f94cb49abbf43744576a6d8f6a9f4cee12c7e2b0786c947c1", "replayed": "9d133f135282487f94cb49abbf43744576a6d8f6a9f4cee12c7e2b0786c947c1", "same": true}, {"engine": "new", "case": "fs-electric/case-accel-75m", "status": "success", "first": "Run on the new engine (LIGHTSIM_ENGINE=new).", "sha256": "9bde628e60a7d724e655bd9f2c6248c831ffdcf364f629edc5478a4811a8bf0a", "replayed": "9bde628e60a7d724e655bd9f2c6248c831ffdcf364f629edc5478a4811a8bf0a", "same": true}]}

## FMU

{"state": "fail", "probe": "NEW-FMU", "why": "the new path refuses a project with an FMU part (NEW-FMU): nothing to compare with fmusim or FMPy"}

## TEXT

{"state": "not available", "why": "the app's library has no part written as text, and the backend reads no text components"}

## QUICK

{"state": "not available", "why": "a case has no run option for quick mode (SimCase has no 'mode'), and the new path runs the full dynamic engine only"}

## SCRIPTS

{"state": "fail", "of_three": 1, "a_worker_on_new_path": true, "a_served": {"probe/hybrid-udds-single": true, "probe/hybrid-hwfet-single": true, "probe/hybrid-mixed-single": true}, "a_runners": {"probe/hybrid-udds-single": {"scripts": {"Hybrid Control Unit": "python"}, "script_calls": {"el-hcu": 136901}, "script_served": {"el-hcu": 136901}, "script_worker": {"own_pid": 1778, "pids": [1800]}}, "probe/hybrid-hwfet-single": {"scripts": {"Hybrid Control Unit": "python"}, "script_calls": {"el-hcu": 76501}, "script_served": {"el-hcu": 76501}, "script_worker": {"own_pid": 1825, "pids": [1847]}}, "probe/hybrid-mixed-single": {"scripts": {"Hybrid Control Unit": "python"}, "script_calls": {"el-hcu": 60001}, "script_served": {"el-hcu": 60001}, "script_worker": {"own_pid": 1872, "pids": [1894]}}}, "b_decision_record": [], "b_records_lack": {}, "c_owner_page": false}

## NOSKIP

{"state": "pass", "a_tests": 158, "a_failed": 0, "a_skipped": 0, "a_windows_only": 2, "b_tests": 158, "b_failed": 6, "b_skipped_engine": 0, "c_ci": true, "detail": {"a": {"tests": 158, "failed": [], "skipped": ["stop latency is judged on Windows only", "stop latency is judged on Windows only"]}, "b": {"tests": 158, "failed": ["test_the_module_is_there_where_it_must_be", "test_new_engine_runs_a_cycle_case_like_today", "test_new_engine_runs_an_acceleration_test", "test_a_failing_new_engine_falls_back_with_a_warning", "test_scripts_tells_todays_worker_from_the_apps_own_process", "test_both_engines_run_the_acceleration_test_and_the_diff_classes_it"], "skipped": ["stop latency is judged on Windows only", "stop latency is judged on Windows only"]}}, "stamp": {"commit": "923c2b9f8971816e1256a1a9387f8088ba433b83", "dirty": null, "lightsim_engine": "0.1.0", "lightsim_engine_sha256": "9ba13a3233c8eff1fddd86b4e86bbe47f478fe3ab89dc0f0379020701149b917", "fast_runner_sha256": "a510b2b8cacea13f689ed940ababde3241a5a6a4bd8d5d5ab930334eec0add8d", "sha256": {"backend/app/solver/new_engine.py": "5dda0d7ffc2e5f4d144b5da476070beaa19f98322d005b5122e76c4066b65822", "backend/app/solver/verdict.py": "7e6f8595e274880bce6c437e4f4cfab3ae97c4bdc5ebdded063f2a7202991e27", "backend/app/solver/evidence.py": "9fe5fb07df1ee49fc0198f11c0d4faf00f2bf8cb769c3e76d18cda80052b914d", "benchmarks/back_to_back/__init__.py": "21d5dccb4ed68b4670b8cb19e292ab2d6682269a22b28bdc8ace1a08856178cb", "benchmarks/back_to_back/__main__.py": "94d871e7fbfc01af4b647b65f2dfa48dec20484b55c0fd08f1030eeedc9a291d", "benchmarks/back_to_back/diff.py": "9293e7f02aceb6e9739b034ac588730945d66117811e5e91ba5fdf47e1474eb7", "benchmarks/back_to_back/mutation.py": "7b4ef81645941f59695dc963a93f915bbf0b1a08d7230502638b897423154eaa", "benchmarks/back_to_back/report.py": "3bf8df44371c811252a7c8141ecfcc99d83ab913855cd6100ac5472e19d12e2b", "benchmarks/back_to_back/runs.py": "48ed60d96ab4359c5c5c719ad3a4697b4622ebda488c56538686e98d1fb9b85c", "benchmarks/back_to_back/stream.py": "02e5aebc5fdc3edad5d0c43cf9e7ebdd9c1d2ffd4f8a6d9eef065b4bbccf9fcd", "benchmarks/back_to_back/traces.py": "d50ff090cd1720b89da900fd190407030e584f0884829402f2414e4b94b6c522", "benchmarks/back_to_back/acceptance.toml": "9faac4708727386f657f81fe20cc72e37c0fe0219b94b1787fd2deeff5df333b", "benchmarks/back_to_back/classes.toml": "62f4c4c0a866a4ab0a0a63e7954ac5d76d118d5447954155817a12024b1145c9", "benchmarks/back_to_back/survivors.toml": "078069bdee7ef5909c6db2196ce73f5cfbe52408c78b709f955043ad2629aee0"}}}

## STATUS

{"state": "fail", "present": 5, "of": 11, "each": {"the energy report": true, "part energies": true, "duty": true, "limits": true, "references": true, "the verdict": false, "live streaming": false, "study stop and progress": false, "FMU parts": false, "text components": false, "quick mode": false}, "documents": "checked by hand at each part's review (not measured here)"}
