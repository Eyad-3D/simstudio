# Release notes

Each release lists what changed, and above all **which results changed and
why**, so that you can tell whether a number you got from an earlier version
still holds. The full record of every change to the reference results is in
[`backend/tests/golden/CHANGES.md`](../backend/tests/golden/CHANGES.md).

## 0.3.0 — first public release (not released yet)

0.2.0 was prepared but never published, so everything listed under
[0.2.0](#020--not-published-its-changes-ship-in-030) below also reaches you
for the first time in this release. Coming from SimStudio 0.1.0, read both
sections.

### Your results will change — here is why

| What changed | Effect on results | Roadmap |
|---|---|---|
| Battery state of charge counts the charge that flows (amp-hours), as battery management systems and datasheets do, and the OCV table is read at that SOC | SOC moves by up to about 2 points at mid-charge for the same energy; energies, fuel and consumption move by at most 0.001 kWh or kg (BEV WLTC ends at 84.93 % instead of 84.61 %, 84.92 % with the air density below; hybrid fuel consumption unchanged to 0.01 l/100 km). Control scripts that switch on SOC switch at slightly different moments, and a run from 90 % down to a 4 % floor gets 0.35 % less energy on the default OCV table | MOD-38 |
| E-Motors stop at their maximum speed: the drive torque falls to zero over the last 2 % below it, and above it the inverter is off (no drive, no regeneration) | A 300 km/h target on the default motor now tops out at 152 km/h with the motor at 11,921 of 12,000 1/min, instead of 269 km/h at 21,238 1/min on made-up torque. The examples do not reach their maximum speeds | MOD-18 |
| Motor and engine maps no longer extend their data: a run that reads a motor's full-load map outside its speed data, a motor's loss map or an engine's fuel map outside its speed or torque data, or a fuel cell's polarization curve outside its current data stops with an error naming the table, the axis, the value and the time | Models whose maps do not cover where they run now fail instead of finishing on made-up values; Data Checks warn before the run. Engines below their full-load curve's first speed (starting) use that point, as before | MOD-18 |
| The friction brake's default inertia is 0.18 kg·m² instead of 0.6 (a 330 mm disc) | Cars with default brakes accelerate and brake a little more easily: BEV WLTC 14.04 → 14.03 kWh/100 km and 0-100 km/h 7.16 → 7.11 s, hybrid EPA city 2.95 → 2.94 and highway 3.30 → 3.29 l/100 km | MOD-18 |
| New library defaults that fit the default E-Motor's 250–396 V map: voltage source and DC-DC output 350 V (were 400 and 800 V), fuel-cell curve 396–250 V (was 420–264 V, now 100 kW at 400 A instead of 105.6 kW); the default engine's full-load peak is 175 N·m (was 178) and its fuel map starts at 800 1/min | Models built on these defaults change; the examples do not use them | MOD-18 |
| A run you stop part-way ends *cancelled* instead of *warning*; a stop that arrives as the run ends no longer marks a complete run | The run list, Results and study tables show a stopped run as *incomplete (stopped at t = …)*, and its per-distance figures stay marked *not valid: run cancelled at t = …*; the examples do not change | VAL-39 |
| A new case kind, *Performance*, for 0-100 km/h and top-speed tests: the Driver holds full throttle until the car reaches the target, then holds it there, and the run reports *Time to … km/h* and *Maximum speed* instead of *Cycle not followed* | Such a run can now be a *success* with valid figures. The time is taken where the car's speed crosses the target, at full throttle: a 0-100 km/h step on the Battery Electric Car takes 7.10 s (as a cycle, its Driver never quite reached 100 km/h). Cases set to *Cycle*, the default, do not change | VAL-39 |
| A *success* also means the physics stayed in range: a motor, engine, battery or fuel cell outside its table data or above its maximum speed for more than 1 % of the run (at least 2 s, the speed trace's allowance) ends the run as *warning* | The message names the part, how far past and for how long (for example *E-Motor 'E-Motor' ran 43 V past its 'Full-Load Torque' table for 30 s of 30 s*), and Consumption, Fuel consumption, CO₂ emissions and a performance test's rows are marked *not valid* with that reason. Runs that followed their cycle on made-up map values used to be a *success*. The examples stay inside their data and do not change | VAL-39 |
| Air drag uses the air density of the Ambient block's temperature and pressure, rho = p / (R · T) (the first Ambient, if a model has several); without one, 20 °C and 101.325 kPa give 1.204 kg/m³ instead of 1.2 | 0.34 % more drag without an Ambient: BEV City 11.11 → 11.12 and WLTC 14.03 → 14.05 kWh/100 km. With an Ambient, its air counts: −7 °C gives 11 % more drag than 23 °C, 35 °C at 85 kPa 20 % less than without one. If you scaled Cd for cold or thin air, as the 0.2.0 known limits advised, undo that when you add an Ambient | MOD-11 |
| Slopes are exact: the weight pulls the car back with m·g·sin of the slope angle and presses on the road with m·g·cos (before: grade ÷ 100, and no cos) | On grades the slope force and rolling resistance are 0.5 % lower at 10 % and 3 % lower at 25 %, and so is the tyres' grip; flat roads do not change | MOD-11 |
| The P2 Hybrid Car example takes its road load as EPA's own coefficients (A 68.64 N, B 0.9093 N/(km/h), C 0.025078 N/(km/h)²) with *Coefficients Include Driveline Losses* ticked, so the driveline drag they hold is no longer counted again in its final drive (98 %); its cases start at re-balanced charges (UDDS 56.74 %, HWFET 58.87 %, Mixed 51.92 %) | EPA city (UDDS) 2.94 → 2.84, highway (HWFET) 3.29 → 3.23, Mixed Cycle 2.93 → 2.88 l/100 km, against EPA's 2.91 and 2.94. The city figure is now below EPA's: the model has no cold start | MOD-11 |
| The P2 Hybrid Car's control script starts or stops the engine only when asked for 0.2 s in a row (at standstill it still stops at once), so one step's reading of the input shaft, which rings for a few steps as the clutch closes, no longer switches it; its cases start at re-balanced charges (UDDS 56.34 %, HWFET 58.92 %, Mixed 51.96 %) | EPA city (UDDS) 2.84 l/100 km unchanged, with 30 engine starts instead of 32; highway (HWFET) 3.23 → 3.24, Mixed Cycle 2.88 l/100 km unchanged. The engine's starts no longer change with small changes to the model or the step (fuel still moves by up to about 0.004 l/100 km at the 10 ms step): with the example as MOD-18 left it, a brake inertia of 0.14 kg·m² or less added an engine start at the 10 ms step only | MOD-18 |
| The E-Motor's and Engine's Speed, a clutch's Slip Speed and the battery's Terminal Voltage hold the state at their own time, as 0.2.0 promised for every channel (before: the state one solver step, 10 ms, earlier); a clutch that starts open shows its slip from t = 0 on, and a clutch's Torque now includes the part the solver adds as the clutch locks | These channels move by one solver step: a motor spun up from rest no longer reads 0 1/min at t = 0.01 s, and a battery's RC-branch voltage (and the open-circuit voltage's fall with SOC) shows in the step it happens instead of one step later; the R0 drop already did. Scripts and PIDs that read them get the current value, so the hybrid example's channels move slightly. No figure changes: the BEV's, and the hybrid's EPA city (2.84 l/100 km), highway (3.24) and Mixed Cycle (2.88), with the same engine starts | ENG-03 |
| Shaft and Final Drive *Transmitted Power* is the power through that part (its input), not the total of every motor and engine on the driveline; Gearboxes, Differentials and Transfer Cases get the same channel, and each of them a *Losses* channel | In the P2 Hybrid Car's Mixed Cycle with a Shaft between the engine and the clutch, at t = 281 s the Shaft shows the engine's 9.75 kW and the Final Drive 8.88 kW, where both showed 9.2 kW; the Battery Electric Car's Final Drive shows what it did (one motor, no gears before it). No summary figure changes | MOD-10 |
| A battery's *internal losses* include the loss in its RC pair (current × the RC pair's voltage), as well as in R0 | Only batteries with an RC pair change: their internal losses rise by about the RC pair's share of the voltage drop. The examples have none | MOD-10 |
| Stored values and summary numbers keep every digit: 0.2.0 rounded each stored value to 5 decimals and the summary to 1 Wh, 1 g and 0.01 per 100 km | No result moves by more than that rounding, but small differences now show: one more kilogram on the Battery Electric Car takes its City Cycle from 11.1207 to 11.1240 kWh/100 km and its final SOC from 88.7640 to 88.7636 % (both read 11.12 kWh/100 km and 88.76 % before). The *Results* page shows at most 3 decimals; run files, study tables and their CSV have every digit | ENG-16 |
| A hybrid's *Cycle* cases run charge-balanced (see *New*) | A hybrid case that starts at a charge its cycle does not end with now reports the fuel of a charge-neutral run: the P2 Hybrid Car's Mixed Cycle started at 30 % gives 2.878 l/100 km instead of the single run's 4.048 (which held the fuel spent recharging the battery). The examples' cases already start balanced and keep their figures (one run); their summaries gain three rows and *Messages* one line | ENG-33 |
| The solver step gets smaller when the tyres' *Slip Stiffness* (or a propeller-type load) is too stiff for 10 ms | Models with stiff tyres no longer give numerical nonsense, and run slower: 5 ms from a *Slip Stiffness* of 20 (with the default share of weight on each wheel), 0.5 ms at 300. The FS Electric example (20) now runs at 5 ms: its 75 m time 3.744 → 3.751 s, 0-100 km/h 2.93 → 2.95 s, time held at 80 kW 3.77 → 3.62 s, 4.22 → 4.23 s at a 40 kW limit; its lap cases do not change. The Battery Electric and P2 Hybrid Cars keep 10 ms | ENG-14 |
| New library defaults with a source (CON-14): a Power Consumer draws 0.25 kW (was 2.5 kW, a heating level; FASTSim's auxiliary load for electric cars), a Wheel's rolling resistance is 0.0085 (was 0.012; the middle of EU tyre label class C, Regulation (EU) 2020/740), and the Engine's fuel map is a synthetic map with a best-efficiency region (best 221 g/kWh at 2,500 1/min; was a map of unknown origin, 277 g/kWh at 2,000 1/min and 100 N·m, now 227) | Models that keep these defaults use less energy: the Battery Electric Car with every value at its default takes 13.32 kWh/100 km on WLTC class 3b instead of 19.98 (the example itself, 14.05, does not change), and a default engine burns about 18 % less fuel at part load. The examples set their own values and do not change | CON-14 |
| Signal blocks (Script, PID, Lookup, Road Profile) that read the output of a signal loop now run after the loop's blocks; before, every block from the loop on ran in the order of its internal id, so such a block could read a value one solver step (10 ms) old | Only models with a loop of signal blocks change, by at most one step's delay on the blocks after the loop; Data Checks name the loop's blocks, their order and the value that arrives a step late. The examples have no loop and do not change | VAL-17 |

### New

- Models from other tools: drop an FMU file (a Co-Simulation FMU of FMI
  2.0 or 3.0, exported from Simulink, Dymola, GT-SUITE or a supplier) on
  the diagram and it runs with the rest of the car as an *FMU* block. Its
  *Properties* show the FMI version, kind, the tool that made it, a badge
  for where it runs (*Runs here*, *Windows only*, *Source only*) and the
  problems FMPy's checks find, in plain words; *Variables and pins* ticks
  the FMU's variables into pins and changes start values. LightSim asks
  once for each FMU whether it may run, and runs it in a separate,
  locked-down process, so a crashing or hanging FMU stops the run, not the
  app. It needs the optional FMU pack (FMPy); see [Use a model from another
  tool](help/how-to/use-an-fmu.md) and *Known issues* for what it does not
  do yet. In the automatic tests, 12 of the FMI standard's 14 Reference
  FMUs (version 0.0.39) give the same results as FMPy's own runner, to
  1e-6; the other two need what this version does not do (StateSpace has
  array signals, Clocks is a Scheduled Execution FMU).
  *Roadmap:* STD-01.
- Battery: *Charge Capacity* (Ah) and *Coulombic Efficiency (charging)*.
  Left at 0, the Charge Capacity comes from the Usable Capacity. Coulombic
  efficiency defaults to 100 %, not the 99 % first proposed: Li-ion cells
  store about 99.9 % of the charge put in (background knowledge,
  unverified), so 99 % would add a 1 % loss they do not have. Charge that
  is not stored counts as the battery's internal losses.
- E-Motor: *Maximum Speed* (1/min). Left at 0, it is the last speed point
  of the full-load curve, so projects from 0.2.0 get a maximum speed with
  no change to their files. It can be changed during a live run.
- Every table axis has an *outside the data* setting in the table editor
  of the parameter dialog: stop the run (*Error*), hold the edge value
  (*Clamp*) or extend the edge slope (*Linear*). The library sets Error on
  the speed and torque axes of the motor's full-load and loss maps and the
  engine's fuel map and on the fuel cell's current, Clamp on the others
  (battery SOC, motor voltage, drag tables, Lookup tables); a setting you
  change is saved with the part. (An engine's full-load curve has no
  setting: below its first speed it gives that point, above its last the
  rev limiter cuts in.)
- Run summary: for a table read outside its data, the share of the run
  outside it and the furthest point; for a motor or engine above its
  maximum speed, the share of the run above it and the highest speed (a
  machine that overshoots its limit on its own drive and falls back, as a
  limiter does, is not counted). These rows appear only when that happened.
- Data Checks compare the maps with each other and with the parts around
  them: a motor's maximum speed and loss map against its full-load map,
  the bus voltage against each motor's voltage axis, a motor full-load map
  that does not start at 0 1/min, an engine's fuel map against its
  full-load curve, and a fuel cell's curve against 0 A and its Maximum
  Current.
- Case settings: *Kind* (Cycle or Performance). A performance test is a
  step in the Driving Task's target from t = 0 (for example `0:100` for
  0-100 km/h, `0:250` for top speed); Run info says *performance test*. A
  target the car never reaches gives only the maximum speed and says so
  in Messages.
- A run status *cancelled*, for a run a stop cut short (shown as
  *incomplete (stopped at t = …)*).
- Vehicle: *Road Load From* (drag and rolling resistance, as before, or
  coefficients A/B/C), with *Road Load A (f0)* in N, *B (f1)* in N/(km/h)
  and *C (f2)* in N/(km/h)², as WLTP publishes them (EPA's lbf, lbf/mph and
  lbf/mph² values × 4.448, × 2.764 and × 1.717). C follows the Ambient's air
  density. *Coefficients Include Driveline Losses*, on by default: target
  coefficients from a coast-down already hold the drag of the gears the
  wheels turn, so the final drives, differentials and transfer cases run
  lossless (gearboxes and motors keep their losses); untick it for
  dyno-set coefficients.
- Ambient: sets the air density for the Vehicle's drag. Its temperature
  and pressure can be set per case, swept in a study or changed during a
  live run. Its default pressure is 101.325 kPa (was 101.3), so an Ambient
  at its defaults gives the air a model without one gets.
- Data Checks: axle gear losses counted twice (coefficients with the tick
  off and a final drive, differential or transfer case below 100 %);
  several Ambients (the first one counts); an Ambient temperature at or
  below −273.15 °C or a pressure of 0 or less (errors), and one outside
  −60 to 60 °C or 50 to 110 kPa (a warning: a pressure typed in bar would
  all but remove the drag; the run warns too, also about such a value set
  per case, swept in a study or edited live, which Data Checks do not see);
  a negative coefficient A or C (a warning: it pushes the car along); a
  road-load coefficient that is not a number or a negative Maximum Speed
  (errors).
- Battery: *Output Power Limit* (kW at the terminals, volts × amps, as a
  Formula Student energy meter measures it), with a *Power Limit Margin*,
  a *Power Check Window*, *Hold Power to Limit* (untick it to only check
  the limit) and a *Voltage Class* (V); 0 turns the limit and the class
  off, as in every existing model. The limit caps discharge only: the
  motors get what is left after the other loads, and recuperation is not
  limited. Being held at it is an *info* message, not a warning, so such
  a run can be a *success*. The run summary then gives the peak terminal
  power, the peak averaged over the window (checked against the limit),
  the time held at (or over) the limit, the highest pack voltage
  (open-circuit at 100 % SOC, or at the terminals while recuperating;
  checked against the class), the lowest pack voltage and the usable
  energy left (it fails when the battery reached its minimum SOC). A
  failed check is a warning. Data Checks warn when the open-circuit
  voltage at 100 % SOC is above the Voltage Class. On the Battery Electric
  Car, an 80 kW limit takes its 0-100 km/h from 7.10 to 12.76 s.
- Summary rows can carry a limit and a *pass* or *fail* marker.
- Presets: the battery's *Apply preset: Formula Student Electric* sets
  80 kW, a 0.5 s window, 600 V, Hold Power to Limit and a 500 A *Max
  Discharge Current* in one step (one undo). Its values come from FS Rules
  2026 v1.1 (FSG) EV 2.2.1, EV 2.2.2, EV 4.1.1 and D 10.4.1, checked
  against FSUK 2026 Rules V1.0 and FSAE Rules 2025 V1.0, which differ in
  detail.
  Check the current season's rules before relying on them.
- Load transfer and downforce: the Vehicle gets a *Centre of Gravity
  Height*, a *Wheelbase*, a *Downforce Area (CzA)* (negative for lift) and
  a *Front Aero Balance*, and each Wheel an *Axle* (Front or Rear).
  Accelerating, braking and standing on a slope move m·(a + g·sin θ)·h/L
  between the axles, from the previous 10 ms step's acceleration, and
  downforce adds ½·ρ·CzA·v² at the Ambient's air density, split by the
  aero balance; it also adds to the wheels' rolling resistance (not to
  coefficient A). An axle that would carry less than nothing lifts: it
  carries nothing, the other axle the rest, and the run warns. New
  channels: each Wheel's *Normal Load* and the Vehicle's *Front Axle Load*
  and *Rear Axle Load*. With its rear wheels spinning on μ 1, a
  rear-driven Formula Student-sized car (300 kg, CG 0.3 m high, 1.55 m
  wheelbase) now accelerates at 6.45 m/s² instead of 5.24. With a CG
  height and a downforce area of 0, the defaults, no result changes; the
  examples keep 0 and have their wheels tagged Front and Rear.
- Acceleration test: a case *Kind* of *Acceleration*, with a *Distance*,
  a *Start line* and a *Reference time*. The Driver holds full throttle
  the whole run with no target, and the run ends at the end of the solver
  step that reaches the line; Data Checks ask for a Target Speed only
  when a case reads one. The summary
  leads with *Time to 75 m* (from the start line, with *pass* and the case
  duration as its limit), *Speed at 75 m*, *Gap to reference time*,
  *Time to 100 km/h* (from t = 0), each battery's peak and mean terminal
  power, and the share of the run a driven wheel spent at the tyres' grip
  limit; the time and speed are read inside the step that crossed the
  line, so the output step does not change them. A car that misses the
  line within the duration gets a warning. The results are marked as
  estimates (Messages, Run info and the summary header say why), and a
  model whose batteries have no Output Power Limit is told how to check
  one. Simulations → *Acceleration test* runs the first acceleration case
  in one click, or adds one first: 75 m, staged 0.30 m behind the start
  line, 25 s time limit (FS Rules 2026 v1.1 (FSG) D 5.1.1, D 5.2.3, and
  D 9.2.1, which applies the 25 s to driverless runs only; FSUK and FSAE
  may differ, check the current season's rules). On the Battery Electric
  Car it gives 5.216 s and 86.2 km/h at the line (5.527 s from rest). No
  existing case changes.
- Lap mode: a case *Kind* of *Lap* drives the model's *Race Track*
  (Driver & Signals), with its *Track layout* and *Laps* set in the case.
  Layouts drawn for LightSim after FS Rules 2026 v1.1 (FSG) D 4.1,
  D 5.1.1, D 6.1 and D 7.1 (FSUK and FSAE may differ, check the current
  season's rules): Autocross, a 979 m closed lap with a slalom, a hairpin
  and a chicane; Skidpad, the right and left circles on the lane centre
  (9.125 m); Acceleration 75 m; or Custom, from curvature and elevation
  tables pasted into the track. A quasi-steady-state lap solver finds the
  fastest speed about every metre from the tyres' grip (downforce, load
  transfer along and across the car, load sensitivity, friction ellipse)
  and the powertrain (the E-Motors' full-load curves through the gears,
  cut to the battery's deliverable power and Output Power Limit); the
  motors, gears, brakes and battery then drive that speed with the drive
  cycles' own models, so the energy, the power limit checks and the
  channels are theirs. The summary leads with the lap, lap 1, total and
  sector times, average speed, energy per lap, RMS battery power, the time
  limited by cornering grip, traction grip, motor, battery, power cap and
  braking, and the lap energy balance error; the Race Track's channels
  give the lap distance, curvature, longitudinal and lateral acceleration
  (in g, a new unit), what limited the car and a map, for the X-Y view.
  Data Checks refuse a lap case without a Race Track, Driver or E-Motor,
  with an engine or clutch on the wheels or with all wheels on one axle,
  and a Custom curvature above 0.5 1/m. The results are estimates, and say so.
  A Formula Student-sized car (280 kg, 96 kW, μ 1.5) laps the Autocross
  in 61.2 s (62.9 s from a standing start), solved in about 0.15 s.
- Formula Student events and points (MOD-43): **FS events** (Simulations
  tab) marks or adds an Acceleration, a Skidpad, an Autocross and an
  Endurance case, runs the four (about 9 s for the FS example) and shows
  *Formula Student points* in the *Cases* tab. A case's new *FS event*
  setting (Acceleration or Lap cases) adds the event's time as the rules
  take it, the rule checks (80 kW EV 2.2.1, 500 A EV 2.2.2, 600 V EV 4.1.1,
  and whether the endurance finished on its energy) and an estimate of its
  points from the case's *Reference time* (the fastest team's time) and,
  for the endurance, *Reference energy* (the most efficient team's), with
  the scoring of FS Rules 2026 v1.1 (FSG) table 3, D 9.1.1, table 11 and
  D 9.4. A broken rule scores 0 (D 10.4.2). An Endurance case stops for
  the driver change at half distance and restarts from rest (D 7.2.3,
  D 7.5); its event time leaves out the restart lap (D 7.2.5) and its
  energy counts regeneration at 90 % (D 7.9.5). The points are estimates,
  not official results; FSUK and FSAE score differently. Batteries now
  record their highest current and terminal voltage for these checks.
  No existing case changes.
- Endurance energy strategy (MOD-44): the Race Track gets *Lift-and-Coast*
  (%), the share of each stretch of acceleration before braking that the
  driver coasts (no drive, no brakes), and *Energy Target* (kWh), the net
  energy the laps may take; above 0, lap mode picks each lap's
  lift-and-coast so the laps end near the target. Both are in a lap case's
  settings too. On the FS example's *Endurance energy* case (30 kW), 20 %
  lift-and-coast takes 12 % less energy (0.203 instead of 0.232 kWh a lap)
  for 0.4 % more time; a 5.0 kWh target ends 0.54 % under it, 4.5 kWh
  0.22 % under. The summary adds *Time limited by lift-and-coast*,
  *Lift-and-coast, mean share*, *Energy target* and *Energy used against
  the target*, and the Race Track's Limit channel the code 7, only in runs
  that coast; a run with neither set does not change. Sweep *Lift-and-Coast*
  for the lap time against the energy.
- Import a lap from a data logger or lap simulator (STD-35): **Import lap**
  (Simulations tab) reads a CSV with a lap's speed against time or
  distance, with layouts for a generic file, GPS loggers, MoTeC i2, AiM
  Race Studio, OpenLAP and TUM laptime-simulation (LightSim's reading of
  those tools, not yet checked against teams' files), a column picker, the
  speed unit, the lap to take (the fastest full lap by default), a preview
  and warnings for gaps, spikes and rows left out. It adds a *Cycle* case
  that drives the lap, optionally repeated to a 22 km endurance with a
  driver change stop, and adds a Driving Task wired to the Driver if the
  car has none. A trace against distance is turned into time (t = ∫ ds / v)
  and keeps its distance within 0.5 %.
- Endurance energy study (STU-38): an endurance case (a Lap case, or a
  Cycle case from an imported lap repeated to 22 km) reports the net
  battery energy, the lowest pack voltage and, from a trace, the RMS
  battery power, with the endurance energy and rule checks (a Cycle case
  gets no points). Its *Cases* tab offers a grid of the accumulator's
  capacity and Output Power Limit (up to 6 × 6) that runs as one saved
  study and shows a map, marking the pairs that run out of energy *DNF*.
  Saved studies with two factors name both.
- A *Traction Control* block (Driver & Signals, MOD-45): wired between the
  Driver's Traction Command and the E-Motors, with the driven wheels' Slip
  and the Vehicle's Speed, it ramps the demand up from rest (*Launch Ramp
  Time*, *Launch Torque*) and then holds the slip at a *Target Slip* (0.1)
  with a PI loop, below the *Minimum Speed* leaving only the ramp. On the
  FS example's 75 m case at a 2 ms step (Kp 1, Ki 20 1/s) it holds the
  slip at 0.095-0.100 until the 80 kW limit takes over, where the slip
  otherwise reaches 7; the 75 m time does not change (3.756 s against
  3.754 s), because LightSim's tyres keep their grip past the peak. Its
  defaults (Kp 0.5, Ki 10 1/s) also stay stable at the 10 ms step. No
  model changes unless you add the block.
- Calibrate lap mode on a logged lap (VAL-38): **Calibrate lap**
  (Simulations tab) builds the track from a logged lap's speed and lateral
  acceleration, fits one factor on every tyre's grip and the Vehicle's
  CzA to the logged speed against distance (least squares), and checks
  the prediction blind on a second lap: lap time error, speed RMS error
  and, with the pack power logged, energy error. **Apply to the model**
  puts the fit in the car. Checked on LightSim's own laps only (blind lap
  time −0.5 %, energy +0.6 %); no real log is bundled.
- Wheel: *Lateral Friction μ_y* (0: the same as μ), *Load Sensitivity
  dμ/dFz* (per kN), *Nominal Load Fz0* (0: the wheel's static load) and
  *Friction Ellipse Exponent* (lap cases). The load sensitivity also acts
  on the tyre force of drive cycles and acceleration tests; at its default
  of 0 no result changes. Vehicle: *Front Track Width* and *Rear Track
  Width*, for the sideways load transfer of lap cases.
- A third example, *FS Electric (generic)*: a typical Formula Student
  electric car to make your own (280 kg, one rear E-Motor through a 4.4
  chain drive and an open differential, a 138s4p 7.2 kWh accumulator with
  the battery's Formula Student Electric preset, load transfer, downforce
  and load-sensitive tyres). Its cases: *Acceleration 75 m* (3.75 s from
  the start line, 119 km/h at the line), *Autocross (flying lap)* (57.7 s
  on LightSim's layout) and *Endurance energy* (23 laps, 22.5 km, with the
  Output Power Limit at a 30 kW endurance setting: 5.33 kWh net at the
  accumulator, 25 % charge left). Its Open-menu entry names each rule value
  with FS Rules 2026 v1.1 (FSG) (FSUK and FSAE may differ, check the
  current season's rules), compares the results with FS Czech Republic
  2025 (acceleration 3.51–6.44 s, median 3.91 s; efficiency 3.19–6.15 kWh,
  median 5.25 kWh) and gives a sweep to try: the Output Power Limit from
  40 to 80 kW on the 75 m case (4.23 to 3.75 s). The other examples do not
  change.
- The diagram's toolbar shows the zoom in % and offers Fit, 50, 100 and
  200 %. It floats over the diagram's top edge, and the bottom panels open
  at 22 % of the workspace instead of 30 % (drag their edge for more), so
  on a 1366 × 768 screen the diagram keeps 40 % of the window with a panel
  open (was 33 %). LightSim remembers the height of a bottom panel you
  opened in an earlier build; choose Reset UI to get the new layout.
  (GUI-03)
- A *Problems* tab (was *Data Checks*) lists every problem the model has
  now: the Data Checks, which run by themselves a moment after a project
  opens and after every change (before, only once you had run them), and
  the latest run's warnings and errors. Each row names its parts and says
  how to fix it. A click, or Enter, selects the part and zooms the diagram
  to it, opening the sub-system it is in; a check about the whole model,
  such as two Vehicles or wheel load shares, selects every part involved
  (before, it named none). The Problems tab carries the count; Messages is
  the log and no longer does, so an error that is not about the model,
  such as a save that fails, brings Messages to the front. (UX-09)
- Data Bus Connections lists one row per signal input: pick the output
  that feeds it in a box you can type in, which offers outputs only, those
  that share a word with the input's name first (Brake Command: Driver ·
  Brake Command), then those with its unit. The list opens over the
  diagram with about ten outputs in view. A link takes two clicks (the
  Battery Electric Car's 13 links: 26 clicks, with no scrolling, before
  65). It has a search box,
  *Unconnected inputs* and *Selected part* filters, and a part's
  right-click menu has *Signals…*, which opens the panel on that part.
  (UX-15)
- A Driving Task can drive a standard cycle picked from a list: WLTC
  class 3b, EPA city (UDDS) and EPA highway (HWFET), grouped by region,
  each with its duration and distance, or *Custom profile* for the typed
  points. Properties sketches the speed over time, with the cycle's phases
  marked, and gives its duration, distance and top speed; a typed profile
  gets the same sketch. Choosing a cycle sets the length of the cases that
  drive it, in one undo step, and Messages says which (when the model has
  one Driving Task, or for the case it is picked in). A case can pick its
  own cycle among its overrides. Searching the component library for a
  cycle's name, such as *udds*, lists it: activate it to add a Driving Task
  that drives it. The three cycles are the ones the examples used; their
  figures are within 0.04 % of the published ones. (CON-16)
- The drive-cycle library has 27 cycles (it had 3): the WLTC of classes 1,
  2, 3a and 3b, their city cycles and each class 3 phase on its own, typed
  from the EU's Regulation 2017/1151 on EUR-Lex; the NEDC from UN
  Regulation No 83 as the EU published it; EPA's FTP-75, US06, SC03, LA92,
  New York City cycle and motorcycle FTP from EPA's schedule files; the
  WMTC motorcycle cycle and its parts, and a long-haul truck route with its
  road grade (804.6 km, and its first 100 km), from FASTSim. Each cycle
  names its source and the reason LightSim may ship it under its sketch,
  and carries a fingerprint (the sum of its 1 Hz speeds and a SHA-256 of
  its file) that the tests check, so no trace can change unseen. The
  WLTC class 3b, UDDS and HWFET files the examples use did not change:
  they equal the EU and EPA tables value for value. (CON-04, CON-31)
- Road Profile: *Grade From Cycle* takes the road's grade from a cycle that
  carries one (the long-haul route), placed along the distance the cycle
  covers, so one cycle gives both the speed and the hills. (CON-11)
- Data register: every row now says why LightSim may ship the data (its
  *reuse basis*), and DATA-REGISTER.md lists, in order, where data may come
  from and the sources it must never come from, such as the UNECE website,
  ev-database.org and EUPL files; the tests fail on a row that names one.
  (CON-31)
- A run on a typed profile instead of a standard cycle says so in its
  messages: its figures cannot be compared with published ones. The
  examples' *City Cycle* and *Mixed Cycle* are such hand-made demo profiles,
  and their descriptions now say so; the Properties sketch of a typed
  profile reads *Custom profile (not a standard cycle)*. The examples'
  Monitor and Hybrid Control Unit ports show the unit of the signal they
  are wired to (km/h, N·m, %, V, A, 1/min) instead of bare numbers, and the
  Battery Electric Car's description no longer names an inverter it does
  not have (the motor's loss map holds the inverter's losses). No result
  changes. (CON-26)
- Run summary, consumption as labs report it: for an electric car the
  energy at the charging socket (*Consumption at the socket (AC)*, from the
  battery's new *Charger Efficiency*, 86 % by default as in FASTSim), the
  US *fuel-economy equivalent* in MPGe (33.705 kWh a gallon) and the *range*
  the battery's usable energy gives at that consumption; for a hybrid
  case with *Charge balance* off the battery's energy change as a share of
  the fuel's and a *charge-corrected* fuel consumption from that one run
  (with it on, the default for a hybrid's cycle, charge balancing gives
  them: see ENG-33); for a case on a cycle with phases each phase's
  distance and consumption (the WLTC's four phases add up to the whole);
  and for the FTP-75 the bags weighted as EPA does. Help: *Consumption
  figures* gives every formula. The Battery Electric Car on WLTC class 3b:
  14.05 kWh/100 km at the battery, 16.34 at the socket, 128.2 MPGe, 426 km;
  its phases 10.63 / 11.27 / 12.91 / 17.94 kWh/100 km. No existing figure
  changes. (CON-05)
- **Simulations → US label**: a US window-sticker estimate, marked *not
  certified*. It runs the model on EPA's city (UDDS) and highway (HWFET)
  cycles and shows each step from the lab figures to the label ones, by
  EPA's derived five-cycle equations (2008 or 2017 coefficients), as
  FASTSim's label module computes them (the tests hold LightSim within
  0.5 % of FASTSim's formulas): city, highway and combined MPGe and
  kWh/100 mi and the range for an electric car, mpg and l/100 km for a car
  with an engine. The Battery Electric Car gets 104 MPGe combined and 213
  miles; the P2 Hybrid Car 56 city, 48 highway and 52 combined mpg (the
  real car's label: 58, 60 and 59). (CON-32)
- Weather presets on the Ambient: *Cold day (−7 °C)* (EPA's cold FTP and
  the EU's low-temperature test), *Standard day (23 °C)* (WLTP),
  *Hot and sunny day (35 °C)* (EPA's air-conditioning test SC03) and *High
  altitude (1,500 m)* (the standard atmosphere: 84.56 kPa, 5.25 °C), each
  with its source. The Battery Electric Car has an Ambient (at its
  defaults, the air it had before) and two new cases, *WLTC, winter day
  (−7 °C, heating on)* at 19.5 kWh/100 km and *WLTC, hot day (35 °C,
  air-con on)* at 18.6, against 18.9 at 20 °C with the same 2.5 kW load.
  Only the air density follows the weather until the climate-control load
  (MOD-41) arrives; the presets' notes say so. No existing case changes.
  (CON-30)
- **Simulations → Vehicle tests**: one-click 0-100 and 80-120 km/h, top
  speed with what limits it, consumption and range at 50, 90 and 120 km/h,
  the steepest grade held at 30 km/h and a virtual coast-down that returns
  the road-load coefficients A, B and C, each from runs of the model as it
  is. The Battery Electric Car: 0-100 km/h in 7.12 s, 80-120 in 4.98 s,
  160 km/h limited by its motor's maximum speed, 9.4 / 14.1 / 19.6 kWh/100
  km at 50 / 90 / 120 km/h. The hybrid's coast-down gives back its EPA
  coefficients within 2 %. (CON-06)
- Value sources: every number and table of a part can carry its source, a
  kind (measured, datasheet, estimated, generated or library default) and
  a confidence (0 not checked, 1 agrees with its source, 2 source and
  method checked, ADVISOR's scale). Properties tags each value that has one
  and counts the values still at their library default; *Value sources*
  records, changes or forgets one. The Battery Electric Car's and P2 Hybrid
  Car's values all carry theirs, from the data register. Data Checks' all
  clear now says how many of the model's values are still at their library
  default with no source. Saved in the project file as each part's
  `parameterSources`; older versions keep it untouched. (CON-13)
- **Home → Templates**: start a project from a pre-wired vehicle template
  with a short form (*Electric car, one motor*, *P2 hybrid car*, *Formula
  Student electric*), each with named slots (Battery, E-Drive 1, Engine,
  Driveline, Chassis, ...); or save any model as a template of your own,
  choosing which values its form asks. Your templates are files in the
  projects folder (`templates/`), versioned, and a project records the
  template and version it came from. (CON-18)
- A fourth example, *Efficient Electric Sedan*, sized after the 2022 Tesla
  Model 3 RWD from EPA's 2022 Test Car List (test mass, road load, rated
  power, gearing), asks what low drag buys you: on WLTC it takes 11.8
  kWh/100 km against the Battery Electric Car's 14.1. On EPA's city and
  highway cycles it gives 190 and 163 MPGe at the socket, against 185.3 and
  170.1 in EPA's own tests of the car; the tests hold it within 5 %. The
  FASTSim vehicle file's mass and motor power, which it cites from
  evspecifications.com, were not used. (CON-07)
- Help: press F1, or click **?** at the top right, to open LightSim's help
  in your web browser. It comes with the app and needs no internet
  connection: two tutorials, how-to guides, a page for every part with its
  ports and parameters, the drive cycles, the keyboard shortcuts, the Script
  API and the documents that come with each release (known issues, release
  notes, what is validated, data sources), with a search box. With a part
  selected on the diagram, F1 opens that part's page. In the desktop app,
  *Help → Documentation* opens the front page, and *Help → Known Limits*
  now opens the known issues there. (LRN-04)
- Help: press F1, or open the **?** menu at the top right, to read
  LightSim's help in a panel beside your work. It comes with the app and
  needs no internet connection: tutorials, how-to guides, a page for every
  part with its ports and parameters, the drive cycles, the keyboard
  shortcuts, the Script API and the documents that come with each release
  (known issues, release notes, what is validated, data sources), with a
  search box. F1 opens the page of what you are on: the parameter in the
  field or card, else the selected part, else the panel the focus is in;
  each panel's **?** next to its tabs opens its how-to page. The panel
  goes back, home and to your web browser (for tabs and bookmarks), and
  can be made wider; links out of the help open in the browser. The **?**
  menu, and the desktop app's *Help* menu, list the main pages, *Report a
  problem…* and the first-steps tour. After an update, the release notes
  open once by themselves. (LRN-04, LRN-09)
- New help pages: *Your first electric car*, a 15-minute tutorial that
  runs the example on the WLTC and builds a small car of your own, with a
  check after each step; three Formula Student lessons (the 75 m time,
  endurance energy, the accumulator's size); a lesson page for each
  example with what happens when, its reference results and exercises
  with answers; a *Script cookbook* of ten control recipes (rate limiter,
  PI controller, table lookup, hysteresis switch, gear shifts,
  regenerative braking by speed, low-pass filter, power limiter, launch
  control, timer); *Results summary values*, which says what every summary
  number means and how it is worked out; *Why a result differs from the
  official figure*; and a glossary of 76 terms, each with a link of its
  own. The automatic tests run every number these pages quote and every
  recipe, and fail when one no longer matches the app. (LRN-07, LRN-10,
  LRN-12, LRN-15, LRN-16, LRN-26)
- On *Results*, rest the pointer on a row of *All summary values* to read
  what it means; **what they mean** in the table's header opens the page
  that defines them all. (LRN-10)
- First steps for new users: a short tour of the screen on the first
  start (skip it with **×** or Esc; the **?** menu shows it again), and a
  slim bar under the ribbon with five steps (*Build*, *Set values*,
  *Choose tests*, *Run*, *Read results*) that tick themselves off as you
  do them; click a step to go to the panel that does it, or hide the bar
  with its **×**. Data Bus Connections, Cases and Results each explain
  themselves in one line the first time you open them. (UX-26)
- A *Start* page opens when LightSim starts and when you press **New**:
  continue with the open project, start from one of the examples (its card
  says what results to expect) or a blank project, or reopen one of the 8
  projects you saved last, each shown with when it was saved, its number
  of parts and a sketch of its diagram. From the first launch to a
  finished run of an example takes two clicks, the example and **Run**.
  Tick *Skip this page: open my last project at start-up* to open straight
  into your last project, as before; work you had not saved always opens
  straight away. The status bar's **+** still makes a blank project in one
  click. Empty panels now offer the next step: an empty diagram *Add a
  part* and *Start from an example*, an empty Monitors panel *Place a
  Monitor*, and an empty Results page a button that names the case it
  runs. (UX-16)
- Every parameter explains itself: rest the pointer on it in *Properties*
  or the parameter dialog, or move to it with Tab, and a card says what it
  is, its usual values (for a Formula Student car, a small car, a mid-size
  EV or a van where that matters), where to find the real number, its
  default and what is allowed. **More in the help (F1)** in the card, or
  F1 in the field, opens the parameter on its part's help page, which
  carries the same texts. In the dialog, tables, scripts and profiles show
  them above their editor. The texts are first drafts. (UX-10, LRN-05)
- A number outside what its parameter allows turns red as you type, with
  a line under it that says what is allowed, such as *Initial SOC must be
  above 0 and at most 100 %.* Data Checks read the same limits, from the
  component library, and say the same. A case's own values in *Cases &
  Parameters* are checked the same way, and so is a lap case's *Laps*,
  which the form no longer quietly changes. An acceleration case's
  *Distance*, *Start line* and *Reference time* turn red as you type, but
  are not yet Data Checks: a run ignores a value outside them. (UX-10)
- Charts zoom and pan: on the *Results* chart, the X-Y view and the
  *Signal Plot*, roll the mouse wheel to zoom in or out around the pointer,
  drag a box to zoom in on it, Shift+drag to move the view, and
  double-click to see the whole run again; a chart with the keyboard focus
  takes +, −, the left and right arrows and 0. The zoom stays when you
  tick another channel and while a run is still adding points. Charts now
  draw every stored point (the highest and lowest of each pixel column, so
  no peak is lost), so zooming in shows each one, and their y axes fit the
  data in view instead of starting at 0. The values under the pointer show
  in the legend under the chart instead of in a pop-up (in the *Signal
  Plot*, in its toolbar). On a 1-hour run (36,001 points), ticking a
  channel redraws in about 25 ms (40 ms with the baseline drawn faint as
  well) instead of 50-110 ms. The **PNG** picture
  shows the view you zoomed to. A screen reader names what each chart
  shows and its time range. The charts are drawn with uPlot (MIT licence)
  instead of Recharts. (RES-05)
- Chart axes fit the data: a y axis spans its unit's data plus 5 %, at
  round ends, and takes in 0 only when that range comes near it, so on the
  Battery Electric Car's City Cycle the SOC (88.76 to 90 %) and the
  terminal voltage fill 88 % and 90 % of the plot (1 % and 2 % on 0.2.0's
  axes from 0). A trace that barely changes fills the plot too: **Axes**,
  above the chart and X-Y view, starts a unit's axis at 0 or sets its
  ends. The sweep view fits its axis the same way and opens on the run's
  first headline number (a test's time, the fuel or the energy
  consumption) instead of the final SOC. The **Time · auto** list reads
  the time in s, min or h (automatic: s up to an hour, min up to 3 hours)
  or plots the run against the distance driven (the Vehicle's *Distance*,
  in m below 1 km, in km above; a stop draws as a vertical line). Ticks
  fall on round steps of the axis's unit at any zoom; the legend reads
  values to 4 significant digits and the time or distance to the samples'
  step, with the time too on a distance axis; and the **CSV** starts with
  `t_s` as before, or with the x axis picked for the chart: `t_min`,
  `t_h`, or `distance_m` or `distance_km` followed by `t_s`. A screen
  reader names each axis's range. (RES-18)
- The *Results* page leads with the run's headline numbers, in a strip
  above the chart: consumption (or fuel consumption and CO₂), distance, the
  final charge and the energy the battery gave and took back; a test's
  time, speed at the line or top speed; a lap case's lap time. A failed
  check comes first. Each keeps its *pass* or *fail* marker, limit and
  *not valid* reason, and the strip says when the results are estimates.
  At 1366 × 768 all of them are in view (before, *Distance driven* and
  *Consumption* were the 5th and 6th rows of a 4-row box). The full table
  folds under *All summary values*, which opens by itself when runs are
  overlaid, and the chart gains 59 px. The plot of a first run opens on
  the target against the actual speed (did the car follow its cycle?),
  the target dashed and drawn on top, with the battery's SOC and power.
  (RES-30)
- Measurement cursors: **Cursors** above the *Results* chart (or C) puts
  two lines, A and B, on it. Type a time into the A or B field under the
  chart, step one stored point at a time with the up and down arrows (ten
  with Page Up and Page Down), or drag a line; the cursors always sit on a
  stored point. A table then gives each plotted signal's value at A and B,
  the difference, and between them its minimum, maximum, mean, RMS (root
  mean square) and integral: kWh from kW, Ah from A, m from km/h, kg from
  kg/h and revolutions from 1/min (mean, RMS and integral weighted by time,
  with the trapezoid rule). On the Battery Electric Car's City Cycle, the
  battery gave 0.4236 kWh from 150 to 300 s. *Time to reach* places A and
  B where a signal first reaches one value and then another, so the time
  between them reads as Δt (0 to 50 km/h: 31 s). The lines follow zoom and
  the distance axis and are in the **PNG**. Each run keeps its cursors
  while you switch views, overlay runs (they are measured at the same
  times, or on the distance axis where the lines cross them) or leave the
  page, until LightSim closes; they are not saved.
  Moving a cursor on a 1-hour run (36,001 points) with 10 signals plotted
  takes about 2 ms. (RES-06)
- The *Results* page keeps each case's choices: the ticked channels, the
  view, the x axis and y axes, the chart's zoom, the X-Y view's X channel
  and the sweep's figure stay for its next runs, when you go to another
  page and back, and when LightSim is opened again (they are kept in the
  app's own storage, not in the project). Before, they went back to the
  defaults every time the page was left. A new run no longer clears the
  runs you overlaid, nor the ones a sweep overlaid: **Clear** in the
  *Overlay* box removes them. (RES-19)
- A run is compared with a baseline, the previous run of its case unless
  you pick another (or *None*) in the new **Baseline** list: the baseline
  is drawn faint and dashed with the run (**Draw the baseline faint on
  the chart** turns that off), each headline number gets a line such as
  *+1.22 (+11.0 %) vs baseline*, and the full summary gets *Baseline*,
  *Change* and *% change* columns, with changes of 1 % or more in bold
  and *~ 0* where a change is within the stored rounding. *What changed*
  lists what differs between the two runs' models: parameters old → new
  with their units, maps and scripts edited, parts added or removed,
  wires and Data Bus links, the case's settings and both runs' live edits;
  a click on a part shows it on the diagram. A run is named after what
  changed since the previous run of its case (*City Cycle · Vehicle Mass
  2,300 kg* instead of the clock time) in the run lists, legends and
  summary; *Run info* edits the name and keeps a note, both stored with
  the run. (RES-10)
- A *Climate Control* part (Base Electric) for heating and
  air-conditioning: its *Heating/Cooling Demand* table turns the first
  Ambient's temperature into the heat the cabin needs, and a *PTC heater*
  (an electric resistance heater, 1 kW of heat per kW) or a *Heat pump*
  turns that into electrical power on its bus. The heat pump's coefficient
  of performance (COP, heat moved per kW of electricity) is a share of the
  ideal (Carnot) value, about 1.9 at −7 °C; below its *Minimum Outside
  Temperature* (−10 °C) the PTC heater takes over, and cooling always runs
  the air-conditioning compressor (COP about 2.2 at 35 °C). Its channels
  are *Drawn Power*, *Heat to Cabin* and *COP*, and the summary gives the
  energy it used and the heating or cooling it delivered. The default
  demand is an estimate for a compact car kept at 21 °C, steady state with
  no warm-up. Added to the Battery Electric Car with an Ambient, it takes
  the WLTC from 14.00 kWh/100 km at 23 °C to 23.76 at −7 °C with the PTC
  heater (1.70 times, close to the 41 % range loss AAA measured at −6.7 °C
  with the heating on, a figure we have not checked at its source), 19.72
  with the heat pump and 16.77 at 35 °C. Data Checks say when a model has
  no Ambient, where it sits at 20 °C and does nothing. Existing models do
  not change. (MOD-41)
- Every run books where the energy goes, part by part: for each battery,
  voltage source, fuel tank, engine, fuel cell, E-Motor, DC-DC converter,
  consumer, clutch, gear, differential, brake, wheel and the Vehicle, the
  energy that went in, came out and was lost and the change in what it
  stores, so that in − out − lost − stored is 0 for each. It is in the run
  result as *partEnergy* (kWh), with each part's peak, mean and RMS power (kW;
  RMS power is what sizes an inverter's or motor's cooling) and the
  Vehicle's air drag, rolling resistance, climbing and acceleration. New
  channels: *Losses* on batteries, engines, fuel cells, Shafts, Final
  Drives, Gearboxes, Differentials and Transfer Cases; *Fuel Power* on
  engines; *Braking Power* on brakes; *Slip Losses* on wheels and clutches;
  *Air Drag Power*, *Rolling Resistance Power*, *Climbing Power* and
  *Acceleration Power* on the Vehicle. The summary's new *Energy balance
  residual* says how far the parts' books together are from closing, as a
  share of the energy the sources gave: 0.01 % (BEV City Cycle), 0.08 %
  (WLTC), 0.02 % (hybrid Mixed Cycle), 0.24 % (hybrid UDDS, with 30 engine starts), 0.39 % (FS 75 m acceleration). The
  Fuel Tank gets a *Fuel Heating Value* (42.9 MJ/kg, petrol) for the fuel's
  energy; it does not change the fuel used. On the Battery Electric Car's
  City Cycle the battery gave 0.881 kWh, the E-Motor lost 0.086, the Final
  Drive 0.017, the tyres' slip 0.004, air drag 0.242, rolling resistance
  0.421 and the 0.25 kW consumer 0.042 kWh. Results → **Energy** (RES-22)
  draws the car's energy as flows; an energy audit table is VAL-03. Bookkeeping
  every part every solver step costs about 3-6 % of a run's time. (MOD-10)
- Battery: *Defined By* *Pack values* (as before) or *Cells*. With *Cells*
  you enter a cell datasheet (capacity, open-circuit voltage curve, minimum
  and maximum voltage, DC resistance, continuous and peak currents, mass)
  and a layout (*Cells in Series* × *Cells in Parallel*, e.g. 96s30p), and
  LightSim builds the pack: its charge capacity, voltage and resistance
  (with interconnect and contactor resistance), and an estimate of its mass
  (cells × cell mass × a *Packaging Factor*), all given in Data Checks
  before the run and in the summary after it. The resistance rises with the
  pulse length (2 to 120 s, as VECTO's tables do), at low charge and in the
  cold (*Cell Resistance Factor* and *Cell Temperature Factor*, estimates by
  default; the cells are at the Ambient's temperature), so the pack sags in
  long pulses and in winter: the Battery Electric Car built from 96s30p
  21700-type cells takes 7.32 s to 100 km/h at −7 °C against 7.09 s at
  20 °C. The battery management system's limits hold: the current never
  exceeds the cells' continuous or, for pulses up to the *Peak Duration*,
  peak current, no cell goes below its minimum or above its maximum
  voltage, and an optional *Weakest Group* (less capacity, more resistance)
  sets them, as the weakest module does in a real string. New channels:
  *Discharge* and *Charge Power Limit* for 2, 10 and 30 s (the state of
  power), *Discharge* and *Charge Current Limit*, *Lowest* and *Highest
  Cell Voltage*; the summary gives the time held at each limit and the
  lowest and highest cell voltage. With *Pack values*, a *Max Discharge
  Current*, *Max Charge Current*, *Minimum* and *Maximum Pack Voltage* (0 =
  none) do the same for the whole pack: at 300 A the Battery Electric Car
  takes 10.02 s to 100 km/h instead of 7.10. A *SOC Derating Band* (both
  modes) lowers the limits linearly to 0 near empty and full, as FASTSim's
  buffers do. The Formula Student car built from 138s4p cells with a 30 A
  peak (120 A) takes 3.886 s over 75 m instead of 3.744. The parameter
  dialog shows only the fields of the mode chosen. Existing models and the
  examples do not change. (MOD-08)
- E-Motor: *Torque Scale*, *Speed Scale* and *Voltage Scale*; Combustion
  Engine: *Engine Scale* (all 100 % by default). They resize the machine
  with its maps, so a sizing sweep stays realistic: torque × k_T with the
  loss at that torque × k_T and the drag and rotor inertia × k_T (a longer
  machine); every speed × k_n and torque ÷ k_n at the same power and the
  loss of the matching point, the Maximum Speed × k_n (a rewound machine);
  the full-load map's voltage axis × k_V; an engine's torque, drag, inertia
  and fuel flow × k at the same fuel use per kWh (EPA ALPHA's engine
  scaling, without its small-engine fuel adjustment). Data Checks show the
  resized machine next to the original (*peak torque 465 N·m (was 310),
  maximum speed 16,000 1/min, peak power 225 kW (was 150)*), check the
  scaled maps against the bus voltage and the maximum speed as for any
  map, and warn outside 50–200 %. All four can be swept in a study. The
  Battery Electric Car's motor at 150 % torque takes it to 100 km/h in
  5.59 s instead of 7.10. Existing models do not change. (MOD-47)
- Wheel: *Tyre Code*. Typing a tyre's size code, such as `205/55 R16 91V`
  (also `P…`, `…ZR…`, `XL`, `LT…`, `…C 107/105R`, or a Formula Student
  `20.5x7.0-13`), fills in the *Wheel Radius* (the unloaded radius, rim ÷ 2
  + width × aspect ratio = 315.95 mm here, × a new *Rolling Radius Factor*
  of 0.97 = 0.3065 m) and, with a load index, estimates of the *Slip
  Stiffness* (18.4), *μ* (1.13), *Lateral Friction μ_y* (1.00), *Nominal
  Load* (half the load-index capacity: 3,017 N for load index 91, 615 kg)
  and *Load Sensitivity* (−0.013 per kN), after Rill's "engineer's guess"
  for a passenger-car tyre as Project Chrono's TMeasy tyre implements it
  (BSD-3-Clause, credited in Help > Third-Party Notices). One undo step
  takes it all back, and the form says what the code means. A *Rolling
  Resistance Label Class* (A to E, EU Regulation 2020/740) sets the *Rolling
  Resistance* to 0.0060-0.0110. Data Checks warn when a wheel carries more
  than its load index allows standing still, when a code cannot be read,
  and (info) when the Wheel Radius is more than 3 % from the code's.
  Existing models do not change. (MOD-48)
- Driving Task: *Profile Axis*, *Time* (as before, the default) or
  *Distance*. Over distance the profile's points are 'distance:speed'
  pairs in m and km/h, and the Driver gets the target speed at the
  distance the car has driven, as lap simulators, track maps, loggers and
  truck missions describe a route: a heavier or weaker car slows for a
  corner at the same place, only later in time. The profile table's x
  axis says *Distance (m)*. A case of kind *Cycle* or *Performance* then
  has a **Laps** field: with *Repeat Profile* ticked, the run ends after
  that many passes through the profile (one pass is its first point to
  its last), with the *Duration* as its time limit (a warning when time
  runs out first). The trace is judged against distance (±2 km/h within
  ±1 s of travel at the target speed). Data Checks refuse a standard drive
  cycle on the distance axis, and warn about a 0 km/h point before the
  end (the car stops there for good) and about Laps with no Driving Task
  over distance. On the Battery Electric Car, a 1,500 m lap with a 100 km/h
  straight starts braking 2.9 m after the profile slows at its 1,927 kg
  and 4.7 m after at 2,500 kg. Existing projects drive over time, as
  before. (ENG-34)
- Each stored point keeps each channel's lowest, highest and
  time-averaged value since the point before it, taken at every solver
  step, so a short peak between two recorded points (a regeneration burst,
  a torque spike) is in the run file: with *Store every* 10 on the Battery
  Electric Car's City Cycle, the battery power's recorded points go down
  to −9.56 kW and the stored lowest value to −11.19 kW, as at every solver
  step. The battery's energies equal the sum over the solver steps of the
  stored mean power to 1e-6. The charts do not draw these values yet.
  Runs take about 15 % longer for it, except when each point is one solver
  step. (ENG-16)
- Before a run, LightSim checks the solver step against the stiffest
  parts of the model: the tyres' *Slip Stiffness* (with the share of the
  weight each wheel carries) and propeller-type loads. When one is too
  stiff for the 10 ms step, the run uses a smaller step (to 0.5 ms at
  least) instead of giving numerical nonsense, and Data Checks and
  *Messages* say which part asked for it and how much longer the run
  takes; a part too stiff even for 0.5 ms is a warning. On the Battery
  Electric Car's City Cycle a *Slip Stiffness* of 300 gave a tyre slip of
  14 (1,400 %) at 10 ms with no message; it now runs at 0.5 ms with a slip
  under 0.001. A clutch that can ring as it closes at the 10 ms step gets
  a note in Data Checks (the P2 Hybrid Car's does). The Battery Electric
  and P2 Hybrid Cars keep the 10 ms step; the FS Electric example (*Slip
  Stiffness* 20) runs at 5 ms. (ENG-14)
- Charge-balanced hybrid runs: a case of kind *Cycle* whose model has an
  engine, a battery and an E-Motor runs the cycle again from the charge the battery
  ended with, until its stored energy changes by less than 1 % of the
  fuel's energy (at most 5 runs), so its fuel figure holds no energy the
  battery gave or took, and the start charge no longer needs setting by
  hand. *Charge balance* in the case settings (Auto, On, Off) turns it on
  or off; Auto leaves out paced runs. The summary gives the start SOC it
  found, the battery's energy change as a share of the fuel's and the
  number of runs, and *Messages* each run's start and end charge. When it
  does not settle, the run is a *warning* and the summary adds *Fuel
  consumption, charge-corrected* from a straight line through the runs.
  Fuel tank: *Lower Heating Value* (MJ/kg, 43 by default). Started at 30,
  50 or 70 %, the P2 Hybrid Car's Mixed Cycle now gives 2.878 l/100 km in
  2 runs (2.8777 from its hand-set start, in 1). (ENG-33)
- Parameter sweeps run side by side on the computer's processor cores:
  the engine runs a sweep's points in worker processes (one per core less
  one, fewer when memory is short; each Script block keeps its own
  locked-down worker), stores each as a run of the project as it ends and
  sends back only its summary, so the app and the engine stay responsive.
  A 16-point mass sweep of the Battery Electric Car's 600 s City Cycle took
  48.6 s with 3 workers on a 4-processor machine, against 127.3 s one run
  after another (2.6× faster), with the same results to the last digit;
  while it ran, the engine answered in 1.5 ms (median; 2.5 ms for 95 % of
  requests, up to 55 ms as a point's run was stored). A sweep can now have
  up to 200 points (was 16). **Stop** stops the running points and leaves
  the rest *not run*. The saved study records how many points ran at once,
  each point's run time and the sweep's wall time, and its line in *Saved
  studies* shows the speed-up. For scripts: `POST /api/studies` runs any
  list of points of a case and answers with each point's summary (the
  WebSocket `/api/studies/run` streams them). (ENG-05)
- Results → **Energy**: where the battery's, fuel's or hydrogen's energy
  went, as a Sankey chart (bands as wide as the energy they carry) from
  the sources through groups (air and rolling, speed and height kept,
  friction brakes, losses in parts, loads, charged back) to each place,
  with what the books cannot place as its own band, *Not accounted for*,
  beside the run's electrical energy balance error; and a table of each
  part's energy in, out, lost and stored change, in kWh and as a share of
  the sources. **CSV** and **SVG** save them. On the Battery Electric
  Car's City Cycle the battery gives 0.887 kWh, air drag takes 27.3 % and
  rolling resistance 47.5 %, and 0.07 % is not accounted for. A case's
  **Energy report** (Cases tab, on by default) turns it off. The
  diagram's lightning button labels each part with its energy and opens a
  bar chart of the losses with a **Hide below** slider; a click on a bar
  selects the part. The energy is added up while the run goes, from the
  power each part already reports; the gears, clutches and spinning parts
  are one row worked out from what is left (MOD-10 will measure them part
  by part). (RES-22)
- A band along the bottom of the Results chart says what held the car
  back at every moment: braking, tyre grip, a set power limit, the battery
  or supply, the motor or engine, coasting, or nothing (the driver's
  demand met), one strip per driveline, with the time in each under the
  chart. The Formula Student car's 75 m acceleration: tyre grip for 1.92 s,
  then the 80 kW limit for 2.05 s. (RES-38)
- Results → **Duty**: each E-Motor's, battery's, engine's, fuel cell's and
  DC-DC converter's highest, lowest, mean and RMS (the mean that sets
  heating) power, torque and current over the run, from the solver's own
  steps, with the time spent above a power you type and CSV export. Each
  part's RMS power and current and peak power are also columns of a
  parameter study. (RES-39)
- Parts and wires you change after a run get a dot on the diagram (an
  edited case one in the Cases tab), and Results says *These results are
  from before 3 changes to the model*, with **Re-run** and **Show
  changes** (part, parameter, old → new). The marks clear when the next
  run finishes. (UX-41)
- These reports change no result: the summary and the channels are as
  before. Runs stored before this version have no Energy, Duty or limit
  band; run their case again.
- Data Checks catch signal wires that give wrong numbers without a
  message (VAL-17): a percentage into an input that expects 0-1 (a
  battery's *SOC* in %, 0-100, into a Script, PID or Lookup Table input set
  to *Fraction*, or into a motor's or brake's command), the other way
  round, and any two different units on one wire (a vehicle speed into a
  rotational speed). A Script's ports get a unit list next to their names,
  a PID a *Setpoint & Feedback Unit* and a Lookup Table an *Input X Unit*
  and *Input Y Unit*; left at *Not set*, a port is not checked. The library
  gives the 0-1 commands (Driver outputs, motor, brake, throttle and
  clutch inputs) the new unit *Fraction* and the road grade *Percent*.
  A loop of signal blocks is named with its blocks only, the order they
  run in each step and the value that arrives one step late.
- Expected values (VAL-35): each case can hold numbers you trust for its
  results (a maker's 0-100 km/h time, last year's measured 75 m time, a
  hand calculation), each with a tolerance in % or in its unit and a
  source, under *Expected values* in the *Cases* tab. After every run the
  *Results* page shows the run's value, the expected value, the gap and a
  coloured grade: *within* the tolerance (green), *near*, within twice it
  (amber), or *outside* (red). *Run info* lists them with the run, and a
  saved study's table shows the gap of each point. Every run also gets two
  automatic hand calculations: its top speed against what each E-Motor's
  maximum speed allows through the gears, and the energy its batteries
  gave against the least the car's motion, climb and road load need.
  [How to](help/how-to/check-against-expected-values.md).
- A reference suite of real cars (VAL-05): four 2022 electric cars
  (Tesla Model 3 RWD, Chevrolet Bolt EUV, Nissan Leaf 40 kWh, MINI Cooper
  SE) built from EPA's Test Car List and FASTSim's vehicle files, with no
  input tuned to the results, drive EPA's city and highway cycles on every
  change. Their energy at the wall is within 9 % of EPA's unadjusted
  figures on all eight (4.5 % on average); a virtual coast-down gives back
  EPA's road load within 0.02 %, and an exact-answer tier of three
  coast-downs is within 0.005 %. [What is validated](VALIDATION-STATUS.md)
  has the table and what it does not show. The files are in
  `backend/validation/` and do not ship in the installer.
- Sources & credits (VAL-37): *Run info* lists the data and methods each
  run rests on, from the [data register](DATA-REGISTER.md): its drive cycle
  and the regulation behind it, the example's values and maps, the
  library's defaults and the values typed into the project, each with its
  licence, required credit and a trust level (source unknown, known source,
  validated), and warns when the run uses values whose source is unknown.
  **BibTeX** and **CSL-JSON** save the citations, starting with LightSim
  and its version. [How to](help/how-to/cite-your-sources.md).
- Example cards and stored results (CON-15): each example carries a card
  (Project → **Card…**): the question it answers, its difficulty and run
  time, what you learn, what happens when, the features it uses, its
  status (demo, plausibility-checked or validated), author, version and
  licence; its expected results are its cases' expected values, each with
  a band and a source (the bands of the plausibility tests, EPA's figures
  for the hybrid, FS Czech Republic 2025 for the FS car). An example opened
  from **Open** or the *Start* page shows its stored results in *Results*
  at once, named *Stored result* (only the comparison signals: speed,
  target, battery SOC and power, motor speed and torque, fuel rate); **Run**
  recomputes them and compares with the stored run. CI checks every example
  has a card and expected values, lands within them, and still gives its
  stored results, which change only with a note in
  `backend/tests/golden/CHANGES.md`. No result changes.
- Results for MATLAB and Python: **MATLAB** on the *Results* page saves the
  run as a `.mat` file (MATLAB and SciPy read it), with a struct per part
  holding `t` and its channels, and `meta` with every channel's unit and
  the run's details (project, case, app version, model fingerprint,
  status, summary, the parameters that differ from the defaults, live
  edits and the figures that are not valid). `matlab/lightsim_run.m` runs
  a case from a MATLAB script and returns a table, and the engine runs a
  case from the command line (`lightsim-backend run project.json --case
  "WLTC Class 3b" --out wltc.mat`). CSV files from *Results* and from
  studies start with a byte-order mark, so Excel shows N·m and °C as
  written. (STD-09)
- **Import from file…** under every table, map and drive profile reads it
  from a CSV file (any separator; a decimal comma or point and thousands
  written 1.000 or 1,000 as the other numbers show, and a question when
  they do not) or an Excel workbook: it
  finds the headers and axes, reads the units written in them and
  converts them (rpm, W, m/s, mph, the FASTSim cycle layout and loggers'
  unit rows), shows the curve or map before anything changes, and names
  the row and cell of anything it refuses. The import is one undo step.
  (STD-10)
- The *Parameters* tab exports every parameter of the model to one Excel
  workbook (tables and maps on sheets of their own) or CSV file, and
  **Import sheet** reads it back with a list of every change before it is
  applied as one undo step; rows with a wrong unit, an unknown part or
  key, or a value the parameter cannot hold are refused with their row
  numbers. **FS template** saves the Formula Student example's sheet to
  fill in. (STD-36)
- Projects can live in any folder as `.lightsim` files: **File → Open…**,
  **File → Open Recent**, **Save As…** (also on the Home tab), a
  double-click on the file (Windows, Linux .deb) or a drop on the window.
  Runs, backups and attached files sit next to the file, the runs and
  backups in folders git ignores. *Recent files* is listed in **Home →
  Open** and on the *Start* page. LightSim looks at the open file every
  few seconds and offers to reload it when it changed on disk (a git pull,
  another window). The window can never name a file itself: only the
  desktop app's own Open and Save dialogs, a double-click or a drop can.
  (PLT-33)
- Project files record their format version and the LightSim that saved
  them. Older files are upgraded step by step as they open; the first save
  keeps the old file as `pre-migration-v1.json` in the project's backups. A
  file from a newer LightSim opens read-only, says which version to
  install, and is never saved over. (PLT-07)
- Running a sweep no longer changes the project: studies (what was swept
  and the results table) are kept with the project's runs, as
  `studies/<id>.json`, so git shows only model edits and two people's
  sweeps never collide. (PLT-34)
- Files can be attached to a project (**Project → Attached**): models from
  other tools (FMUs), AI models (ONNX) and measured data are copied into
  the project's resources folder, listed with their size and a fingerprint
  (SHA-256 hash) so a missing or changed file shows in the list and in
  Data Checks. **Export** of a project with attached files saves a
  `.lightsim.zip` with all of them, and **Import** opens one. Parameters
  of the new *file* type pick an attached file. A project carrying
  attached code (FMUs, AI models, programs) asks once whether you trust it
  before its first run; its Script blocks get the code review instead
  (PLT-35). (STD-02)
- **Export** saves `<id>.lightsim` (it was `<id>.json`); **Import** takes
  `.lightsim`, `.json` and `.zip` files.
- Run LightSim models without the app. The `lightsim` Python package runs
  a case in your own Python process (`import lightsim as ls;
  r = ls.run("bev-car", case="City Cycle")`), gives the summary figures by
  stable key, the channels, CSV, MATLAB MAT-file and JSON export and a
  pandas table, and lets a script change values (units are checked:
  `p.set("Vehicle.mass_kg", "1.9 t")`), add and wire parts, add cases,
  check and save. The `lightsim` command (`python -m lightsim` in
  `backend/`, or the desktop engine `lightsim-backend`) has `run`,
  `check`, `export`, `show`, `params`, `parts`, `examples`, `schema`,
  `notebook` and `version`, each with `--json`, and fixed exit codes for
  CI pipelines: 0 done, 1 Data Checks failed, 2 run not valid, 3 usage or
  file error. Help → *Python API* and *Command-line tool*. (AI-02)
- Every run summary figure has a stable key next to its label, such as
  `distance_km` or `el-battery.final_soc_pct` (a part's figures are named
  after the part's id, so renaming the part keeps them); runs keep it,
  and the Results page compares a run with its baseline by it. Results
  are unchanged. (AI-07)
- LightSim's file formats are published: a written specification of the
  project file, the runs folder, results, studies and the engine's API
  and live-run messages, with JSON Schemas generated from the engine
  (`docs/spec/`, and in the help under *Reference*). Its licence, CC BY
  4.0 for the formats only, awaits the owner's confirmation. The engine's
  API reference at `/docs` no longer loads anything from the internet.
  (AI-07)
- AI access rules for the coming AI assistant connection: off by default;
  only projects in folders you allow, and not marked `"noAi"`; changes
  need your confirmation; a project with Script blocks runs only after
  you trust it, and each run asks first; runs stop after 300 s; every call
  is logged on your computer. Set with `lightsim ai …`. (AI-01)
- *What LightSim reads and writes* (Help → *Reference*) lists the file
  formats that work today, each with the test that checks it, those
  planned and those built only on request. (STD-34)
- **Copy for AI** (*Project* tab): copies a short Markdown summary of the
  model and its last run to the clipboard, to paste into any chatbot: the
  parts and wiring, every value changed from the library defaults (with
  units), the cases, the run's key results with their *not valid* notes,
  the Data Check messages, the version and a line saying the results are
  not validated. The examples' summaries are 4.9 to 6.2 KB (the target is
  under 8 KB). **Hide values** replaces the values and results with `[hidden]`.
  LightSim sends nothing. (AI-30)
- **AI assistants can use LightSim** over the Model Context Protocol (MCP):
  `lightsim-backend mcp` is a local server that an AI app starts and talks
  to over stdin and stdout, with no network port. Nine tools list the
  projects, outline a model (the Copy for AI text), read a part, run the
  Data Checks, run a case, query a run's channels (minimum, maximum, mean,
  end value and a thinned series of at most 500 points), compare two runs,
  explain a message and edit a model. Answers stay under 20 KB. It follows
  the MCP revision of 28 July 2026 (stateless, long runs as Tasks,
  confirmations as `input_required`) and still answers apps that use the
  older handshake. It is read-only unless you confirm in the AI app:
  saving an edit and running a project with Script blocks ask first (on
  Windows, Script projects do not run unless you allowed it when
  connecting); a project file with `"noAI": true` stays hidden; an
  assistant's run stops after 5 minutes; every request goes into a local
  log, `.ai/audit.jsonl` in the projects folder. (AI-03)
- **A skill pack for AI assistants**: nine short guides in the open Agent
  Skills format (build a BEV, build a P2 hybrid, wiring rules, units and
  parameters, Script blocks, verifying a model, *not valid* flags, what
  LightSim cannot do, explaining a result). Their component reference and
  known-limits summary are generated from the app. A connected assistant
  reads them from LightSim. (AI-08)
- **Connect AI** (*Project* tab) adds LightSim to Claude Desktop, Claude
  Code, VS Code (GitHub Copilot), the GitHub Copilot CLI, OpenAI Codex,
  Gemini CLI or Cursor with one click, keeping a backup of the app's
  settings file, and shows when an assistant last used LightSim. In a
  terminal: `lightsim-backend mcp install --client <app>`, `uninstall`
  and `status`. `scripts/mcp/build-mcpb.py` builds an MCP Bundle (.mcpb)
  for one-click installs; it runs the installed LightSim or says where to
  get it. (AI-29) See *Use an AI assistant with LightSim* in the help.

- Scripts from someone else are shown before they run. Opening or
  importing a project whose Script blocks hold code you have not approved
  on this computer shows that code with **Run scripts** and **Open without
  running scripts**, and **Run** asks again; the engine itself refuses to
  run such code, for every kind of run. Code you type, the examples' code
  and the scripts of the projects you had already saved count as approved;
  an approval holds for that exact code
  ([how](help/how-to/open-a-project-with-scripts.md)). (PLT-35)
- The script process is locked down further: on Windows it runs at the low
  integrity level, so it cannot change your files, and it cannot start
  programs. On macOS, Apple's sandbox stops it writing files, starting
  programs and using the network. (PLT-35)
- Update checks that ask first. The desktop app asks once whether to check
  for updates once a day (**Help → Updates** changes the answer); a check
  sends only the version and platform. A new version shows what it changes
  in your results and offers *Install on quit*, *Skip this version* or
  *Later*; nothing installs without asking. Releases reach users in stages
  ([how](help/how-to/check-for-updates.md)). (PLT-18)
- For labs and company IT: the setup program installs silently for all
  users (`/S /allusers`), a new MSI package installs and upgrades with
  `msiexec /quiet`, and a machine-wide `policy.json` fixes update checks,
  the script prompt, the examples and the projects folder for every user
  ([how](help/how-to/deploy-for-it.md)). (PLT-36)
- The build can sign every Windows file (the app, the engine, their
  libraries, the installers and the MSI) and builds a signed, notarised
  macOS version for Apple silicon, once the owner's certificates are set
  up; this release is unsigned and has no Mac version unless its download
  page says otherwise. Every Windows build is scanned with Microsoft
  Defender and test-installed. (PLT-32, PLT-13)
- **Help → What Stopped You?…** in the desktop app opens LightSim's Idea
  form on GitHub in your browser, with your version filled in, so you can
  say what kept you from finishing a task. Nothing is sent unless you
  submit the form. (BIZ-35)
- How to help, and how to report a security problem privately:
  `CONTRIBUTING.md` and `SECURITY.md` in the repository, with bug and idea
  forms for GitHub issues. Code contributions need a signed contributor
  licence agreement, which is not ready yet. (BIZ-13)
- A draft licence FAQ, *Can I use LightSim for …?*
  (`docs/licensing/licence-faq.md`), answers 20 everyday cases, such as
  Formula Student teams, theses, lab PCs and company trials. It is a draft
  and the end-user licence agreement still decides. (BIZ-29)

### Fixed

- A battery's SOC can be compared with measured SOC: a constant 1C
  discharge now empties it in one hour whatever the shape of its OCV
  table (before, 3,611 s on the default table and 3,666 s on a steeper
  one).
- A motor could run far past the end of its maps with no message (a
  300 km/h target: 21,238 rpm on a map that ends at 12,000). Reaching the
  maximum speed and leaving a map's data are now named in Messages; these
  first-touch messages, the motor's voltage-axis note and the engine's rev
  limiter are *info*, so a brief touch no longer turns a run into a
  *warning*.
- A stop pressed while a paced run was on its last step turned the
  complete run into a *warning* with no message saying why.
- A run an error stopped part-way (a table set to *Error*, a failing
  script) showed its Consumption, Fuel consumption, CO₂ emissions and a
  performance test's Maximum speed, which cover only the part it ran, as
  valid; they are now marked *not valid: run stopped by an error at
  t = …*, as for a stop, and a performance test it cut short no longer
  says the car fell short of its target.
- A run whose motor ran on a voltage past its map for the whole cycle, or
  whose motor the wheels drove above its maximum speed, could be a
  *success* with valid consumption. The run status now judges the time
  outside, as above; Lookup blocks are left out, because their table is a
  controller's schedule, not physics.
- The Ambient block's temperature and pressure were ignored: air drag
  always used 1.2 kg/m³.
- On grades the slope force used the grade ÷ 100 instead of the sine of
  the slope angle, and rolling resistance and tyre grip ignored the slope
  (0.5 % too much at a 10 % grade, 3 % at 25 %).
- The P2 Hybrid Car counted the driveline drag that EPA's road-load
  coefficients already hold a second time.
- One Ctrl+Z after deleting with the Delete or Backspace key brings back
  the parts together with their wires, also for a selection of several
  parts and wires. Before, the first Ctrl+Z brought a part back without its
  wires, and saving then lost them. (UX-39)
- Dragging the end of a wire to another pin is one undo step, and the wire
  stays where it was when the new connection is refused.
- Delete or Backspace in a map's cell clears the cell; it no longer also
  deletes the part selected on the diagram.
- Monitor and Script blocks you add yourself can be wired in the Data Bus
  panel; before, only the examples' ones could, because their links were
  written into the project file. (UX-40)
- Chart picture export saves the whole chart at twice its size on screen,
  legend included; before, it saved a 28 × 28 px legend icon. CSV export
  quotes fields as RFC 4180 says, so an element label with a comma in it
  stays in one column. (RES-37)
- The Open and Restore… menus and the diagram's right-click menu close on
  Esc, on a click anywhere outside them (the diagram too) and when a dialog
  opens; before, the Open menu stayed over the diagram, and even over the
  parameter dialog. (GUI-33)
- On a 1366-px screen, numbers in the Properties and Cases panels are shown
  in full (the Driver's I Gain of 0.08 read "0.0"), parameter labels stay on
  one line with the full text in a tooltip, and the *Cases & Parameters*
  tab is titled *Cases*. (GUI-34)
- The status bar counts the errors the model has now, from the latest Data
  Checks, instead of every error ever logged, which never cleared. Once a
  model has been checked, its checks, the error count and the red badges
  on parts follow it as you edit. (UX-09)
- The Data Checks summary in Messages is information, not an error, and
  reads "1 error, 0 warnings" instead of "1 error(s), 0 warning(s)"; before,
  Messages kept counting it after the model was fixed. (UX-09)
- A signal link between two inputs or two outputs is refused with the
  reason; before, it was added with a warning and passed no data. Links
  are logged as "from → to" instead of "↔". (UX-15)
- The Signal Plot below the diagram follows a paced (live) run while it
  runs; before, it stayed on the run's first point until the run ended.
  (RES-03)
- Text, field borders, wires and chart lines meet the WCAG AA contrast
  minimum in both themes (4.5:1 for text, 3:1 for the rest). Before, grey,
  amber and red status text in the light theme, the white-on-blue LightSim
  title, Run and Save buttons and the green and red status text in the
  dark theme, text fields' borders, the electrical wires on white, the
  mechanical wires on the dark diagram and three of the ten chart colours
  on dark charts were too faint. Status colours, the accent and field
  borders now have a shade per theme, and wires, part outlines, layer
  swatches and port dots share one set of domain colours. Chart legends
  and the Results summary's run columns show names in plain text next to
  a mark in the series colour. (GUI-02)
- Part names are never smaller than 11 px on screen, whatever the zoom;
  before, they shrank with the diagram (7 px when a 1366 × 768 window was
  fitted, under 2 px zoomed all the way out). Until you zoom or pan, the
  diagram also re-fits when a bottom panel closes or the window grows;
  before, it re-fitted only when it got smaller. Fits leave room for the
  names under the lowest parts. (GUI-03)
- Delete or Backspace on the Results page no longer deletes the part
  selected on the diagram hidden behind it. (UX-16)
- Data Checks refuse 40 values that cannot be right. A negative inertia,
  drag coefficient, frontal area, initial speed, driver gain, battery
  resistance or time constant, maximum charge power, brake, clutch or
  propeller torque, friction coefficient μ, rolling resistance, cycle
  scale, re-entry speed, CO₂ factor or H₂ consumption is an error, and so
  is a zero or negative voltage, slip stiffness, propeller reference
  speed, idle speed, tank capacity, fuel density or fuel-cell current, and
  a default gear below 1. Before, these ran without a word (a Driver I
  Gain of −1 ended in "did not drive the cycle"), or the solver quietly
  used another value (0 for a negative μ, 0.1 for a slip stiffness of 0
  or less, 1 1/min for an idle speed of 0). (UX-10)
- A case could run with its own values out of range, such as an Initial
  SOC of 150 %: Data Checks did not look at them. They now do, and name
  the case. (UX-10)
- Data Checks name a parameter as the app labels it and say what is
  allowed in words: *Coulombic Efficiency of 'HV Battery Pack' must be
  above 0 and at most 100 % — got 0.* instead of *Coulombic efficiency
  … must be in (0, 100]*. Text or an infinite value in any number
  parameter is now an error, not only in those that had a range, and a
  value just below 0, such as −0.0005 %, no longer passes where 0 is the
  least allowed. (UX-10)
- Clearing a number in *Cases & Parameters* stored 0; the field now keeps
  its value until you type a number, as in *Properties*. A red field's
  border was too faint on the dark theme (3.4:1); it now uses the theme's
  error colour. (UX-10)
- Time read-outs on the charts showed raw floats, such as
  0.30000000000000004 s at a 0.1 s step, and so did the CSV's time column;
  and the sweep view opened on the final SOC, drawn flat on a 0-100 axis.
  (RES-18)
- The summary's scroll box could not be reached with the keyboard. (RES-30)

- The desktop app fetched spell-check dictionaries from Google's servers
  when it started on Linux, and looked for a network proxy on Windows,
  though LightSim promises to contact nothing outside your computer. Both
  are off; a test of every packaged build now fails on any request beyond
  the computer. (PLT-18)

### Upgrading from a 0.2.0 build

- A battery without a Charge Capacity gets it from its Usable Capacity ÷
  the OCV table's mean voltage (the Battery Electric Car's 62 kWh on the
  default table = 179.7 Ah), so it still gives out its Usable Capacity from
  full to empty. Project files are not changed.
- A model whose motor loss map or engine fuel map is narrower than its
  full-load map, whose fuel map does not reach the full-load curve's first
  speed, whose motor full-load map starts above 0 1/min, whose motor has a
  Maximum Speed beyond its full-load data, or whose fuel cell has a Maximum
  Current beyond its polarization curve (or a curve that starts above 0 A),
  now stops where it used to hold the map's edge value. Data Checks name each
  case before the run: extend the map, or set that axis to *Clamp* in the
  table editor so that the run finishes on the held edge value, as in
  0.2.0. Unlike 0.2.0, a run that then spends more than 1 % of its time
  (at least 2 s) outside the data ends as *warning* with its per-distance
  figures marked *not valid*, and Data Checks keep naming the gap.
- A voltage source, DC-DC converter, fuel cell, engine or brake left at its
  library default takes the new default (see the table above).
- Vehicles keep taking their road load from drag and rolling resistance.
  An Ambient already in a 0.2.0 project now sets the air density; a
  project with several Ambients runs, with a warning, on the first one.
- Wheels in 0.2.0 projects are on the Front axle, so their Rear Axle Load
  reads 0 and nothing shifts. Set *Axle* to Rear on the rear wheels before
  giving the Vehicle a CG height; Data Checks refuse a CG height while all
  the wheels are on one axle.
- Runs you stopped in 0.2.0 keep their *warning* status and their
  *stopped at t = …* note. Cases load as kind *Cycle*.
- The *Data Checks* tab is now *Problems*; a saved layout keeps it where it
  was. A signal link between two inputs or two outputs saved by an earlier
  build still loads: Data Bus Connections shows it in amber with a remove
  button, and Data Checks still report it. So does a link to a part or port
  that is gone.
- LightSim opens on the *Start* page, and **New** on the Home tab opens
  it too: choose *Blank project* there for the empty project **New** made
  before. Tick *Skip this page* on it to open your last project at
  start-up, as before.
- The examples' WLTC, UDDS and HWFET cases name their drive cycle
  instead of carrying its points, and give exactly the same results. A
  case of yours that carries its own points keeps them, and they still
  win over a cycle picked on the part. 0.2.0 does not know drive cycles:
  opened there, a Driving Task or case that names one drives the typed
  profile instead, with no warning.
- A project that holds one of the 40 values Data Checks now refuse (see
  Fixed) stops before the run with an error that names the part and what
  is allowed; set the value inside the limits. The examples hold none.
  So does a case whose own value is out of range: change or remove it in
  *Cases & Parameters*. A stored value out of range shows red in its field
  as soon as the project opens.
- Project files are now in format 2 (PLT-07). A 0.2.0 file opens as
  before: its studies move out of the file into the project's runs folder
  (they stay listed under *Saved studies*), and the first save keeps the
  file as it was in **Project → Restore…**'s folder as
  `pre-migration-v1.json`. No results change.
- Going back to 0.2.0: it opens format 2 files, but shows no studies (they
  are next to the runs now) and keeps attached files only as a list.
- The first start of 0.3.0 counts the scripts in the projects already in
  your projects folder as approved. A project you receive later asks
  before its scripts run.
- The desktop app asks once whether to check for updates; until you say
  yes it checks nothing.
- Going back to 0.2.0: it cannot open a project with a study point that
  says *cancelled*. It still lists and opens stored runs that say
  *cancelled*, but drops them from the list if it has to rebuild its run
  index.
  A case's Kind is kept but ignored.
  A run's name and note are kept but ignored.

## 0.2.0 — not published: its changes ship in 0.3.0

The app is renamed from SimStudio to LightSim; your projects come along
(see [Upgrading from 0.1.0](#upgrading-from-010)).

This release is about trust: results that no longer depend on hidden
settings, a run status you can rely on, and example cars that behave like
real ones. It is still an early version: nothing is validated against
measured vehicles yet (see [What is validated](VALIDATION-STATUS.md)), and
[Known issues and limits](KNOWN-LIMITS.md) lists what is still wrong.

### Your results will change — here is why

If you ran your own models in 0.1.0, expect different numbers. Each change
below makes results more correct; none is a tuning.

| What changed | Effect on results | Roadmap |
|---|---|---|
| Controllers (Driving Task, Script, PID, Lookup, gear choice) now update every 10 ms instead of once per case step | Results no longer depend on the case step. Models that used a 1 s step change the most: the old hybrid example went from 19.34 to 7.76 l/100 km with this change alone | ENG-01 |
| Each recorded point now holds the state at its own time, and runs stop exactly at the case duration | Every channel moves one point later; distance reads 0 at t = 0 | ENG-03 |
| Motors can only draw what the battery or fuel cell can supply; braking energy the battery cannot take goes to the friction brakes | Runs that reached a battery or fuel-cell limit no longer drive on energy that does not exist | ENG-02 |
| The electric motor's spin losses are counted once, and the library's default motor maps were corrected | Models using the default E-Motor use about 15 % less energy | MOD-04 |
| The combustion engine delivers its full rated power, cuts fuel when coasting, and stops at a rev limiter; a CO₂ figure is added | Engines now reach their full-load curve (the old model fell 28 % short of it) and use no fuel when coasting in gear | MOD-05 |
| A run where the car did not follow its cycle, did not move, or ran out of energy is no longer called a success, and figures that cannot be trusted are flagged "not valid" with the reason | Some runs that said "success" now say "warning" or "failed" | VAL-02 |
| Wheel load shares of the connected wheels are scaled to add up to 100 % | Models whose shares did not add up change: a single axle left at the default 25 % per wheel now has twice the rolling resistance and grip | MOD-06 |
| The Driver recuperates up to what the battery can take (before, 80 % of a charge limit with the default settings), and regeneration a motor's supply cannot take is reported in a new summary row | Recuperation can rise in models with a charge-power limit (23 % in a 20 kW test); the examples do not change | MOD-02 |
| A parameter changed during a live run stays in force when the gearbox shifts | Before, the first shift quietly restored the saved value | ENG-04 |
| Gear, final-drive and differential losses act on the power actually flowing through each gear, in both directions, wherever the driveline starts | Hybrids use more fuel (the hybrid example +5 to +10 %); before, a hybrid's gearbox and final drive could lose nothing | MOD-03 |
| A run that starts at speed starts every wheel, gear, motor and closed-clutch engine at that speed | The first seconds of such runs change; before, a motor behind an open differential started at 0 rpm | MOD-19 |

Runs of models with Script blocks take longer than in 0.1.0 at the default
1 s step (the hybrid example about 11 s against 7.9 s on a test machine),
because controllers now run every 10 ms and scripts run in a process of
their own; on computers with 4 or more cores that process keeps a second
core busy while such a run goes at full speed. Other models take about as
long as before, and runs with fine steps are much faster.

### New example cars

- **Battery Electric Car** — now the 2021 Cupra Born (values from FASTSim,
  Apache-2.0): 14.0 kWh/100 km on WLTC at the battery, 0–100 km/h in 7.2 s,
  160 km/h top speed. A second case adds heating or air-conditioning.
- **P2 Hybrid Car** — now sized after the Hyundai Ioniq Hybrid with EPA
  road-load data and a charge-sustaining control strategy: 2.95 l/100 km on
  the EPA city cycle and 3.30 on the highway cycle, against 2.91 and 2.94
  for the real car in EPA's tests.
- Examples now come with the app instead of being copied once, so fixed and
  new examples reach you when you update. Opening an example gives you an
  unsaved copy; the original is never overwritten. You can hide examples.

### New

- Runs are kept on disk with their project, so they survive a restart and
  switching projects. Each run remembers the exact model, settings, app
  version and live edits that produced it (Results → ⓘ Run info), and can be
  reopened as a model.
- Parameter sweeps are saved with the project as studies, each with its
  results table (Cases & Parameters, with a CSV download).
- The last 20 versions of each project are kept as backups (Project →
  Restore…).
- The diagram gets most of the window; parts can be added with Enter, a
  double-click, or a click on the part and then on the diagram; the zoom
  stays readable.
- Charts keep their real peaks: a −2,769 N·m torque dip used to be drawn as
  −316 N·m.
- Help → Known Limits and Help → Third-Party Notices.

### Fixed

- New, Open, Import and closing the window ask before throwing away unsaved
  changes.
- Number fields can be cleared and retyped; Esc in a table cell no longer
  closes the dialog; values that are not numbers are refused instead of
  silently dropped.
- Stopped or failed sweep points are marked and left out of the sweep
  curve.
- The Signal Plot and the Results chart draw on the first run.
- Saving keeps node sizes and pin positions.
- Data Checks catch models that cannot drive (no energy source, no path to
  the wheels, no speed demand) and two signals wired into one input.

### Security

- The simulation engine now answers only the app's own window: websites
  cannot reach it, and in the desktop app every request needs a key that
  changes each time the app starts.
- Data Checks never run a model's Script code. During a run, scripts can
  only do calculations, and they now run in a process of their own: the
  engine stops it if a step takes more than 2 s, it has a memory cap, and on
  Linux the system also blocks its file and network access. This is much
  harder to get around than before, but not a full sandbox, least of all on
  Windows ([details](KNOWN-LIMITS.md)): only open projects from people you
  trust.
- Saves are crash-proof, and a project changed on disk by another window is
  not overwritten without asking.

### Upgrading from 0.1.0

- On its first launch LightSim copies your projects from SimStudio's folder
  into its own (see
  [Where your work is saved](../README.md#where-your-work-is-saved));
  SimStudio's folder is left as it was. Examples you had are kept as your own
  projects; the updated examples are listed separately.
- The window layout, theme and font size start from their defaults, and a
  recovery draft of unsaved changes is not carried over: save your work in
  SimStudio before you switch.
- Runs from 0.1.0 were not saved, so the run history starts empty.
- LightSim installs as a new app beside SimStudio. Uninstall SimStudio once
  your projects open in LightSim; uninstalling it leaves its projects folder
  in place.

### Licence

LightSim is free for evaluation, learning, research and other
non-commercial use under the end-user licence agreement (`EULA.txt`);
commercial use needs a separate licence. It includes open-source software
under its own licences (Help → Third-Party Notices).
