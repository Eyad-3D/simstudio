## How it computes

The tank holds a fuel mass m, in kg. It starts at *Capacity* × *Initial
Fill* and falls by the fuel every Combustion Engine in the model burns:

dm/dt = −(sum of the engines' fuel rates)

*Fill Level* is m ÷ *Capacity* and *Fuel Mass* is m. The tank does not stop
a draw itself: each engine reads the mass and stops firing when it reaches
0 kg, and then only drags.

The energy view books m × *Fuel Heating Value* as stored energy. The
heating value also sets each engine's *Fuel Power*, so it changes the
energy breakdown but not the fuel burnt. *Fuel Density* and *CO₂ per kg of
Fuel* do not change the run; the run summary uses them for litres and CO₂.

Without a Fuel Tank, engines run on an inexhaustible supply at 42.9 MJ/kg.
A model with two Fuel Tanks is refused: "Only one fuel.tank element per
model is supported."
