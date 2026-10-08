# Results summary values

Every number a run's summary can show, what it means and how LightSim
works it out. The summary is on the *Results* page: the headline numbers
above the chart, and the full table, **All summary values**, under it. A
row appears only when the model has the part it is about, or when what
it counts happened.

In the names below, *part* stands for the part's name on your diagram
(such as *HV Battery Pack*) and *N* for a number the case sets (such as
the 75 in *Time to 75 m*). Energies are in kWh, powers in kW; "the
battery" means each battery in the model.

How sure a number is: the run's status and the *not valid* marks say
whether the run's own checks hold (see [below](#not-valid-and-the-run-status)).
Whether the model matches a real car is a separate question:
[Validation](../../VALIDATION-STATUS.md) and
[Known issues](../../KNOWN-LIMITS.md).

## Distance and energy use

| Row | Unit | What it is |
|---|---|---|
| **Distance driven** | km | How far the Vehicle went: its speed added up over the run |
| **Consumption** | kWh/100km | The energy the batteries gave, minus the energy braking put back into them, per 100 km. It is the energy at the battery's terminals, not at the charging socket, so it leaves out the charger's losses ([why it differs from an official figure](../theory/why-numbers-differ.md)). It does not count fuel, a fuel cell's or a voltage source's energy. Shown only when the car drove more than 100 m and the batteries gave more than they took back |
| **Fuel consumption** | l/100km | The fuel the engines burnt, in litres (the mass divided by the Fuel Tank's *Fuel Density*, 0.745 kg/l for petrol when there is no tank), per 100 km. It is not corrected for a change in the battery's charge: a hybrid case that ends with less charge than it started with looks better than it is. Shown only after 100 m and when fuel was burnt |
| **CO₂ emissions** | g/km | The fuel burnt times the Fuel Tank's *CO₂ per kg of Fuel* (3.17 kg CO₂ per kg of petrol when there is no tank), per km. Only what comes out of the exhaust; not what making the fuel or the electricity cost |
| **Electrical energy balance error** | % | Energy that no source supplied or took up, as a share of all the energy that went through the electrical buses. It shows the solver's last-resort limits at work; it is 0 in a sound model. Above about 0.1 %, look at what the run's messages say |
| **Simulated duration** | s | The time the run covered: the case's *Duration*, or less when a test ended at its line or the run stopped |

## Batteries

| Row | Unit | What it is |
|---|---|---|
| ***part* — final SOC** | % | The battery's state of charge (SOC, how full it is) at the end of the run. LightSim counts the charge that flows in and out, in amp-hours, as a battery management system does |
| ***part* — energy delivered** | kWh | The energy that left the battery at its terminals while it discharged |
| ***part* — energy recuperated** | kWh | The energy that went back into the battery at its terminals, from braking with the motor (recuperation) or from an engine charging it |
| ***part* — internal losses** | kWh | The energy turned into heat inside the battery: in its resistances, and charge that the *Coulombic Efficiency* does not store |
| ***part* — usable energy left** | kWh | The energy the battery can still give before it reaches its *Minimum SOC*, from its open-circuit voltage. Shown when the battery has an *Output Power Limit* or a *Voltage Class*; it fails (red) when the battery reached its minimum SOC |
| ***part* — minimum pack voltage** | V | The lowest voltage at the battery's terminals during the run. Shown with *usable energy left* |
| ***part* — maximum pack voltage** | V | The highest voltage of the battery: at its terminals while charging, or its open-circuit voltage at 100 % SOC, whichever is higher. Checked against the *Voltage Class* (Formula Student: 600 V) |
| ***part* — peak terminal power** | kW | The highest power at the battery's terminals at any solver step. Shown with an *Output Power Limit*, and in an acceleration test |
| ***part* — peak terminal power, averaged** | kW | The highest power at the battery's terminals averaged over the *Power Check Window* (Formula Student: 500 ms). Checked against the *Output Power Limit* |
| ***part* — time held at the output power limit** | s | How long the battery held the motors back to keep to its *Output Power Limit* (with *Hold Power to Limit* on) |
| ***part* — time over the output power limit** | s | How long the battery's power was above its *Output Power Limit*, when *Hold Power to Limit* is off and the limit is only checked |
| ***part* — mean terminal power** | kW | In an acceleration test: the battery's net energy over the run divided by the run's time |

## Motors, engines and other sources

| Row | Unit | What it is |
|---|---|---|
| ***part* — time limited by supply** | s | How long the E-Motor gave less torque than it was asked for because its battery, fuel cell or voltage source could not supply the power (a power limit, a battery at its minimum SOC) |
| ***part* — regeneration not recovered** | kWh | Braking energy the E-Motor's command asked for that its supply could not take back (a full or charge-limited battery, a fuel cell, a one-way DC-DC): the motor braked that much less and the friction brakes more |
| ***part* — fuel used** | kg | The fuel an engine burnt, from its fuel map |
| ***part* — energy supplied** | kWh | The energy a fuel cell or a voltage source gave to its bus |
| ***part* — time above maximum speed** | % | The share of the run an E-Motor or engine ran above its *Maximum Speed*. Shown only when that happened; it ends the run as *warning* when it lasts more than 1 % of the run and at least 2 s |
| ***part* — highest speed** | 1/min | The highest speed of that E-Motor or engine, shown with the row above |
| ***part* — time outside its *what* (*axis*)** | % | The share of the run a part read one of its tables (*what*, such as its full-load map) outside its data on one axis (*axis*, such as its speed). Shown only when that happened |
| ***part* — furthest *axis* outside its *what*** | the axis's unit | How far outside its data that table was read, shown with the row above |

## Performance and acceleration tests

| Row | Unit | What it is |
|---|---|---|
| **Time to *N* km/h** | s | From t = 0 to the moment the car's speed first reaches *N* km/h, read between the two solver steps around it. A performance case reports its target; an acceleration test reports *Time to 100 km/h* |
| **Maximum speed** | km/h | The highest speed in a performance case |
| **Time to *N* m** | s | In an acceleration test: from the moment the car crosses the start line (the case's *Start line*, such as 0.3 m) to the moment it crosses the line *N* m past it (the case's *Distance*). Checked against the case's *Duration* |
| **Speed at *N* m** | km/h | The car's speed as it crosses that line |
| **Gap to reference time** | s | *Time to N m* minus the case's *Reference time*; below 0 the car is quicker than the reference |
| **Time at the tyres' grip limit** | % | The share of an acceleration test that a driven wheel spent at its tyres' grip limit (spinning) |

## Lap cases

| Row | Unit | What it is |
|---|---|---|
| **Lap time** | s | The fastest lap's time |
| **Lap 1 time** | s | The first lap's time, from the Vehicle's *Initial Speed*; shown when the case drives more than one lap |
| **Total time** | s | All the laps' times added up; shown with *Lap 1 time* |
| **Sector *N* time** | s | The fastest lap's time in each sector of the track |
| **Average speed** | km/h | The distance of all laps divided by their total time |
| **Speed at the finish** | km/h | On a track that is not closed: the speed at its end |
| **Energy per lap** | kWh | The energy the sources gave the buses, minus what they took back, divided by the number of laps |
| **RMS battery power** | kW | The root mean square of the batteries' power over the laps: the steady power that would heat the cells as much as the real, changing one. Use it to check the cells' continuous rating and the accumulator's cooling |
| **Time limited by cornering grip** | s | Over all laps, the time the car went as fast as its tyres' sideways grip allows in a corner |
| **Time limited by traction grip** | s | The time the driven tyres could not put down more drive |
| **Time limited by motor** | s | The time the motors gave all the torque they have |
| **Time limited by battery** | s | The time the battery could give no more power (a battery at its minimum SOC, its voltage) |
| **Time limited by power cap** | s | The time the battery's *Output Power Limit* held the car back |
| **Time limited by braking** | s | The time the car braked as hard as its tyres allow for the corner ahead |
| **Lap energy balance error** | % | How far the energy the laps took (speeding up, road load, slopes, brakes, gear and motor losses, other loads) is from the energy the sources gave, as a share of the latter. Above 0.5 % the energy and lap times are marked *not valid* |

## Not valid and the run status

The word at the top of the chart is the run's status:

- *success*: the car followed its target speed (or, in a test, ran to its
  end) and its parts stayed inside their data.
- *warning*: the run finished, but something is off; *Messages* and
  *Problems* say what.
- *cancelled*: you stopped it. *failed*: an error stopped it.

A summary value that the run's own checks rule out is marked *not valid*,
with the reason. *Consumption*, *Fuel consumption* and *CO₂ emissions*
are not valid when the car did not follow its cycle, when the battery
reached its minimum SOC (for *Consumption*) or the fuel tank ran empty
(for fuel and CO₂), when a part ran outside its data or above its maximum
speed for too long, and when the run stopped early (the figures cover
only part of the cycle). The README's
[run status section](../../../README.md#run-status-and-not-valid-figures)
lists every rule.

## Comparing with the baseline

When a run has a baseline (the run before it of the same case, or one you
pick), each value also shows its change and its change in %, such as
*+1.22 (+11.0 %) vs baseline*. A change smaller than the value's rounding
shows as *~ 0*. [How to compare two runs](../how-to/compare-two-runs.md).
