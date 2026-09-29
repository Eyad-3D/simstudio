# Glossary

The words LightSim uses, in plain terms.

**Case**: one simulation job on a model: how long it runs, its step, its
kind and, if it has any, its overrides. A model can have several cases,
such as a city cycle and a highway cycle. The list next to **Run** picks the
active case.

**Acceleration case**: a case of kind *Acceleration*, such as a Formula
Student 75 m test. The Driver holds full throttle the whole run, with no
target speed, and the run is timed from the start line to the line its
*Distance* sets; the results are estimates. **Acceleration test** on the
*Simulations* tab adds one and runs it.

**Channel**: one quantity a run records over time, such as *Vehicle ·
Vehicle Speed* or *HV Battery Pack · SOC*. The *Results* page plots
channels.

**Component, part**: one thing in the model, such as a battery, a motor or
a wheel, taken from the library in the *Components* panel. On the diagram
each part is a box; its values are its parameters.

**Data Bus**: the links that carry signals between parts. They are listed,
and made, in the *Data Bus Connections* panel.

**Data Checks**: LightSim's checks of a model before it runs: wiring,
missing signals, values out of range, tables that do not fit together.
They run by themselves after every change; *Problems* lists what they find.

**Domain**: what a wire carries. *Electrical* wires carry power at a
voltage, *mechanical* wires a turning shaft. A wire joins two ports of the
same domain.

**Drive cycle**: the speed a car must follow over time, as in a
consumption test. LightSim has three standard ones (WLTC class 3b, UDDS,
HWFET) and takes profiles you type.

**Driver**: the part that works the accelerator and brakes to follow the
target speed. It compares the target with the car's speed (a PI controller:
it reacts to the gap and to how long the gap has lasted).

**Driving Task**: the part that gives the target speed, from a drive cycle
or a typed profile.

**Example**: a model that comes with LightSim. It opens as a copy, so you
can change it; **Save** keeps your copy as a project of your own.

**Full-load map**: the most torque a motor or engine can give at each speed
(and, for an E-Motor, each voltage).

**HWFET**: the EPA Highway Fuel Economy Test cycle, 765 s of highway
driving, used for US fuel-economy ratings.

**Lap case**: a case of kind *Lap*. The car drives the model's *Race
Track* (its layout and laps are the case's own) as fast as its tyres,
motors and battery allow, as a lap simulation does, and the motors and
battery then drive that speed for the energy. The case's *Duration*, *Step*
and *Pacing* do not apply; the results are estimates.

**Live run**: a run slowed down to a pace you can watch (*Pacing* 1× to
30× in the case settings), so you can change values while it runs.

**Loss map**: a table of the power a motor and its inverter lose, by speed
and torque. The engine's equivalent is its fuel map.

**Monitor**: a part that only shows values: link signals to its inputs and
watch them in the *Monitors* panel while a run goes.

**Not valid**: a mark on a summary figure that the run's checks rule out,
with the reason, for example *cycle not followed*.

**OCV**: open-circuit voltage, the voltage a battery shows with no current
flowing. It depends on the state of charge.

**Override**: a parameter value that applies to one case only. The model's
own value stays as it is for the other cases.

**Performance case**: a case of kind *Performance*, for 0-100 km/h and top
speed. The Driver holds full throttle until the car reaches the target
speed, and the run reports the time it took.

**Port, pin**: a connection point on a part: a dot on its edge for wires,
or a signal input or output in *Data Bus Connections*.

**Problems**: the tab that lists what the Data Checks found and what went
wrong in the latest run, each with a line on how to fix it.

**Profile**: a list of `time:speed` pairs typed into a Driving Task, such
as `0:0; 30:50; 120:50`, in s and km/h. The speed changes in a straight
line between the points.

**Recuperation**: braking with the motor running as a generator, which
charges the battery instead of heating the brakes.

**Run**: one simulation of a case. Its status is *success*, *warning*,
*cancelled* or *failed*.

**Sample Time**: how often a Script, PID or Lookup block runs, in s. At 0 it
runs at every solver step.

**Script**: a part that runs a Python function of your own at every step
(see the [Script API](reference/script-api.md)).

**SOC**: state of charge, how full a battery is, from 0 to 100 %.

**Solver step**: the time step the physics is worked out at, at most 10 ms.
The case's *Step* is how often results are stored, not the solver step.

**Start page**: the page LightSim opens on, and that **New** shows: go on
with the open project, start from an example or a blank project, or reopen
a recent one. Tick *Skip this page* to open your last project instead.

**Sub-system**: a box on the diagram that holds parts of its own, to keep a
large model tidy. Double-click it to go inside.

**Summary**: a run's totals on the *Results* page: the headline numbers
above the chart (consumption, distance, final SOC, energy; a test's time)
and the full table under it, *All summary values*.

**Sweep, study**: runs of one case over a range of values of one
parameter. The study is the table of their results, saved with the
project.

**Target speed**: the speed the Driving Task asks for at each moment.

**Topology, diagram**: the drawing of the model's parts and wires in the
*Topology* panel.

**UDDS**: the EPA Urban Dynamometer Driving Schedule, 1,369 s of city
driving, used for US city fuel-economy ratings.

**Vehicle Load Share**: the part of the car's weight a wheel carries, in %.
The shares of all wheels should add up to 100 %.

**WLTC**: the Worldwide harmonised Light vehicles Test Cycle, used for
consumption and range ratings in the EU and elsewhere. Class 3b is the one
for most passenger cars: 1,800 s in four phases, Low to Extra High.
