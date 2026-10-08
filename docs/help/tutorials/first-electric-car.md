# Your first electric car

In this tutorial you run the example electric car on the standard test
cycle, change its battery and compare the runs, then build a small
electric car of your own from seven parts and compare it with the
example. After each step a **Check** says what you should see. It takes
about fifteen minutes.

If LightSim is new to you, do [Your first run](first-run.md) first: it
shows the screen in five minutes.

## Part 1: the example on the test cycle

### 1. Run the WLTC

On the *Start* page, under *New from an example*, click **Battery
Electric Car**. In the list next to **Run** at the top right, pick *WLTC
Class 3b*: the Worldwide harmonised Light vehicles Test Cycle (WLTC), the
1,800 s test that cars are rated on in Europe. Press **Run**. The run
takes about half a minute; when it ends, click **Show results** in the
notice at the bottom right.

**Check:** the run ends as *success*, and the headline numbers read
*Consumption* 14.05 kWh/100 km and *Distance driven* 23.267 km.

*Consumption* is the energy that left the battery minus the energy that
braking put back, per 100 km ([how each number is worked out](../reference/results.md)).

### 2. Change the battery and compare

1. Click the **Home** tab, then the **HV Battery Pack** on the diagram.
2. In *Properties* on the right, set *Usable Capacity* from 62 to 40 and
   press Enter.
3. Press **Run** again, and **Show results** when it ends.

The *Results* page compares the new run with the one before, the
baseline ([how](../how-to/compare-two-runs.md)).

**Check:** *Consumption* still reads 14.05 kWh/100 km, but the battery's
final SOC (state of charge, how full it is) falls from 84.92 % to
82.11 %.

The car needs the same energy, which is a larger share of a smaller
battery. LightSim does not make the car lighter when the battery
shrinks: the mass is the Vehicle's own value. Write down the 14.05
kWh/100 km: you compare your own car with it at the end.

## Part 2: your own car

### 3. Start a blank project and add the parts

On the *Home* tab, click **New**, then **Blank project** on the *Start*
page. If LightSim asks whether to save the example, choose **Don't
save**.

The *Components* panel on the left is the library. To add a part, type its
name in **Search components…**, then double-click it in the list (or move
to it with the arrow keys and press Enter). Add these seven:

| Part | What it is here |
|---|---|
| **Vehicle** | The car's body: its mass and its air and rolling resistance |
| **Driving Task** | The speed the car must follow over time |
| **Driver** | Presses the accelerator and brake to follow that speed |
| **HV Battery Pack** | The battery |
| **E-Motor** | The electric motor and its inverter |
| **Final Drive** | A fixed gear between the motor and the wheel |
| **Wheel** | One driven wheel that stands for the whole car's wheels |

Each new part is named after its type and a number, such as *E-Motor 1*.
Drag the battery, motor, final drive and wheel into a row: it makes the
wiring easier.

**Check:** *Problems*, a tab in the panel under the diagram, lists 5
errors, among them *Vehicle present but no connected wheels — it will not
move*. Nothing is wired yet.

### 4. Wire the power path

Each part has ports, the small dots on its edges. A wire joins two ports
of the same kind: electrical to electrical, mechanical (a turning shaft)
to mechanical. Drag from one port to the other; hover over a port to see
its name.

1. *HV Battery Pack 1*'s **Positive Terminal (+)** to *E-Motor 1*'s
   **Positive Terminal (+)**.
2. The battery's **Negative Terminal (−)** to the motor's **Negative
   Terminal (−)**.
3. The motor's **Mechanical Shaft** to *Final Drive 1*'s **Flange In**.
4. The final drive's **Flange Out** to *Wheel 1*'s **Mechanical Shaft**.

The Vehicle needs no wire: the wheels push it along.

**Check:** *Problems* now lists 2 errors, both about missing signals,
such as *Driver 'Driver 1' has no Target Speed signal*.

### 5. Link the signals

Signals are numbers that parts pass to each other, such as a speed or a
command. Open *Data Bus Connections*, another tab under the diagram. It
lists every signal input, one row each, with a box for its source. For
each of these three inputs, click the box in its row and pick the source
from the list (type a few letters to narrow it):

| Input | Source |
|---|---|
| Driver 1 · Target Speed | Driving Task 1 · Target Speed |
| Driver 1 · Actual Speed | Vehicle 1 · Vehicle Speed |
| E-Motor 1 · Traction Command | Driver 1 · Traction Command |

The Driver now compares the target speed with the car's speed and tells
the motor how hard to push, or to brake by running as a generator
(recuperation).

**Check:** *Problems* has no errors left, and one warning: *Wheel load
shares add up to 25 %, not 100 %*.

### 6. Let the wheel carry the car

A wheel's *Vehicle Load Share* is the part of the car's weight it
carries; the library's wheel carries a quarter, for a car with four. Your
one wheel stands for all four. Click the warning: LightSim selects *Wheel
1*. In *Properties*, set *Vehicle Load Share* to 100.

**Check:** *Problems* says *All data checks passed*.

### 7. Give it your values and the test cycle

Make it a small city car, lighter than the example:

1. Click *Vehicle 1*. Set *Vehicle Mass* to 1200, *Drag Coefficient (Cd)*
   to 0.3 and *Frontal Area* to 2.1.
2. Click *HV Battery Pack 1* and set *Usable Capacity* to 30.
3. Click *Driving Task 1* and pick *WLTC class 3b* in *Drive Cycle*.
   LightSim sets the case, *Case 1*, to the cycle's 1,800 s.

Press **Run**, and **Show results** when it ends.

**Check:** the run ends as *success* with *Distance driven* 23.267 km.
*Consumption* should be between 9.5 and 11.5 kWh/100 km: LightSim gives
10.48 kWh/100 km with the values above.

If your run is a *warning* or *failed*, *Problems* says why: compare your
wires and signals with steps 4 and 5.

### 8. Compare it with the example

Your car uses 10.48 kWh/100 km on the WLTC, the example 14.05: 25 % less.
It is 727 kg lighter, its tyres roll more easily (the library's Wheel
has a *Rolling Resistance Coeff.* of 0.0085, the example's wheels
0.011), and its air drag is about the same (the drag coefficient times
the frontal area is 0.63 m² against the example's 0.62 m²). So the gain
comes from the mass and the tyres: less energy to speed the car up and
less rolling resistance.

To see which value matters most, change one at a time and run again, or
[run a parameter sweep](../how-to/parameter-sweep.md) of the mass.

## What you learnt

- A case picks the drive cycle; *Consumption* is energy at the battery
  per 100 km.
- A car is parts, wires (power) and signals (commands and speeds).
- *Problems* says what is missing, a moment after each change.
- The default values of the library are a mid-size car's; set them to
  your car's. Rest the pointer on a value in *Properties* to see what it
  means and where to find the real number; the
  [component reference](../reference/components/index.md) lists them all.

## Next

- [Pick a drive cycle](../how-to/pick-a-drive-cycle.md), such as the EPA
  city cycle.
- [Wire control signals](../how-to/wire-control-signals.md) says more
  about *Data Bus Connections*.
- **Save** keeps the project; [restore a version](../how-to/restore-a-version.md)
  if an edit goes wrong.
- On a Formula Student team? Start the
  [Formula Student lessons](../lessons/fs-1-acceleration.md).
