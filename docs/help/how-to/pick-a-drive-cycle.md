# Pick a drive cycle

A drive cycle is the speed a car must follow over time, as in a
consumption test. A **Driving Task** part gives it to the Driver. It can
drive one of the standard cycles that come with LightSim, or a profile of
points you type. The [drive cycle reference](../reference/drive-cycles.md)
lists the standard ones.

## For the whole model

1. Click the Driving Task on the diagram (in the examples, *Vehicle Task*).
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

## Which wins

- A case's own cycle wins over everything else.
- A case's own *Profile* override, without a cycle, wins over the part's
  cycle.
- Otherwise the part's cycle wins over its typed profile.

The *Scale* and *Repeat Profile* settings of the Driving Task apply to a
cycle too.

## Good to know

- A case of kind *Performance* runs until the car reaches its target, so it
  keeps its own *Duration*. A case of kind *Acceleration* or *Lap* follows
  no target speed at all: it ignores the Driving Task, its cycle and its
  profile.
- LightSim has three standard cycles today; others, and cycle files of
  your own, are not in the list yet: type or paste their points into the
  *Profile* instead ([Known issues](../../KNOWN-LIMITS.md)).
- A project that names a cycle, opened in LightSim 0.2.0, drives the typed
  profile instead, with no warning.
