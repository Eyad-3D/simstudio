# Compare two runs

After you change the model and run it again, the *Results* page compares
the new run with the run before it: each summary value says how much it
changed, a list says what you changed, and the new run is named after it.

## See what an edit changed

1. Run a case, for example the Battery Electric Car's *City Cycle*.
2. Click the **Home** tab, then the **Vehicle** on the diagram.
3. In *Properties*, set *Vehicle Mass* to 2300 and press Enter.
4. Press **Run**.

The *Results* page opens on the new run, named *Vehicle Mass 2,300 kg*
after what changed. On the left, **Baseline** reads *Previous run of this
case*, and *What changed* lists *Vehicle · Vehicle Mass 1,927 → 2,300 kg*.
The earlier run is drawn faint and dashed with the new lines, and each
headline number above the chart says how much it changed: *Consumption*
reads 12.34 kWh/100 km, *+1.22 (+11.0 %) vs baseline*.

Click a line in *What changed* to show that part selected on the diagram.

## Read the changes

- **All summary values** under the chart has four columns for each value:
  *This run*, *Baseline*, *Change* and *% change*.
- A change of 1 % or more is in bold.
- *~ 0* means the change is no larger than the rounding the value is
  stored with: treat it as no change (see
  [Known issues](../../KNOWN-LIMITS.md)). In the example, *Distance
  driven* reads ~ 0.
- A dash means there is nothing to compare: the baseline has no such
  value, or, for *% change*, its value is 0.
- *% change* is a share of the baseline's value, also for a value in %:
  the final SOC's 88.76 → 88.63 % is −0.15 %.

## Pick another baseline

1. Open the **Baseline** list on the left, under the run list.
2. Choose a run, or *None* to compare with nothing.

The case keeps your choice for its next runs. *Previous run of this case*
is always the run of the same case before the one shown, left out when it
was stopped or failed.

Untick **Draw the baseline faint on the chart** to hide the baseline's
lines; the numbers still compare with it. A run you tick under *Overlay*
is drawn in full colour instead.

## Name a run and keep a note

A run is named after what changed since the previous run of its case, for
example *Vehicle Mass 2,300 kg*, or *Vehicle Mass 2,300 kg +1 more* when
two things changed. A run with nothing changed keeps its clock time, and a
sweep's runs are named by their swept value.

1. Click **ⓘ** (*Run info*) next to the run list.
2. Type a name in **Name** and press Enter.
3. Type anything you want to remember about the run in **Note**.

The name and the note are stored with the run, so they are there when you
open the project again. Empty the name to go back to the clock time.

## What the list covers

*What changed* compares the copies of the model the two runs kept: each
parameter as the run used it (the case's own value, if it sets one), maps,
tables and scripts (as *edited*), parts added or removed, wires and Data
Bus links, the case's settings, and the parameters edited while either run
was going. Moving or renaming a part changes no result, so it is left out.
For a run stored before runs kept a copy of their model, the list says so;
its numbers are still compared.

## See what changed since the results on screen

When you change the model after a run, the run shown in *Results* no
longer matches it:

- each part you edited or added gets a small dot at its top left on the
  diagram, a wire you added gets a dot along it, and an edited case gets a
  dot next to the case list in the *Cases* tab;
- *Results* says *These results are from before 3 changes to the model*.
  Click **Show changes** for the list (part, parameter, old → new, with
  units; click a part to show it on the diagram), or **Re-run** to run the
  case again.

The marks clear when the new run finishes. Picking an older run in the run
list compares the model with that run instead.
