# Import a table from a file

A motor map, a battery table or a loss map for each temperature often
comes as a spreadsheet. Every table in LightSim, of one, two or more axes,
and the Map block read one from an Excel workbook (`.xlsx`), a CSV file or
a text file, or from cells you copy in Excel and paste. A file and a paste
go through the same steps: LightSim finds the data, reads the names and
units from its titles, and shows you what it read before anything changes.

A whole drive cycle (a speed, and a road grade, against time or distance)
can instead join the **Drive Cycle** lists as a cycle of the project's
own: see [Use a cycle of your own](pick-a-drive-cycle.md#use-a-cycle-of-your-own).
A Driving Task's or Road Profile's profile has its own **Import from
file…** button, which reads two columns as described under *Profiles*
below.

## From a file

1. Open the part's table: double-click the part on the diagram (its
   parameters open in a window of their own), or click the table's
   **Edit…** in *Properties*.
2. Click **Import…** in the table's toolbar (on a Map block, above its
   inputs) and choose the file.
3. Check the preview (below), correct anything that was read wrongly, and
   click **Apply**.

## By pasting

Copy the cells in Excel (titles, units and all), click a cell of the table
and press Ctrl+V, or click **Paste**.

- A plain block of numbers that fits where you paste it goes straight into
  the cells, as it always has.
- Anything else opens the same preview, and its first line says why: the
  cells have titles or units, the numbers are written with a decimal comma
  or thousands separators, the block holds a table with its own axes, or
  it does not fit the table shown. Text is never pasted into number cells,
  and no cell is left out without the preview saying so.

## The preview

Nothing changes until you click **Apply**, and Apply is one undo step:
**Undo** (Ctrl+Z) brings the old table back. The preview shows:

- **Found**: the layout LightSim recognised (see *Layouts* below) and
  where it is, for example *A matrix (pivot table) (A2:H12)*, and how the
  text was read: its separator, and a decimal comma or point.
- **In the data / Is / Its unit**: each column, row or label that holds an
  axis or values, with the title it was read from, what it was matched to
  (an axis or the values of the table), why (*by its name and unit*), and
  the unit it was read in with the conversion, for example *rpm = 1/min*
  or *kN·m → N·m*. Change any of them in their lists and the preview reads
  the data again at once.
- **Questions**: where nothing in the data says which axis a column is,
  LightSim reads them in order and asks you to check, for example *Nothing
  in the data says what row 2 is: it was read as Speed*. It never guesses
  without saying so.
- **Problems**, each with its cell: a cell that is not a number, a
  breakpoint missing, a point that is in the data twice with two values, a
  unit of another kind (bar for a power), an axis the table does not have,
  or one it has that the data does not. **Apply** stays off until they are
  solved; the start of the sheet is shown with the cells named in red.
- **The table as it will be stored**, in the table's own grid with its
  plot: page through a held axis, switch the output of a Map block, and
  check the curves before you apply.

## Layouts

LightSim finds the data however it sits on the sheet: under title rows,
with empty rows and columns around it and notes beside it. These layouts
are recognised (the examples are cells, a tab between columns):

**Two columns**: an axis and its values.

```
SOC [%]    OCV [V]
0          300
50         350
100        400
```

**A matrix (a pivot table)**: one axis along the first row, another down
the first column, the values in between. The top-left cell may name both,
rows first, `Torque [Nm] \ Speed [rpm]` (or `/`, `|` between them), name
the values (`Loss [kW]`), or be empty.

```
Torque [Nm] \ Speed [rpm]   0      4000   8000
0                           0.1    0.6    1.2
150                         1.5    2.4    3.9
```

**A pivot table with more key columns**: as Excel's PivotTable writes
several row fields, a key column per axis on the left, blank where a key
repeats.

```
T [°C]   Speed [rpm]   0     100
25       0             0.1   1.1
         4000          0.6   1.9
90       0             0.4   1.5
         4000          0.9   2.3
```

**A list**, a column per axis and one or more value columns, the rows in
any order: the parameter sheet's long form. Several value columns become
several outputs of a Map block, sharing the grid.

```
Temperature [°C]   Speed [rpm]   Torque [Nm]   Loss [kW]   Efficiency [%]
90                 4000          100           2.5         93
25                 0             0             0.35        95
…
```

**A table for each breakpoint of a further axis** ("a map for each
temperature"): any of the layouts above, several times, one under another
or side by side, each with a label in a cell above it, or one per
worksheet with the breakpoint in the sheet's name or a label cell. The
label names the axis and its breakpoint; with two further axes it names
both:

```
Temperature = 25 °C        T [°C]: 90        T = 25 °C, U = 400 V
```

A label may also be split over two cells (`T [°C]` | `25`), and a sheet
may be named `25 °C`. With several worksheets, the preview's **Sheet**
list reads one sheet or *Every sheet (one per breakpoint)*. Several tables
without labels are not put together: the preview reads the first and lets
you pick another under **Table**. An axis with a single breakpoint that
the table does not have (all the data at `U = 400 V`) is left out, and
the preview says so.

## Names and units

LightSim reads a name and unit from titles written as `Speed [rpm]`,
`Speed (1/min)`, `Speed / rpm`, `Speed in rad/s` or `Torque_Nm`, from a
row of units under the titles, and from units as people write them
(`rpm`, `Nm`, `kW`, `degC`, `°C`, `%`). Words in brackets, as in `Loss
map (supplier data)`, are a title, not a unit.

Each column, row or label is matched to the table's axes and values by its
name (`n` and `rpm` name a speed) and by the kind of its unit (a torque
unit for the Torque axis), and its numbers are converted to the unit the
table stores. A column without a unit is read in the table's own unit,
and the preview says so. The table then shows the numbers in the units
chosen for it ([Work in other units](work-in-other-units.md)).

**Numbers**: in a file or paste with `3,7` the decimal mark is a comma
(3.7) and `1.000` is one thousand; with `3.7` it is a point and `1,000` is
one thousand; exponents (`1.5e3`) are read too. The preview says which
convention it found. When the numbers cannot show it (only `1,000` or
`1.000`, which can be 1 or 1000), the preview asks; change it under
**Decimal mark**.

## A table with holes

A table of two or more axes needs a value at every point of its grid. When
the data misses some, the preview counts them and names the first, and
offers two choices: **Do not import** (fill the data in first), or **Fill
each from the nearest breakpoint with a value**: each missing point takes
the value of the nearest point that has one, counted in steps along the
axes (on a tie, the first in the grid's order). The preview says how many
points were filled.

## A Map block from the data

On a Map block the import can also set the block itself: tick **Set the
Map block's inputs and outputs from the data**. Each axis becomes an input
and each value column an output, named from its title and given its unit:
a unit LightSim converts for signals (rpm, rad/s, °C, kW, %) picks its
quantity, the numbers are stored in that quantity's signal unit and shown
in the unit of the file; another unit (bar, kWh) is kept as the port's
unit text, as it is. The block's old inputs and outputs are replaced; a
wire stays on a port whose name the data has again. See
[Use a Map block](use-a-map-block.md).

## Profiles

A Driving Task's or Road Profile's **Import from file…** reads two columns
of numbers from a CSV or `.xlsx` file, picked by their headers (`time`,
`speed`, `distance`, `grade`); pick others under *… from*. A speed
without a unit is read as m/s up to 45 and km/h above, a grade as a
fraction up to 0.3 and % above, and the preview asks whether that is
right. Rows after the first empty row are left out; type a range such as
`B3:F20` under **Cells** to read only those cells.

## From the command line

`lightsim-backend import-table FILE --part motor.emotor --param power_loss`
prints what it read from two columns, a matrix or a profile, or the
problems, and `--json` prints it as JSON. `--decimal comma` or
`--decimal point` sets a CSV file's decimal mark. It reads the layouts of
the profile importer above; the other layouts are read in the app.
