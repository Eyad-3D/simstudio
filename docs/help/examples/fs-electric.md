# FS Electric (generic)

**The question it answers:** how fast is a Formula Student electric car
over 75 m, how long does one lap take, and how much energy does it need
for the endurance event?

**Level:** for Formula Student teams; the three
[Formula Student lessons](../lessons/fs-1-acceleration.md) start from it.
**Each run takes:** under 1 s for the acceleration, about 1 s for a lap,
10 to 15 s for the endurance.

## The car

A Formula Student electric car to make your own: typical values, not a
real car. Replace them with your team's data.

- 280 kg with the driver; centre of gravity 0.28 m high; wheelbase 1.53 m.
- One rear E-Motor (230 N·m, about 100 kW) through a 4.4 chain drive and
  an open differential; the front wheels are not driven and have brakes.
- An Accumulator (the Formula Student word for the high-voltage battery)
  of 138 cells in series and 4 in parallel (138s4p): 580 V full,
  7.2 kWh, 0.3 Ω, with the battery's *Formula Student Electric* preset.
- Drag area (CdA) 1.2 m², downforce area (CzA) 3.0 m², 45 % of it on the
  front; tyres with a friction coefficient μ of 1.5 that falls by 0.2 per
  kN of load.

The rules it follows are FS Rules 2026 v1.1 (Formula Student Germany,
FSG): at most 80 kW at the accumulator outlet (EV 2.2.1), judged on a
500 ms moving average (D 10.4.1); at most 600 V DC (EV 4.1.1); a 75 m
acceleration run (D 5.1.1) staged 0.3 m behind the line (D 5.2.3); an
endurance of about 22 km (D 7.1.3). FSUK and FSAE may differ: check the
current season's rules.

## What happens when

On *Acceleration 75 m*:

1. **0 to 0.2 s.** Full throttle from standstill. The rear tyres spin: the
   motor gives more torque than they can put down, and the model has no
   traction control.
2. **From 0.2 s.** The Accumulator reaches 80 kW and the battery holds it
   there; from now on the power limit, not the tyres, sets the pace.
3. **3.74 s.** The car crosses the 75 m line at 119 km/h.

The *Autocross (flying lap)* and *Endurance energy* cases drive
LightSim's own 979 m layout in lap mode (a lap simulation: the car goes as
fast as its tyres, motor and battery allow at each point of the track).
The summary says for how long each limit held the car back, for example
*Time limited by power cap*.

## Reference results

| Case | Result |
|---|---|
| *Acceleration 75 m* | 3.744 s from the start line, 118.88 km/h at the line, 0-100 km/h in 2.93 s |
| *Autocross (flying lap)* | 57.721 s for the flying lap, 60.32 km/h average |
| *Endurance energy* (23 laps, 30 kW limit) | 1,428.53 s, 5.33 kWh net from the Accumulator, 25.02 % SOC left |

At FS Czech Republic 2025 the 35 electric teams took 3.51 to 6.44 s over
75 m (median 3.91 s, the case's reference time), and the 14 teams scored
for efficiency used 3.19 to 6.15 kWh in the endurance (median 5.25 kWh).
These results are estimates: see the messages each run gives, and
[Known issues](../../KNOWN-LIMITS.md).

## Features it uses

- An *Acceleration* case, timed from the start line to the finish line
  ([how each number is worked out](../reference/results.md)).
- *Lap* cases on a Race Track, with the laps set per case.
- The battery's *Output Power Limit* with a 0.5 s check window, held to
  the limit, and a 600 V *Voltage Class*.

## Exercises

1. **Halve the power.** On *Acceleration 75 m*, set the Accumulator's
   *Output Power Limit* from 80 to 40 kW. How much slower is the car?

   **Answer:** 3.744 to 4.223 s, 0.479 s slower, and 95.95 km/h at the
   line instead of 118.88 km/h.

2. **Use worn tyres.** On *Acceleration 75 m*, set the *Friction
   Coefficient μ* of *Wheel RL* and *Wheel RR* from 1.5 to 1.3. What
   changes?

   **Answer:** 3.744 to 4.015 s. The car spends 63.8 % of the run at the
   tyres' grip limit instead of 48.4 %: with less grip, the tyres, not
   the power limit, hold it back for longer.

3. **Turn the endurance power down.** The *Endurance energy* case sets
   its own power limit, 30 kW, as an override (a value that applies to
   this case only). Pick the case, open the *Cases* tab on the right and,
   under *Parameter overrides*, set *Accumulator · Output Power Limit (0 =
   none)* to 25. What do you gain and what do you lose?

   **Answer:** The car needs 1,454.696 s instead of 1,428.53 s (26 s
   more) and ends with 34.16 % SOC instead of 25.02 %. Lesson 3,
   [Size the accumulator](../lessons/fs-3-accumulator.md), uses this
   trade.
