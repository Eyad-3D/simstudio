# Use LightSim results in MATLAB and Python

LightSim saves a run as a MATLAB `.mat` file, the format MATLAB and
Python's SciPy read. Formula Student teams usually get MATLAB free from
their competition; Python with SciPy is free for everyone.

## Save a run as a .mat file

1. On the *Results* page, pick the run in the list at the top left.
2. Click **MATLAB** above the chart.

The file is named after the project, the case and the run, such as
`Battery Electric Car - City Cycle - Vehicle Mass 2,300 kg.mat`. It holds
every channel of the run, not only the ticked ones.

## What is in the file

- **One struct per part**, named after the part with spaces and dashes
  turned into underscores: `HV_Battery_Pack`, `E_Motor`. Each holds `t`,
  the time in s, and one column per channel of the part, such as
  `HV_Battery_Pack.SOC` or `E_Motor.Shaft_Torque`. A point where a channel
  has no value is `NaN` (not a number).
- **`meta`**, the run's details:
  - `meta.units.E_Motor.Shaft_Torque` is the channel's unit (`N·m`), and
    `meta.labels` its name as LightSim shows it (`E-Motor · Shaft Torque`);
  - `project`, `case_name`, `run_name`, `run_note`, `started` (UTC),
    `status`, `app_version` and `model_hash`, the fingerprint of the model
    that ran;
  - `summary`, the run's summary values with their units;
  - `not_valid`, the figures the run's checks rule out, with why
    ([Run status](../../KNOWN-LIMITS.md));
  - `changed_parameters`, every parameter that differs from the library's
    default, and `live_edits`, the values changed while the run went;
  - `run_card_json`, all of this as one JSON text.

## Read it in MATLAB

```matlab
R = load('Battery Electric Car - City Cycle.mat');
plot(R.Vehicle.t, R.Vehicle.Vehicle_Speed)
ylabel(['Speed [' R.meta.units.Vehicle.Vehicle_Speed ']'])
```

## Read it in Python

```python
from scipy.io import loadmat
R = loadmat("Battery Electric Car - City Cycle.mat", simplify_cells=True)
t, soc = R["HV_Battery_Pack"]["t"], R["HV_Battery_Pack"]["SOC"]
print(R["meta"]["units"]["HV_Battery_Pack"]["SOC"])  # %
```

`simplify_cells=True` turns the structs into Python dictionaries.

## Run LightSim from a MATLAB script

`lightsim_run.m` runs a case of a saved project with LightSim's engine and
returns a MATLAB table. It is in the `matlab` folder of LightSim's source
(`matlab/lightsim_run.m`); copy it to a folder on your MATLAB path.

1. Save the project in LightSim and note its file (**File → Open Projects
   Folder** shows where it is).
2. In MATLAB:

   ```matlab
   T = lightsim_run('fs-electric.json', 'Acceleration 75 m');
   plot(T.t, T.Vehicle_Vehicle_Speed)
   ```

`T` has a column `t` and one column per channel, named `Part_Channel`;
`T.Properties.VariableUnits` holds the units. `[T, meta] = lightsim_run(…)`
also returns the run's details as above. The run happens on your computer,
with no window and no network.

`lightsim_run` finds the engine of an installed LightSim (Windows:
`%LOCALAPPDATA%\Programs\LightSim\resources\backend\lightsim-backend.exe`;
Linux `.deb`: `/opt/LightSim/resources/backend/lightsim-backend`). With the
AppImage, or a LightSim installed elsewhere, give the engine's path:
`lightsim_run(project, case, 'Engine', 'C:\...\lightsim-backend.exe')`, or
set the environment variable `LIGHTSIM_ENGINE` to it.

If Data Checks find errors, `lightsim_run` stops with them as its error
message. If the run ends with figures that are not valid, it warns and
still returns the table.

## The same from the command line

The engine runs a case and writes the file without MATLAB:

```text
lightsim-backend run fs-electric.json --case "Acceleration 75 m" --out accel.mat
lightsim-backend run fs-electric.json --case "Acceleration 75 m" --out accel.csv
```

With `--out` ending in `.csv`, it writes CSV and the run's details next to
it as `accel.runcard.json`. `--json` prints the outcome and the summary as
JSON. It ends with code 0 when the run is done, 1 when Data Checks found
errors, 2 when a figure is not valid or the run failed, and 3 when the
command or a file is wrong.
