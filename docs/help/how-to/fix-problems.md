# Find and fix problems in a model

LightSim checks a model by itself a moment after you open or change it:
these are its Data Checks. They look for parts that are not wired, signals
that are missing, values out of range and tables that do not fit together.
The *Problems* tab lists what they find, and what went wrong in the latest
run.

## Read the list

*Problems* is a tab in the panel under the diagram. A badge on the tab
counts the errors and warnings, and the status bar at the bottom of the
window counts the errors (click it to open *Problems*). Each row gives:

- the level: an **error** stops the run, a **warning** lets it run but
  says a result may be off, an **info** is a note (an error that starts
  with *Case '…'* or names a case stops only that case's runs);
- the parts it is about;
- what is wrong, and under it a line starting **How to fix:**;
- where it comes from: *Data Checks*, or the run (case and time) that
  reported it.

## Go to the part

Click a row, or move to it with Tab and press Enter. LightSim shows the
diagram, opens the sub-system the part is in, selects the part and zooms to
it. A check about several parts, such as two Vehicles in one model, selects
all of them. *Properties* then shows the part's values.

Fix the value or the wiring as the row says. The list follows your edits:
the row goes away once the problem is gone.

## Run anyway

**Run** refuses to start while there is an error, and *Messages* says how
many to fix first. Warnings do not stop a run.

To check again by hand, click **Run Data Checks** in the *Problems* tab, or
**Checks** on the *Simulations* tab.

## Good to know

- A row from a run finds its part by the name the message quotes; it stays
  until the next run, even once the model is fixed
  ([Known issues](../../KNOWN-LIMITS.md)).
- An all-clear says what was checked. It does not mean the results are
  right.
- *Messages*, next to *Problems*, is the log: every run, save and message,
  in order. It comes to the front by itself when something fails, such as
  a save.
