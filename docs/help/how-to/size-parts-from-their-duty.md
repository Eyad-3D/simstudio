# Size parts from their duty

A motor's, inverter's or battery's cooling is set by the load it carries
on average, not by its peak. The *Duty* view in *Results* gives each part's
highest, lowest, mean and RMS (root-mean-square, the mean that sets
heating: a part's losses grow with the square of its current) power,
torque and current over the run.

## Read the Duty view

1. Run a case.
2. In *Results*, click **Duty** above the chart.
3. Read the **RMS** column next to **Highest**. Each E-Motor, battery,
   engine, fuel cell and DC-DC converter has its own rows.
4. Type a power in **Time above** to see how long each power stayed above
   it, in seconds.

On the Battery Electric Car's *City Cycle*, the E-Motor's shaft power peaks
at 21.89 kW with an RMS of 6.88 kW, and the battery's current peaks at
65.12 A with an RMS of 20.67 A.

## What the numbers come from

- **Highest**, **Lowest**, **Mean** and **RMS** come from the solver's own
  steps (every fourth one, 40 ms at the usual 10 ms step), not from the
  stored points, so a short peak between two stored points still counts.
- **Time above** comes from the stored points: set the case's **Store
  every** to 1 and a small **Step** when it must be exact.
- An E-Motor's **DC current** is its electrical power over its bus voltage.
  The current in the motor's own windings (its phase current) is not
  modelled.

## Use the duty in a study

Each part's RMS power, RMS current and peak power are columns of a
parameter study's table, named like *E-Motor — RMS Shaft power*. See
[Run a parameter sweep](parameter-sweep.md).

**CSV** above the table saves it, with the time above your threshold.
