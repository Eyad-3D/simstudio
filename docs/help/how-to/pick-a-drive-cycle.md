# Pick a drive cycle

A drive cycle is the speed a car must follow over time, as in a
consumption test. A **Driving Task** part gives it to the Driver. It can
drive one of the standard cycles that come with LightSim, a cycle of your
own imported from a file, or a profile of points you type. The
[drive cycle reference](../reference/drive-cycles.md) lists the standard
ones.

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

## Use a cycle of your own

A cycle you measured or were given (a logged drive, a company test cycle,
a lap) can join the list beside the standard ones. LightSim keeps it in the
project file, so it goes wherever the project goes.

1. Save it as a CSV or Excel (`.xlsx`) file with a header row: a time
   column (s) or a distance column (m), and a speed column, a road grade
   column (%), or both. Units in the headers are read and converted:
   `Time [s]`, `Distance [km]`, `Speed (m/s)`, `Speed [mph]`,
   `Grade [%]`; a units row under the header works too.
2. Click the Driving Task and, in *Properties*, open the **Drive Cycle**
   list and choose **Import a cycle from a file…** at its end (it is in
   the case's list in the *Cases* tab, and in a Road Profile's list,
   too).
3. Click **Choose a file…** (or drop the file on the window). LightSim
   shows what it read: the columns it took for each role, their units, a
   sketch and the cycle's length. Change a column, a unit or **Points
   against** (*Time* or *Distance*) if it guessed wrong; a row it cannot
   read is named with its number.
4. Give the cycle a name and click **Add to project**.

The cycle is now chosen for that Driving Task, under *This project* in the
list, and every Drive Cycle list of the project offers it. Choosing it
sets the cases' *Duration* as a standard cycle does. Save the project to
keep it. **Undo** takes an import back out. To rename or remove the
project's cycles, choose **This project's cycles…** at the end of the list:
a cycle a part or a case still uses cannot be removed, and the list says
which.

A cycle of your own is not a standard one: the run says so, and its
figures compare only with runs on the same cycle.

### A cycle against distance

A file whose first column is a distance (a logged lap, a lap simulator's
output, a route) becomes a cycle against distance. A Driving Task reads it
against the distance the car has driven, whatever its **Profile Axis**
says, as described [below](#drive-a-speed-against-distance): set the
case's **Laps** to 1 to end the run at the cycle's end. Choosing it sets
the case's *Duration* to the time its own speeds take, which a car that
lags needs a little more of. A cycle against distance may have a grade
only (a road's hills): a Road Profile can take it, a Driving Task cannot.

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
   cycles are speeds against time. (A file of yours against distance can
   be [imported as a cycle](#a-cycle-against-distance) instead of typed.)
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

The long-haul truck route carries the road's grade as well as its speed,
and so may a cycle of your own. To drive its hills, add a **Road Profile**
part, choose the same cycle in its **Grade From Cycle** list, and link its
*Road Grade* to the Vehicle's in *Data Bus Connections*. The grade of a
cycle against time is placed along the distance the cycle covers, so each
hill stays where it is when the car falls behind the cycle; a cycle against
distance gives it where it says. Only cycles marked *with grade* (or
*grade only*) are offered.

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
  FASTSim's copy) and why LightSim may ship it. For a cycle of your own it
  names the file it was imported from.
- A cycle of your own has no phases, and a project that uses one, opened
  in a LightSim from before cycles of your own, cannot run until the part
  is set to another cycle: that version says it does not include the
  cycle, and keeps the cycle in the file when it saves.
- Which WLTC? Most cars are class 3b. The class follows from the car's
  rated power per kilogram: class 3 above 34 W/kg (3a when its top speed is
  below 120 km/h), class 2 from 22 to 34 W/kg, class 1 at 22 W/kg or less.
- A project that names a cycle, opened in LightSim 0.2.0, drives the typed
  profile instead, with no warning.
