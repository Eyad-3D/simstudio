# See where the energy went

Every run of a case gets an *Energy* view in *Results*: a Sankey chart (a
flow chart whose bands are as wide as the energy they carry) from the
battery or the fuel to where the energy ended up, and a table of the energy
that went into, came out of and was lost in each part.

## Read the Energy view

1. Run a case. In the *Cases* tab on the right, **Energy report** is ticked
   unless you untick it.
2. In *Results*, click **Energy** above the chart.
3. Read the chart from left to right: the sources (the battery, the fuel or
   the hydrogen), the energy in, where it went in groups, and each place on
   its own on the right.
4. Point at a band or a box to see its energy and its share of the sources.
   Click a box on the right that names a part to show the part on the
   diagram.
5. Scroll down to the table: each part's **In**, **Out**, **Lost** and
   **Stored change**, in kWh, and its loss as a share of the sources. Every
   part keeps its own books, so each gear, differential, clutch, brake and
   wheel has a row of its own, and In − Out − Lost − Stored change is 0 on
   every row. *Rotating parts* is a driveline's spinning parts together
   (motor rotors, gears, shafts and wheels): the energy in their speed.

On the Battery Electric Car's *City Cycle*, the battery gives 0.887 kWh:
air drag takes 0.242 kWh (27.3 %), rolling resistance 0.421 kWh (47.5 %),
the E-Motor loses 0.086 kWh (9.7 %), the Final Drive 0.017 kWh (1.9 %),
each front tyre's slip 0.002 kWh, the Power Consumer uses 0.042 kWh (4.7 %)
and 0.07 kWh (7.9 %) is charged back into the battery by braking.

## What the groups mean

| Group | What is in it |
|---|---|
| Driving: air and rolling | The work done against air drag and the tyres' rolling resistance |
| Kept as speed or height | The car's speed at the end of the run (kinetic energy), the height it gained, and the spinning parts' speed |
| Friction brakes | Energy the brakes turned into heat |
| Losses in parts | Each E-Motor, engine, fuel cell and DC-DC converter, the battery's internal resistance, each gear and differential, each clutch's slip and each tyre's slip |
| Used by loads | The Power Consumers (12 V loads) and the Climate Control (heating and air-conditioning) |
| Charged back | Energy braking put back into the battery |
| Not accounted for | The sources less everything above: where the parts' books together do not close |

A part that gave energy back over the run, such as a car that ends slower
than it started or lower than it began, is drawn on the left as a source.

## Check that the numbers add up

Above the chart, **Not accounted for (energy balance residual)** says how
much of the sources' energy the books cannot place, and the table's last
row repeats it. Each part's own books close, and each part books what
the solver's step did to it, so the energy out of one part is the energy
into the next and what is left is rounding: about 1e-11 % on the
examples. It is the summary's *Energy balance residual*, there as a share
of the energy the sources gave up and with the sign the other way round
(energy the books cannot place is + there, − here). Anything larger means
the books missed something: report it. Beside it is the run's
**Electrical energy balance error**, the same figure as in the summary.
Above 1 % the number turns orange: check the run's messages, and see
[Known issues](../../KNOWN-LIMITS.md).

In a lap case the lap's own energy pass books the gears and the friction
brakes, so they are in the Vehicle's row and named in the chart, not given
part by part.

## Show the energy on the diagram

1. On the *Home* tab, click the lightning button in the toolbar above the
   diagram.
2. Each part shows the energy it lost, and what went in and came out, in
   kWh, under its name.
3. A chart at the top right lists the parts by the energy they lost.
   Drag **Hide below** to hide the parts under that share of the sources.
4. Click a part's bar to select it on the diagram.

The labels and the chart show the run picked in *Results*. Click the
lightning button again, or the × on the chart, to hide them.

## Save the numbers

- **CSV** above the chart saves the table and the chart's bands.
- **SVG** saves the chart as a picture you can scale.

## Turn the report off

Untick **Energy report** in the *Cases* tab. The case's next runs have no
Energy view; their other results do not change.
