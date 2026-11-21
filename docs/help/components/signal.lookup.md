## How it computes

The Lookup Table has no state: its output is its table read at its inputs.

- **1D.** out = the *1D Table* at Input X. Between two points the output
  follows a straight line (linear interpolation).
- **2D.** out = the *2D Table* at Input X and Input Y. The table is a set
  of curves along its inner axis, one for each value of its outer axis; the
  output is read on the two curves either side of the point, along the
  inner axis, then on the straight line between those two values. The
  curves may have different points in between, but they must all start and
  end at the same values of the inner axis: the run refuses a table whose
  curves cover different ranges (*its sheets do not all cover the same
  range of its second axis*).
- **Past the table's data**, each axis follows its own outside-the-data
  setting: *clamp* (the default) holds the edge value, *linear* continues
  the edge segment's slope, and *error* stops the run with a message that
  names the table and the axis. A table of a single point is a constant.
- **Sample Time.** At 10 ms or less (0 included) the table is read
  continuously, at every moment the solver computes. Longer, the block
  takes its inputs at each sample (once at the start, then at the first
  10 ms instant at or after each multiple of the *Sample Time*), reads the
  table at them, and holds the output until the next sample.
- **Units.** Its ports have no unit: an input arrives as a number in the
  unit the app shows for the port it comes from (a Vehicle's speed in km/h,
  a battery's state of charge in %), and the output leaves as a number in
  the unit of the input it feeds. *Input X Unit* and *Input Y Unit* convert
  nothing; Data Checks use them to warn when the wired signal's unit
  differs.
