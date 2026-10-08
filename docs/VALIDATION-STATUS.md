# What is validated, and what is not

- Last reviewed: 7 October 2026, for version 0.3.0. This page is updated
  with every release, together with [Known issues and limits](KNOWN-LIMITS.md).

**In one line:** the energy four production electric cars use on EPA's
city and highway test cycles has been compared with EPA's official results,
blind (no input tuned to them): LightSim is within 8 % on all eight
figures (4.1 % on average). Nothing else is validated: the engine is
tested against exact answers, and the example cars are checked for
believable numbers, but that is not the same as validation.

Three words are used carefully on this page:

- **Verified** — the code gives the answer the equations say it should
  (a hand-calculated result, or a quantity that must be conserved).
- **Plausibility-checked** — a result falls inside a band taken from real
  vehicles of the same class. It shows the model is not badly wrong; it does
  not show it is right, because some inputs were tuned or invented.
- **Validated** — a result is compared with measurements of the same real
  vehicle, on the same test, with inputs that were not tuned to that
  measurement, and the error is reported. Only the reference suite below
  meets this.

## Verified: the engine does what its equations say

Each of these is an automatic test that runs on every change
(`backend/tests/`):

| What | How it is checked | Test file |
|---|---|---|
| Time and distance | at a constant 10 m/s, distance is exactly 10 m × t at every recorded time; runs stop exactly at the case duration | `test_time_base.py` |
| Energy is conserved | in a pure acceleration the battery's energy equals the car's kinetic energy plus the tyre-slip loss within 0.5 % (a 5 % leak anywhere would fail); the summary energy equals the integrated power channel | `test_energy.py`, `test_source_limits.py` |
| Battery and fuel-cell limits | at minimum charge, maximum charge and power limits the motor gets only what the source can give; regen above the charge limit goes to the friction brakes | `test_source_limits.py` |
| Battery output power limit and voltage class | the limit holds the terminal power: the recorded volts × amps stay within 0.1 % of a 20 kW limit (2.5e-7 measured), the motor draws no more, and the time held at the limit equals the motor's time limited by supply; recuperation is not limited (−99 kW under a 20 kW limit); with the limit only checked, the car drives as without it and the check fails at 119.95 kW against 80 kW; the check's moving average gives 40 kW for a 0.2 s, 100 kW pulse over a 0.5 s window and matches a brute-force integral to 1e-12; a 630 V pack (150 cells at 4.2 V) is flagged against 600 V by Data Checks and the run, and a 588 V pack recuperating at 627.5 V by the run; the usable energy left matches hand trapezoids of the OCV table; a check a stopped run passed so far is marked not valid | `test_power_limit.py`, `test_data_checks.py` |
| Load transfer and downforce | the rear axle gains m·a·h/L under a steady acceleration and the front axle under braking within 0.5 % (0.011 % and 0.034 % measured), each step's loads follow the previous step's acceleration, and the axles carry the weight together; downforce adds ½·ρ·CzA·v² at −7 °C within 0.1 % (5e-10 measured), split by the aero balance, and adds to the rolling resistance; with the driven wheels spinning, the acceleration matches μ·m·g·s/(m + 2J/r² ∓ μ·m·h/L) within 0.5 % driving either axle (0.03 % measured); a lifting axle carries nothing, the other the whole weight, and the car accelerates at μ·g; a lift greater than the weight leaves every wheel at 0 and the run says so; standing on a 20 % grade moves m·g·sin θ·h/L to the rear axle; with a CG height and a downforce area of 0 each wheel keeps its share of m·g·cos θ and the examples' results stay within their golden tubes (they were identical to 1e-6 when load transfer was added); Data Checks refuse a CG height while all wheels are on one axle | `test_load_transfer.py`, `test_data_checks.py`, `test_golden.py` |
| Battery charge | a constant 1C discharge from 100 % reaches 0 % at 3,600 ± 1 s for differently shaped OCV tables; a battery without an Ah value gives out its Usable Capacity from full to empty; coulombic efficiency acts on charge only; the SOC change matches the current that flowed | `test_battery_charge.py`, `test_source_limits.py` |
| Controller timing | results do not depend on the recording step (1, 0.1 and 0.02 s give the same figures, and the hybrid example's engine starts and stops at the same solver step at 1, 0.1 and 0.01 s) | `test_control_rate.py`, `test_verdict.py` |
| Road load | air drag follows the Ambient's air density, rho = p / (R · T): −7 °C gives 1.113 times the drag of 23 °C within 0.5 %, also when set as a case value or a live edit, and so does a coefficient C; 85 kPa gives 85 / 101.325 of the drag at 101.325 kPa within 0.5 %; an Ambient at its defaults gives what a model without one gets, and of two Ambients the first sets the air; uphill, the car slows by g · (sin θ + rolling resistance · cos θ) of the slope angle within 0.1 % at 10 and 25 % grades, and a coefficient A takes the cos too; launching up a 25 % grade, the tyres' grip tops out at μ · m · g · cos θ within 0.1 %; a coast-down of a car entered with road-load coefficients gives back A, B and C within 0.5 %; with *Coefficients Include Driveline Losses* the motor gives the road load through the final drive and differential with no loss, without it through their efficiencies; Data Checks catch axle losses counted twice, a negative coefficient A or C, several Ambients and an Ambient pressure or temperature out of range or in the wrong unit, and the run warns about such a value set per case | `test_road_load.py`, `test_data_checks.py` |
| Engine | full throttle gives the full-load curve; fuel is cut on overrun; the rev limiter cuts fuel and torque; CO₂ follows fuel with the tank's factor | `test_engine.py` |
| Electric motor | spin losses are counted once; a powered motor has no extra drag | `test_motor_losses.py` |
| Maximum speed and map edges | a motor stops at its maximum speed (a 300 km/h target holds it between 97 and 100 % of 12,000 rpm), also when that is set lower or changed during a run; a motor driven above it gives no drive torque and is reported; a run that leaves a table set to *Error* stops and names the table, the axis, the value and the time; *Clamp* and *Linear* give the edge value and the edge slope and are reported with their time outside; a motor or engine held at its limiter is not counted as over speed, also when a light one overshoots it by a step (a free motor at 12,000 or 3,000 rpm, a free engine of 0.05 kg·m²), while an engine the wheels drive above it is; every table a run reads is counted once per solver step, and the library defaults pass the map cross-checks, which catch a motor map that does not start at 0 rpm and a fuel-cell curve that does not start at 0 A | `test_map_edges.py`, `test_engine.py` |
| Run status | a car that cannot follow the cycle, does not move, or runs out of energy is never reported as a success; untrustworthy figures carry a "not valid" flag; a motor run past its map's voltage data or driven above its maximum speed for longer than 1 % of the run (at least 2 s) is never a success, and the message and flag name the motor, how far past and for how long, while a Lookup block past its table is not judged; a run a stop cut short is *cancelled*, one stopped as it ended is not, and one an error cut short has its per-distance figures marked not valid; a 0-100 km/h performance test succeeds with the time of a full-throttle run, read where the speed crosses the target, and then holds the target without switching between throttle and brakes; a top-speed test reports the car's highest speed; a stopped performance test does not say the car fell short, and a car that starts at its target gets no time | `test_verdict.py`, `test_examples_plausible.py`, `test_api.py`, `test_map_edges.py` |
| Parameter limits | a value just outside any of the 81 limits in the component catalog is exactly one Data Checks error that says what is allowed, and the edges and the library's defaults pass; so is a case's own value, which then stops that case's runs (not the other cases'); every library default, and every example value and case value, keeps to the limits; each limit prints the same in the engine and the app | `test_data_checks.py`, `test_library_text.py`, `test_broken_models.py` |
| Drive cycles | the bundled WLTC class 3b, EPA city (UDDS) and highway (HWFET) traces have their published duration, and their published distance within 0.1 % (they are 0.001, 0.005 and 0.032 % off); the WLTC's 1 Hz speeds sum to 83,758.6 km/h; a Driving Task set to a cycle drives that trace, a case's own typed points still win over it, and a cycle this version does not have is a Data Checks error and stops the run, also when a case sets it (it then stops that case only) | `test_cycles.py`, `test_data_checks.py` |
| Acceleration test | with the tyres' grip made unlimited (μ 100, slip stiffness 100, 1 ms step) the timed 75 m of the Battery Electric Car matches a hand-integrated RK4 point mass within 1 % (+0.10 % measured, speed at the line −0.18 %); the run ends in the solver step that reaches the line, and the time and speed read inside it are the same at a 0.01 and a 0.1 s output step and match where a full-throttle performance run's distance channel crosses 75 m (5.527 s); a 0.3 m start line starts the timer (5.216 s) and the gap to a reference time is signed; with no target wired the Driver holds full throttle and the run succeeds, also through the app, whose Data Checks ask for a Target Speed only once the model has a case that is not an acceleration test or a lap case; a car that misses the line within the duration gets one warning and no time, a stopped run none; a Formula Student-sized car is at the grip limit ≥ 95 % of the run at μ 1.5 (100 % measured) and never at μ 3; the peak and mean terminal power match the power channel and the energy rows, and with the Formula Student preset the power check passes at 80 kW; the case, its line and its reference time survive a save | `test_acceleration.py`, `test_project_roundtrip.py` |
| Lap mode | on the skidpad (9.125 m radius) the cornering speed equals the closed form √(μ·m·g/(m/R − μ·½·ρ·CzA)) with and without downforce (to 1e-6 %) and the timed circle takes 2πR/v within 0.5 % (−0.02 % and −0.03 %, rounded to 1 ms); with a CG height, track widths and a load sensitivity of −0.2/kN the speed is the root of the four wheels' grip (to 1e-8); a lap case on the 75 m straight takes the time-domain acceleration test's time within 2 % (+0.33, +0.42 and 0.00 % at μ 1.2, 1.5 and 2.5, +0.85 % with load transfer and load sensitivity); over an Autocross lap the battery's net energy matches the kinetic energy, road load, friction brakes and gear and motor losses, from the recorded channels, within 0.5 % (+0.002 %; +0.03 % where the battery's maximum-power point limits the motor, also with a 1 kW consumer; +0.002 % with the consumer behind a 90 % DC-DC), and so does the summary's lap energy balance; with a second E-Motor on the front wheels through a 6.5 final drive the car passes that motor's top speed (86.7 km/h) on a 600 m straight and the balance closes (0.002 %); after a live μ edit the next laps corner within the new μ and lap 3 takes a fresh run's time; a battery that reaches its minimum SOC, or a motor past its Full-Load Torque table, marks the lap times not valid; with 100 N·m brakes the friction brakes are never asked for more than their Max Torque and the lap lies between the stock-brake and the friction-only lap, and regeneration taken away during the lap marks the lap times not valid; a 1 km lap solves in under 1 s (0.14 s); 1 m spacing is within 0.1 % of 0.25 m (0.07 %); flying laps after a standing first lap repeat to 1e-6; the time limited by each factor adds up to the total, and the time limited by a 60 kW Output Power Limit equals the battery check's time held at it; the layouts close and keep to FS Rules 2026 v1.1 (FSG) D 4.1, D 5.1.1, D 6.1 and D 7.1; the run and Data Checks refuse engines and clutches (also an engine and generator that drive no wheels), a missing Race Track, a one-axle car, a Custom curvature above 0.5 1/m or not starting at 0 m, a fraction of a lap and a model without a Driver; a load sensitivity slows the time-domain launch, and without one the tyre's μ and the drive cycles' results are unchanged (identical golden results at 1e-6) | `test_lapsim.py`, `test_golden.py` |
| Regressions | the electric and hybrid examples' results are compared with stored reference results; every change to them is listed in `backend/tests/golden/CHANGES.md` | `test_golden.py` |
| Exact answers | coast-down under air drag alone and with rolling resistance, a constant-torque launch, braking distance v₀²/2a, coasting up a 25 % grade stops at the height h = v₀²/2g, the energy a clutch loses joining two inertias (½·J₁J₂/(J₁+J₂)·ω²) and a battery RC pair's step response each match their closed-form solution within 0.5 % | `test_numerics.py` |
| Step convergence | the error against the exact answer halves when the solver step halves, for the vehicle and for the battery RC pair | `test_numerics.py` |
| Stability | a grid of tyre *Slip Stiffness* 10–300 × solver step 2.5–20 ms, for a launch (slip stays below 0.1) and a car braked to a stop (stays below 0.5 km/h). Today the launch is stable only at stiffness 10 up to the 10 ms step and 30 up to 5 ms (the brake hold also at stiffness 10 with a 20 ms step) (see [Known issues and limits](KNOWN-LIMITS.md)); the test lists the unstable cells, so a new instability fails it and so does a fixed one until the list is shortened | `test_numerics.py` |
| Speed | the first 120 s of two example cases (BEV City, hybrid Mixed) run at most 10 % slower than on the commit a change starts from (CI's performance job, same runner, best of 5 each) | `test_performance.py` |

Each bug fixed in 0.2.0 has a test that fails if it comes back. This was
checked once, for 0.3.0, by putting each 0.1.0 behaviour back into a scratch
copy of the engine and running the whole suite (for MOD-04 and MOD-05 the
engine code, not the old default maps):

| Fixed in 0.2.0 | Tests that fail when it is undone |
|---|---|
| Controllers update every 10 ms (ENG-01) | `test_control_rate.py`, `test_verdict.py`, `test_examples_plausible.py`, `test_expansion.py`, `test_golden.py` |
| Each recorded point holds the state at its own time (ENG-03) | `test_time_base.py`, `test_control_rate.py`, `test_solver.py`, `test_engine.py`, `test_live_params.py`, `test_source_limits.py`, `test_verdict.py`, `test_numerics.py` (constant-torque launch), `test_golden.py` |
| Motors draw only what their supply can give (ENG-02) | `test_source_limits.py`, `test_verdict.py` |
| Motor spin losses counted once (MOD-04) | `test_motor_losses.py`, `test_golden.py` |
| The engine reaches its full-load curve (MOD-05) | `test_engine.py`, `test_examples_plausible.py`, `test_golden.py` |
| A run that did not drive its cycle is no success (VAL-02) | `test_verdict.py` |
| Wheel load shares scaled to 100 % (MOD-06) | `test_numerics.py` (coast-down with rolling resistance, both stability grids), `test_solver.py` |
| The Driver recuperates up to the charge limit (MOD-02) | `test_source_limits.py` |
| Live edits survive gear shifts (ENG-04) | `test_live_params.py` |
| Gear losses act on the power through each gear (MOD-03) | `test_gear_losses.py`, `test_motor_losses.py`, `test_golden.py` |
| Every part starts at the vehicle's initial speed (MOD-19) | `test_solver.py`, `test_engine.py` |
| Formula Student events (MOD-43, MOD-44, STU-38) | the dynamic points follow FS Rules 2026 v1.1 (FSG) D 9.1.1 and table 11 (Pmax at Tmin, Pmin at and past Tmax, hand-worked points between) and the efficiency D 9.4 (75 points at EFmin, 18.75 at 1.5 EFmin, 0 past 2 EFmin); the FSG 2020 efficiency formulas give the FSG score calculator's results on hand-worked values (the calculator ships no test values); the four events of the FS example run in under 20 s (8.9 s measured); the skidpad time is the mean of the two circles, the endurance energy counts regeneration at 90 %, the endurance stops at half distance and its time leaves out the restart lap; a voltage over 600 V or an empty pack scores 0; lift-and-coast trades time for energy monotonically, and an energy target of 5.0 or 4.5 kWh ends within 2 % of it (−0.54 %, −0.22 %); an endurance from an imported trace reports its net energy, RMS power and lowest pack voltage | `test_fs_events.py` |
| Lap import (STD-35) | a lap simulator's trace against distance keeps its distance within 0.5 % (TUM and OpenLAP layouts, LightSim's own lap); a MoTeC-like session gives its fastest full lap; semicolons, decimal commas and speed spikes are read and flagged; a lap repeated to 22 km has one driver change stop; an imported lap runs as a cycle through the API | `test_laplog.py` |
| Traction Control (MOD-45) | on the FS example's 75 m at a 2 ms step the slip stays within ±0.02 of the 0.1 target from 0.4 s until the power limit takes over, with no spike above 0.3 once moving (about 7 without the block); the defaults stay stable at the 10 ms step | `test_traction_control.py` |
| Lap mode calibration (VAL-38) | on LightSim's own laps (0.9 × grip, 2.5 m² CzA, with noise), calibrated on the Autocross and checked blind on it driven the other way, the lap time is within 5 % (−0.5 %), the speed RMS under 4 km/h and the energy within 5 % (0.6 %). This checks the method only: no real logged lap has been used | `test_calibrate.py` |

## Validated: electric cars against EPA's tests (reference suite v1)

The reference suite (`backend/validation/`, roadmap VAL-05) builds four
production electric cars from published data and drives EPA's city (UDDS)
and highway (HWFET) cycles. Each result is compared with EPA's
*unadjusted* energy from the wall socket for the same car, the figure
behind the window sticker before EPA's real-world adjustment. CI runs it on
every change (`test_reference_suite.py`).

- **Data**: test weight, road-load coefficients, gearing and results from
  EPA's 2022 Test Car List and fueleconomy.gov; the motor, battery,
  auxiliary and charger values from FASTSim's Apache-2.0 vehicle files
  (sources in `backend/validation/suite.json` and the
  [data register](DATA-REGISTER.md), DR-61 to DR-66). The Model 3's motor
  power is EPA's rated 257 hp (192 kW), since FASTSim's 239 kW comes from
  a website LightSim may not use (case version 2, suite 1.1, 8 October
  2026: its gaps were +7.0 and +8.6 %).
- **Blind**: no input was tuned to these results. The motor's efficiency is
  FASTSim's one generic curve, the same for all four cars; the rules that
  turn the data into a model are fixed in `suite.json` and the suite's
  [README](../backend/validation/README.md).
- **Metric**: Wh per km at the wall, LightSim's battery energy plus the
  battery's own losses divided by a charger efficiency of 0.86.

Measured on 7 October 2026 (the Model 3 on 8 October):

| Car | Cycle | EPA, Wh/km | LightSim, Wh/km | Gap | EPA's repeat tests |
|---|---|---|---|---|---|
| 2022 Tesla Model 3 RWD | UDDS | 113.0 | 118.6 | +5.0 % | one test |
| | HWFET | 123.1 | 132.2 | +7.4 % | one test |
| 2022 Chevrolet Bolt EUV | UDDS | 117.5 | 118.2 | +0.6 % | 114.1-121.1 (6 % apart) |
| | HWFET | 140.6 | 151.1 | +7.5 % | 139.8-141.3 |
| 2022 Nissan Leaf (40 kWh) | UDDS | 119.3 | 117.6 | −1.4 % | 118.3-120.4 |
| | HWFET | 148.3 | 154.3 | +4.1 % | 148.2-148.4 |
| 2022 MINI Cooper SE | UDDS | 123.5 | 115.1 | −6.8 % | one test |
| | HWFET | 145.9 | 145.9 | 0.0 % | one test |

Mean of the gaps' sizes 4.1 %, largest 7.5 %, against a tolerance of 15 %
(blind). The suite also checks, for each car:

| Check | Tolerance | Measured |
|---|---|---|
| A virtual coast-down from 130 km/h gives back EPA's road load, 20-120 km/h | 2 % | 0.01-0.02 % |
| Halving the solver step (10 to 5 ms) moves the city energy | 0.5 % | 0.16-0.18 % |
| Exact-answer tier: three coast-downs with closed-form answers | 0.5 % | 0.000-0.005 % |

What this does and does not show:

- The charger efficiency alone moves every figure: 0.82 or 0.90 instead of
  0.86 shifts them by about ±5 %. EPA's own repeat tests of one car differ
  by up to 6 % (the Bolt EUV's city tests). Gaps below about 5 % are
  within that noise.
- The highway figures are higher than EPA's for three cars of four. The
  motor's generic curve and the flat 350 V battery are the likeliest
  reasons; a calibrated tier (motor losses tuned on one cycle, checked on
  the other) is the next step.
- It covers steady energy use on two gentle cycles at 20-25 °C: no
  acceleration, top speed, cold weather, heating, ageing or range test.
- The Bolt EUV and the Leaf use FASTSim's powertrain values of related
  models (2017 Bolt EV, 2016 Leaf 30 kWh) with the 2022 cars' own motor
  power and battery size where FASTSim has no file; each case file says
  which.
- Four cars of one class (compact and mid-size electric cars) is a small set; the
  roadmap asks for seven, within ±5 % calibrated.

## Plausibility-checked: the example cars

`backend/tests/test_examples_plausible.py` holds the examples to bands from
real cars of their class. The electric and hybrid cars' test mass and road
load come from public data; their motor, engine and battery maps are generic (invented, marked
*synthetic* in [the data register](DATA-REGISTER.md)).

| Example | Figure | LightSim | Reference | Band in the test |
|---|---|---|---|---|
| Battery Electric Car (2021 Cupra Born values from FASTSim) | WLTC energy at the battery | 14.1 kWh/100 km | about 15–16 kWh/100 km rated at the charging socket, charging losses included (background knowledge, unverified) | 13–17 kWh/100 km |
| | 0–100 km/h | 7.1 s | 7.3 s (maker's figure, background knowledge) | ±10 % |
| | Top speed | 160 km/h | 160 km/h (limited) | ±2 %, and within the motor's maximum speed |
| P2 Hybrid Car (Hyundai Ioniq Hybrid test mass and EPA road load) | EPA city cycle (UDDS) fuel | 2.84 l/100 km (no cold start) | 2.91 l/100 km (EPA 2022 test car list) | 2–5 l/100 km, and at most 4.5 after correcting for the battery's change of charge |
| | EPA highway cycle (HWFET) fuel | 3.24 l/100 km | 2.94 l/100 km (EPA 2022 test car list) | same as the city cycle |
| | Battery charge at the end | same as at the start | charge-sustaining | within 1 % of the start |
| FS Electric (generic) (typical Formula Student values, no real car) | 75 m acceleration, from the start line | 3.75 s, 119 km/h at the line, 0–100 km/h in 2.95 s | FS Czech Republic 2025, best times of 35 EV teams: 3.51–6.44 s, median 3.91 s | 3.5–4.5 s, 100–130 km/h, 2.5–4.0 s; within 0.5 % at a 1 ms step; faster with each 10 kW of Output Power Limit from 40 to 80 kW (4.23 to 3.75 s) |
| | Endurance energy (lap mode, 22.5 km, Output Power Limit 30 kW) | 5.33 kWh net at the accumulator, 18 % of the energy drawn recuperated, 25 % charge left, 22.9 kW RMS | FS Czech Republic 2025 efficiency, 14 scored teams: 3.19–6.15 kWh, median 5.25 kWh | 3.19–6.15 kWh, 10–40 % recuperated, at least 10 points above the minimum charge, 15–35 kW RMS |
| | Rule values (FS Rules 2026 v1.1 (FSG)) | 80.0 kW peak at the terminals, 594 V at most, 155 A at most | EV 2.2.1 80 kW, EV 4.1.1 600 V DC, EV 2.2.2 500 A | volts × amps ≤ 80 kW + 0.1 % and the power check passes; ≤ 600 V and ≤ 500 A in every case |

Why this is not validation: the hybrid's road load is EPA's target
coefficients for that car (with the axle's losses counted once), but its
engine and motor maps are generic, and it has no cold start, so its city
figure reads below EPA's, whose city test starts cold; the electric car's
motor loss map is generic and its reduction ratio was chosen so the
motor's maximum speed gives the real top speed. Close numbers here mean
the model is in the right range, not that it predicts a new vehicle within
a known error. The FS car is no real car at all: its values are typical,
and its results are compared with the spread of a whole competition's
field, which shows only that its pace and energy use are those of a
Formula Student car.

## Not validated

Everything but the energy use above, including every component model on its own: battery
(internal resistance only; no current or voltage limit, no ageing, no
temperature), electric motor and inverter (generic loss maps), combustion
engine (no warm-up, turbo lag or restart cost), gearbox and clutch, tyres
and brakes, fuel cell, DC-DC converter and auxiliary loads. There is no
thermal model. Lap mode's lap times and energy are not compared with any
real car or logged lap: VAL-38's calibration has been checked only on
LightSim's own laps. See [Known issues and limits](KNOWN-LIMITS.md) for what is
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

The one claim the reference suite supports today, in this form only:
*"LightSim reproduces the EPA unadjusted city (UDDS) and highway (HWFET)
energy at the wall of 4 production electric cars of model year 2022 within
±9 % (mean 4.5 %), blind: no input tuned to those results"*, with a link to
this page. It says nothing about any other figure, and "accurate" or
"validated" without that context stays off limits.
