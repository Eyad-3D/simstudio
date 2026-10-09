# Efficient Electric Sedan

**The question it answers:** what does low drag buy you? The same layout
as the [Battery Electric Car](bev-car.md), with a sleeker, lighter-rolling
body, on EPA's city and highway cycles and on the WLTC.

**Level:** first steps. **Each run takes:** 5 to 30 s on a laptop.

## The car

An efficient electric sedan sized after the 2022 Tesla Model 3 RWD: a test
mass of 1,928 kg and its road load as EPA's own coefficients A, B and C
from EPA's 2022 Test Car List (they hold the tyres' rolling resistance, the
air drag and the axle's drag), 192 kW rated power, a 9.0 reduction gear and
a 54 kWh battery. Its motor maps are the Battery Electric Car's, scaled to
192 kW, so the difference between the two cars is mostly the road load.

The power path is the Battery Electric Car's: **HV Battery Pack** →
**HV Bus** → **E-Motor** → **Final Drive** → **Differential** → the two
driven wheels, with a 0.25 kW *Power Consumer* for the small loads.

## What happens when

On the *EPA city (UDDS)* case (1,369 s, 12 km, [Drive cycles](../reference/drive-cycles.md))
the car starts and stops often, and braking with the motor gives much of
the energy back. On the *EPA highway (HWFET)* case (765 s, 16.5 km) it
cruises at 50 to 96 km/h, where air drag takes most of the energy. The
*WLTC Class 3b* case drives the same cycle as the Battery Electric Car's,
so the two can be compared run against run.

## Reference results

These are the results LightSim gives today. *At the socket* counts the
charger's losses (86 % efficiency, as FASTSim takes it), as EPA's tests do.

| Case | Consumption (battery) | At the socket | MPGe |
|---|---|---|---|
| *EPA city (UDDS)* | 9.50 kWh/100 km | 11.05 kWh/100 km | 189.6 |
| *EPA highway (HWFET)* | 11.06 kWh/100 km | 12.86 kWh/100 km | 162.8 |
| *WLTC Class 3b* | 11.78 kWh/100 km | 13.69 kWh/100 km | 152.9 |

EPA's tests of the real car gave 185.3 MPGe in the city and 170.1 on the
highway (unadjusted, before the label's corrections): LightSim's figures
are within 2.3 % and 4.3 %. The model has no cold start or warm-up, and
its motor and inverter losses are generic, not the car's
([why the numbers differ](../theory/why-numbers-differ.md)).

## Features it uses

- Road load given as EPA's coefficients A/B/C on the Vehicle, with
  *Coefficients Include Driveline Losses* ticked.
- The lab-style consumption figures in the summary: at the socket, MPGe
  and the range ([Results reference](../reference/results.md)).
- Three cases that each pick a standard drive cycle.

## Exercises

Pick the case in the list next to **Run**, change the value in
*Properties* and press **Run**. The *Results* page compares the new run
with the one before ([how](../how-to/compare-two-runs.md)). Set the value
back afterwards, or close the copy without saving.

1. **Compare it with the Battery Electric Car.** Run *WLTC Class 3b* on
   both examples. How much less energy does the sedan use?

   **Answer:** 11.78 against 14.05 kWh/100 km at the battery: 16 % less,
   with almost the same mass. Its lower road load is worth more than the
   Battery Electric Car's smaller size.

2. **Spoil its shape.** On *EPA highway (HWFET)*, raise the Vehicle's
   *Road Load C (f2)* (the coefficient that grows with the speed squared:
   air drag) by 20 %, from 0.024725 to 0.02967. How much does the highway
   consumption rise?

   **Answer:** 11.06 to 12.02 kWh/100 km, 8.7 % more: air drag is about
   half the highway's energy, so 20 % more of it costs nearly half that.

3. **Add 200 kg.** On *EPA city (UDDS)*, set the *Vehicle Mass* from
   1,928 to 2128 kg. What changes?

   **Answer:** 9.50 to 9.64 kWh/100 km, only 1.5 % more for 10.4 % more
   mass: in town, much of the energy for speeding up comes back through
   recuperation.

4. **Turn the heating on.** On *EPA city (UDDS)*, set the *Power Consumer*
   to 2.5 kW. How far does the range fall?

   **Answer:** From 543.3 to 310.3 km: at city speeds the heater draws
   almost as much as driving does, so the consumption rises by three
   quarters (9.50 to 16.64 kWh/100 km).
