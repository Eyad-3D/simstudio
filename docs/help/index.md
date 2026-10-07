# LightSim help

LightSim simulates how much energy a vehicle uses, how far it goes and how
fast it accelerates. You build the car from parts on a diagram, choose the
speed it must follow over time (a drive cycle), press **Run** and read the
results.

New to LightSim? [Your first run](tutorials/first-run.md) opens an example
car from the *Start* page, runs it and reads the results. Then
[Build an electric car from scratch](tutorials/from-scratch.md) shows how
the parts fit together.

## What is in this help

- **Tutorials** take you through LightSim once, from start to end.
- **How-to guides** answer one question each, for example
  [how to pick a drive cycle](how-to/pick-a-drive-cycle.md) or
  [how to run a parameter sweep](how-to/parameter-sweep.md).
- **Examples** describe the cars that come with LightSim and the results
  to expect from them.
- **Reference** lists every [part in the library](reference/components/index.md)
  with its ports and parameters, the [drive cycles](reference/drive-cycles.md),
  the [keyboard shortcuts](reference/keyboard-shortcuts.md), the
  [Script API](reference/script-api.md), the
  [Python API](reference/python-api.md) and
  [command-line tool](reference/command-line.md) for running models without
  the app, the [file formats LightSim reads and writes](../FORMATS.md) and
  their [specification](../spec/README.md), and the engine's API.
- **Theory** explains how the solver works through a run, step by step.
- **Validation**, **Known issues**, **Release notes** and **Data sources**
  are the documents that come with every release.
- The [Glossary](glossary.md) explains the words LightSim uses.

## Before you rely on a number

LightSim is an early version. The physics of its parts are simplified,
nothing has been checked against measured vehicles yet, and some results
are known to be wrong. Use it to learn and to compare versions of one
model with each other. [Known issues](../KNOWN-LIMITS.md) lists what can be
wrong today and what to do about it; [Validation](../VALIDATION-STATUS.md)
says what the automatic tests check.

## Opening this help

- Press **F1** in LightSim. With a part selected on the diagram, F1 opens
  that part's page; otherwise it opens this one.
- Click the **?** button at the top right of the window, next to the text
  size buttons.
- In the desktop app, choose **Help → Documentation**.

The help opens in your web browser, so you can keep pages open in tabs and
bookmark them. LightSim serves it from your computer: it needs no internet
connection. To find a page, type any words from it into **Search the help**
at the top left.

This help is new in LightSim 0.3 and still a first draft. If a page does
not match what you see in the app, the app is right.
