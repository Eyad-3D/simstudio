# Work in other units

Every value of a part is kept in the unit the library gives it: km/h for
a vehicle speed, 1/min for a motor's speed, kW for a power. The project
file, the engine, Data Checks, studies, the parameter sheet and AI
assistants all use that unit. You can still see and type each value in
the unit you work in.

## Choose the unit of a value

In *Properties* or a part's parameter window, a unit drawn with a dotted
underline and a small arrow can change. Click it (or Tab to it and press
Enter). Its chooser offers two different things:

- **Show in another unit**: the same value, converted. 36 km/h shows as
  10 m/s, and what you type in the field is then read in m/s: type 20 and
  the part keeps 72 km/h. Choosing a unit changes no value; the unit
  marked *as saved* shows the value as before.
- **The number is in another unit**: for a number that was typed in the
  wrong unit. The number stays as it is on screen and the value changes:
  50 shown in km/h, said to be in mph, becomes 80.4672 km/h, shown as
  50 mph. This is one step: **Undo** (Ctrl+Z) takes it back.

**Cancel** or Esc closes the chooser and changes nothing. A unit without
the arrow, such as % or a number with no unit, has no other unit to show.

## Tables

Joined to the top of a table's grid, each axis and the values have their
unit: click one, or double-click a column's header in a one-axis table
(or select it and press Enter), or the top-left cell of a map for its
values, for the same chooser, for the whole axis or all the values. The
cells and the plot under them show, take and copy numbers in the units
shown: what you copy is what you see, and a block pasted from Excel is
read in them ([Edit a table or a map](edit-a-table.md)).

## What is kept

The units you chose are saved in the project, for each part and
parameter, and for each axis of a table, so they come back when you open
the project, copy the part, undo or redo. The values in the file stay in
the library's units. A value changed elsewhere (an AI assistant's edit,
an [imported table](import-a-table-from-a-file.md), a
[parameter sheet](edit-parameters-in-a-spreadsheet.md)) shows converted
like any other.

A value outside its limits is named in the unit shown ("must be at least
0 m/s"), as are the help card's default and typical values. Data Checks,
case values and sweeps give the library's unit.

## Which units

Speed (km/h, m/s, mph), rotational speed (1/min, that is rpm; rad/s),
torque, power (kW, W, MW, hp, PS), energy, voltage, current, charge,
mass, mass flow, distance, time, temperature (°C, K, °F), force,
pressure, curvature, resistance, area, rotational inertia, the road-load
coefficients B and C (per km/h, per m/s, lbf per mph), specific fuel
consumption, heating value and fuel density. The table import reads the
same units, from one list.
