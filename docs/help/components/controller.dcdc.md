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
(V) and is that bus's source. It gives what the bus takes, as far as its
input side can give it, and draws P_in = P_out ÷ η.

- **Consumers behind it** (Power Consumers, Climate Controls): their demand
  ÷ η is asked of the input bus, where the converter counts as one of that
  bus's consumers. If the source there cannot carry all its consumers, each
  gets the same share, the ones behind the converter too.
- **Driving motors behind it**: their requests ÷ η are asked of the input
  bus, where the converter counts as one of that bus's motors and shares
  what the consumers leave.
- On a Voltage Source's bus it gets all it asks. With its input left open
  it gets nothing.
- **One way**: a motor behind it feeds back only what the loads on its own
  bus use at that moment. The rest shows as the motor's *regeneration not
  recovered*, and *Messages* says: "DC-DC '…' passes power one way only —
  regenerative torque on its output bus limited."
- A *Power Setpoint* linked in this mode is not used; the run goes ahead
  and *Messages* says so.

So a battery behind the converter stops at its *Minimum SOC* and a hydrogen
tank at empty, as they do for the loads on their own bus.

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
