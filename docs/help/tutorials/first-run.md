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

Press **Run**, or Ctrl+Enter. LightSim changes to the *Results* page and
draws the run as it goes. The City Cycle takes a few seconds.

## 4. Read the results

- The chart shows two channels, the battery's *SOC* (state of charge: how
  full the battery is, in %) and its *Discharge Power*. Tick other channels
  in the list on the left to add them, for example *Vehicle Speed* under
  *Vehicle*. Type in **Search channels…** to find one.
- The table under the chart sums up the run (scroll it for more rows): the
  battery's final SOC, 88.76 %, *Distance driven* 7.292 km and
  *Consumption* 11.12 kWh/100 km.
- The word at the top of the chart says how the run went. *success* means
  the car followed its target speed and its parts stayed inside their data.
  It does not mean the numbers match a real car.

## 5. Change a value and compare

1. Click the **Home** tab at the top, then the **Vehicle** on the diagram.
2. In *Properties*, set *Vehicle Mass* to 2300 and press Enter. The car
   now weighs 2,300 kg instead of 1,927 kg.
3. Press **Run** again.
4. On the *Results* page the list at the top left shows the new run. Under
   *Overlay*, tick the earlier run: the chart draws both, and the summary
   gets a column for each.

*Consumption* reads 12.34 kWh/100 km against 11.12: the heavier car uses
11 % more energy on the same drive.

## Next

- [Build an electric car from scratch](from-scratch.md).
- [Pick a drive cycle](../how-to/pick-a-drive-cycle.md), such as the WLTC
  that cars are rated on.
- [Run a parameter sweep](../how-to/parameter-sweep.md) to try many masses
  at once.
