## How it computes

The converter is an averaged model with a constant efficiency η, the
*Conversion Efficiency* (%). Its input side is *Input +* and *Input −*, its
output side *Output +* and *Output −*. P_in is the power it draws at its
input (*Power In*), P_out the power it gives at its output (*Power Out*).
What it does depends on what is on its output bus.

### Output bus with a battery: power setpoint

When an HV Battery Pack sits on the output bus, the converter delivers a
power and the battery balances the rest of that bus.

- Its setpoint P_set is the *Power Setpoint* input when wired, else *Power
  Setpoint (battery-bus mode)* (kW). A negative value counts as 0.
- It asks P_set ÷ η of its input bus. On a battery's or fuel cell's bus it
  is one of that bus's consumers and gets the same share s as the others
  (s = 1 unless the source cannot carry them all); on a Voltage Source's
  bus, s = 1.
- It delivers P_out = min(s·P_set, its share of what the output bus takes
  in). What the bus takes in is the battery's charge window (its *Max
  Charge Power*, charge limits and full charge) plus what the bus's loads
  use at that moment; converters on one bus share it in proportion to their
  setpoints. So it throttles back as the battery fills.
- It draws P_in = P_out ÷ η.

Its setpoint, as far as its input serves it (s·P_set), adds to what the
output bus's motors and consumers may draw from the battery's window.

### Otherwise: output voltage

With no battery on its output bus, it holds its output at *Output Voltage*
(V), and gives whatever power that bus takes. It draws P_in = P_out ÷ η.

In this mode no bus manager watches either side. The motors on its output
bus are not held to any supply window (the run warns: "Buses fed by a
DC-DC converter holding its output voltage have no bus manager yet: their
motors are not held to the supply's window."), and what it draws is not
part of its input battery's or fuel cell's window, so it can take that
source past its limits.

### Losses

The energy view books P_in − P_out as loss (the *Losses* output); the
converter stores nothing. It has no power rating.

### Refused

- At a power setpoint, its input must be on a bus with an HV Battery Pack,
  Fuel Cell Stack or Voltage Source of its own. Behind another DC-DC
  converter, or with its input left open, the run is refused: "a DC-DC
  converter feeding a battery bus works at its power setpoint, and its
  input must be on a bus with a battery, fuel cell or voltage source of its
  own (not behind another DC-DC converter, nor left open): connect its
  input to such a bus".
- The *Power Setpoint* input exists only at a power setpoint. Linked while
  the converter holds its output voltage, the run is refused: "Port
  'sig_setpoint_in' of '…' is linked, but its block has no such signal
  port."
