# Measure between two times

Two measurement cursors, A and B, are lines on the *Results* chart. A
table under the chart reads each plotted signal at A and at B, and what it
did between them: for example the energy the battery gave from 150 to
300 s, or the time the car took from 0 to 100 km/h.

## Read the energy between two times

1. Run a case. The *Results* page opens with the battery's
   **Discharge Power** ticked on the left.
2. Click **Cursors** above the chart, or press C. Lines A and B appear
   on the chart, with their time fields and a table under it.
3. Click the **A** field under the chart, type `150` and press Tab.
4. Type `300` in the **B** field and press Enter.
5. Read the **Integral** column in the *Discharge Power* row.

On the Battery Electric Car's *City Cycle*, it reads 0.4236 kWh. Next to
the fields, *Δt* is the time between A and B, here 150 s.

## Time to reach a speed

1. Click **Cursors**, if the cursors are off.
2. In **Time to reach**, pick the signal, for example *Vehicle Speed*.
   The list offers the ticked signals.
3. Type `0` in **from** and `100` in **to**.
4. Click **Place A, B**.

A goes where the signal first reaches the first value, and B where it
first reaches the second one after that, so *Δt* is the time it took. On
the *City Cycle*, 0 to 50 km/h takes 31 s. A signal that never reaches a
value says so next to the button, and the cursors stay where they were.

## What the table shows

A row for each plotted signal, with its unit: when runs are overlaid, a
row for each run too, all read at the same times. On the distance axis,
the other runs are read where the lines cross them: where each had driven
as far as the primary run at A and at B.

| Column | What it is |
|---|---|
| A, B | The value at each cursor |
| B − A | How much it changed from A to B |
| Min, Max | The lowest and the highest value between A and B |
| Mean | The average between A and B, weighted by time |
| RMS | The root mean square (the square root of the mean of the squares), weighted by time: a typical size for a signal that swings up and down, such as a current that charges and discharges |
| Integral | The sum over time of a rate: kWh from kW, Ah from A, m from km/h, kg from kg/h and revolutions from 1/min. Other units show — |

A dash also means the signal has no stored point there, for example an
overlaid run that ended earlier.

## Moving the cursors

- The cursors always sit on stored points: a typed time goes to the
  nearest one.
- In a time field, the up and down arrows move the cursor one stored
  point, Page Up and Page Down ten.
- On the chart, drag a line: near one, the pointer shows ↔. A drag
  anywhere else still zooms, and a double-click anywhere else shows the
  whole run again.
- The lines follow the zoom. When the chart runs against the distance
  driven, they stay at their times: during a stop, several points share
  one distance, so the line stands still while the time changes.
- The **PNG** picture shows the lines.

Each run keeps its own cursors. They stay while you switch between the
chart, table and X-Y views, overlay other runs or leave the *Results*
page, until LightSim closes. They are not saved with the project.

The integral adds up the stored points (the trapezoid rule), so it can
differ a little from the summary's energy, which adds up every solver
step: see [Known issues](../../KNOWN-LIMITS.md).
