# Project file

A LightSim project is one JSON file: the parts on the diagram, the wires
and signal links between them, the cases to run and the parameter studies
run so far. The app keeps your projects in its projects folder (see
*Where your work is saved* in the README), one `<project id>.json` each;
the examples are in `backend/projects/` of the repository. The JSON Schema
is [`schemas/project.schema.json`](schemas/project.schema.json).

```json
{
  "id": "bev-car",
  "name": "Battery Electric Car",
  "schemaVersion": 1,
  "systems": [{ "id": "sys-root", "name": "Battery Electric Car", "parentId": null,
                "elements": [ … ], "connections": [ … ] }],
  "dataBusConnections": [ … ],
  "cases": [ … ],
  "studies": [ … ]
}
```

A reader must ignore fields it does not know: the app stores a few of its
own (and future versions add more), and keeps them when it saves.

## Project

| Field | Type | Meaning |
|---|---|---|
| `id` | text, required | The project's id. In the projects folder it is also the file name. Letters, digits, `-`, `_` and `.`, at most 128 characters |
| `name` | text, required | The name shown in the app |
| `schemaVersion` | whole number, default 1 | The version of this format the file was written in |
| `description` | text or null | A short summary, shown in the Open menu (the examples have one) |
| `systems` | list of [System](#system), required | The diagram: the top system (its `parentId` is null) and any sub-systems |
| `dataBusConnections` | list of [Signal link](#signal-link) | The signal links (the *Data Bus* panel) |
| `cases` | list of [Case](#case) | The cases to run |
| `studies` | list of [Study](#study) | Parameter studies run on the project, oldest first |
| `noAi` | true or false, default false | true hides the project from AI tools, whatever folders they may see (see AI access in the [command-line reference](../help/reference/command-line.md#ai-access)) |

## System

The diagram is a tree of systems. The top system has `parentId` null; a
part with `isSubSystem` opens the system named by its `subSystemId`.

| Field | Type | Meaning |
|---|---|---|
| `id` | text, required | The system's id |
| `name` | text, required | Its name |
| `parentId` | text or null | The system it sits in; null for the top one |
| `elements` | list of [Part](#part) | The parts in it |
| `connections` | list of [Wire](#wire) | The physical wires between its parts |

## Part

A part (`ElementInstance`) is one block on the diagram. Its type,
`componentDefId`, names a part of the library (`GET /api/library`, or
`lightsim parts`), which defines its ports, its parameters with their units
and limits, and their defaults. The file stores only the values that differ
from the library's defaults, in `parameterOverrides`.

| Field | Type | Meaning |
|---|---|---|
| `id` | text, required | The part's id, unique in the project. Results name channels and summary figures after it |
| `componentDefId` | text, required | Its type in the library, e.g. `motor.emotor` |
| `label` | text, required | The name shown on the diagram |
| `position` | `{x, y}`, required | Where it sits on the diagram, in diagram units |
| `parameterOverrides` | object | Parameter key → value, for the values that differ from the library's default. A number is in the parameter's unit; see [Values](#values) |
| `dynamicPorts` | list of [Port](#port) | Signal ports added to this part (Script and Monitor blocks) |
| `portSides` | object | Port id → `left`, `right`, `top` or `bottom`: where a pin is drawn |
| `portOffsets` | object | Port id → 0 to 1: where along its side a pin is drawn |
| `tableOutside` | object | Table parameter key → one of `error`, `clamp`, `linear` per axis: what the run does outside the table's data, overriding the library's setting |
| `size` | `{width, height}` or null | Its size on the diagram; null for the default |
| `isSubSystem` | true or false | true when the part opens a sub-system |
| `subSystemId` | text or null | The system it opens |

### Values

- **number**: a JSON number in the parameter's unit (`"unit"` in the
  library; `-` for none). Limits (`minimum`, `exclusiveMinimum`,
  `maximum`) are in the library too.
- **boolean**, **enum** (one of the library's `options`), **string**:
  as JSON.
- **code** (a Script block's `code`): Python source text. The function
  `step(t, dt, inputs, state, params)` is called every solver step; the
  [Script API](../help/reference/script-api.md) describes it.
- **table1d**: an object keyed by the axis value as text:
  `{"0": 300, "4000": 300, "12000": 95}`. The library's `axes` gives the
  axis's name and unit; the values are in the parameter's unit.
- **table2d**: the same, one level deeper: outer axis value → inner axis
  value → value, e.g. a motor's full-load torque by voltage, then speed.

A Driving Task's `cycle` names a bundled drive cycle by id (`wltc-3b`,
`udds`, `hwfet`; `GET /api/cycles`), which it drives instead of its typed
`profile`. A `profile` is text: `time:speed` pairs in s and km/h,
separated by `;`.

## Port

The ports of a part come from the library. Script and Monitor blocks add
their own in `dynamicPorts`, with the same fields.

| Field | Type | Meaning |
|---|---|---|
| `id` | text, required | The port's id on its part |
| `name` | text, required | The name shown on the diagram |
| `direction` | `input`, `output` or `bidirectional`, required | Which way it carries a signal (physical ports are `bidirectional`) |
| `kind` | `power`, `signal`, `mechanical`, `electrical`, `thermal` or `fluid`, required | What it carries; only ports of one kind connect |
| `unitGroup` | text or null | The quantity of a signal (`Power`, `Velocity` …); the library's `unitGroups` maps it to a unit |
| `side` | `left`, `right` or null | Where the pin is drawn by default |
| `polarity` | `positive`, `negative` or null | An electrical terminal's polarity |

## Wire

A wire (`Connection`) joins two physical ports of parts in one system.
Which end is the source does not matter to the solver.

| Field | Type | Meaning |
|---|---|---|
| `id` | text, required | The wire's id |
| `sourceElementId`, `sourcePortId` | text, required | One end: a part's id and its port's id |
| `targetElementId`, `targetPortId` | text, required | The other end |

## Signal link

A signal link (`DataBusConnection`) takes a signal output to a signal
input, across systems. The app writes the output as end 1; readers should
accept either order.

| Field | Type | Meaning |
|---|---|---|
| `id` | text, required | The link's id |
| `element1Id`, `port1Id` | text, required | One end |
| `element2Id`, `port2Id` | text, required | The other end |

## Case

A case is one run setup: how long, what the Driver does, and its own
values for any parameter.

| Field | Type | Meaning |
|---|---|---|
| `id` | text, required | The case's id |
| `name` | text, required | The name shown in the app |
| `kind` | `cycle`, `performance`, `acceleration` or `lap`, default `cycle` | `cycle`: follow the Driving Task's speed. `performance`: full throttle until the target speed, then hold it (0-100 km/h, top speed). `acceleration`: full throttle over `endDistance` from the start line. `lap`: drive the model's Race Track |
| `duration` | number, s, default 600 | How long the run lasts (an acceleration test's time limit). Not used by lap cases |
| `timeStep` | number, s, default 1 | How often results are stored. The solver itself steps at most 10 ms |
| `outputEvery` | whole number, default 1 | Store every Nth of those points |
| `realtimeFactor` | number, default 0 | 0: run as fast as possible; N: pace the run at N times real time |
| `endDistance` | number, m, or null | Stop when the car has driven this far past the start line; null or 0: run the duration |
| `startLine` | number, m, default 0 | Distance driven before the timer starts (Formula Student: 0.30 m) |
| `referenceTime` | number, s, or null | A time to compare an acceleration test with |
| `parameterOverrides` | object | Part id → parameter key → value: this case's own values, layered over the parts' own. A Race Track's layout and laps are set here |

## Study

A study is a parameter sweep the app ran: the values swept on one case,
and a results table with a row per point. Studies stay with the project
after their runs leave the run history. Its schema is
[`schemas/study.schema.json`](schemas/study.schema.json).

| Field | Type | Meaning |
|---|---|---|
| `id` | text, required | The study's id |
| `startedAt` | whole number, ms since 1970, required | When it started |
| `caseId` | text, required | The case it ran |
| `caseName` | text | The case's name then |
| `factors` | list of factor, required | What was swept: `elementId`, `paramKey`, the labels then (`elementLabel`, `paramLabel`), `unit` and the `values` |
| `kpis` | list of `{label, unit}` | The table's columns: the runs' summary rows |
| `points` | list of point | A row per point: `values` (one per factor), `runId` (the run may since have been deleted), `status` (`success`, `warning`, `failed`, `cancelled` or `not run`), `incomplete` (why the run did not finish), `kpis` (column label → value) and `notValid` (column label → why) |

In version 1 a study's columns are keyed by the summary rows' **labels**,
not their [keys](results.md#summary-keys). A later version will key them by
key; readers should take a column's unit from `kpis`.
