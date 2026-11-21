## How it computes

The Map has no state and no sample time: at every moment the solver
computes, each output is its own map read at the present values of all the
inputs.

- **The axes.** Each input is one axis of every map, in the inputs' order.
  All the outputs share one grid of breakpoints.
- **Between breakpoints** each output follows straight lines along each
  axis in turn (multilinear interpolation: with two inputs, the four grid
  points around the operating point weighted by how near it is to each).
- **Past an input's data** that input's outside-the-data setting decides:
  *clamp* (the default) holds the edge value, *linear* continues the edge
  segment's slope, and *error* stops the run, naming the table and the axis.
  After a run that read past the data on a *clamp* or *linear* input, the
  messages say which output and which input, for how long, how far past the
  edge, and what the map gave there.
- **No input.** An output of a Map with no input is a constant: its single
  value.
- **Units.** Unlike the Lookup Table, the Map's inputs and outputs carry
  the units you give them: a signal wired in is converted into the input's
  unit, and an output is converted into the unit of the input it feeds.

The [how-to page](../../how-to/use-a-map-block.md) shows how to set one up
and edit its maps.
