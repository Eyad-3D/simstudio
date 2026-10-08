# Pick a drive cycle

A drive cycle is the speed a car must follow over time, as in a
consumption test. A **Driving Task** part gives it to the Driver. It can
drive one of the standard cycles that come with LightSim, or a profile of
points you type. The [drive cycle reference](../reference/drive-cycles.md)
lists the standard ones.

## For the whole model

1. Click the Driving Task on the diagram (in the electric and hybrid
   examples, *Vehicle Task*).
2. In *Properties*, open the **Drive Cycle** list under the parameters
   table and choose a cycle, for example *WLTC class 3b*.

Under the list, a sketch shows the speed over time with the cycle's phases
marked, and a line gives its duration, distance and top speed. A message in
*Messages* says which cases now run the cycle's full length: choosing a
cycle sets the *Duration* of the cases of kind *Cycle* that drive it, when
the model has one Driving Task.

To go back to your own points, choose **Custom profile (typed points)**:
the **Profile** button comes back, which opens the points in a table
(**Edit…**).

## For one case only

A case can drive its own cycle while the others keep the model's.

1. Open the *Cases* tab on the right and pick the case in its **Case** list.
2. Under *Parameter overrides*, choose the Driving Task in the
   **Element…** list and **Drive Cycle** in the next list, then click
   **Add override**.
3. Choose the cycle in the new row. The case's *Duration* changes to the
   cycle's length, if the case is of kind *Cycle*.

To undo it, click the × at the end of the row.

## Add a Driving Task that drives a cycle

Type the cycle's name or id, such as *udds* or *wltc*, in the *Components*
panel's **Search components…**. The cycles are listed under *Drive cycles*,
below the parts that match (*drive cycle* lists them all). Click one, or
press Enter on it: LightSim adds a Driving Task set to that cycle. Link its
*Target Speed* to the Driver's in *Data Bus Connections*
([how](wire-control-signals.md)).

## Drive a speed against distance

A lap, a track map, a logger's lap or a truck route gives the speed
against the distance driven, not against time. The Driving Task can follow
that too:

1. Click the Driving Task and, in *Properties*, set **Profile Axis** to
   *distance*. Leave **Drive Cycle** on *Custom profile*: the standard
   cycles are speeds against time.
2. Open the **Profile** (**Edit…**) and type or paste 'distance:speed'
   points, in m and km/h: the table's first column now says *Distance
   (m)*. Start at the speed the car sets off towards (for example 5 km/h,
   not 0: at 0 km/h the car stays put).
3. To drive it lap after lap, tick **Repeat Profile**. One lap is the
   profile's first point to its last.
4. In the *Cases* tab, type the number of **Laps** for the case, and give
   it a *Duration* long enough to drive them: it is now the time limit.

The Driver gets the target speed at the distance the car has driven, so a
heavier or weaker car slows for a corner at the same place, only later in
time. The run is judged against distance. A point of 0 km/h stops the car
there for good ([Known issues](../../KNOWN-LIMITS.md)).
## A route with hills

The long-haul truck route carries the road's grade as well as its speed.
To drive its hills, add a **Road Profile** part, choose the same cycle in
its **Grade From Cycle** list, and link its *Road Grade* to the Vehicle's
in *Data Bus Connections*. The grade is placed along the distance the cycle
covers, so each hill stays where it is when the car falls behind the
cycle. Only cycles marked *with grade* are offered.

## Which wins

- A case's own cycle wins over everything else.
- A case's own *Profile* override, without a cycle, wins over the part's
  cycle.
- Otherwise the part's cycle wins over its typed profile.

The *Scale* and *Repeat Profile* settings of the Driving Task apply to a
cycle too. A cycle scaled away from 100 %, repeated past its end, or read
against distance is no longer the standard cycle: the run says so, and
gives no per-phase figures.

## Good to know

- A case of kind *Performance* runs until the car reaches its target, so it
  keeps its own *Duration*. A case of kind *Acceleration* or *Lap* follows
  no target speed at all: it ignores the Driving Task, its cycle and its
  profile.
- LightSim has 27 standard cycles. Under the sketch, a line names the
  document each comes from (an EU regulation, EPA's schedule files or
  FASTSim's copy) and why LightSim may ship it. Cycle files of your own are
  not in the list yet: type or paste their points into the *Profile*
  instead ([Known issues](../../KNOWN-LIMITS.md)).
- Which WLTC? Most cars are class 3b. The class follows from the car's
  rated power per kilogram: class 3 above 34 W/kg (3a when its top speed is
  below 120 km/h), class 2 from 22 to 34 W/kg, class 1 at 22 W/kg or less.
- A project that names a cycle, opened in LightSim 0.2.0, drives the typed
  profile instead, with no warning.
