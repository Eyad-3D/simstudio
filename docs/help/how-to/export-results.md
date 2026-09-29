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
comma in a part's name is quoted.

## The chart as a picture

Click **PNG** above the chart. The picture shows the chart as it is on the
screen, zoomed in if you zoomed, with its legend under it, at twice its
size. It is not offered for the table view.

## A sweep's table

A parameter sweep is saved as a study at the bottom of the *Cases* tab.
The download button next to a study saves its table, a row per swept value
with every summary figure, as CSV
([more about sweeps](parameter-sweep.md)).

## The whole project

**Export** on the *Home* tab saves the project as a JSON file, and
**Import** opens one. Runs are not part of the file.
