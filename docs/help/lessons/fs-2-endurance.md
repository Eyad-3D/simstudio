# Formula Student 2: endurance energy

The second Formula Student lesson. You run the endurance event, about
22 km, and read how much energy the car takes from its accumulator, how
hard it works the cells and how long it runs at its power limit. It takes
about 15 minutes. Do
[Formula Student 1](fs-1-acceleration.md) first.

## 1. Run the endurance case

Open **FS Electric (generic)** from the *Start* page again, or go on with
your copy from lesson 1. In the list next to **Run**, pick *Endurance
energy* and press **Run**. The run takes about 10 to 15 s; click
**Show results** in the notice it ends with.

This is a *Lap* case: the car drives 23 laps of LightSim's own 979 m
Autocross layout, as fast as its tyres, motor and accumulator allow at
each point (a lap simulation). The case sets the accumulator's *Output
Power Limit* to 30 kW for itself, a typical endurance setting: teams turn
the power down to make the energy last. You can see this override in the
*Cases* tab on the right, under *Parameter overrides*.

**Check:** the run ends as *success* after 1,428.53 s (*Total time*),
22.515 km driven, with *Accumulator — final SOC* 25.02 %.

## 2. Read the energy

Open **All summary values** under the chart.

- *Accumulator — energy delivered* is the energy that left the
  accumulator; *energy recuperated* is what braking put back.
- The difference is the net energy the event took. It is also *Energy per
  lap* times the number of laps.

**Check:** 6.5 kWh delivered and 1.167 kWh recuperated: 5.33 kWh net,
and *Energy per lap* 0.2319 kWh. Braking gives back 18 % of the energy
drawn.

The 14 teams scored for efficiency at FS Czech Republic 2025 used 3.19 to
6.15 kWh in the endurance (median 5.25 kWh), so this car sits near the
middle. Real endurance energy is higher than a lap simulation's: the
driver is not ideal and there is traffic and a driver change.

## 3. Read how hard the cells work

The accumulator's cells heat up with the current through them. Two
numbers tell you how hard the event works them:

- *RMS battery power*: the root mean square of the accumulator's power, a
  steady power that would heat the cells as much as the real, changing
  one. Use it to check your cells' continuous rating and cooling.
- *Accumulator — time held at the output power limit*: how long the car
  ran at its 30 kW limit.

**Check:** *RMS battery power* reads 22.853 kW, and the accumulator was
held at its limit for 703.27 s, about half the event. *Accumulator —
minimum pack voltage* is 472.94 V.

Tick *Accumulator · Discharge Power* and *Accumulator · Terminal Voltage*
to see it over the event: the voltage falls as the charge goes down and
dips each time the car pulls out of a corner.

## 4. See what limits the lap

The summary also says, for each limit, how long it held the car back over
the event: *Time limited by cornering grip*, *traction grip*, *motor*,
*battery*, *power cap* and *braking*.

**Check:** *Time limited by power cap* reads 703.266 s and *Time limited
by cornering grip* 434.603 s. At 30 kW the car spends more time held back
by its power limit than by its tyres in the corners.

## 5. With your team's own speed trace

LightSim cannot read a lap simulator's or logger's file yet (roadmap
STD-35). Until then, two ways get close:

- Change the Race Track to your event: its *Layout* is set per case in
  the *Cases* tab; *Custom* reads your own curvature table.
- Drive your logged speed as a drive cycle: add a **Driving Task** from
  the library, give it a *Profile* of `time:speed` pairs (paste them from
  a spreadsheet into the profile table), link its *Target Speed* to the
  Driver's *Target Speed* in *Data Bus Connections*, and add a case of
  kind *Cycle* as long as your trace. The
  [how-to](../how-to/pick-a-drive-cycle.md) says more about profiles. A speed trace driven
  as a cycle gives somewhat different energy from lap mode;
  [Known issues](../../KNOWN-LIMITS.md) says why.

## What you learnt

- A *Lap* case drives the endurance on a Race Track; its *Laps* and power
  limit are the case's own.
- Net energy is energy delivered minus energy recuperated, or *Energy per
  lap* times the laps.
- *RMS battery power* and the time at the power limit tell you how hard
  the cells work.

Next: [Formula Student 3: size the accumulator](fs-3-accumulator.md).
