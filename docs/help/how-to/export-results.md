# Export results

LightSim keeps finished runs with their project, but only the newest 20 of
each and up to a disk budget. Export what you need to keep, or to work on
in a spreadsheet or another program.

## Channels as a CSV file

1. On the *Results* page, pick the run in the list at the top left.
2. Tick the channels you want in the list on the left.
3. Click **CSV** above the chart.

The file is named after the run's case, such as `lightsim-City Cycle.csv`.
Its first column is `t_s`, the time in s, unless you picked another x
axis in the chart view's **Time · auto** list: then it is `t_min` or
`t_h` for minutes or hours, or `distance_km` or `distance_m` followed by
`t_s` for the distance. Then
comes one column per ticked channel, headed with its name and unit, such
as `HV Battery Pack · SOC [%]`, and a row for every stored point of the
run. Spreadsheets open it directly; a
comma in a part's name is quoted. The file is UTF-8 with a byte-order
mark (a marker at its start), so Excel shows units such as N·m and °C as
written.

## The whole run for MATLAB or Python

Click **MATLAB** above the chart. LightSim saves the run as a `.mat` file:
every channel of every part, with its unit, and the run's details (project,
case, app version, status, summary, the parameters that differ from the
library's defaults). MATLAB opens it with `load`, Python with SciPy's
`scipy.io.loadmat`. What is inside, and how to run LightSim from a MATLAB
script: [Use LightSim results in MATLAB and Python](use-results-in-matlab-and-python.md).

## The chart as a picture

Click **PNG** above the chart. The picture shows the chart as it is on the
screen, zoomed in if you zoomed, with its legend under it, at twice its
size. It is not offered for the table view.

## A sweep's table

A parameter sweep is saved as a study at the bottom of the *Cases* tab.
The download button next to a study saves its table, a row per swept value
with every summary figure, as CSV
([more about sweeps](parameter-sweep.md)).

## Every parameter as a spreadsheet

**Export sheet** on the *Parameters* tab saves every parameter of every
part to one Excel workbook, and **Import sheet** reads it back:
[Edit parameters in a spreadsheet](edit-parameters-in-a-spreadsheet.md).

## The whole project

**Export** on the *Home* tab saves the project as a JSON file, and
**Import** opens one. Runs are not part of the file.
