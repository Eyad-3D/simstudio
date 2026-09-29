# Wire control signals

Signals are numbers that parts pass to each other while a run goes: a
target speed, a command between −1 and 1, a battery's state of charge. A
signal goes from a part's output to another part's input. Power and shafts
are wired on the diagram; signals are linked in the *Data Bus Connections*
panel.

## Link an input to its source

1. Open *Data Bus Connections*, a tab in the panel under the diagram.
   There is one row for every signal input in the model:
   *source box* → *Part · Input [unit]*.
2. Click the box in the input's row. The list shows every output in the
   model, those whose names share a word with the input first (for the
   Brake Command input, *Driver · Brake Command*), then those with the
   input's unit.
3. Type to narrow the list (every word you type must match), then click
   the output, or pick it with the arrow keys and Enter.

The link is made: the box shows the source and its unit. To change it, pick
another source in the same box; to remove it, click the bin at the end of
the row (**Remove connection**).

## Find the row you need

- Type in **Search signals…** to list only the rows that mention a part or
  signal.
- Tick **Unconnected inputs** to list the inputs with no source. An input
  with no source reads 0.
- Select a part on the diagram and tick **Selected part** to list its
  signals only. Right-click a part and choose **Signals…** to do both at
  once.

## Rules

- A link goes from an output to an input. LightSim refuses a link between
  two inputs or two outputs, and says why in *Messages*.
- An input takes one source; an output can feed many inputs (the Driver's
  Brake Command feeds every brake).
- Signal units are not checked: LightSim passes the number as it is. Check
  that both ends of a link show the same unit.
- A row in amber is a link that passes nothing: one saved by an earlier
  version between two inputs or two outputs, or one to a part or port that
  is gone. Remove it.

## Show the links on the diagram

The diagram does not draw signals by default. Open *Layer Configurations*
under the diagram and tick *Signal / data-bus links* to draw each link as a
dashed line.

## Scripts and Monitors

A **Script** or **Monitor** part has no signal ports of its own: add them
in its *Properties* (**+ input**, **+ output**). They then have rows in
*Data Bus Connections* like any other part. The
[Script API](../reference/script-api.md) says how a script reads its inputs
and sets its outputs.
