## How it computes

The consumer asks for P_dem: the *Power demand* input when it is wired,
else *Constant Power Draw* (kW). A negative value counts as 0. The power it
draws does not depend on the voltage: it takes the current I = P ÷ U, with
U the voltage between its terminals.

On a bus whose source is an HV Battery Pack or a Fuel Cell Stack, the
consumers are served first, all cut back by one share s when the source
cannot carry them (see the Battery's page,
[HV Battery Pack](battery.generic.md)). Its *Drawn Power* is then
P = s·P_dem. On a Voltage Source's bus, or behind a DC-DC Converter that
holds its output voltage, s = 1 and it always draws P_dem.

The energy view books all the power it draws as loss; it stores nothing.
