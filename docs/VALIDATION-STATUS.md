# What is validated, and what is not

- Last reviewed: 23 September 2026, for version 0.2.0. This page is updated
  with every release, together with [Known issues and limits](KNOWN-LIMITS.md).

**In one line:** LightSim's results have **not** been validated against
measured vehicle data yet. The engine is tested against exact answers, and
the two example cars are checked for believable numbers, but that is not
the same as validation.

Three words are used carefully on this page:

- **Verified** — the code gives the answer the equations say it should
  (a hand-calculated result, or a quantity that must be conserved).
- **Plausibility-checked** — a result falls inside a band taken from real
  vehicles of the same class. It shows the model is not badly wrong; it does
  not show it is right, because some inputs were tuned or invented.
- **Validated** — a result is compared with measurements of the same real
  vehicle, on the same test, with inputs that were not tuned to that
  measurement, and the error is reported. Nothing is validated yet.

## Verified: the engine does what its equations say

Each of these is an automatic test that runs on every change
(`backend/tests/`):

| What | How it is checked | Test file |
|---|---|---|
| Time and distance | at a constant 10 m/s, distance is exactly 10 m × t at every recorded time; runs stop exactly at the case duration | `test_time_base.py` |
| Energy is conserved | in a pure acceleration the battery's energy equals the car's kinetic energy plus the tyre-slip loss within 0.5 % (a 5 % leak anywhere would fail); the summary energy equals the integrated power channel | `test_energy.py`, `test_source_limits.py` |
| Battery and fuel-cell limits | at minimum charge, maximum charge and power limits the motor gets only what the source can give; regen above the charge limit goes to the friction brakes | `test_source_limits.py` |
| Battery charge | a constant 1C discharge from 100 % reaches 0 % at 3,600 ± 1 s for differently shaped OCV tables; a battery without an Ah value gives out its Usable Capacity from full to empty; coulombic efficiency acts on charge only; the SOC change matches the current that flowed | `test_battery_charge.py`, `test_source_limits.py` |
| Controller timing | results do not depend on the recording step (1, 0.1 and 0.02 s give the same figures) | `test_control_rate.py`, `test_verdict.py` |
| Road load | air drag follows the Ambient's air density, rho = p / (R · T): −7 °C gives 1.113 times the drag of 23 °C within 0.5 %, also when set as a case value or a live edit, and so does a coefficient C; 85 kPa gives 85 / 101.325 of the drag at 101.325 kPa within 0.5 %; an Ambient at its defaults gives what a model without one gets, and of two Ambients the first sets the air; uphill, the car slows by g · (sin θ + rolling resistance · cos θ) of the slope angle within 0.1 % at 10 and 25 % grades, and a coefficient A takes the cos too; launching up a 25 % grade, the tyres' grip tops out at μ · m · g · cos θ within 0.1 %; a coast-down of a car entered with road-load coefficients gives back A, B and C within 0.5 %; with *Coefficients Include Driveline Losses* the motor gives the road load through the final drive and differential with no loss, without it through their efficiencies; Data Checks catch axle losses counted twice, a negative coefficient A or C, several Ambients and an Ambient pressure or temperature out of range or in the wrong unit | `test_road_load.py`, `test_data_checks.py` |
| Engine | full throttle gives the full-load curve; fuel is cut on overrun; the rev limiter cuts fuel and torque; CO₂ follows fuel with the tank's factor | `test_engine.py` |
| Electric motor | spin losses are counted once; a powered motor has no extra drag | `test_motor_losses.py` |
| Maximum speed and map edges | a motor stops at its maximum speed (a 300 km/h target holds it between 97 and 100 % of 12,000 rpm), also when that is set lower or changed during a run; a motor driven above it gives no drive torque and is reported; a run that leaves a table set to *Error* stops and names the table, the axis, the value and the time; *Clamp* and *Linear* give the edge value and the edge slope and are reported with their time outside; a motor or engine held at its limiter is not counted as over speed, also when a light one overshoots it by a step (a free motor at 12,000 or 3,000 rpm, a free engine of 0.05 kg·m²), while an engine the wheels drive above it is; every table a run reads is counted once per solver step, and the library defaults pass the map cross-checks, which catch a motor map that does not start at 0 rpm and a fuel-cell curve that does not start at 0 A | `test_map_edges.py`, `test_engine.py` |
| Run status | a car that cannot follow the cycle, does not move, or runs out of energy is never reported as a success; untrustworthy figures carry a "not valid" flag; a motor run past its map's voltage data or driven above its maximum speed for longer than 1 % of the run (at least 2 s) is never a success, and the message and flag name the motor, how far past and for how long, while a Lookup block past its table is not judged; a run a stop cut short is *cancelled*, one stopped as it ended is not; a 0-100 km/h performance test succeeds with the time of a full-throttle run, read where the speed crosses the target, and then holds the target without switching between throttle and brakes; a top-speed test reports the car's highest speed; a stopped performance test does not say the car fell short, and a car that starts at its target gets no time | `test_verdict.py`, `test_examples_plausible.py`, `test_api.py`, `test_map_edges.py` |
| Regressions | the two examples' results are compared with stored reference results; every change to them is listed in `backend/tests/golden/CHANGES.md` | `test_golden.py` |

## Plausibility-checked: the two example cars

`backend/tests/test_examples_plausible.py` holds both examples to bands from
real cars of their class. Their test mass and road load come from public
data; their motor, engine and battery maps are generic (invented, marked
*synthetic* in [the data register](DATA-REGISTER.md)).

| Example | Figure | LightSim | Reference | Band in the test |
|---|---|---|---|---|
| Battery Electric Car (2021 Cupra Born values from FASTSim) | WLTC energy at the battery | 14.1 kWh/100 km | about 15–16 kWh/100 km rated at the charging socket, charging losses included (background knowledge, unverified) | 13–17 kWh/100 km |
| | 0–100 km/h | 7.2 s | 7.3 s (maker's figure, background knowledge) | ±10 % |
| | Top speed | 160 km/h | 160 km/h (limited) | ±2 %, and within the motor's maximum speed |
| P2 Hybrid Car (Hyundai Ioniq Hybrid test mass and EPA road load) | EPA city cycle (UDDS) fuel | 2.84 l/100 km (no cold start) | 2.91 l/100 km (EPA 2022 test car list) | 2–5 l/100 km, and at most 4.5 after correcting for the battery's change of charge |
| | EPA highway cycle (HWFET) fuel | 3.23 l/100 km | 2.94 l/100 km (EPA 2022 test car list) | same as the city cycle |
| | Battery charge at the end | same as at the start | charge-sustaining | within 1 % of the start |

Why this is not validation: the hybrid's road load is EPA's target
coefficients for that car (with the axle's losses counted once), but its
engine and motor maps are generic, and it has no cold start, so its city
figure reads below EPA's, whose city test starts cold; the electric car's
motor loss map is generic and its reduction ratio was chosen so the
motor's maximum speed gives the real top speed. Close numbers here mean
the model is in the right range, not that it predicts a new vehicle within
a known error.

## Not validated

Everything else, including every component model on its own: battery
(internal resistance only; no current or voltage limit, no ageing, no
temperature), electric motor and inverter (generic loss maps), combustion
engine (no warm-up, turbo lag or restart cost), gearbox and clutch, tyres
and brakes, fuel cell, DC-DC converter and auxiliary loads. There is no
thermal model. See [Known issues and limits](KNOWN-LIMITS.md) for what is
known to be wrong or missing.

## Rules for any accuracy claim

Until a figure on this page is marked *validated*, LightSim's README,
website, store listings and papers do not claim accuracy. When they do,
every claim states:

1. **the data set** (which vehicles and tests, and where the data is
   published),
2. **how many vehicles**,
3. **the metric** (mean or maximum error, on which quantity),
4. **the tolerance** reached,
5. **blind or calibrated** — whether any input was tuned to the data it is
   compared with,

and links to a public report with the method and the full results. For
example: *"Reproduces the EPA unadjusted city and highway energy of 7
production electric cars within ±5 % after calibrating the e-drive; blind
predictions within ±15 %."*

Not to be claimed until then: "accurate", "validated", "certified", or that
LightSim replaces any named commercial tool.
