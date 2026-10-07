# Runs and results

A run's result lists its status, its messages, the channels it recorded
and its summary figures. The app keeps each finished run in the projects
folder; the `lightsim` command-line tool and Python package give the same
result and can write it as CSV, MAT or JSON.

## The runs folder

```
<projects folder>/
  <project id>.json            the project
  runs/<project id>/
    index.json                 a list of the stored runs, without channels
    <run id>.json.gz           one stored run, gzip-compressed JSON
```

- `index.json` is `{"runs": [ … ]}`: each entry is a stored run without
  `result` and `snapshot`, plus its `summary` rows and `bytes` (the file's
  size). It is rebuilt from the run files when it is missing or wrong, so
  read the run files when in doubt.
- The app keeps at most 500 MB of runs per project and 2 GB in all,
  deleting the oldest first.

## Stored run

A stored run (`<run id>.json.gz` after unzipping) follows
[`schemas/run.schema.json`](schemas/run.schema.json).

| Field | Type | Meaning |
|---|---|---|
| `id` | text, required | The run's id |
| `caseId`, `caseName` | text, required | The case it ran, and its name then |
| `startedAt` | whole number, ms since 1970, required | When it started |
| `status` | `success`, `warning`, `failed` or `cancelled`, required | As the result's |
| `result` | [Result](#result), required | What the run gave |
| `sweepId`, `sweepParam`, `sweepValue`, `sweepUnit` | optional | For a point of a parameter sweep: the sweep, the swept parameter, its value and unit |
| `incomplete` | text, optional | Why the run is not a complete result (stopped, failed, connection lost) |
| `snapshot` | optional | What made the run: `project` and `case` exactly as they ran (a sweep's value included), `appVersion`, `modelHash` (SHA-256 of the project as JSON with sorted keys) and `liveEdits` (`t`, `elementId`, `key`, `value` of each value changed while it ran). Runs stored before 0.3 have none |
| `name`, `note` | text, optional | The run's name (by default what changed since the previous run of its case) and the user's note |

## Result

A result (`SimResult`, [`schemas/result.schema.json`](schemas/result.schema.json)):

| Field | Type | Meaning |
|---|---|---|
| `caseId` | text | The case that ran |
| `status` | `success`, `warning`, `failed` or `cancelled` | *success*: the run finished, followed its target, stayed inside its data and nothing warned. *warning*: it finished but something is off (Messages say what). *failed*: an error stopped it, or the Data Checks did not let it start. *cancelled*: it was stopped |
| `messages` | list of `{level, text}` | What the run reported: `level` is `info`, `warning` or `error`, `text` the message |
| `channels` | list of [Channel](#channel) | The recorded signals |
| `summary` | list of [Summary row](#summary-row) | The figures, in the order the app shows them |

### Channel

| Field | Type | Meaning |
|---|---|---|
| `elementId`, `portId` | text | The part and the port (or state, such as `sig_soc`) it records. Together they name the channel: `el-battery:sig_soc` |
| `label` | text | "part label · port name", as the app shows it |
| `unit` | text | Its unit |
| `timeSeries` | list of `{t, value}` | `t` in s; `value` is null where the channel has no data yet (a gap, not a zero) |

Every channel of a run has the same times. Point 0 is the state at t = 0,
each later point the state at its own time, and the last point is at the
case's duration (or where the run stopped).

### Summary row

| Field | Type | Meaning |
|---|---|---|
| `key` | text | The figure's [stable key](#summary-keys). Empty on runs stored before 0.3 |
| `label` | text | What the app shows. It names the part by its label and may change between versions: do not look figures up by label |
| `value` | number | The figure, rounded as the app shows it |
| `unit` | text | Its unit |
| `notValid` | text or null | Why the figure is not valid (for example `cycle not followed`); null when it is |
| `limit` | number or null | A check's limit, in the row's unit |
| `passed` | true, false or null | Whether the value kept to its check |

## Summary keys

Every summary figure has a key that never changes its meaning. A key with
a dot is a part's figure: the part's **id**, a dot, then the figure
(`el-battery.final_soc_pct`), so renaming a part does not change it. A key
without a dot is the run's. The unit is at the end of the name.

Which figures a run has depends on the model and the case: a figure
appears only when it applies (a fuel figure needs an engine, the lap
figures a lap case, the "outside" figures a run that left a table's data).

### The run's figures

| Key | Label in the app | Unit |
|---|---|---|
| `distance_km` | Distance driven | km |
| `consumption_kwh_per_100km` | Consumption (net battery energy) | kWh/100km |
| `fuel_consumption_l_per_100km` | Fuel consumption | l/100km |
| `co2_g_per_km` | CO₂ emissions | g/km |
| `energy_balance_error_pct` | Electrical energy balance error | % |
| `simulated_duration_s` | Simulated duration | s |
| `max_speed_kmh` | Maximum speed (performance test) | km/h |
| `time_to_target_s` | Time to *target* km/h (performance test) | s |
| `accel_time_s` | Time to *distance* m (acceleration test, from the start line) | s |
| `accel_end_speed_kmh` | Speed at *distance* m | km/h |
| `accel_gap_to_reference_s` | Gap to reference time | s |
| `time_to_100_kmh_s` | Time to 100 km/h (acceleration test, from t = 0) | s |
| `grip_limit_time_pct` | Time at the tyres' grip limit | % |
| `lap_time_s` | Lap time (the fastest lap) | s |
| `lap1_time_s` | Lap 1 time | s |
| `total_time_s` | Total time | s |
| `sector1_time_s`, `sector2_time_s` … | Sector 1 time … (of the fastest lap) | s |
| `average_speed_kmh` | Average speed | km/h |
| `finish_speed_kmh` | Speed at the finish (open tracks) | km/h |
| `energy_per_lap_kwh` | Energy per lap | kWh |
| `rms_battery_power_kw` | RMS battery power | kW |
| `time_limited_by_cornering_grip_s`, `time_limited_by_traction_grip_s`, `time_limited_by_motor_s`, `time_limited_by_battery_s`, `time_limited_by_power_cap_s`, `time_limited_by_braking_s` | Time limited by … | s |
| `lap_energy_balance_error_pct` | Lap energy balance error | % |

### A part's figures

`<id>` is the part's id.

| Key | Label in the app | Unit |
|---|---|---|
| `<id>.final_soc_pct` | *Battery* — final SOC | % |
| `<id>.energy_delivered_kwh` | *Battery* — energy delivered | kWh |
| `<id>.energy_recuperated_kwh` | *Battery* — energy recuperated | kWh |
| `<id>.internal_losses_kwh` | *Battery* — internal losses | kWh |
| `<id>.peak_terminal_power_kw` | *Battery* — peak terminal power | kW |
| `<id>.peak_terminal_power_averaged_kw` | *Battery* — peak terminal power, averaged (checked against the Output Power Limit) | kW |
| `<id>.time_at_power_limit_s` | *Battery* — time held at (or over) the output power limit | s |
| `<id>.max_pack_voltage_v` | *Battery* — maximum pack voltage (checked against the Voltage Class) | V |
| `<id>.min_pack_voltage_v` | *Battery* — minimum pack voltage | V |
| `<id>.usable_energy_left_kwh` | *Battery* — usable energy left | kWh |
| `<id>.mean_terminal_power_kw` | *Battery* — mean terminal power (acceleration test) | kW |
| `<id>.time_limited_by_supply_s` | *E-Motor* — time limited by supply | s |
| `<id>.regen_not_recovered_kwh` | *E-Motor* — regeneration not recovered | kWh |
| `<id>.fuel_used_kg` | *Engine* — fuel used | kg |
| `<id>.energy_supplied_kwh` | *Fuel cell* or *voltage source* — energy supplied | kWh |
| `<id>.time_above_max_speed_pct` | *Motor or engine* — time above maximum speed | % |
| `<id>.highest_speed_rpm` | *Motor or engine* — highest speed | 1/min |
| `<id>.outside_<table>_<axis>_time_pct` | *Part* — time outside its '*Table*' table (*axis*); `<table>` is the parameter key, `<axis>` the axis name in lower case with `_` for spaces and signs | % |
| `<id>.outside_<table>_<axis>_furthest` | *Part* — furthest *axis* outside its '*Table*' table | the axis's unit |

## Exports

The app's *Results* tab exports a run as CSV. The command-line tool and
the Python package (`Result.to_csv`, `to_mat`, `to_json`) write three
formats.

### CSV

One row per stored point; the first column is `Time [s]`, then one column
per channel headed `label [unit]` (the app's export puts the x axis of its
chart first instead). An empty cell is a gap. Numbers use a dot as the
decimal sign, text is UTF-8.

### MAT

A MATLAB Level 5 MAT-file (what `save -v6` writes), which MATLAB, GNU
Octave and `scipy.io.loadmat` read:

| Variable | Holds |
|---|---|
| `time` | the times, s (a column) |
| one per channel | its values (a column; NaN for a gap), named after the channel's label with other signs as `_` (`HV_Battery_Pack_SOC`) |
| `units` | a struct: channel variable → unit |
| `channel_keys` | a struct: channel variable → `elementId:portId` |
| `kpis` | a struct: summary key (with `.` and other signs as `_`) → value |
| `kpi_units` | a struct: the same names → unit |
| `info` | a struct: `project`, `case`, `case_id`, `status` |

### JSON (`lightsim-result`)

The JSON `lightsim run -o result.json` writes, and `lightsim export` and
`lightsim.read_run` read back. Schema:
[`schemas/lightsim-result.schema.json`](schemas/lightsim-result.schema.json).

```json
{
  "format": "lightsim-result", "formatVersion": 1,
  "project": "Battery Electric Car", "caseId": "case-city", "caseName": "City Cycle",
  "status": "success", "valid": true,
  "kpis": {"distance_km": 7.292, "consumption_kwh_per_100km": 11.12, "…": 0},
  "units": {"distance_km": "km", "…": ""},
  "notValid": {},
  "summary": [{"key": "distance_km", "label": "Distance driven", "value": 7.292, "unit": "km"}],
  "messages": [{"level": "info", "text": "…"}],
  "time": [0.0, 1.0, "…"],
  "channels": {"el-battery:sig_soc": {"label": "HV Battery Pack · SOC", "unit": "%",
                                      "values": [90.0, 89.99, "…"]}}
}
```

`valid` is true when the status is *success* and no figure is marked not
valid. `lightsim run --json` prints the same without `time` and `channels`.
