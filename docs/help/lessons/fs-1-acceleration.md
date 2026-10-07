# Formula Student 1: your 75 m acceleration time

This is the first of three lessons for Formula Student teams. In it you
time the 75 m acceleration event, find out what holds the car back at
each moment, and see how much the 80 kW power limit costs. It takes about
15 minutes.

You need no LightSim experience; if the screen is new to you, do
[Your first run](../tutorials/first-run.md) first (5 minutes).

## 1. Open the Formula Student car

On the *Start* page, under *New from an example*, click **FS Electric
(generic)**. It is a typical electric Formula Student car, not a real one:
280 kg with the driver, one rear E-Motor of about 100 kW, and a 7.2 kWh
accumulator (the Formula Student word for the high-voltage battery).
[Its page](../examples/fs-electric.md) lists all its values.

It opens as a copy, so you can change it as you like and **Save** it as
your team's own project.

## 2. Run the acceleration test

In the list next to **Run** at the top right, pick *Acceleration 75 m*.
This case is an acceleration test: the driver holds full throttle from
standstill, and the run is timed from the start line, 0.3 m in front of
the car (FS Rules 2026 D 5.2.3), to the 75 m line. Press **Run**. The run
takes under a second.

**Check:** the headline numbers read *Time to 75 m* 3.744 s and *Speed at
75 m* 118.88 km/h. *Gap to reference time* is -0.166 s: the case's
reference time is 3.91 s, the median of the 35 electric teams at FS Czech
Republic 2025, so this car is 0.166 s quicker than the median.

## 3. Find what limits the car

Two limits can hold an electric FS car back: the tyres' grip (the motor
could push harder, but the tyres would only spin) and the power limit
(the rules allow at most 80 kW from the accumulator, EV 2.2.1).

1. Open **All summary values** under the chart.
2. Find *Time at the tyres' grip limit* and *Accumulator — time held at
   the output power limit*.
3. On the left, tick *Accumulator · Discharge Power* and *Wheel RL ·
   Longitudinal Slip* to plot them.

**Check:** *Time at the tyres' grip limit* reads 48.4 %, and the
accumulator was *held at the output power limit* for 3.77 s.
*Messages* says the accumulator was held at 80 kW from t = 0.20 s.

So the car is grip-limited only at the launch: in the first 0.2 s the
rear tyres spin (the slip line jumps), because the model has no traction
control. After that the 80 kW limit sets the pace to the line: the
*Discharge Power* line stays flat at 80 kW.

## 4. See what the power limit costs

What if your accumulator or motor can give only part of the 80 kW? Sweep
the limit: run the test once for each value.

1. Open the *Cases* tab on the right and scroll to *Parameter sweep*.
2. Choose *Accumulator* and *Output Power Limit (0 = none) (kW)*.
3. Set **From** 40, **to** 80, **in** 5 steps: 40, 50, 60, 70 and 80 kW.
4. Click **Run sweep**, then **Sweep** above the chart on the *Results*
   page.

**Check:** the sweep plots *Time to 75 m* against the power limit: 4.223 s
at 40 kW, 4.032 s at 50 kW, 3.902 s at 60 kW, 3.81 s at 70 kW and 3.744 s
at 80 kW.

Each 10 kW less costs more time than the one before, and below about
60 kW the car is slower than the median team. The
[parameter sweep how-to](../how-to/parameter-sweep.md) says more about
sweeps and their saved results table.

## 5. Make it your car

Change the values that matter most for acceleration to your team's:

- *Vehicle* · *Vehicle Mass*, with the driver;
- *Wheel RL* and *Wheel RR* · *Friction Coefficient μ* (your tyres' grip;
  1.5 is a typical race tyre on a warm track);
- *E-Motor* · *Full-Load Torque* and *Chain Drive* · *Transmission
  Ratio*.

Rest the pointer on a value in *Properties* to see what it means and
where to find the real number. Then run the test again.

**Check:** with worn tyres, μ 1.3 on both rear wheels, the time rises
from 3.744 s to 4.015 s, and *Time at the tyres' grip limit* from 48.4 %
to 63.8 %.

## What you learnt

- An *Acceleration* case times the 75 m from the start line and compares
  the time with a reference.
- The summary says how long the tyres' grip and the power limit each held
  the car back.
- A sweep shows what a design choice costs in time.

Next: [Formula Student 2: endurance energy](fs-2-endurance.md). These
results are estimates; [Known issues](../../KNOWN-LIMITS.md) lists what
the model leaves out (for example traction control and tyre temperature).
