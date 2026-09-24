# Release notes

Each release lists what changed, and above all **which results changed and
why**, so that you can tell whether a number you got from an earlier version
still holds. The full record of every change to the reference results is in
[`backend/tests/golden/CHANGES.md`](../backend/tests/golden/CHANGES.md).

## 0.3.0 (not released yet)

### Your results will change — here is why

| What changed | Effect on results | Roadmap |
|---|---|---|
| Battery state of charge counts the charge that flows (amp-hours), as battery management systems and datasheets do, and the OCV table is read at that SOC | SOC moves by up to about 2 points at mid-charge for the same energy; energies and consumption are unchanged (BEV WLTC ends at 84.93 % instead of 84.61 %; hybrid fuel unchanged to 0.01 l/100 km). Control scripts that switch on SOC switch at slightly different moments, and a run from 90 % down to a 4 % floor gets 0.35 % less energy on the default OCV table | MOD-38 |
| E-Motors stop at their maximum speed: the drive torque falls to zero over the last 2 % below it, and above it the inverter is off (no drive, no regeneration) | A 300 km/h target on the default motor now tops out at 152 km/h with the motor at 11,921 of 12,000 1/min, instead of 269 km/h at 21,238 1/min on made-up torque. The examples do not reach their maximum speeds | MOD-18 |
| Motor and engine maps no longer extend their data: a run that reads a motor's full-load map outside its speed data, a motor's loss map or an engine's fuel map outside its speed or torque data, an engine's full-load curve outside its speed data, or a fuel cell's polarization curve outside its current data stops with an error naming the table, the axis, the value and the time | Models whose maps do not cover where they run now fail instead of finishing on made-up values; Data Checks warn before the run. Engines below their full-load curve's first speed (starting) use that point, as before | MOD-18 |
| The friction brake's default inertia is 0.18 kg·m² instead of 0.6 (a 330 mm disc) | Cars with default brakes accelerate and brake a little more easily: BEV WLTC 14.04 → 14.03 kWh/100 km, hybrid EPA city 2.95 → 2.94 and highway 3.30 → 3.29 l/100 km | MOD-18 |
| New library defaults that fit the default E-Motor's 250–396 V map: voltage source and DC-DC output 350 V (were 400 and 800 V), fuel-cell curve 396–250 V (was 420–264 V, now 100 kW at 400 A instead of 105.6 kW); the default engine's full-load peak is 175 N·m (was 178) and its fuel map starts at 800 1/min | Models built on these defaults change; the examples do not use them | MOD-18 |

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
  the speed and torque axes of motor and engine full-load, loss and fuel
  maps and on the fuel cell's current, Clamp on the others (battery SOC,
  motor voltage, drag tables, Lookup tables); a setting you change is saved
  with the part.
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

### Upgrading from 0.2.0

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
  table editor to get the 0.2.0 behaviour back.
- A voltage source, DC-DC converter, fuel cell, engine or brake left at its
  library default takes the new default (see the table above).

## 0.2.0 — first public release (early version)

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
