# Edit a table or a map

Some parameters are tables: a value against one input, such as a motor's
drag torque against its speed, or against several inputs, such as a
motor's power loss against its speed and torque (a map). Drive profiles
are tables too. Each input is an *axis*, and the numbers along it, where
the table has a value, are its *breakpoints*. Double-click a part to open
its parameters in a window of their own: every table there is a grid,
with a toolbar above it and a plot under it.

## Change cells

1. Click a cell and type a number. **Enter** keeps it and moves down,
   **Tab** moves right, **Esc** cancels the edit.
2. To change a number without typing it again, double-click the cell (or
   press F2) and edit the text in it.

The cell you are typing in is drawn raised, with a blue edge. Text that
is not a number turns the cell red and the line under the grid says why:
nothing is kept until it is a number. A breakpoint must be new on its
axis; one typed out of order moves its row or column so that the axis
keeps increasing.

**Delete** clears the selected cells: in a one-axis table a value becomes
0, in a map the cell is left empty (no value at that point). **Add row**
and **Delete row(s)** in the toolbar add a row at the end or delete the
selected rows; a map has a button for each of its axes. Every change is
in the project at once and is one step for **Undo** (Ctrl+Z).

## Copy and paste

The toolbar's **Copy** and **Paste** buttons do what Ctrl+C and Ctrl+V do
in the grid, in the same text that Excel and other spreadsheets copy and
paste.

- **Copy** copies the selected cells. With one cell selected, or none,
  it copies the whole table with its headers.
- **Paste** pastes from the selected cell on, to the right and down. A
  block larger than the table adds rows (and, in a map, columns). A whole
  table copied from LightSim goes back where it came from, wherever the
  selection is: copy a table, change it in Excel, copy it there and click
  **Paste**.

A paste with text that is not a number, or a breakpoint twice, changes
nothing, and the line under the grid says which cell is wrong. If
LightSim is not allowed to read the clipboard, **Paste** says so under
the grid: click a cell and press Ctrl+V instead.

## Read the plot

The plot under the grid draws the table as it is now: it follows each
edit, paste and undo, in the units the table shows. The axis titles name
each axis and its unit.

- A one-axis table is one curve, with a point at each breakpoint.
- A map is a curve per breakpoint of one axis. **Along x** chooses the
  axis that runs along the plot's x axis; the curves are the breakpoints
  of another axis, each named in the legend under the plot by its value
  and unit (*Speed 3000 1/min*).
- The point of the selected cell has a ring around it, and the line under
  the plot names it with its value.
- Point at a curve: the other curves fade, and a line under the plot
  gives its value at that place along x. The mouse wheel zooms, a double-click shows the whole table again, and with
  the plot selected the + and − keys zoom and the arrow keys move along x.

**Hide plot** folds the plot away, for more room; **Show plot** brings it
back.

## Units

The line joined to the top of the grid gives the unit of each axis and of
the values. Click a unit for its chooser. In a one-axis table you can
also double-click a column's header (or select it and press Enter); in a
map, the top-left cell names the values and their unit, and opens their
chooser the same way. See [Work in other units](work-in-other-units.md).

To read a whole table from a CSV, text or Excel file, see
[Import a table from a file](import-a-table-from-a-file.md).
