# Tables and the Map block

This page is the contract for table parameters with any number of axes
and for the Map block: how a table is defined in the part library, how its
data are stored in a project file, and how the engine reads and runs them.
The app (definition, storage, Data Checks, editor, parameter sheet) and the
engine are both built to it. There is no limit on the number of axes in this
contract: one, two, three and more are the same model, and code tells them
apart by the number of axes only.

## A table parameter's definition

A table is a parameter of a part in the library
(`backend/app/library/components.json`, `GET /api/library`). Its
definition has these fields besides the ones every parameter has (`key`,
`label`, `default`, help texts):

| Field | Meaning |
|---|---|
| `type` | `table1d`, `table2d` or `table`. All three are the same model. `table1d` (exactly one axis) and `table2d` (exactly two) are the names the library and project files have used so far and stay valid for good; a table with any number of axes is `table`. Code reads the number of axes from `axes`, never from the type name |
| `axes` | A list of one or more axes, in order. Each has a `name`, the `unit` its breakpoints are stored in (`-` for none) and `outside`: what a run does past the axis's data (`error`, `clamp` or `linear`; absent for a table that is not interpolated, such as gear ratios, which behaves as `clamp`) |
| `unit` | The unit the table's values are stored in |

An axis's *breakpoints* are the numbers along it where the table has a
value.

## The stored form

A table's value (its `default` in the library, a part's
`parameterOverrides[key]` in a project) is a nested JSON object, one level
per axis, in the order of `axes`:

- At each level the keys are the breakpoints of that level's axis, as
  text in the axis's stored unit: any text a JSON number reader reads
  (`"1500"`, `"0.25"`, `"-40"`, `"1e-7"`). The app writes the shortest
  text JavaScript gives for the number. Keys may be in any order; two keys
  of one object that read as the same number are an error.
- Below the last axis are the values: finite JSON numbers in the
  parameter's `unit`.
- **A table with three or more axes is a full grid**: every object at one
  level has the same breakpoints (compared as numbers), so every point of
  the grid has a value.
- A table with one or two axes keeps the rules it has had: a two-axis
  table may leave a point out (an object of the second axis may have
  breakpoints the others do not), and each first-axis breakpoint's object
  is then read as a one-axis table of its own.
- A table has at least one breakpoint on every axis. An axis with one
  breakpoint is constant along that axis: it is never outside its data.

### The order the engine reads

The engine sorts each axis's breakpoints increasing and reads the values
as one row-major array with the first axis outermost: with `n_k`
breakpoints on axis `k`, the value at breakpoint indexes
`(i_0, i_1, …, i_(N-1))` is element

    ((i_0 · n_1 + i_1) · n_2 + i_2) · … · n_(N-1) + i_(N-1)

so the last axis changes fastest. This is how it reads every number of axes
(`engine/crates/lsim-ir/src/table.rs`, `TableData`).

### Reading a table between and past its points

A table is linear between its breakpoints along each axis (multilinear: a
one-axis table is a broken line, a two-axis table is bilinear, and so on),
as the app's one- and two-axis tables are. Past an axis's data it
follows that axis's outside rule: `error` stops the run with a message
naming the table and the axis (by its place and, for a Map block's input,
its name: `… on its first (Speed) axis …`), `clamp` holds the edge value,
`linear` continues the edge segment's slope. The time spent past each
axis's data is reported per axis. Tables the engine's own parts define
for themselves may interpolate with monotone cubics instead (C¹, no
overshoot), for any number of axes too; every table of the library and
of a Map block is linear.

## The per-axis lists

A part keeps two per-axis settings for each of its table parameters, both
lists with one entry per axis in the order of `axes`:

| Field of the part | Meaning |
|---|---|
| `tableOutside[key]` | The outside rule of each axis (`error`, `clamp`, `linear`), overriding the library's `outside`. Absent: the library's. A list whose length is not the number of axes is a Data Check error and the run ignores it |
| `displayUnits[key].axes` | The unit each axis is shown and typed in (null: the stored unit); `displayUnits[key].value` the values'. Display only: every number in the file stays in its stored unit. A unit the app does not know, or of another kind, is ignored |

## Examples

### One axis

Library:

```json
{"key": "drag_torque", "label": "Drag Torque", "unit": "N·m", "type": "table1d",
 "axes": [{"name": "Speed", "unit": "1/min", "outside": "clamp"}],
 "default": {"0": 0.5, "6000": 1.2, "12000": 2.0}}
```

The engine reads breakpoints `[0, 6000, 12000]` 1/min and values
`[0.5, 1.2, 2.0]` N·m. `tableOutside: {"drag_torque": ["linear"]}` makes
it continue past 12000 1/min on the slope of its last segment.

### Two axes

Library:

```json
{"key": "power_loss", "label": "Power Loss", "unit": "kW", "type": "table2d",
 "axes": [{"name": "Speed", "unit": "1/min", "outside": "error"},
          {"name": "Torque", "unit": "N·m", "outside": "clamp"}],
 "default": {"0":    {"0": 0.1, "200": 0.9},
             "6000": {"0": 0.4, "200": 3.2}}}
```

Breakpoints `[0, 6000]` and `[0, 200]`; values row-major
`[0.1, 0.9, 0.4, 3.2]`: (0 1/min, 0 N·m), (0, 200), (6000, 0),
(6000, 200). The editor shows Speed along the columns and Torque down the
rows. A part that shows Speed in rad/s keeps
`displayUnits: {"power_loss": {"axes": ["rad/s"]}}`; the file's keys stay
in 1/min.

### Three axes

Library (no part of today's library has three axes yet; a table of type
`table` takes any number):

```json
{"key": "cell_resistance", "label": "Cell Resistance", "unit": "mΩ", "type": "table",
 "axes": [{"name": "Temperature", "unit": "°C", "outside": "clamp"},
          {"name": "SOC", "unit": "%", "outside": "clamp"},
          {"name": "Current", "unit": "A", "outside": "linear"}],
 "default": {"0":  {"20": {"-100": 3.1, "100": 2.9}, "80": {"-100": 2.6, "100": 2.4}},
             "25": {"20": {"-100": 1.9, "100": 1.8}, "80": {"-100": 1.5, "100": 1.4}}}}
```

Breakpoints `[0, 25]`, `[20, 80]`, `[-100, 100]`; values row-major
`[3.1, 2.9, 2.6, 2.4, 1.9, 1.8, 1.5, 1.4]`, the value at
(25 °C, 20 %, 100 A) being element `(1·2 + 0)·2 + 1 = 5`, 1.8 mΩ.
`tableOutside: {"cell_resistance": ["clamp", "error", "linear"]}` and
`displayUnits: {"cell_resistance": {"axes": ["K", null, null]}}` (the
temperature shown in kelvin) are lists of three. Every SOC object has the breakpoints 20 and 80 and every
Current object -100 and 100: a file in which one of them lacks a point is
a Data Check error.

## The Map block

The Map block (library id `signal.map`, name *Map*, category *Driver &
Signals*) reads one or more output signals from tables over one or more
input signals. Its entry is in `components` of
`backend/app/library/components.json`, which the engine embeds (a part the
app has and the engine does not run yet would wait in the list
`awaitingEngine` beside it, which the app's library, `GET /api/library`,
also holds and the engine does not read; it is empty). The user names its
inputs and outputs, gives each a unit
and fills in the tables; its pins are wired like any signal block's. The
older *Lookup Table* block (`signal.lookup`, one or two inputs, one
output) stays as it is for the projects that use it.

### Ports

The Map block's library entry has no ports of its own
(`allowDynamicPorts: true`, `ports: []`): every port is the part's own, in
`dynamicPorts`, as for the Script and FMU blocks.

- **Inputs** are its `dynamicPorts` with `direction: "input"`. They are
  the axes of its tables, in the order they appear in `dynamicPorts`.
- **Outputs** are its `dynamicPorts` with `direction: "output"`, one
  table each.
- Every port has `kind: "signal"`. Its `name` is what the user typed
  (shown on the diagram and as the axis's or table's name). Its `id` is
  made once, when the port is added, from its first name (`Input 1` →
  `input_1`), unique on the part, and never changes: a rename changes the
  `name` only, so the port's wires, its table under `maps` and its display
  unit stay where they are. Nothing refers to a Map port by its name.
- A port's `unitGroup` is its quantity: the library's `unitGroups` maps it
  to the unit the numbers are stored in (`Rotational Speed` → `1/min`,
  `Power` → `kW`; `No Unit`, or none, → `-`). An input's breakpoints are
  stored in its group's unit and an output's values in its. The signal
  wired in is read in that unit and the outputs are given in theirs:
  nothing is converted on the wire, as for every signal block (Data Checks
  warn when a wire joins two different units).
- The user picks a **unit**, not a quantity: the editor lists every unit
  the app knows (`backend/app/dataio/units.json`), grouped by quantity.
  Picking a unit of one of the library's unit groups sets the port's
  `unitGroup` to that group and, when the unit is not the one the group
  stores, the port's display unit (`displayUnits.maps.axes[i]` for input
  `i`, `displayUnits.maps.outputs[id]` for an output), so a port given
  rad/s shows and takes rad/s at once while its numbers stay in 1/min.
- `unitText` (optional text) is a port's unit when the app has no unit
  group for it: a unit the app knows of another quantity (kWh, bar) or one
  the user types. The port's `unitGroup` is then `No Unit`. The text is
  shown with the port's numbers (the grid's headers and units line, the
  plot) and on its pin's tooltip, and is never converted. It means nothing
  on a port of another unit group.

### Parameters

| Key | Type | Meaning |
|---|---|---|
| `maps` | `maps` | The outputs' tables: an object keyed by output port id, each value a table in the [stored form](#the-stored-form) with one axis per input, in input order. Default `{}`. Its variability is `fixed`: an edit during a run takes effect on the next run |

The tables of one Map block share **one grid**: every output's table has
the same breakpoints at every level, and each is a full grid, whatever the
number of inputs. A block with outputs and no inputs holds a number per
output (a constant) in `maps`; a block with no outputs holds `{}`.

The per-axis lists of [the per-axis lists](#the-per-axis-lists) are kept
under the key `maps`, one entry per input:

- `tableOutside.maps`: each input's outside rule. Absent, or an absent
  entry: `clamp`.
- `displayUnits.maps.axes`: the unit each input is shown in, and
  `displayUnits.maps.outputs`: an object of output port id → the unit
  that output's values are shown in. Display only, as for every table.

### Example: two inputs, two outputs

```json
{
  "id": "el-map", "componentDefId": "signal.map", "label": "Motor Map",
  "position": {"x": 400, "y": 200},
  "dynamicPorts": [
    {"id": "speed", "name": "Speed", "direction": "input", "kind": "signal", "unitGroup": "Rotational Speed"},
    {"id": "torque", "name": "Torque", "direction": "input", "kind": "signal", "unitGroup": "Torque"},
    {"id": "loss", "name": "Loss", "direction": "output", "kind": "signal", "unitGroup": "Power"},
    {"id": "efficiency", "name": "Efficiency", "direction": "output", "kind": "signal", "unitGroup": "Percent"}
  ],
  "parameterOverrides": {
    "maps": {
      "loss":       {"0": {"0": 0.1, "200": 0.9}, "6000": {"0": 0.4, "200": 3.2}},
      "efficiency": {"0": {"0": 0,   "200": 80},  "6000": {"0": 0,   "200": 94}}
    }
  },
  "tableOutside": {"maps": ["error", "clamp"]},
  "displayUnits": {"maps": {"axes": ["rad/s", null], "outputs": {"loss": "W"}}}
}
```

Its tables have the axes Speed (1/min, stops the run past 0–6000) and
Torque (N·m, held at 0 and 200). At Speed 3000 1/min and Torque 100 N·m
its Loss pin gives 1.15 kW and its Efficiency pin 43.5 %. The engine reads
each output's values row-major as for any table: Loss
`[0.1, 0.9, 0.4, 3.2]`, Efficiency `[0, 80, 0, 94]`.

### What editing does

These are the app's rules, so that a file it writes keeps the shared grid:

- **Adding an input** adds an axis at the end with one breakpoint, `"0"`:
  every value `v` of every output becomes `{"0": v}`, so the existing
  values become that slice.
- **Removing an input** keeps the slice at one breakpoint of it, which the
  user picks: every output's table is replaced by its sub-table at that
  breakpoint, and that input's entries leave `tableOutside.maps` and
  `displayUnits.maps.axes`.
- **Adding an output** adds a table of the shared grid filled with 0.
  **Removing an output** removes its table; the last output of a block
  with inputs cannot be removed, since the grid lives in the tables.
- **Renaming** a port changes its `name` only (its id stays): its wires,
  table and display unit are untouched.
- **Changing a port's unit to another quantity** (another unit group, or
  a unit text) keeps its numbers (they are read in the new unit) and drops
  its display unit. **Picking another unit of the same quantity** changes
  only its display unit: the numbers stay as stored and are shown
  converted, as the unit chooser's *Show in another unit* does.
- **Adding a breakpoint** on any axis adds it to every output; the new
  points are filled by copying the nearest breakpoint's values (for a
  library table of three or more axes the same, on its one table).
  **Removing a breakpoint** removes its slice from every output. The last
  breakpoint of an axis cannot be removed.

## Data Checks

Data Checks report each of these as an error naming the part, the table
and, where there is one, the axis and breakpoint:

- a table that is not nested one level per axis, an empty object, a key
  that is not a number, two keys of one object that are the same number,
  or a value that is not a finite number;
- a table of three or more axes that is not a full grid;
- a `tableOutside` list whose length is not the number of axes (as today);
- a Map block with no input or no output, a port of a Map block whose kind
  is not `signal` or whose direction is `bidirectional`, a `maps` whose
  keys are not exactly the output port ids, an output table with another
  number of levels than the block has inputs, or output tables whose
  breakpoints differ (not one shared full grid).

## The parameter sheet

The parameter sheet (*Export sheet* and *Import sheet*, `.xlsx`) gives a
table of one or two axes a sheet laid out as today (a one-axis table in
two columns; a two-axis map with the first axis along a row and the
second down the first column). A table of three or more axes, and a Map
block's `maps` at any number of inputs, gets a sheet in **long form**: a
title row, a header row with one column per axis and then one column per
value, each header `Name [unit]` in the stored unit, and then one row per
point of the grid, the first axis outermost and the last changing
fastest. A Map block's value columns are its outputs, in port order.

```
Motor Map · Maps
Speed [1/min]   Torque [N·m]   Loss [kW]   Efficiency [%]
0               0              0.1         0
0               200            0.9         80
6000            0              0.4         0
6000            200            3.2         94
```

On import the columns are read by their place (axes first, in order, then
values); a header's unit may be another unit of the same kind, which is
converted; the rows may be in any order, but they must make a full grid,
each point once. A missing point, a point twice, a cell that is not a
number or a unit of another kind refuses the sheet with the row it is in.
The CSV export keeps a table as JSON text in its Value cell, at any depth.

## How the engine runs them

A run takes a Map block and tables of any number of axes as it takes
every other part, in a full run and in quick mode; nothing is refused for
their number of axes.

1. **Tables of any number of axes** (`engine/crates/lsim-ir`:
   `TableData`, `interp::Table`): breakpoints per axis, values row-major
   (first axis outermost), an outside rule, a unit and a name per axis;
   multilinear between points, each axis's outside rule past them, an axis
   with one breakpoint constant; the stop of an `error` axis and the
   report of time spent past each axis's data, per axis. A table that
   varies along one or two axes is computed as it always has been (the
   same numbers to the last digit); three or more are computed cell by
   cell from the grid's points.
2. **A library table** of type `table`, `table1d` or `table2d` is read
   from the stored form above by its number of axes
   (`lsim_lib::table::stored_data`): one and two axes by their own rules,
   three or more as a full grid.
3. **The Map block**, `signal.map` (`lsim_lib::blocks::signals::map`,
   mapped in `lsim-project`'s importer): its inputs and outputs from
   `dynamicPorts` (inputs in order are the axes), each port's unit its
   unit group's (`unitGroups`), one table per output from `maps` (a
   parameter named after the output: `loss_map`), the per-input
   `tableOutside.maps`; each output is its map read at the inputs' values
   at every step (a smooth signal function, no state, no sample time). A
   block with outputs and no inputs gives each output its number.

## Where this differs from the decisions it was built from

- A table of two axes is not held to a full grid: today's two-axis maps
  may leave a point out and every existing project must open and save
  unchanged, so the full grid starts at three axes. A Map block's tables
  are full grids at any number of inputs.
- A Map port's unit is its `unitGroup`, the unit every signal port
  already has, rather than a free unit: a signal carries its group's unit
  on the wire, so the Map reads and gives numbers in the units its
  neighbours use, and any other unit of the same kind is a display unit.
- The display units of a Map block's outputs need a place of their own,
  `displayUnits.maps.outputs`, since one block has several value units.
- A port's unit that has no unit group is kept as `unitText` on the port;
  only the Map block's editor, its plot and the pin's tooltip show it.
