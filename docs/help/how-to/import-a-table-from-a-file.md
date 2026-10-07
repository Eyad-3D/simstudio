# Import a table from a file

A motor map, a battery table or a speed trace often comes as a spreadsheet.
Every table, map and profile grid in LightSim reads one from a CSV file or
an Excel workbook (`.xlsx`). Pasting cells from Excel with Ctrl+V works too.

1. Select the part and open its table: double-click the part on the
   diagram, or click the table's **Edit…** in *Properties*.
2. Click **Import from file…** under the table and choose the file.
3. Check the preview: the curve (or the map's first rows and columns), how
   many points, and the unit each column was read in.
4. Click **Apply**. The import replaces the whole table; **Undo**
   (Ctrl+Z) brings the old one back.

## How LightSim finds the data

- **CSV files** may separate columns with commas, semicolons, tabs or `|`;
  with semicolons or tabs, a decimal comma (`3,7`) is read as 3.7.
- **Workbooks**: LightSim reads the first sheet with numbers in it. Pick
  another under **Sheet**. Old `.xls` files must be saved as `.xlsx` first.
- **A table or profile** takes two columns of numbers: LightSim picks them
  by their headers (`SOC`, `time`, `speed`), and you can pick others under
  *… from*. Rows after the first empty row are left out.
- **A map** needs the column values (for example speeds) along a row, the
  row values (for example torques) down the column to their left, the map
  in between, and the cell where they meet empty or labelled, such as
  `Torque [Nm] \ Speed [rpm]` (rows \ columns). If the labels say the
  file's columns are LightSim's rows, LightSim swaps them; tick or untick
  **Swap rows and columns** to change it.
- **Cells**: type a range such as `B3:F20` to read only those cells, for
  a sheet that holds several tables.

## Units

LightSim reads the unit from each header: `speed [km/h]`, `n (rpm)`,
`Torque in Nm`, a row of units under the headers (as data loggers write
them) or a name such as `speed_meters_per_second` (the FASTSim drive-cycle
layout). It converts the numbers to the unit the table is stored in, for
example W to kW or rpm (1/min) to 1/min. A unit of the wrong kind, such as
kg for a speed, is refused.

A column without a unit is read in the table's own unit, except:

- **a speed**: values up to 45 are read as m/s, higher ones as km/h, and
  the preview asks whether that is right;
- **a grade**: values up to 0.3 are read as a fraction (0.05 = 5 %),
  larger ones as %, and the preview asks.

Change any unit in the preview's *Unit of …* lists.

## When a file is refused

The preview lists what stops the import, with the row and cell, for
example *Row 7: 'n/a' in cell C7 (Torque) is not a number* or *Row 12:
SOC 40 % is below the row before (60 %); SOC must increase*. Fix the file,
or pick other cells, and import it again. Nothing in the model changes
until you click **Apply**.

## From the command line

`lightsim-backend import-table FILE --part motor.emotor --param power_loss`
prints what it read, or the problems, and `--json` prints it as JSON.
