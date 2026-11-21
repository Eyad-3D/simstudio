## How it computes

The tank holds a hydrogen mass m, in kg. It starts at *Capacity* × *Initial
Fill* and falls by the hydrogen every Fuel Cell Stack in the model uses:

dm/dt = −(sum of the stacks' H₂ consumption)

*Fill Level* is m ÷ *Capacity* and *H₂ Mass* is m. The tank does not stop
a draw itself: each stack reads the mass and offers its bus no power once
it reaches 0 kg (see [Fuel Cell Stack](fuelcell.stack.md)).

Hydrogen's heating value is fixed at 119.96 MJ/kg (33.3 kWh/kg). The energy
view books m × 119.96 MJ/kg as stored energy.

Without a Hydrogen Tank, stacks run on an inexhaustible supply. A model with
two Hydrogen Tanks is refused: "Only one fuel.h2_tank element per model is
supported."
