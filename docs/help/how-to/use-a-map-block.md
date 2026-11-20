# Use a Map block

A Map block reads one or more output signals from tables over input
signals you choose: a motor's loss and efficiency against its speed,
torque and temperature, say. You name each input and give it a unit, and
it becomes an *axis* of the map; each output has a table over those axes,
and all outputs share one grid of *breakpoints* (the input values where
the tables have values). A Map block has any number of inputs and
outputs.

**This version's engine does not run a Map block yet.** A case of a
project with one fails at once with `Not run [NEW-TABLE-AXES]` and the
block's name; the block, its tables and the project save and check as
usual, and run once the engine has it
([Known issues](../../KNOWN-LIMITS.md#does-not-run-in-this-version)).

## Add the block and its inputs and outputs

1. In the *Components* panel, find **Map** under *Driver & Signals* and
   drag it onto the diagram (or double-click it).
2. Double-click the block. Its parameter window opens at *Inputs* and
   *Outputs*.
3. Click **Add input** for each input. Type its name over *Input 1*, pick
   its unit in the list beside it (*Rotational Speed (1/min)*,
   *Temperature (°C)* …) and, in the next list, what a run does past its
   data: hold the edge value, extend the edge slope, or stop the run.
4. Click **Add output** for each output, and name it and pick its unit
   the same way.

The inputs are the map's axes, in their order. A new input starts with one
breakpoint, 0, so the values you had become that slice. To remove an
input, click its bin and pick the breakpoint whose values the map keeps.
An output's bin removes its table; the last output stays while the block
has inputs, since the grid is kept in the tables. Renaming a port removes
its wires.

The numbers are in the unit you pick for the port, and so is the signal
wired to it: nothing is converted on the wire, as for every signal block
(Data Checks warn when a wire joins two different units). To see the
numbers in another unit of the same kind, click the unit in the line above
the grid ([Work in other units](work-in-other-units.md)).

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

## Wire it

Wire its inputs and outputs in *Data Bus Connections*, as for any signal
block ([Wire control signals](wire-control-signals.md)). Between
breakpoints each output follows straight lines along each input; past an
input's data it does what that input's list says.
