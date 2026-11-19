# Run a parameter sweep

A sweep runs one case several times, each time with a different value of
one parameter, and plots a result against that value: for example the
energy an electric car uses against its mass.

## Set it up and run it

1. Pick the case to sweep in the list next to **Run** at the top right.
2. Open the *Cases* tab on the right. *Parameter sweep* is under the case
   settings and overrides; scroll down to it.
3. Choose the part in the **Element…** list and the parameter in the next
   list, for example *Vehicle* and *Vehicle Mass (kg)*. Only numeric
   parameters can be swept.
4. Choose how to give the values, in the list under them:
   - **Even steps**: set **From**, **to** and **in** … **steps**. Each
     value is the same amount above the one before.
   - **Log steps**: the same fields, but each value is the same factor
     above the one before (10, 100, 1000 in 3 steps). Use it for a value
     that spans several powers of ten. Both ends must be above 0.
   - **List of values**: type the values, separated by commas or spaces,
     with a point for decimals: `1200, 1350, 1500`. They run in the order
     you type them; a value typed twice runs once.

   A sweep has at most 200 values. The values it will run show under the
   fields; a value outside the parameter's limits is named in red there.
5. Click **Run sweep**. The runs go side by side, one per processor core
   (all but one, and fewer when memory is short); *Messages* says how many
   at a time. While they run, the status bar and the line under
   **Run sweep** say how many points are done and about how long is left,
   worked out from the points that have ended. **Stop** stops the runs
   going and leaves the rest not run.

The part's own value does not change: each run applies its value only while
it runs, as a case override does.

The page stays as it is while the sweep runs and when it ends. A notice at
the bottom right says how it ended, for example *Sweep finished: 5 of 5
points complete*, with **Show results** and **Study charts**. Tick
**Always open Results** in it to have the *Results* page open by itself
after each run and sweep.

## Read the result

- **Study charts** in the notice, or the **Study** button above the chart
  on the *Results* page, shows a chart for each result against the swept
  value, from the saved study. Pick the study in the list at the top and
  the results to chart under **Figures**; the view opens on the
  headline numbers. A value that the run's checks rule *not valid* is
  drawn hollow; a point whose run stopped, failed or did not run is left
  out, and the line above the charts says how many. Log-spaced values are
  drawn on a log axis.
- **Sweep** above the chart plots one summary figure against the swept
  value from the runs in the *Results* history, to begin with the run's
  first headline number; pick another figure in the list next to it. A
  point whose run stopped or failed is left out, or drawn hollow with
  **Show incomplete** ticked.
- To compare the runs over time, click **Chart**, then **Overlay family**
  in the *Overlay* box on the left: every run of the sweep is drawn.
- The sweep is also kept as a study, under *Saved studies* at the bottom
  of the *Cases* tab: a table with a row per value, the run's status and
  every summary figure. Click the study's name to show or hide its table;
  the chart button next to it opens its charts on the *Results* page, and
  the download button saves the table as CSV. A study stays after its runs
  leave the *Results* history. It is kept on disk with the project's runs,
  not in the project file, so a sweep does not change the project and
  needs no **Save**.

## Good to know

- A sweep of a Driving Task's *Scale* or a battery's *Initial SOC* can
  show the car failing to follow its cycle: check the status of each row.
- Results keeps the 20 newest runs; a large sweep can push older runs out
  of the list. The study table and the Study view keep every point, and
  every run stays on disk within the project's disk budget for stored runs.
- The study's line under its name says how many runs went at a time, how
  long the sweep took and how much faster that was than one run after
  another.
- The time left is a guess from the pace so far: points that take longer
  than the first ones make it grow.
- The runs of a sweep show only when it ends: a sweep does not draw its
  runs live.
