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
4. Set **From**, **to** and **in** … **steps** (at most 16). The values the
   sweep will run show under the fields.
5. Click **Run sweep**. The runs go one after the other; **Stop** ends the
   sweep.

The part's own value does not change: each run applies its value only while
it runs, as a case override does.

## Read the result

- On the *Results* page, click **Sweep** above the chart. It plots a
  summary figure, such as *Consumption*, against the swept value; pick
  another figure in the list next to it. A point whose run stopped or
  failed is left out, or drawn hollow with **Show incomplete** ticked.
- To compare the runs over time, click **Chart**, then **Overlay family**
  in the *Overlay* box on the left: every run of the sweep is drawn.
- The sweep is also saved with the project as a study, under *Saved
  studies* at the bottom of the *Cases* tab: a table with a row per value,
  the run's status and every summary figure. Click the study's name to
  show or hide its table; the download button next to it saves the table
  as CSV. A study stays after its runs leave the *Results* history, and is
  kept on disk once you save the project.

## Good to know

- A sweep of a Driving Task's *Scale* or a battery's *Initial SOC* can
  show the car failing to follow its cycle: check the status of each row.
- Results keeps the 20 newest runs; a large sweep can push older runs out
  of the list. The study table keeps every point.
