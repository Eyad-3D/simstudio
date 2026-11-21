# Use a Map block

A Map block reads one or more output signals from tables over input
signals you choose: a motor's loss and efficiency against its speed,
torque and temperature, say. You name each input and give it a unit, and
it becomes an *axis* of the map; each output has a table over those axes,
and all outputs share one grid of *breakpoints* (the input values where
the tables have values). A Map block has any number of inputs and
outputs.

## Add the block and its inputs and outputs

1. In the *Components* panel, find **Map** under *Driver & Signals* and
   drag it onto the diagram (or double-click it).
2. Double-click the block. Its parameter window opens at *Inputs* and
   *Outputs*.
3. Click **Add input** for each input. Type its name over *Input 1*, pick
   its unit in the list beside it (the units are grouped by quantity:
   *1/min* or *rad/s* under *Rotational Speed*, *°C* under
   *Temperature* …) and, in the next list, what a run does past its data:
   hold the edge value, extend the edge slope, or stop the run.
4. Click **Add output** for each output, and name it and pick its unit
   the same way.

The inputs are the map's axes, in their order. A new input starts with one
breakpoint, 0, so the values you had become that slice. To remove an
input, click its bin and pick the breakpoint whose values the map keeps.
An output's bin removes its table; the last output stays while the block
has inputs, since the grid is kept in the tables. A new name keeps the
port's wires and its table.

A signal is wired in and out in the unit of its quantity that LightSim
uses for signals (1/min for a rotational speed, kW for a power): nothing
is converted on the wire, as for every signal block (Data Checks warn
when a wire joins two different units). Pick another unit of the same
quantity, such as rad/s, and the port's numbers are shown and typed in it
at once; the signal stays in 1/min. Picking a unit of another quantity
keeps the numbers as they are and reads them in the new unit. You can
also change how the numbers are shown by clicking the unit in the line
above the grid ([Work in other units](work-in-other-units.md)).

A unit of a quantity LightSim does not use for signals (kWh, bar), or
one you type after **Another unit: type it…**, is shown with the
port's numbers, in the plot and on its pin, as it is: LightSim cannot
convert it, and other places (*Data Bus Connections*, Data Checks)
show the port as having no unit.

## Fill in the tables

Under the inputs and outputs is the table editor: pick the output in
**Output**, the two inputs the grid shows in **Rows** and **Columns**, and
the breakpoint each other input is held at. Type, paste from Excel, and
add or remove breakpoints as in any table
([Edit a table or a map](edit-a-table.md#tables-of-three-or-more-axes)): a
breakpoint added or removed changes every output, so the outputs keep one
grid. The plot under the grid draws the slice shown.

For a large map, export the parameter sheet: the Map block's sheet is a
long list, a column per input and per output and a row per point
([Edit parameters in a spreadsheet](edit-parameters-in-a-spreadsheet.md)).

## Import the map

Click **Import…** above the inputs and choose an Excel, CSV or text file,
or paste the cells into the grid: a list, a matrix, a matrix for each
temperature and the other layouts in
[Import a table from a file](import-a-table-from-a-file.md#layouts). The
data's axes and value columns are matched to the block's inputs and
outputs by name and unit. Tick **Set the Map block's inputs and outputs
from the data** to make an input of each axis and an output of each value
column instead, named and with units from the titles: a new block can be
set up from a file in one step, and Undo takes it back in one step.

## Wire it

Wire its inputs and outputs in *Data Bus Connections*, as for any signal
block ([Wire control signals](wire-control-signals.md)). Between
breakpoints each output follows straight lines along each input; past an
input's data it does what that input's list says.

## Run it

A Map block runs in a full run and in quick mode. Its outputs are read
from their maps at its inputs' values at every step of the solver, with
no delay and no sample time, and are channels of the run like any
block's (in the outputs' units). An input set to hold its edge value or
to extend its edge slope that is read past its data is reported after
the run: *'Motor Map' output 'Loss': read past its data on its input
'Speed' for 74.2 s from t = 1531.40 s …*. An input set to stop the run
stops it where it first leaves its data, with a message that names the
output's map (`loss_map`, after the output's id) and the input
(*… on its first (Speed) axis, which allows no values outside*).
