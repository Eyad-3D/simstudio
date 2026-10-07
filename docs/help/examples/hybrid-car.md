# P2 Hybrid Car

**The question it answers:** how much fuel does a parallel hybrid save in
town and on the highway, and what does its control script decide at each
moment?

**Level:** second steps; it has a control script. **Each run takes:** 10
to 30 s on a laptop.

## The car

A P2 parallel hybrid sized after the Hyundai Ioniq Hybrid Blue: a
1.6-litre petrol engine, a clutch, a 32 kW E-Motor on the input of a
6-speed gearbox, and a 1.56 kWh battery. "P2" says where the motor sits:
between the engine's clutch and the gearbox, so it can drive the car with
the engine stopped and the clutch open.

The test mass (1,474 kg) and the road load come from the EPA 2022 Test
Car List: EPA's own coefficients A, B and C (the road load, the force
that holds the car back at each speed:
A + B·v + C·v²). They already hold the axle's own drag, so the final drive
runs without losses. A 0.5 kW *12 V Loads* part stands for the lights,
fans and electronics.

The **Hybrid Control Unit** is a [Script](../reference/script-api.md): a
short Python function that runs at every solver step. It starts the car
on the motor alone, stops the engine at standstill, when coasting and at
low demand, and keeps the battery near 55 % charge.

## What happens when

On *EPA city (UDDS)* (the EPA Urban Dynamometer Driving Schedule,
1,369 s of city driving):

1. **Pulling away.** The motor alone moves the car; the engine stays off
   and the clutch open.
2. **Speeding up harder.** When the driver asks for more than the motor
   should give, the script starts the engine and closes the clutch; the
   engine drives the car and the motor helps or charges the battery.
3. **Braking.** The motor brakes as a generator and charges the battery;
   the engine stops.
4. **At every stop** the engine is off. Over the whole cycle the engine
   starts 30 times.

Tick *Engine · Speed*, *HV Battery · SOC* and *Vehicle · Vehicle Speed*
on the *Results* page to see the engine switch on and off. The script's
own outputs, such as *engine_on* (1 while the engine runs), are under
*Hybrid Control Unit*.

## Reference results

Each case starts at the battery charge that it ends with, so the fuel is
not flattered by a battery that ran down.

| Case | Fuel consumption | CO₂ | Distance |
|---|---|---|---|
| *EPA city (UDDS)* | 2.84 l/100 km | 67.0 g/km | 11.99 km |
| *EPA highway (HWFET)* | 3.24 l/100 km | 76.4 g/km | 16.507 km |
| *Mixed Cycle* | 2.88 l/100 km | 68.0 g/km | 9.556 km |

EPA's own tests of the Ioniq Blue give 2.91 l/100 km in the city and
2.94 l/100 km on the highway. The city figure here is lower because the
model has no cold start: its engine is warm from the first second
([why the numbers differ](../theory/why-numbers-differ.md)). *CO₂* is
the fuel burnt times the Fuel Tank's CO₂ factor (3.17 kg CO₂ per kg of
petrol), per km ([how each number is worked out](../reference/results.md)).

## Features it uses

- A Script block with its own input and output ports, linked in *Data Bus
  Connections* ([the Script API](../reference/script-api.md),
  [recipes](../reference/script-cookbook.md)).
- A gearbox with a shift schedule, a clutch and a combustion engine with
  a fuel map.
- Road load set as coefficients A, B and C instead of drag and rolling
  resistance.
- Cases that change the battery's start charge only for themselves.

## Exercises

Pick the case in the list next to **Run**, change the value in
*Properties* and press **Run**. The *Results* page compares the new run
with the one before. Set the value back afterwards.

1. **Double the 12 V loads.** On *EPA city (UDDS)*, set the *12 V Loads*'
   *Constant Power Draw* from 0.5 to 1.0 kW. How much more fuel does the car use?

   **Answer:** 2.84 to 3.30 l/100 km, 16 % more. In town the car is slow,
   so 0.5 kW more for 1,369 s is a large share of the energy per
   kilometre; the engine has to make it, through the battery.

2. **Add 200 kg on the highway.** On *EPA highway (HWFET)*, set the
   Vehicle's *Vehicle Mass* from 1,474 to 1674. How much more fuel?

   **Answer:** 3.24 to 3.28 l/100 km, about 1 % more. On the highway the
   car hardly speeds up or brakes, so mass counts only through rolling
   resistance; air drag, which mass does not change, takes most of the
   energy.

3. **Compare city and highway.** Run *EPA city (UDDS)* and *EPA highway
   (HWFET)*. Where does the hybrid use less fuel per 100 km, and why?

   **Answer:** In the city: 2.84 against 3.24 l/100 km. In town the
   hybrid recovers braking energy and stops its engine at every stop; on
   the highway it drives at steady speed with the engine on, where a
   hybrid has little to gain and air drag is higher.
