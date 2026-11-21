## How it computes

All five terminals are at one voltage, and the currents into them add up
to 0. A terminal with nothing wired carries no current. The node loses and
stores nothing.

Everything wired to the node is one bus. When the bus's source is an HV
Battery Pack or a Fuel Cell Stack, the motors, consumers and DC-DC
converters on it share that source's power window (see the Battery's page,
[HV Battery Pack](battery.generic.md)). The node itself computes the same
either way.

*Throughput Power*:

- when one terminal is wired straight to the positive terminal of the
  bus's source (a battery, Voltage Source, Fuel Cell Stack or a DC-DC
  Converter's output), it is U·I at that terminal: the power the source
  feeds in, negative while the bus feeds the source back;
- otherwise it is the sum of U·I over the terminals that bring power in.
