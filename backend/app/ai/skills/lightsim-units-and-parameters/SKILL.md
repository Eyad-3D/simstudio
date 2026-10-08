---
name: lightsim-units-and-parameters
description: How LightSim parameters work - their units, tables, per-case values and limits - with the full reference of every library part and the data sources never to take values from. Use before setting any value, when converting a data-sheet number, when suggesting where a value, map or drive cycle could come from, or to find a part's parameter keys and port ids.
license: See README.md of the LightSim skill pack
metadata:
  version: "0.3.0"
  app: LightSim 0.3.0
---

# Units and parameters

## Each parameter has one unit, and LightSim does not convert

Every parameter is stored in the unit its definition lists (`element_read`
shows it): kW, kWh, kg, m, N·m, 1/min (rpm), %, km/h. Convert data-sheet
values yourself before setting them:

- power in hp: × 0.7457 → kW; torque in lbf·ft: × 1.356 → N·m;
- mass in lb: × 0.4536 → kg; speed in mph: × 1.609 → km/h;
- a battery's energy is its **usable** energy in kWh, not the gross figure;
- percentages are 0-100, not 0-1 (a 98 % efficiency is 98).

With `model_edit`, a value may be a number (in the parameter's unit) or a
text with that unit ("150 kW"); a text in another unit is refused, never
converted. Write the decimal sign as a dot ("1.5 kg"): a comma only groups
thousands ("1,500 kg"), and "1,5 kg" is refused.

## Kinds of value

- **number**: with limits (`minimum`, `exclusiveMinimum`, `maximum`); a
  value outside them is a Data Check error.
- **enum**: one of its `options` (a Wheel's `axle` is "Front" or "Rear").
- **boolean**: true or false.
- **table1d**: `{x: value}`, keys as text ("1500": 120.5).
- **table2d**: `{outer: {inner: value}}`, e.g. an E-Motor's
  `full_load_torque` by voltage, then speed. Each axis has an *outside
  the data* setting: Error stops the run, Clamp holds the edge, Linear
  extends it.
- **code**: a Script block's Python.

## Where a value comes from

A part uses its library default unless the part sets its own value. A
case can set a value for that case only (`model_edit` `set` with
`case`), which is how a study varies one value without editing the model.
`element_read` with `case` shows which source each value has.

*Fixed* parameters are built into the model when a run starts; *tunable*
ones can also change during a live run in the app.

## Where values may come from

Suggest values from the user's own data sheets and measurements first, and
say where any other value you suggest comes from. Never suggest, fetch or
copy data from a source LightSim bans because its terms do not allow
reuse (the UNECE website, ev-database.org, share-alike or GPL datasets,
standards sold by their publisher such as China's CLTC, and others):
`references/data-sources.md` lists every one, with why and what to use
instead. It is generated from the same list LightSim's data register is
checked against. A user may import such data into their own project
themselves; do not fetch it for them.

## The full reference

`references/components.md` lists every library part: its id (the `type`
for `add`), its ports and its parameters with units, defaults and limits.
It is generated from the app's library, so it matches this version. Read
only the part you need (over MCP: `lightsim://components/<type>`).
