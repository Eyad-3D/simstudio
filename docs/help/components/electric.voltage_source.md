## How it computes

The source holds U = *Nominal Voltage* between its terminals, whatever the
current. *Supplied Power* is U·I, with I the current out of its positive
terminal: negative while the bus feeds power back into it.

It hands its bus no power window, so nothing on its bus is held back: the
motors draw and recuperate what their commands ask for, the Power
Consumers and Climate Controls get all of their demand, and a DC-DC
Converter at a power setpoint that draws from it is served in full.

It has no losses and stores nothing: the energy view counts what it gives
as energy supplied to the model.
