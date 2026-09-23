# What is validated, and what is not

- Last reviewed: 23 September 2026, for version 0.1.0. This page is updated
  with every release, together with [Known issues and limits](KNOWN-LIMITS.md).

**In one line:** SimStudio's results have **not** been validated against
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
| Controller timing | results do not depend on the recording step (1, 0.1 and 0.02 s give the same figures) | `test_control_rate.py`, `test_verdict.py` |
| Engine | full throttle gives the full-load curve; fuel is cut on overrun; the rev limiter cuts fuel and torque; CO₂ follows fuel with the tank's factor | `test_engine.py` |
| Electric motor | spin losses are counted once; a powered motor has no extra drag | `test_motor_losses.py` |
| Run status | a car that cannot follow the cycle, does not move, or runs out of energy is never reported as a success; untrustworthy figures carry a "not valid" flag | `test_verdict.py` |
| Regressions | the two examples' results are compared with stored reference results; every change to them is listed in `backend/tests/golden/CHANGES.md` | `test_golden.py` |

## Plausibility-checked: the two example cars

`backend/tests/test_examples_plausible.py` holds both examples to bands from
real cars of their class. Their test mass and road load come from public
data; their motor, engine and battery maps are generic (invented, marked
*synthetic* in [the data register](DATA-REGISTER.md)).

| Example | Figure | SimStudio | Reference | Band in the test |
|---|---|---|---|---|
| Battery Electric Car (2021 Cupra Born values from FASTSim) | WLTC energy at the battery | 14.0 kWh/100 km | about 15–16 kWh/100 km rated at the charging socket, charging losses included (background knowledge, unverified) | 13–17 kWh/100 km |
| | 0–100 km/h | 7.2 s | 7.3 s (maker's figure, background knowledge) | ±10 % |
| | Top speed | 160 km/h | 160 km/h (limited) | ±2 %, and within the motor's maximum speed |
| P2 Hybrid Car (Hyundai Ioniq Hybrid test mass and EPA road load) | EPA city cycle (UDDS) fuel | 2.67 l/100 km | 2.91 l/100 km (EPA 2022 test car list) | 2–5 l/100 km, and at most 4.5 after correcting for the battery's change of charge |
| | EPA highway cycle (HWFET) fuel | 3.13 l/100 km | 2.94 l/100 km (EPA 2022 test car list) | same as the city cycle |
| | Battery charge at the end | same as at the start | charge-sustaining | within 1 % of the start |

Why this is not validation: the hybrid's road load was fitted to EPA's
coefficients for that car, its engine and motor maps are generic, and it has
no cold start; the electric car's motor loss map is generic and its
reduction ratio was chosen so the motor's maximum speed gives the real
top speed. Close numbers here mean the model is in the right range, not
that it predicts a new vehicle within a known error.

## Not validated

Everything else, including every component model on its own: battery
(internal resistance only; no current or voltage limit, no ageing, no
temperature), electric motor and inverter (generic loss maps), combustion
engine (no warm-up, turbo lag or restart cost), gearbox and clutch, tyres
and brakes, fuel cell, DC-DC converter and auxiliary loads. There is no
thermal model. See [Known issues and limits](KNOWN-LIMITS.md) for what is
known to be wrong or missing.

## Rules for any accuracy claim

Until a figure on this page is marked *validated*, SimStudio's README,
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
SimStudio replaces any named commercial tool.
