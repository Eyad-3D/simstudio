# Write a part as equations in a text file

Each part of your car is a set of equations. The new engine can read a
part's equations from a text file instead of its built-in ones: a *text
component*, in the engine's text format (`engine/docs/text-format.md`, a
small Modelica-like language). Use one to try a change to a part's physics
without changing LightSim.

There is no text editor in the app: you write the file in any editor and
name it in the project file.

## Start from the part's own text

The engine prints the equations it builds a part from. With the engine's
Python module (`lightsim_engine`):

```python
import lightsim_engine as le
project = open("my-car.json", encoding="utf-8").read()
text = le.part_text(project, "case-wltc", "el-battery")  # case id, part id
open("battery.mo", "w", encoding="utf-8").write(text)
```

That file gives exactly the built-in part's result. Change its equations
from there.

## Name the file in the project

In the project file, give the part a `textFile`: the file's path, absolute
or from your projects folder.

```json
{"id": "el-battery", "componentDefId": "battery.generic", "label": "HV Battery Pack",
 "textFile": "parts/battery.mo", "parameterOverrides": {"capacity_kWh": 60}}
```

The part keeps its values, pins and links; only its equations come from
the file. The file holds one component with the part's own ports (the same
names and kinds) and every parameter its values set.

## How it runs

Today's engine reads no text components, so a case with one runs on the
new engine whatever `LIGHTSIM_ENGINE` says. Its first message says so.

A case the new engine does not run fails instead of running the built-in
part in the file's place: its message gives the reason's code and
`NEW-TEXT` (for example `Not run [NEW-LAP] [NEW-TEXT]` for a lap case). A
file that cannot be read fails the run with `NEW-TEXT`; a text that does
not fit the part fails with the engine's code:

| Code | Why |
|---|---|
| `TEXT-PARSE` | the text does not read as one component (the parser's words follow) |
| `TEXT-PORTS` | its ports are not the part's own, of the same kinds |
| `TEXT-PARAMS` | it declares no parameter that the part's values set |
| `TEXT-NO-PART` | the id names no part of the case's model (a sub-system's container has no equations of its own) |
| `TEXT-SAMPLED` | a Script, FMU or Traction Control part, whose code runs outside the equations |

[Known issues](../../KNOWN-LIMITS.md) lists what text components do not do.
