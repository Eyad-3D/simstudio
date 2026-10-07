# Battery Electric Car

**The question it answers:** how much energy does a compact electric car
use on the standard test cycle, and how much do its mass, its shape and
its heating change that?

**Level:** first steps. **Each run takes:** 5 to 30 s on a laptop.

## The car

A compact electric car modelled on the 2021 Cupra Born 58 kWh (the VW
ID.3 class), with the values of FASTSim's vehicle file (Apache-2.0):
1,927 kg, a drag coefficient (Cd) of 0.27, 2.31 m² frontal area, a 150 kW
E-Motor and a 62 kWh battery with 58 kWh usable. A 0.25 kW *Power
Consumer* stands for the lights and other small loads.

The power path is: **HV Battery Pack** → **HV Bus** → **E-Motor** →
**Final Drive** (one fixed gear) → **Differential** → the two front
wheels. The **Vehicle Task** gives the speed to follow and the **Driver**
works the accelerator and brakes to follow it. Each part's page in the
[component reference](../reference/components/index.md) says what its
values mean.

## What happens when

On the *WLTC Class 3b* case (the Worldwide harmonised Light vehicles Test
Cycle, 1,800 s, [Drive cycles](../reference/drive-cycles.md)):

1. **0 to 589 s, Low phase.** City driving with many stops. Each stop
   gives some energy back: the motor brakes as a generator
   (recuperation) before the friction brakes take over.
2. **589 to 1,022 s, Medium phase.** Faster town and suburban roads.
3. **1,022 to 1,477 s, High phase.** Main roads up to about 97 km/h.
4. **1,477 to 1,800 s, Extra High phase.** Motorway up to 131 km/h, where
   air drag takes most of the energy.

Tick *HV Battery Pack · Discharge Power* on the *Results* page to see
this: short peaks as the car speeds up, below zero while it brakes, and
the highest steady power in the last phase.

## Reference results

These are the results LightSim gives today. The help's tests run the
example and fail if one of them changes.

| Case | Consumption | Distance | Final SOC |
|---|---|---|---|
| *City Cycle* | 11.12 kWh/100 km | 7.292 km | 88.76 % |
| *WLTC Class 3b* | 14.05 kWh/100 km | 23.267 km | 84.92 % |
| *WLTC, heating/air-con on* | 18.89 kWh/100 km | 23.267 km | 83.19 % |

*Consumption* is the energy that left the battery, minus the energy that
went back in, per 100 km: energy at the battery's terminals
([how each number is worked out](../reference/results.md)). A car of this
class is rated about 15 to 16 kWh/100 km on the WLTC at the charging
socket, which also counts the charger's losses
([why the numbers differ](../theory/why-numbers-differ.md)).

## Features it uses

- A standard drive cycle picked in the Driving Task's *Drive Cycle*
  ([how](../how-to/pick-a-drive-cycle.md)).
- A case that changes one part's value only for itself (an override): the
  heating case sets the Power Consumer to 2.5 kW.
- A live case, *City Cycle (live, 10×)*, that runs ten times faster than
  real time so you can change values while it runs.
- Monitors that show the car's speed and the battery's state while it runs.

## Exercises

Pick the case in the list next to **Run**, change the value in
*Properties* and press **Run**. The *Results* page compares the new run
with the one before ([how](../how-to/compare-two-runs.md)). Set the value
back afterwards, or close the copy without saving.

1. **Turn the heating on.** Run *WLTC Class 3b*, then *WLTC, heating/air-con
   on*. How much more energy per 100 km does a 2.5 kW heater take?

   **Answer:** 14.05 to 18.89 kWh/100 km, 4.84 kWh/100 km more (34 %).
   The heater draws the same power the whole 1,800 s, so it counts most
   where the car is slow: in the City Cycle it would add even more per
   kilometre.

2. **Add 200 kg.** On *WLTC Class 3b*, set the Vehicle's *Vehicle Mass*
   from 1,927 to 2127. How much does the consumption rise?

   **Answer:** 14.05 to 14.79 kWh/100 km, 5.3 % more for 10.4 % more
   mass. Rolling resistance and speeding up grow with the mass, air drag
   does not; and part of the extra energy for speeding up comes back
   through recuperation.

3. **Make it slipperier.** On *WLTC Class 3b*, set the Vehicle's *Drag
   Coefficient (Cd)* from 0.27 to 0.24. What changes?

   **Answer:** 14.05 to 13.42 kWh/100 km, 4.5 % less. Air drag counts
   most on the motorway phase, so a better shape matters more there than
   in town.

4. **Fit a smaller battery.** On *WLTC Class 3b*, set the HV Battery
   Pack's *Usable Capacity* from 62 to 40 kWh. Does the car use less
   energy?

   **Answer:** No. *Consumption* stays at 14.05 kWh/100 km and the final
   SOC falls from 84.92 % to 82.11 %: the same energy is a larger share
   of a smaller battery. LightSim does not make the car lighter when the
   battery shrinks; change the *Vehicle Mass* too for that.
