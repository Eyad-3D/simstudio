## How it computes

LightSim does not compute the FMU's model: the FMU does, in a separate,
locked-down process, and LightSim exchanges values with it at fixed
instants, as tools that run co-simulation FMUs do.

- **When it exchanges.** Once at the start of the run (t = 0) and then at
  every multiple k·h of h, the *Communication Step* (10 ms when it is 0).
  Any step above 0 is used as it is, one shorter than 10 ms too.
- **At each exchange** LightSim gives the FMU the inputs it read at the
  last exchange, lets it step from the last exchange's time to this one,
  and takes its outputs there.
- **Between exchanges** its outputs hold. When an exchange changes an
  output, the solver restarts from that moment.
- **Units.** Its pins have no unit: an input arrives as a number in the
  unit the app shows for the port it comes from, and an output leaves as a
  number in the unit of the input it feeds. Make the FMU's variables agree
  with those units.
- **A second run from the start** (an acceleration test's second run)
  starts the FMU again in a new process.
