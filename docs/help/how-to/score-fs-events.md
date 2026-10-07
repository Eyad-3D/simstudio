# Score the Formula Student dynamic events

LightSim can run the four dynamic events of an electric Formula Student
car and estimate their points, so you can judge a design change in
competition points instead of seconds. The scoring follows Formula Student
Rules 2026 v1.1 (FSG). FSUK and FSAE score differently: check the current
season's rules.

## Run the four events

1. Open your car, or the example *FS Electric (generic)* from
   **Home → Open**. The car needs a *Race Track* (Driver & Signals).
2. On the *Simulations* tab of the ribbon, click **FS events**.
3. LightSim marks a case for each event, or adds one where none fits:
   - *Acceleration*: an Acceleration case over 75 m, staged 0.30 m behind
     the start line (rules D 5.1.1, D 5.2.3);
   - *Skidpad*: a Lap case on the Skidpad layout, 2 laps; the time is the
     mean of the right and the left circle of the last lap (D 4.2.2);
   - *Autocross*: a Lap case, 1 lap of the Autocross layout (D 6.2.1);
   - *Endurance*: a Lap case of about 22 km (D 7.1.3), 22 laps of the
     Autocross layout. The car stops for the driver change at half
     distance and starts again from rest; the lap it restarts on is left
     out of the event time, as the rules leave out the driver change lap
     (D 7.2.5).
4. The four cases run one after the other. The *Cases* tab on the right
   then shows **Formula Student points**: each event's time and points,
   the efficiency points and the total.

Marking the cases is one step: **Undo** takes it back.

## Give the points a reference

Points depend on the other teams. Each event's points are worked out
against the fastest team's time, so the table shows no points until you
give it one:

1. In the *Cases* tab, pick the event's case in the **Case** list.
2. Type the fastest team's time in **Reference time (s)**, for example
   from last season's results.
3. For the endurance, also type the most efficient team's energy in
   **Reference energy (kWh)**, and its time in **Its time (s)** if it was
   not the fastest team.
4. Click **Run events** above the table, or **Run case** for one case.

A time faster than the reference gets the full points; one slower than
the rules' Tmax (1.35 to 1.7 times the reference) gets the minimum.

## Read the rule checks

Each event's run also checks the electric car's rules:

- **Rule check: power (EV 2.2.1)**: at most 80 kW out of the accumulator.
  With the battery's *Formula Student Electric* preset this is the 500 ms
  average the rules use; without it, the highest power over a solver step,
  which is stricter.
- **Rule check: current (EV 2.2.2)**: at most 500 A, the highest current
  over a solver step.
- **Rule check: voltage (EV 4.1.1)**: at most 600 V, at full charge or at
  the terminals while recuperating.
- **Endurance finished on its energy**: the accumulator did not run down
  to its minimum charge before the last lap.

A failed check marks the row *fail*, warns in *Messages* and scores the
event 0 points: the rules disqualify a run that breaks them (D 10.4.2). An
endurance the car does not finish scores no endurance or efficiency points.

## Good to know

- The endurance energy counts regenerated energy at 90 % (D 7.9.5), and
  the efficiency factor is the driving time squared times that energy
  (D 9.4.2).
- The 3 min driver change is not driven: the tractive system is off then
  (D 7.5.5), so it uses no energy. Battery recovery during the stop is
  not modelled.
- Lap mode is an estimate with an ideal driver: lap times are usually
  optimistic. See [Known issues](../../KNOWN-LIMITS.md).
- You can mark any Lap or Acceleration case yourself: pick the event in
  **FS event** in its case settings.
- A sweep of an event case gives the points against the swept value in its
  study table.
