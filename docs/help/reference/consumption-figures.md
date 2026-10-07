# Consumption figures

The run summary in *Results* gives a car's energy and fuel use the way test
labs and regulators report it, so you can set a LightSim figure next to a
published one. This page says how each figure is worked out. Rows appear
only where they apply: a car with a battery and no engine gets the electric
rows, a hybrid the fuel rows.

A figure is marked *not valid* when the figure it is worked out from is,
for example when the car did not follow its cycle or the battery ran empty.

## Electric cars

| Row | Unit | How it is worked out |
|---|---|---|
| **Consumption** | kWh/100 km | The energy the batteries gave out minus what they took back, over the distance driven: the energy at the battery terminals (DC). |
| **Consumption at the socket (AC)** | kWh/100 km | Consumption ÷ the battery's **Charger Efficiency** (86 % by default, FASTSim's value): the energy taken from the wall to put that energy back, charging losses included. WLTP and EPA quote this figure. |
| **Fuel-economy equivalent (MPGe, AC)** | MPGe | 33.705 ÷ (AC kWh per mile). EPA counts 33.705 kWh of electricity as one US gallon of petrol. |
| **Range at this consumption** | km | The battery's usable energy ÷ Consumption. The usable energy is what the pack gives at open-circuit voltage from 100 % SOC down to its **Minimum SOC**. It is the range on the cycle the case drove, not a certified range test. |

## Hybrids

| Row | Unit | How it is worked out |
|---|---|---|
| **Fuel consumption** | l/100 km | The fuel the engines burned ÷ the Fuel Tank's density, over the distance driven. |
| **Battery energy change, share of fuel energy** | % | The energy the battery gave out minus what it took in, as a share of the fuel's energy (42.9 MJ/kg, petrol). Positive: the battery ended emptier. Under 1 % the run counts as charge-balanced (SAE J1711's rule), and the fuel figure needs no correction. |
| **Fuel consumption, charge-corrected** | l/100 km | Fuel × (1 + battery energy given ÷ engine work), per distance: the battery's energy is counted as if the engine had made it at its own average efficiency over the run. A battery that ended emptier makes the corrected figure higher. It is an estimate; repeating the cycle until the charge closes gives the exact figure. |

## Per phase

When a case of kind *Cycle* drives a standard cycle that has phases (the
WLTC's Low, Medium, High and Extra High, or the FTP's bags), the summary
gives each phase's **distance**, and its **consumption** (electric cars) or
**fuel consumption** (cars with an engine), over that phase alone. The
phase distances add up to the distance driven. A phase that the run did not
reach is left out.

For the FTP-75 the summary also gives the bags weighted the way EPA weights
them: **FTP weighted consumption** or **FTP weighted fuel consumption** =
0.43 × (bag 1 + bag 2) + 0.57 × (bag 3 + bag 2), each part being the energy
or fuel of its two bags over their distance. LightSim has no cold start, so
bag 1 and bag 3 come out the same.

## Good to know

- These figures are simulated, not certified.
- The charge correction and the per-phase rows only report; they do not
  change how the car is driven. Balancing a hybrid's charge by repeating
  the cycle is planned ([Known issues](../../KNOWN-LIMITS.md)).
