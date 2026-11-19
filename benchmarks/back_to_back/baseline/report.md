# Step 2's acceptance lines, measured

Report made at commit c8b17e0c95, Python 3.11.17, lightsim_engine 0.1.0, AMD Ryzen 9 7900X 12-Core Processor. What it reads was made by:

- commit a6f63e58d7, lightsim_engine 0.1.0 (444c61389b): 691 raw records; stop.json, replay.json, mutation.json, noskip.json, text.json, quick.json

| line | state |
|---|---|
| B2B | fail |
| STRICT | fail |
| SCHEMA | pass |
| FAULTS | fail |
| META | pass |
| ANALYTIC | pass |
| ENERGY | vacuous |
| STREAM | pass |
| FMU | not measured |
| TEXT | pass |
| QUICK | fail |
| SCRIPTS | fail |
| NOSKIP | pass |
| STATUS | fail |

## B2B

25 of 26 items as expected; 1917 differences: b 147, c 680, unclassified 1090.

| item | expect | got | today 10 ms / 1 ms | new | a | b | c | unclassified |
|---|---|---|---|---|--:|--:|--:|--:|
| aero-bev/case-udds | run | run | success / success | success |  | 10 | 35 | 45 |
| aero-bev/case-hwfet | run | run | success / success | success |  | 10 | 35 | 40 |
| aero-bev/case-wltc | run | run | success / success | success |  | 10 | 35 | 40 |
| bev-car/case-city | run | run | success / success | success |  | 12 | 35 | 53 |
| bev-car/case-wltc | run | run | success / success | success |  | 11 | 35 | 46 |
| bev-car/case-wltc-hvac | run | run | success / success | success |  | 11 | 35 | 46 |
| bev-car/case-wltc-winter | run | run | success / success | success |  | 11 | 47 | 46 |
| bev-car/case-wltc-summer | run | run | success / success | success |  | 11 | 47 | 47 |
| bev-car/case-city-live | run | run | success / success | success |  | 12 | 35 | 53 |
| fs-electric/case-accel-75m | run | run | success / success | success |  | 1 | 80 | 17 |
| fs-electric/case-autocross | refuse:NEW-LAP | NEW-LAP | success / success | failed |  |  |  |  |
| fs-electric/case-endurance | refuse:NEW-LAP | NEW-LAP | success / success | failed |  |  |  |  |
| hybrid-car/case-udds | refuse:NEW-BALANCE | NEW-BALANCE | success / success | failed |  |  |  |  |
| hybrid-car/case-hwfet | refuse:NEW-BALANCE | NEW-BALANCE | success / success | failed |  |  |  |  |
| hybrid-car/case-mixed | refuse:NEW-BALANCE | NEW-BALANCE | success / success | failed |  |  |  |  |
| hybrid-car/case-mixed-live | run | run | success / success | success |  | 6 | 39 | 132 |
| probe/hybrid-udds-single | run | run | success / success | success |  | 2 | 39 | 126 |
| probe/hybrid-hwfet-single | run | run | success / success | success |  |  | 39 | 104 |
| probe/hybrid-mixed-single | run | run | success / success | success |  | 6 | 39 | 132 |
| probe/bev-performance | run | NEW-CASE-KIND ✗ | success / success | failed |  |  |  |  |
| probe/fs-event-acceleration | refuse:NEW-FS-EVENT | NEW-FS-EVENT | success / success | failed |  |  |  |  |
| probe/bev-cells | refuse:NEW-CELLS | NEW-CELLS | success / success | failed |  |  |  |  |
| probe/bev-fmu | run | run | success / success | success |  | 12 | 35 | 53 |
| probe/no-module | refuse:NEW-MISSING | NEW-MISSING | success / success | failed |  |  |  |  |
| bev-car/case-city-checked | run | run | warning / warning | warning |  | 11 | 35 | 55 |
| bev-car/case-city-checked-w0 | run | run | warning / warning | warning |  | 11 | 35 | 55 |

Items without today's two reference runs (not judged): none.

Notes of what the verdict does not judge, by code ([note_codes]): none.

Each compared item's ledger: every leaf of the union of today's two runs and the new run, compared (both today's 1 ms run and the new run have it), by source (a leaf of a channel only the new run records, compared with the port the data bus wires to it), 10 ms only (compared, and today's 10 ms run says another text or flag there than its 1 ms run), reported (one of them lacks it) or a gap (only today's 10 ms run has it); it balances when the five make the union.

| item | union | compared | by source | 10 ms only | reported | gaps | balanced |
|---|--:|--:|--:|--:|--:|--:|---|
| aero-bev/case-udds | 562939 | 460100 | 89102 | 0 | 13737 | 0 | yes |
| aero-bev/case-hwfet | 314999 | 257460 | 49842 | 0 | 7697 | 0 | yes |
| aero-bev/case-wltc | 739657 | 604493 | 117117 | 0 | 18047 | 0 | yes |
| bev-car/case-city | 256313 | 211149 | 39117 | 0 | 6047 | 0 | yes |
| bev-car/case-wltc | 766694 | 631530 | 117117 | 0 | 18047 | 0 | yes |
| bev-car/case-wltc-hvac | 766694 | 631530 | 117117 | 0 | 18047 | 0 | yes |
| bev-car/case-wltc-winter | 766701 | 631537 | 117117 | 0 | 18047 | 0 | yes |
| bev-car/case-wltc-summer | 766713 | 631545 | 117117 | 0 | 18051 | 0 | yes |
| bev-car/case-city-live | 256313 | 211149 | 39117 | 0 | 6047 | 0 | yes |
| fs-electric/case-accel-75m | 163350 | 131170 | 10045 | 2 (messages/info: Battery 'Accumulator' held at its Output Power Limit (# kW at the terminals) from t = # s — the motors get what is left after the other loads. The run summary says for how long./0/text: 10 ms "Battery 'Accumulator' held at its Output Power Limit (80 kW at the terminals) from t = 0.37 s — the motors get what is left after the other loads. The run summary says for how long.", 1 ms "Battery 'Accumulator' held at its Output Power Limit (80 kW at the terminals) from t = 0.21 s — the motors get what is left after the other loads. The run summary says for how long.", new "Battery 'Accumulator' held at its Output Power Limit (80 kW at the terminals) from t = 0.20 s — the motors get what is left after the other loads. The run summary says for how long."; references/Energy from the sources at least the motion and road load (hand calculation)/0/note: 10 ms '½·m·v² change 0.0424 kWh + height 0.0000 kWh + road load 0.0106 kWh, summed over the recorded speed; every other loss adds to it', 1 ms '½·m·v² change 0.0424 kWh + height 0.0000 kWh + road load 0.0105 kWh, summed over the recorded speed; every other loss adds to it', new '½·m·v² change 0.0424 kWh + height 0.0000 kWh + road load 0.0105 kWh, summed over the recorded speed; every other loss adds to it') | 22128 | 5 (limits/lanes/E-Motor/0/changes/2/0; limits/lanes/E-Motor/0/changes/2/1; limits/lanes/E-Motor/0/seconds/machine; messages/info: Solver step reduced to # ms (from # ms) because the tyres' Slip Stiffness (Wheel 'Wheel FL': #) is too stiff for the larger step: the run takes about # times as long./0/level; messages/info: Solver step reduced to # ms (from # ms) because the tyres' Slip Stiffness (Wheel 'Wheel FL': #) is too stiff for the larger step: the run takes about # times as long./0/text) | yes |
| hybrid-car/case-mixed-live | 331682 | 265435 | 60180 | 0 | 6059 | 8 (energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/elementId; energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/group; energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/kWh; energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/label; energy/sinks/el-wheel-rr\|Wheel RR — tyre slip/0/elementId) | yes |
| probe/hybrid-udds-single | 755883 | 604435 | 137080 | 0 | 14368 | 0 | yes |
| probe/hybrid-hwfet-single | 422762 | 338224 | 76680 | 0 | 7858 | 0 | yes |
| probe/hybrid-mixed-single | 331682 | 265435 | 60180 | 0 | 6059 | 8 (energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/elementId; energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/group; energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/kWh; energy/sinks/el-wheel-rl\|Wheel RL — tyre slip/0/label; energy/sinks/el-wheel-rr\|Wheel RR — tyre slip/0/elementId) | yes |
| probe/bev-fmu | 265340 | 217167 | 42126 | 0 | 6047 | 0 | yes |
| bev-car/case-city-checked | 256350 | 211185 | 39117 | 1 (messages/warning: Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power, averaged over # s, reached # kW at t = # s against # kW./0/text: 10 ms "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power, averaged over 0.5 s, reached 23.5 kW at t = 150.01 s against 23 kW.", 1 ms "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power, averaged over 0.5 s, reached 23.5 kW at t = 150.00 s against 23 kW.", new "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power, averaged over 0.5 s, reached 23.5 kW at t = 150.00 s against 23 kW.") | 6047 | 0 | yes |
| bev-car/case-city-checked-w0 | 256350 | 211185 | 39117 | 1 (messages/warning: Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power reached # kW at t = # s against # kW./0/text: 10 ms "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power reached 23.6 kW at t = 150.01 s against 23.6 kW.", 1 ms "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power reached 23.6 kW at t = 150.00 s against 23.6 kW.", new "Battery 'HV Battery Pack' broke its Output Power Limit: its terminal power reached 23.6 kW at t = 150.00 s against 23.6 kW.") | 6047 | 0 | yes |

By rule: b acceleration-ends-at-the-line: 1; b bev-city-recuperation: 3; b gears-today-rule: 143; c each-run-says-what-ran-it: 51; c engine-note: 17; c engine-preparation-notes: 34; c new-records-more: 43; c today-first-point-before-its-step: 484; c today-integrator-checks: 34; c today-run-summary: 17; unclassified None: 1090

Unclassified, by item and field:

- probe/hybrid-udds-single | channels.max | value: 29
- hybrid-car/case-mixed-live | channels.min | value: 25
- probe/hybrid-mixed-single | channels.min | value: 25
- probe/hybrid-udds-single | channels.min | value: 24
- probe/hybrid-hwfet-single | channels.min | value: 22
- probe/hybrid-hwfet-single | channels.max | value: 21
- probe/hybrid-udds-single | channels.values | value: 19
- probe/hybrid-hwfet-single | channels.values | value: 19
- hybrid-car/case-mixed-live | channels.max | value: 18
- probe/hybrid-mixed-single | channels.max | value: 18
- hybrid-car/case-mixed-live | channels.mean | value: 12
- probe/hybrid-mixed-single | channels.mean | value: 12
- hybrid-car/case-mixed-live | partEnergy.peakPower | value: 10
- probe/hybrid-mixed-single | partEnergy.peakPower | value: 10
- hybrid-car/case-mixed-live | partEnergy.energyInReverse | value: 8
- probe/hybrid-mixed-single | partEnergy.energyInReverse | value: 8
- bev-car/case-city | partEnergy.energyIn | value: 7
- bev-car/case-city | energy.parts.inKWh | value: 7
- bev-car/case-city-live | partEnergy.energyIn | value: 7
- bev-car/case-city-live | energy.parts.inKWh | value: 7
- hybrid-car/case-mixed-live | partEnergy.energyIn | value: 7
- probe/hybrid-hwfet-single | channels.mean | value: 7
- probe/hybrid-hwfet-single | partEnergy.peakPower | value: 7
- probe/hybrid-mixed-single | partEnergy.energyIn | value: 7
- probe/bev-fmu | partEnergy.energyIn | value: 7
- probe/bev-fmu | energy.parts.inKWh | value: 7
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

## STRICT

116 refusals: NEW-BALANCE 3, NEW-BUILD 16, NEW-CASE-KIND 1, NEW-CELLS 1, NEW-FAILED 38, NEW-FS-EVENT 1, NEW-LAP 2, NEW-LIMITS 53, NEW-MISSING 1.

Items not as expected: probe/bev-performance (NEW-CASE-KIND, expected run).

Other failures: 0.

## SCHEMA

0 errors in 683 results (jsonschema 4.26.0 (Draft 2020-12)).

## FAULTS

288 of 288 faults measured; today's engine fails or warns on 138; violations (today fails or warns, the new path says success): 1; hidden by the engine's preparation notes: 0.

Today: {'success': 150, 'warning': 70, 'failed': 68}. New path: {'success': 124, 'warning': 29, 'failed': 135}. Refused: {'NEW-BUILD': 16, 'NEW-FAILED': 35, 'NEW-LIMITS': 53}. Engine notes on the new runs: {'INIT-START-ASSUMED': 46, 'START-ALIAS-CONFLICT': 205, 'START-NOT-SOLVED': 210}. Notes of what the verdict does not judge, by code: {}.

Mutation: {'tool': 'mutmut 3.8.1', 'mutants': 1391, 'by_state': {'killed': 965, 'survived': 412, 'no tests': 14}, 'complete': True, 'survived': 412, 'without_reason': 412}.

Violations: bev-car:el-driver.driver_kp:zero.

Hidden: none.

Flips (today's engine passes, the new path warns or fails; counted, not in the pass mark): none.

Unsaid (a warning or error of today's run, in its words without their numbers, a row it makes not valid or a check it fails, that the new path's run does not give; counted, not in the pass mark): 5 on 1 faults
- bev-car:el-driver.driver_kp:zero: 5

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

Worst error 2.66e-14 s against 1e-06 s; the 1 %/2 s rule labelled as LightSim's own: True. Sources: 40 CFR 1066.425 (annual edition of 1 July 2024) (cfr/CFR-2024-title40-vol37-sec1066-425.xml, SHA-256 90187363ce0f): same; Regulation (EU) 2017/1151, Annex XXI, Sub-Annex 6 (legislation.gov.uk's copy) (wltp/eur-2017-1151-annex-Sub-6.html, SHA-256 1c465cf11633): same; Reference FMUs v0.0.41 (Modelica Association, BSD-2-Clause) (reference-fmus/Reference-FMUs-0.0.41.zip, SHA-256 62babca76b9c): same; fmusim v0.7.0 for x86_64-windows (Modelica Association, BSD-2-Clause) (fmusim-0.7.0/fmusim-x86_64-windows.zip, SHA-256 bcabfa115022): same; fmusim v0.7.0 for x86_64-linux (Modelica Association, BSD-2-Clause) (fmusim-0.7.0/fmusim-x86_64-linux.zip, SHA-256 85d19b9b23ad): same.

## ENERGY

E1: Counter({'pass': 17}). E2: {'exact_kw': 5.431390245600108, 'today_kw': 5.431390245600112, 'new_kw': 5.4313902456001095, 'new_status': 'success', 'state': 'pass'}. E3: {'state': 'vacuous', 'rows': [{'id': 'aero-bev/case-udds', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'aero-bev/case-hwfet', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'aero-bev/case-wltc', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'bev-car/case-city', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'bev-car/case-wltc', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'bev-car/case-wltc-hvac', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'bev-car/case-wltc-winter', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}, {'id': 'bev-car/case-wltc-summer', 'state': 'pass', 'step_s': 1.0, 'lanes_limited_s': 0, 'fast_flagged_s': 0, 'lanes': [], 'fast': []}], 'met_a_limit': 0}.

## STREAM

S1 pass: bev-car/case-city-live today-normal: {'points': 601, 'recorded': 601, 'unequal': 0, 'lacking_channels': 0, 'first_unequal': None}; bev-car/case-city-live new: {'points': 601, 'recorded': 601, 'unequal': 0, 'lacking_channels': 0, 'first_unequal': None}; hybrid-car/case-mixed-live today-normal: {'points': 601, 'recorded': 601, 'unequal': 0, 'lacking_channels': 0, 'first_unequal': None}; hybrid-car/case-mixed-live new: {'points': 601, 'recorded': 601, 'unequal': 0, 'lacking_channels': 0, 'first_unequal': None}

S2 pass: {"live": {"today": {"stops": 20, "shown": 20, "p95_s": 0.13520880899886834, "median_s": 0.07733184649987379, "max_s": 0.13744758399843704, "mark_s": 0.25, "state": "pass", "unstopped_s": 14.327647274003539, "unstopped_status": "success", "unstopped_first": "Case 'WLTC Class 3b' solved: 1800 steps \u00d7 1 s (100 sub-steps each), 1801 points recorded, 70 result channels."}, "new": {"stops": 20, "shown": 20, "p95_s": 0.15722108499903698, "median_s": 0.10340357200038852, "max_s": 0.16888865999499103, "mark_s": 0.25, "state": "pass", "unstopped_s": 1.4659372180030914, "unstopped_status": "success", "unstopped_first": "Run on the new engine (LIGHTSIM_ENGINE=new)."}}, "study": {"today": {"stops": 20, "shown": 20, "p95_s": 0.6550562849952257, "median_s": 0.28046338399872184, "max_s": 0.6636576219971175, "mark_s": 1.0, "state": "pass", "unstopped_s": 61.53351425800065, "unstopped_statuses": ["success"], "stopped_statuses": [["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "not run"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"]]}, "new": {"stops": 20, "shown": 19, "p95_s": 0.7265231870042044, "median_s": 0.30633782900258666, "max_s": 0.7265231870042044, "mark_s": 1.0, "state": "pass", "unstopped_s": 6.956968548998702, "unstopped_statuses": ["success"], "stopped_statuses": [["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "not run", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "not run", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"], ["cancelled before running", "cancelled while running", "success"]]}}, "machine": "shared (the step 1 lane may be running): an indication, not the judged number"}

S3 pass: {"records_engine": true, "records_engine_version": true, "records_solver_options": true, "runs": [{"engine": "today", "case": "bev-car/case-city", "status": "success", "first": "Case 'City Cycle' solved: 600 steps \u00d7 1 s (100 sub-steps each), 601 points recorded, 70 result channels.", "snapshot": {"engine": "today", "engineVersion": "0.2.0", "solverOptions": {"timeStep": 1.0, "solverStep": 0.01, "outputEvery": 1.0}, "liveEdits": []}, "sha256": "eb633bf576231a26e0f7358dedca8e84cad16f843f593a117613855c38eafeef", "replayed": "eb633bf576231a26e0f7358dedca8e84cad16f843f593a117613855c38eafeef", "same": true}, {"engine": "today", "case": "fs-electric/case-accel-75m", "status": "success", "first": "Case 'Acceleration 75 m' solved: 399 of 2500 steps \u00d7 0.01 s, ended at 75.3 m driven at t = 3.985 s (2 sub-steps each), 400 points recorded, 65 result channels.", "snapshot": {"engine": "today", "engineVersion": "0.2.0", "solverOptions": {"timeStep": 0.01, "solverStep": 0.005, "outputEvery": 1.0}, "liveEdits": []}, "sha256": "e971f51b5c5b9c3efe12c3804fbda814b66887b9d8f619b7379c8307623f041d", "replayed": "e971f51b5c5b9c3efe12c3804fbda814b66887b9d8f619b7379c8307623f041d", "same": true}, {"engine": "new", "case": "bev-car/case-city", "status": "success", "first": "Run on the new engine (LIGHTSIM_ENGINE=new).", "snapshot": {"engine": "new", "engineVersion": "0.1.0", "solverOptions": {"rtol": 1e-06, "atol": 1e-08, "outputStep": 1.0, "traceStep": 0.1}, "liveEdits": []}, "sha256": "b1605a80943ab7a45b8efa965e25b0cd7d73bbc0b6bf84c6ce0d7dad85e783df", "replayed": "b1605a80943ab7a45b8efa965e25b0cd7d73bbc0b6bf84c6ce0d7dad85e783df", "same": true}, {"engine": "new", "case": "fs-electric/case-accel-75m", "status": "success", "first": "Run on the new engine (LIGHTSIM_ENGINE=new).", "snapshot": {"engine": "new", "engineVersion": "0.1.0", "solverOptions": {"rtol": 1e-06, "atol": 1e-08, "outputStep": 0.01, "traceStep": 0.01}, "liveEdits": []}, "sha256": "b44c0707af06d7cf4b64f30ecbe1c6163b437f2e6a5cff989d53657fe7e435d0", "replayed": "b44c0707af06d7cf4b64f30ecbe1c6163b437f2e6a5cff989d53657fe7e435d0", "same": true}, {"engine": "new", "case": "bev-car/case-wltc live edit", "status": "success", "first": "Run on the new engine (LIGHTSIM_ENGINE=new).", "snapshot": {"engine": "new", "engineVersion": "0.1.0", "solverOptions": {"rtol": 1e-06, "atol": 1e-08, "outputStep": 1.0, "traceStep": 0.1}, "liveEdits": [{"t": 303.0, "elementId": "el-vehicle", "key": "cd", "value": 0.35, "note": null}]}, "sha256": "85456c4ee4614221faa93a6588b1d8a91128ac58185a4af65d9dc4bf202e93c3", "replayed": "85456c4ee4614221faa93a6588b1d8a91128ac58185a4af65d9dc4bf202e93c3", "same": true}]}

## FMU

{"state": "not measured", "probe": "run", "why": "the Reference FMUs and fmusim were not run here (LIGHTSIM_REFERENCE_FMUS and LIGHTSIM_FMUSIM not set): the line is measured on Windows CI (backend/tests/test_new_engine_fmu.py)"}

## TEXT

{"state": "pass", "parts": 92, "same": 92, "different": [], "not_cases": {"aero-bev/case-udds": [], "bev-car/case-city": [], "fs-electric/case-accel-75m": [], "hybrid-car/case-udds": ["el-hcu TEXT-SAMPLED"]}, "not_cases_unexpected": [], "stamp": {"commit": "a6f63e58d711deb550e6922769c10814c2bc8cc8", "dirty": null, "lightsim_engine": "0.1.0", "lightsim_engine_sha256": "444c61389b4eedbc66d8859ce62a333210cd098a0bb3593bd568d5cfcfd0eb75", "fast_runner_sha256": "74a73d7137b3e9b6335b20286050ab8404c3c9b95e4a643fcda2fd363423f46a", "sha256": {"backend/app/solver/new_engine.py": "63fcee7808dc9de9d7cbc4f4d7655881009049b1b86e11cf60b71f4325435bb5", "backend/app/solver/verdict.py": "d70f11924aa2161dd273c08e444cfeab2740d38a7b3944ca0d50c10ec44f4660", "backend/app/solver/evidence.py": "9fe5fb07df1ee49fc0198f11c0d4faf00f2bf8cb769c3e76d18cda80052b914d", "benchmarks/back_to_back/__init__.py": "21d5dccb4ed68b4670b8cb19e292ab2d6682269a22b28bdc8ace1a08856178cb", "benchmarks/back_to_back/__main__.py": "161d4a012958723ff43be2279fcc611fc953223353e54010dc56e3652b964954", "benchmarks/back_to_back/diff.py": "95d4b21d544bca2f0ef2e210112a162074593cbe39f6091be742ac4123c9a108", "benchmarks/back_to_back/item5.py": "417e14f450c008724f63fac1b54f488a47e3758e0131a70e5bf7cda9b7492acc", "benchmarks/back_to_back/mutation.py": "7b4ef81645941f59695dc963a93f915bbf0b1a08d7230502638b897423154eaa", "benchmarks/back_to_back/report.py": "b4fe45cd0a3c524879f4d57d9420a56a43e635eabe87b8f3abcc4e67462550b5", "benchmarks/back_to_back/runs.py": "f8964768a1984b86615fff284abd666e9b46f92bb8e0ab6c59eb9bd0b55e2727", "benchmarks/back_to_back/stream.py": "234f4598d4dfbda143a9ca2afd55d83d82bf6e75abe5ae1d5af3826d1e89840e", "benchmarks/back_to_back/traces.py": "d50ff090cd1720b89da900fd190407030e584f0884829402f2414e4b94b6c522", "benchmarks/back_to_back/acceptance.toml": "a5cc8f3f98ff12988fc7c5e37161014c965436695329b42e6c81e259e28a6ac9", "benchmarks/back_to_back/classes.toml": "13e62bd99542669491c1428598e0ff1f92bc0d99d1ab22c5ea3779bcfc08437b", "benchmarks/back_to_back/survivors.toml": "078069bdee7ef5909c6db2196ce73f5cfbe52408c78b709f955043ad2629aee0"}}}

## QUICK

{"state": "fail", "cases": 16, "ran": ["aero-bev/case-udds", "aero-bev/case-hwfet", "aero-bev/case-wltc", "bev-car/case-city", "bev-car/case-wltc", "bev-car/case-wltc-hvac", "bev-car/case-wltc-winter", "bev-car/case-wltc-summer", "bev-car/case-city-live"], "refused": {"fs-electric/case-accel-75m": "QUICK-KIND", "fs-electric/case-autocross": "QUICK-KIND", "fs-electric/case-endurance": "QUICK-KIND", "hybrid-car/case-udds": "QUICK-BALANCE", "hybrid-car/case-hwfet": "QUICK-BALANCE", "hybrid-car/case-mixed": "QUICK-BALANCE", "hybrid-car/case-mixed-live": "QUICK-SAMPLED"}, "unlisted": [], "flagged": {}, "out_of_band": ["aero-bev/case-udds:el-battery.internal_losses_kwh -1.1311%", "aero-bev/case-wltc:el-battery.energy_recuperated_kwh -1.1901%", "aero-bev/case-wltc:el-battery.internal_losses_kwh -1.3182%", "bev-car/case-wltc:el-battery.energy_recuperated_kwh -1.3990%", "bev-car/case-wltc:el-battery.internal_losses_kwh -1.3947%", "bev-car/case-wltc-hvac:el-battery.energy_recuperated_kwh -1.6277%", "bev-car/case-wltc-hvac:el-battery.internal_losses_kwh -1.2410%", "bev-car/case-wltc-winter:el-battery.energy_recuperated_kwh -1.7497%", "bev-car/case-wltc-winter:el-battery.internal_losses_kwh -1.0735%", "bev-car/case-wltc-summer:el-battery.energy_recuperated_kwh -1.5525%", "bev-car/case-wltc-summer:el-battery.internal_losses_kwh -1.3193%"], "not_in_full": [], "docs": {"codes": ["QUICK-NOT-FOUND", "QUICK-KIND", "QUICK-SAMPLED", "QUICK-CELLS", "QUICK-LIMITS", "QUICK-MISSING", "QUICK-BALANCE", "QUICK-BUILD", "QUICK-FAILED", "QUICK-ERROR"], "missing_in_help": [], "missing_in_known_limits": []}, "stamp": {"commit": "a6f63e58d711deb550e6922769c10814c2bc8cc8", "dirty": null, "lightsim_engine": "0.1.0", "lightsim_engine_sha256": "444c61389b4eedbc66d8859ce62a333210cd098a0bb3593bd568d5cfcfd0eb75", "fast_runner_sha256": "74a73d7137b3e9b6335b20286050ab8404c3c9b95e4a643fcda2fd363423f46a", "sha256": {"backend/app/solver/new_engine.py": "63fcee7808dc9de9d7cbc4f4d7655881009049b1b86e11cf60b71f4325435bb5", "backend/app/solver/verdict.py": "d70f11924aa2161dd273c08e444cfeab2740d38a7b3944ca0d50c10ec44f4660", "backend/app/solver/evidence.py": "9fe5fb07df1ee49fc0198f11c0d4faf00f2bf8cb769c3e76d18cda80052b914d", "benchmarks/back_to_back/__init__.py": "21d5dccb4ed68b4670b8cb19e292ab2d6682269a22b28bdc8ace1a08856178cb", "benchmarks/back_to_back/__main__.py": "161d4a012958723ff43be2279fcc611fc953223353e54010dc56e3652b964954", "benchmarks/back_to_back/diff.py": "95d4b21d544bca2f0ef2e210112a162074593cbe39f6091be742ac4123c9a108", "benchmarks/back_to_back/item5.py": "417e14f450c008724f63fac1b54f488a47e3758e0131a70e5bf7cda9b7492acc", "benchmarks/back_to_back/mutation.py": "7b4ef81645941f59695dc963a93f915bbf0b1a08d7230502638b897423154eaa", "benchmarks/back_to_back/report.py": "b4fe45cd0a3c524879f4d57d9420a56a43e635eabe87b8f3abcc4e67462550b5", "benchmarks/back_to_back/runs.py": "f8964768a1984b86615fff284abd666e9b46f92bb8e0ab6c59eb9bd0b55e2727", "benchmarks/back_to_back/stream.py": "234f4598d4dfbda143a9ca2afd55d83d82bf6e75abe5ae1d5af3826d1e89840e", "benchmarks/back_to_back/traces.py": "d50ff090cd1720b89da900fd190407030e584f0884829402f2414e4b94b6c522", "benchmarks/back_to_back/acceptance.toml": "a5cc8f3f98ff12988fc7c5e37161014c965436695329b42e6c81e259e28a6ac9", "benchmarks/back_to_back/classes.toml": "13e62bd99542669491c1428598e0ff1f92bc0d99d1ab22c5ea3779bcfc08437b", "benchmarks/back_to_back/survivors.toml": "078069bdee7ef5909c6db2196ce73f5cfbe52408c78b709f955043ad2629aee0"}}}

## SCRIPTS

{"state": "fail", "of_three": 1, "a_worker_on_new_path": true, "a_served": {"probe/hybrid-udds-single": true, "probe/hybrid-hwfet-single": true, "probe/hybrid-mixed-single": true}, "a_runners": {"probe/hybrid-udds-single": {"scripts": {"Hybrid Control Unit": "python"}, "script_calls": {"el-hcu": 136901}, "script_served": {"el-hcu": 136901}, "script_worker": {"own_pid": 317330, "pids": [317352]}}, "probe/hybrid-hwfet-single": {"scripts": {"Hybrid Control Unit": "python"}, "script_calls": {"el-hcu": 76501}, "script_served": {"el-hcu": 76501}, "script_worker": {"own_pid": 317392, "pids": [317414]}}, "probe/hybrid-mixed-single": {"scripts": {"Hybrid Control Unit": "python"}, "script_calls": {"el-hcu": 60001}, "script_served": {"el-hcu": 60001}, "script_worker": {"own_pid": 317463, "pids": [317485]}}}, "b_decision_record": [], "b_records_lack": {}, "c_owner_page": false}

## NOSKIP

{"state": "pass", "a_tests": 165, "a_failed": 0, "a_skipped": 0, "a_windows_only": 2, "b_tests": 165, "b_failed": 6, "b_skipped_engine": 0, "c_ci": true, "detail": {"a": {"tests": 165, "failed": [], "skipped": ["stop latency is judged on Windows only", "stop latency is judged on Windows only"]}, "b": {"tests": 165, "failed": ["test_the_module_is_there_where_it_must_be", "test_new_engine_runs_a_cycle_case_like_today", "test_new_engine_runs_an_acceleration_test", "test_a_failing_new_engine_falls_back_with_a_warning", "test_scripts_tells_todays_worker_from_the_apps_own_process", "test_both_engines_run_the_acceleration_test_and_the_diff_classes_it"], "skipped": ["stop latency is judged on Windows only", "stop latency is judged on Windows only"]}}, "stamp": {"commit": "a6f63e58d711deb550e6922769c10814c2bc8cc8", "dirty": null, "lightsim_engine": "0.1.0", "lightsim_engine_sha256": "444c61389b4eedbc66d8859ce62a333210cd098a0bb3593bd568d5cfcfd0eb75", "fast_runner_sha256": "74a73d7137b3e9b6335b20286050ab8404c3c9b95e4a643fcda2fd363423f46a", "sha256": {"backend/app/solver/new_engine.py": "63fcee7808dc9de9d7cbc4f4d7655881009049b1b86e11cf60b71f4325435bb5", "backend/app/solver/verdict.py": "d70f11924aa2161dd273c08e444cfeab2740d38a7b3944ca0d50c10ec44f4660", "backend/app/solver/evidence.py": "9fe5fb07df1ee49fc0198f11c0d4faf00f2bf8cb769c3e76d18cda80052b914d", "benchmarks/back_to_back/__init__.py": "21d5dccb4ed68b4670b8cb19e292ab2d6682269a22b28bdc8ace1a08856178cb", "benchmarks/back_to_back/__main__.py": "161d4a012958723ff43be2279fcc611fc953223353e54010dc56e3652b964954", "benchmarks/back_to_back/diff.py": "95d4b21d544bca2f0ef2e210112a162074593cbe39f6091be742ac4123c9a108", "benchmarks/back_to_back/item5.py": "417e14f450c008724f63fac1b54f488a47e3758e0131a70e5bf7cda9b7492acc", "benchmarks/back_to_back/mutation.py": "7b4ef81645941f59695dc963a93f915bbf0b1a08d7230502638b897423154eaa", "benchmarks/back_to_back/report.py": "b4fe45cd0a3c524879f4d57d9420a56a43e635eabe87b8f3abcc4e67462550b5", "benchmarks/back_to_back/runs.py": "f8964768a1984b86615fff284abd666e9b46f92bb8e0ab6c59eb9bd0b55e2727", "benchmarks/back_to_back/stream.py": "234f4598d4dfbda143a9ca2afd55d83d82bf6e75abe5ae1d5af3826d1e89840e", "benchmarks/back_to_back/traces.py": "d50ff090cd1720b89da900fd190407030e584f0884829402f2414e4b94b6c522", "benchmarks/back_to_back/acceptance.toml": "a5cc8f3f98ff12988fc7c5e37161014c965436695329b42e6c81e259e28a6ac9", "benchmarks/back_to_back/classes.toml": "13e62bd99542669491c1428598e0ff1f92bc0d99d1ab22c5ea3779bcfc08437b", "benchmarks/back_to_back/survivors.toml": "078069bdee7ef5909c6db2196ce73f5cfbe52408c78b709f955043ad2629aee0"}}}

## STATUS

{"state": "fail", "present": 8, "of": 11, "each": {"the energy report": true, "part energies": true, "duty": true, "limits": true, "references": true, "the verdict": false, "live streaming": true, "study stop and progress": true, "FMU parts": false, "text components": true, "quick mode": false}, "documents": "checked by hand at each part's review (not measured here)"}
