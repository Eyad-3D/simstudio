# Python API

The `lightsim` Python package runs LightSim models from a Python script or
a Jupyter notebook, without opening the app. It loads a project, checks
it, runs a case and gives you the results; it can also change a project
and save it, so you can try variants in a loop. Everything runs inside
your Python process: no server, no window, nothing on the network.

The [command-line tool](command-line.md) does the same from a terminal.
The files it reads and writes are described in the
[file format specification](../../spec/README.md).

## Get it

The package is the `backend/` folder of the LightSim repository, with
Python 3.11 or newer:

```text
cd backend
pip install -r requirements.txt
python -c "import lightsim; print(lightsim.__version__)"
```

Run your scripts from `backend/`, or put `backend/` on `PYTHONPATH`. Or
build a wheel and install it in any Python 3.11 environment:
`python scripts/build-wheel.py`, then `pip install
backend/dist-wheel/lightsim-*.whl`, which also gives you the `lightsim`
command. LightSim is not on PyPI yet (see
[Known issues](../../KNOWN-LIMITS.md)). `pandas` is optional: it gives
you the results as a table (`Result.df`).

## The calls

| Call | Gives |
|---|---|
| `ls.run(project, case=None)` | Runs one case and returns a `Result`. `project` is a file path, an example id (`"bev-car"`) or a `Project`; `case` is a case's name or id (the first case if left out) |
| `ls.check(project)` | The Data Checks (the *Problems* list): a list of `Check` with `level`, `text`, `fix`, `element_ids` and `case_id` (the case an error stops alone; None: every run) |
| `ls.load(project)` | A `Project` to read, change, check, run and save |
| `ls.read_run(path)` | A `Result` from a run the app stored (`.json.gz`) or one you saved as JSON |
| `ls.examples()` | The example ids |
| `ls.library()` | Every part type, with its parameters (key, unit, default) and ports |
| `ls.schemas()` | The JSON Schemas of the file formats |

A `Result` has:

| Attribute | Holds |
|---|---|
| `status` | `success`, `warning`, `failed` or `cancelled` |
| `ok`, `valid` | `ok`: the run is a success. `valid`: and no figure is marked not valid |
| `kpis` | The summary figures by [stable key](../../spec/results.md#summary-keys): `{"distance_km": 7.292, …}` |
| `units`, `not_valid` | Each figure's unit, and why a figure is not valid |
| `summary` | The full rows: `key`, `label`, `value`, `unit`, `not_valid`, `limit`, `passed` |
| `time`, `channels` | The times (s) and the recorded signals by `"elementId:portId"`; `channel(ref)` finds one by key or label |
| `messages`, `checks` | What the run reported, and the Data Checks when they stopped it |
| `to_csv(path)`, `to_mat(path)`, `to_json(path)` | Write the results as CSV, a MATLAB MAT-file or JSON |
| `to_parquet(path)` | Write the results as an Apache Parquet file: a `time` column and one per channel, units in each column's metadata. Needs `pip install pyarrow`, which LightSim does not include |
| `df` | The channels as a pandas table, with the units in `df.attrs["units"]` |

A `Project` has `get`, `set`, `unit`, `add`, `remove`, `connect`,
`route`, `disconnect`, `add_case`, `check`, `run`, `save` and `copy`,
shown below. Parts are named by their label (`"E-Motor"`) or id, and
parameters by their key after a dot: `"Vehicle.mass_kg"`. Find the keys
with `lightsim params <project>` or `lightsim parts <type>`.

Errors raise `ls.LightSimError` with a message that says what exists:
`No case 'Nope' in 'Battery Electric Car'. Cases: 'City Cycle' …`.

## Examples

Each example runs as it stands from `backend/`; LightSim's tests run them
all.

### 1. Run a case

```python
import lightsim as ls

r = ls.run("bev-car", case="City Cycle")
print(r.status, r.kpis["consumption_kwh_per_100km"], r.units["consumption_kwh_per_100km"])
```

### 2. Print the summary as the app shows it

```python
import lightsim as ls

r = ls.run("bev-car", case="City Cycle")
for k in r.summary:
    flag = f"  (not valid: {k.not_valid})" if k.not_valid else ""
    print(f"{k.label}: {k.value} {k.unit}{flag}")
```

### 3. Check a project before running it

```python
import lightsim as ls

for c in ls.check("hybrid-car"):
    print(c.level, c.text, c.fix or "")
```

### 4. Save the results for a spreadsheet and for MATLAB

```python
import lightsim as ls

r = ls.run("bev-car", case="City Cycle")
r.to_csv("city.csv")    # Time [s], then one column per channel: "label [unit]"
r.to_mat("city.mat")    # load('city.mat') in MATLAB or Octave
r.to_json("city.json")  # read back with ls.read_run("city.json")
```

### 5. Read one channel

```python
import lightsim as ls

r = ls.run("bev-car", case="City Cycle")
soc = r.channel("HV Battery Pack · SOC")
print(soc.unit, soc.values[0], soc.values[-1], "at", r.time[-1], "s")
```

### 6. Get the results as a pandas table

```python
import lightsim as ls

r = ls.run("bev-car", case="City Cycle")
df = r.df  # needs pandas
print(df["HV Battery Pack · SOC"].describe())
print(df.attrs["units"]["HV Battery Pack · SOC"])
```

### 7. Change a value, with its unit

```python
import lightsim as ls

p = ls.load("bev-car")
print(p.get("Vehicle.mass_kg"), p.unit("Vehicle.mass_kg"))
p.set("Vehicle.mass_kg", "1.9 t")     # converted to 1900 kg
try:
    p.set("Vehicle.mass_kg", "150 kW")  # refused: kW is not a mass
except ls.LightSimError as e:
    print(e)
```

A case can set a value itself (the hybrid's cases set the battery's start
charge), and a case's own value wins over the part's. `p.get(ref,
case=...)` shows the value a case uses; to change it for that case, give
the case: `p.set("HV Battery.initial_soc_pct", 70, case="EPA city (UDDS)")`.

### 8. Sweep a value in a loop

```python
import lightsim as ls

base = ls.load("bev-car")
for mass in (1600, 1800, 2000, 2200):
    r = base.copy().set("Vehicle.mass_kg", mass).run("City Cycle")
    print(mass, "kg:", r.kpis["consumption_kwh_per_100km"], "kWh/100km")
```

### 9. Give a case its own values

```python
import lightsim as ls

p = ls.load("bev-car")
case = p.add_case("EPA city, loaded", duration=1369,
                  values={"Vehicle.mass_kg": "2.3 t", "Vehicle Task.cycle": "udds"})
print(p.get("Vehicle.mass_kg", case=case), "kg in the new case,",
      p.get("Vehicle.mass_kg"), "kg in the others")
print(p.run(case).kpis["distance_km"], "km")
```

### 10. Add a part and wire it

```python
import lightsim as ls

p = ls.load("bev-car")
heater = p.add("electric.constant_drive", label="Heater", power_kW="2 kW")
ground = p.add("boundary.ground", label="Heater Ground")
p.connect("HV Bus.t4", f"{heater}.pos")
p.connect(f"{heater}.neg", f"{ground}.t1")
print([c.text for c in p.check() if c.level == "error"] or "no errors")
p.save("bev-with-heater.json")
```

### 11. Re-link a signal and remove a part

```python
import lightsim as ls

p = ls.load("bev-car")
p.remove("BMS Monitor")  # its wires and signal links go with it
motor = p.element("E-Motor").id
link = next(d.id for d in p.model.dataBusConnections
            if (d.element2Id, d.port2Id) == (motor, "sig_demand_in"))
p.disconnect(link)
print([c.text for c in p.check() if c.level == "error"])  # the motor has no command
p.route("Driver.sig_traction_cmd", "E-Motor.sig_demand_in")
print([c.text for c in p.check() if c.level == "error"] or "no errors")
```

### 12. Read a run the app stored

```python
import lightsim as ls

ls.run("bev-car", case="City Cycle").to_json("stored.json")
r = ls.read_run("stored.json")  # also takes runs/<project id>/<run id>.json.gz
print(r.case_name, r.status, len(r.channels), "channels")
```

### 13. Stop a run that takes too long

```python
import lightsim as ls

r = ls.run("bev-car", case="WLTC Class 3b", time_limit_s=60)
print(r.status)  # "cancelled" if it took longer than 60 s
```

A case paced for watching in the app, such as *City Cycle (live, 10×)*,
runs as fast as your computer allows, with the same figures as in the
app. Give `paced=True` to keep its pace.

## Scripts and AI tools

Scripts you write yourself can do anything your own account can. AI tools
(such as the LightSim MCP server for AI assistants) go through
`lightsim.ai_access.AgentSession` instead, which keeps to the rules you set
with `lightsim ai`: see [AI access](command-line.md#ai-access).
