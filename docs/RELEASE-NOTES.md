# Release notes

Each release lists what changed, and above all **which results changed and
why**, so that you can tell whether a number you got from an earlier version
still holds. The full record of every change to the reference results is in
[`backend/tests/golden/CHANGES.md`](../backend/tests/golden/CHANGES.md).

## 0.2.0 — first public release (early version)

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

Runs of models with Script blocks take somewhat longer than in 0.1.0 at the
default 1 s step (the hybrid example about 9.7 s against 7.9 s on a test
machine), because controllers now run every 10 ms; other models take about
as long as before, and runs with fine steps are much faster.

### New example cars

- **Battery Electric Car** — now the 2021 Cupra Born (values from FASTSim,
  Apache-2.0): 14.0 kWh/100 km on WLTC at the battery, 0–100 km/h in 7.2 s,
  160 km/h top speed. A second case adds heating or air-conditioning.
- **P2 Hybrid Car** — now sized after the Hyundai Ioniq Hybrid with EPA
  road-load data and a charge-sustaining control strategy: 2.67 l/100 km on
  the EPA city cycle and 3.13 on the highway cycle, against 2.91 and 2.94
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
- Data Checks never run a model's Script code, and during a run Scripts can
  only do calculations (they cannot open files, start programs or reach the
  network). This is a restriction, not a full sandbox: only open projects
  from people you trust.
- Saves are crash-proof, and a project changed on disk by another window is
  not overwritten without asking.

### Upgrading from 0.1.0

- Your projects stay where they are. Examples you had are kept as your own
  projects; the updated examples are listed separately.
- The window layout resets once, because the default layout changed.
- Runs from 0.1.0 were not saved, so the run history starts empty.

### Licence

SimStudio is free for evaluation, learning, research and other
non-commercial use under the end-user licence agreement (`EULA.txt`);
commercial use needs a separate licence. It includes open-source software
under its own licences (Help → Third-Party Notices).
