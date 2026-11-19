# Edit parameters in a spreadsheet

Send every parameter of a model to one Excel workbook, let your team check
or fill it in, and read it back. LightSim lists every change before it
applies one.

## Export the sheet

1. Open the *Parameters* tab of the ribbon.
2. Click **Export sheet**. LightSim saves `<project> - parameters.xlsx`.

The *Parameters* sheet has a row per parameter of every part: *Part*,
*Part ID*, *Part type*, *Parameter*, *Key*, *Value*, *Unit*, the library's
*Default*, *Changed* (yes where the value is not the default), and two
columns for your team, *Source* (where the number comes from) and *Notes*.
Each table, map and drive profile has a sheet of its own, named after its
part and parameter, laid out as the table import reads it
([Import a table from a file](import-a-table-from-a-file.md)). Scripts are
not in the sheet.

**Export CSV** saves the *Parameters* sheet alone as a CSV file; tables
are then text in their *Value* cell. Use the `.xlsx` file to edit tables.

For a new Formula Student car, **FS template** saves the sheet of the
*FS Electric (generic)* example to fill in with your car's numbers. Import
it into a copy of that example (Start page → *FS Electric (generic)*).

## Change values

Change numbers in the *Value* column, or on a table's sheet. Keep *Part
ID*, *Key* and *Unit* as they are: LightSim finds each row by its part id
and key, and refuses a row whose unit is not the parameter's (write the
value in the unit the row names). On/off values are `TRUE` or `FALSE`;
choices must be written as one of the options LightSim offers.

## Import it back

1. On the *Parameters* tab, click **Import sheet** and choose the file.
2. The list shows each change: the row, the part and parameter, the value
   now and the value from the sheet.
3. Click **Apply**. All changes are one step: **Undo** (Ctrl+Z) takes
   them all back.

If a row does not fit the model (a part or key the model does not have, a
wrong unit, text where a number belongs), the list names its row and
nothing is applied until you fix the file. A value outside a parameter's
limits is applied, and Data Checks report it.

*Source* and *Notes* are not read back, and LightSim does not record when
each value last changed.

## From the command line

```text
lightsim-backend params export car.json --out car-parameters.xlsx
lightsim-backend params import car.json car-parameters.xlsx --out car-new.json
```

The import prints the changes and, with `--out`, writes the changed model
to a new project file.
