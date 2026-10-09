# Your first run

In this tutorial you run an example car, read how much energy it used,
change one value and compare the two runs. It takes about five minutes.

## 1. Open the example

LightSim starts on the *Start* page. Under *New from an example*, click
**Battery Electric Car**, a compact electric car modelled on the 2021 Cupra
Born. Its card says what results to expect. (Later, **New** on the *Home*
tab, or the *Start* tab, brings you back to this page.)

An example opens as a copy: change it as you like. The example itself stays
as it is, and **Save** keeps your copy as a project of your own.

## 2. Look at the model

The diagram in the middle, the *Topology* panel, shows the car's parts and
the wires between them:

- The **HV Battery Pack** (HV: high voltage) feeds the **E-Motor** through
  the **HV Bus**. The **Power Consumer** stands for the lights, heating and
  other loads that also draw from the battery.
- The E-Motor turns the **Final Drive**, a fixed gear, which turns the
  **Differential** and, through it, the two front wheels.
- The **Vehicle Task** gives the speed the car must follow, and the
  **Driver** works the accelerator and brakes to follow it.

Wires carry electrical power (the electrical domain) or turning shafts (the
mechanical domain). Parts also pass numbers to each other, such as the
target speed from the Vehicle Task to the Driver: these signals are listed
in *Data Bus Connections*, one of the tabs under the diagram.

Click the **E-Motor**. The *Properties* panel on the right shows its
values: its maps (tables of torque and losses against speed) and its
maximum speed. Press **F1** to open its page in this help.

## 3. Run it

The list at the top right, next to **Run**, holds the model's cases. A case
is one simulation job: the drive cycle, how long it runs and a few
settings. *City Cycle* is picked, a 600 s drive in town.

The *Results* tab already holds a run of each case, named *Stored result*:
the example's results as LightSim stored them, with only a few signals.

Click the **Results** tab, then press **Run**, or Ctrl+Enter. LightSim
draws the run as it goes. The City Cycle takes a few seconds. (Run from
another page, the page stays as it is, and a notice at the bottom right
offers **Show results** when the run ends.) Your run is
compared with the stored one, and its numbers read *~ 0 vs baseline*: the
same.

## 4. Read the results

**Check:** the run ends as *success* with *Consumption* 11.11 kWh/100 km.

- The numbers above the chart sum up the run: *Consumption*
  11.11 kWh/100 km, *Distance driven* 7.292 km, the battery's final SOC,
  88.76 %, and the energy it gave and took back. **All summary values**
  under the chart opens the full list.
- The chart opens on the *Target Speed* (dashed) over the *Vehicle Speed*:
  where the car follows its cycle, the two lie on each other. With them
  are the battery's *SOC* (state of charge: how full the battery is, in %)
  and its *Discharge Power*. Tick other channels in the list on the left to
  add them. Type in **Search channels…** to find one.
- Each axis fits its data, so a small change fills the plot: the SOC axis
  runs from 88.7 to 90.1 %.
- Rest the pointer on the chart: the legend under it reads the values at
  that time. Roll the mouse wheel over the chart to zoom in on a moment;
  a double-click shows the whole run again
  ([all the chart's keys](../reference/keyboard-shortcuts.md#on-a-chart)).
- Under the numbers, a line compares *Consumption* with the value the
  example expects, 11.12 kWh/100 km ± 3 %, and reads **within**.
  *Hand calculations: 2 passed* folds away two checks LightSim works out
  itself ([more](../how-to/check-against-expected-values.md)).
- The word at the top of the chart says how the run went. *success* means
  the car followed its target speed and its parts stayed inside their data.
  It does not mean the numbers match a real car.

## 5. Change a value and compare

1. Click the **Home** tab at the top, then the **Vehicle** on the diagram.
2. In *Properties*, set *Vehicle Mass* to 2300 and press Enter. The car
   now weighs 2,300 kg instead of 1,927 kg.
3. Press **Run** again. When it ends, click **Show results** in the
   notice at the bottom right.
4. On the *Results* page the new run is named *Vehicle Mass 2,300 kg*,
   after what you changed. The earlier run is drawn faint and dashed with
   it, and *What changed* on the left lists *Vehicle · Vehicle Mass
   1,927 → 2,300 kg*.

**Check:** *Consumption* reads 12.34 kWh/100 km, *+1.22 (+11.0 %) vs
baseline*.

The heavier car uses 11 % more energy on the same drive than the
earlier run, the baseline. [How to compare two runs](../how-to/compare-two-runs.md).

## Next

- [Your first electric car](first-electric-car.md), in 15 minutes.
- [Pick a drive cycle](../how-to/pick-a-drive-cycle.md), such as the WLTC
  that cars are rated on.
- [Run a parameter sweep](../how-to/parameter-sweep.md) to try many masses
  at once.
