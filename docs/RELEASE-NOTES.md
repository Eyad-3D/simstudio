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
| The P2 Hybrid Car's control script starts or stops the engine only when asked for 0.2 s in a row (at standstill it still stops at once), so one step's reading of the input shaft, which rings for a few steps as the clutch closes, no longer switches it; its cases start at re-balanced charges (UDDS 56.34 %, HWFET 58.92 %, Mixed 51.96 %) | EPA city (UDDS) 2.84 → 2.83 l/100 km with 30 engine starts instead of 32, highway (HWFET) 3.23 → 3.24, Mixed Cycle 2.88 l/100 km unchanged. The results no longer jump with small changes to the model or the step: with the example as MOD-18 left it, a brake inertia of 0.14 kg·m² or less added an engine start at the 10 ms step only | MOD-18 |

### New

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
- Runs you stopped in 0.2.0 keep their *warning* status and their
  *stopped at t = …* note. Cases load as kind *Cycle*.
- Going back to 0.2.0: it cannot open a project with a study point that
  says *cancelled*. It still lists and opens stored runs that say
  *cancelled*, but drops them from the list if it has to rebuild its run
  index.
  A case's Kind is kept but ignored.

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
