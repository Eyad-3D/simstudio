# Formula Student 3: size the accumulator

The third Formula Student lesson. You trade endurance time against energy
with the power limit, see how much energy each number of cells in parallel
leaves at the finish, and keep the results for your design review. It
takes about 20 minutes. Do [Formula Student 2](fs-2-endurance.md) first.

## 1. Sweep the endurance power limit

The power limit decides how fast the car laps and how much energy it
uses. Sweep it on the endurance:

1. Pick *Endurance energy* in the list next to **Run**.
2. Open the *Cases* tab on the right and scroll to *Parameter sweep*.
3. Choose *Accumulator* and *Output Power Limit (0 = none) (kW)*; set
   **From** 25, **to** 35, **in** 3 steps: 25, 30 and 35 kW.
4. Click **Run sweep**. The three runs take about 20 s.
5. Click **Show results** in the notice it ends with, then **Sweep**
   above the chart, and pick *Total time* in the list next to it; then
   pick *Accumulator — final SOC*.

The swept value takes the place of the case's own 30 kW for each run.

**Check:** the *Total time* is 1,498.286 s at 25 kW, 1,477.22 s at 30 kW
and 1,460.819 s at 35 kW; the final SOC 57.33 %, 52.12 % and 47.00 %.

Each 5 kW more saves 16 to 21 s over the event and uses about 5 points
more charge. Where the time is worth more points than the energy depends
on your event's scoring formulas.

## 2. How many cells in parallel?

The example's accumulator is 138 cells in series and 4 in parallel
(138s4p), with 3.5 Ah cells: 4 × 3.5 = 14 Ah, its *Charge Capacity*. With
3 cells in parallel it would hold 10.5 Ah; with 5, 17.5 Ah. Sweep it at
the 30 kW limit:

1. In the *Cases* tab, choose *Accumulator* and *Charge Capacity (0 = from
   Usable Capacity) (Ah)*; set **From** 10.5, **to** 17.5, **in** 3 steps.
2. Click **Run sweep** and look at each run's status on the *Results*
   page, or in the study's table under *Saved studies*.

**Check:** with 3 in parallel the car finishes with 34.79 % SOC and
1.51 kWh usable energy left. With 4 the car finishes with 52.12 % SOC and
3.25 kWh usable energy left; with 5, 62.22 % and 4.986 kWh.

So all three finish at 30 kW, and margin of 3.25 kWh.
Fewer cells in parallel would also make the car lighter and raise the
accumulator's resistance; the sweep changes only the charge. To see
both, set the values by hand for a single run.

**Check:** with *Charge Capacity* 10.5 Ah, *Usable Capacity* 5.41 kWh,
*Series Resistance R0* 0.4 Ω (4/3 of 0.3 Ω) and a *Vehicle Mass* of 274 kg
(six kilograms of cells less), the car still finishes with 34.70 % SOC.

## 3. Pick your accumulator

Put both sweeps together: the accumulator must finish the event at the
power limit you plan to drive, with a margin for a real driver, a cold
day and older cells. Your team sets the margin; 10 % of the usable energy
is a common place to start (background knowledge, check it against your
own logged data).

Set the values of your own cells on the *Accumulator* in *Properties*:
its *Open-Circuit Voltage* table (the voltage of the whole pack against
SOC), *Charge Capacity*, *Usable Capacity* and *Series Resistance R0*.
Its help page lists each value and where to find it on a cell datasheet.

## 4. Keep the results for your design review

Each sweep is saved with the project as a study, under *Saved studies* at
the bottom of the *Cases* tab: a table with a row per value and every
summary figure. The download button next to the study's name saves it as
CSV for a spreadsheet; [export results](../how-to/export-results.md) says
how to save the chart and a run's channels too. **Save** keeps the
project with its studies.

A single design-review pack, with the model, its values and the results
in one file, is not in LightSim yet (roadmap RES-14).

## What you learnt

- A sweep of the power limit shows the time-against-energy trade.
- A sweep of the charge capacity shows how much energy each number of
  cells in parallel leaves at the finish.
- A run that runs out of charge fails and says where; its per-distance
  figures are marked *not valid*.

These results are estimates. [Known issues](../../KNOWN-LIMITS.md) lists
what lap mode leaves out; calibrate the car against a lap it has driven
before you trust a lap time.
