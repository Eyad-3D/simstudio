# Glossary

The words LightSim uses, in plain terms.

**Acceleration case**: a case of kind *Acceleration*, such as a Formula
Student 75 m test. The Driver holds full throttle the whole run, with no
target speed, and the run is timed from the start line to the line its
*Distance* sets; the results are estimates. **Acceleration test** on the
*Simulations* tab adds one and runs it.

**Accumulator**: the Formula Student word for the car's high-voltage
battery. In LightSim it is an **HV Battery Pack**; the FS example names
it *Accumulator*.

**Ambient**: the part that sets the air's temperature and pressure, and
with them the air density that air drag uses. Without one, LightSim uses
20 °C and 101.325 kPa.

**Axle**: a pair of wheels, front or rear. A Wheel's *Axle* says which
one it is on, for load transfer and for lap cases.

**Baseline**: the run the *Results* page compares a run with: the previous
run of its case, unless you pick another in the **Baseline** list. Each
summary value then says how much it changed, and the baseline's lines are
drawn faint with the run's.

**Case**: one simulation job on a model: how long it runs, its step, its
kind and, if it has any, its overrides. A model can have several cases,
such as a city cycle and a highway cycle. The list next to **Run** picks the
active case.

**Cd, drag coefficient**: how easily air flows round the car's shape, a
number with no unit (about 0.25 to 0.35 for a passenger car). Air drag
grows with Cd times the frontal area.

**CdA, CzA**: the drag area and the downforce area, in m²: the drag or
downforce coefficient times the frontal area. Formula Student teams often
quote these instead of the coefficients.

**Channel**: one quantity a run records over time, such as *Vehicle ·
Vehicle Speed* or *HV Battery Pack · SOC*. The *Results* page plots
channels.

**Charge Capacity**: how much charge a battery holds from full to empty,
in Ah (amp-hours). For a pack, a cell's capacity times the cells in
parallel. LightSim counts the SOC in amp-hours.

**Clutch**: a part that joins or separates two shafts, such as an engine
and a gearbox. While it slips, the two turn at different speeds and the
clutch turns power into heat.

**Component, part**: one thing in the model, such as a battery, a motor or
a wheel, taken from the library in the *Components* panel. On the diagram
each part is a box; its values are its parameters.

**Coulombic efficiency**: the share of the charge put into a battery that
it stores; Li-ion cells store nearly all of it.

**Cursors**: the two lines A and B you can place on the *Results* chart
(**Cursors**, or the C key) to read each signal at two times and measure
between them ([how](how-to/measure-between-two-times.md)).

**Data Bus**: the links that carry signals between parts. They are listed,
and made, in the *Data Bus Connections* panel.

**Data Checks**: LightSim's checks of a model before it runs: wiring,
missing signals, values out of range, tables that do not fit together.
They run by themselves after every change; *Problems* lists what they find.

**Differential**: the gear set that lets the two wheels of an axle turn
at different speeds in a corner while it shares the drive between them.
A locked differential makes them turn together.

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

**E-Motor**: the electric motor and its inverter (the electronics that
feed it from the battery) as one part, described by its full-load and
loss maps.

**Energy delivered, energy recuperated**: what left a battery at its
terminals, and what went back in from braking. The difference is the net
energy the run took from it ([all summary values](reference/results.md)).

**Example**: a model that comes with LightSim. It opens as a copy, so you
can change it; **Save** keeps your copy as a project of your own.

**Final drive**: the fixed gear between the gearbox, or the motor, and
the differential or wheels. Its ratio says how many times the input turns
for one turn of the output.

**Frontal area**: the area of the car seen from the front, in m².

**Fuel map**: a table of how much fuel an engine burns at each speed and
torque.

**Full-load map**: the most torque a motor or engine can give at each speed
(and, for an E-Motor, each voltage).

**Gearbox**: a part with several gear ratios, chosen by its own shift
schedule or a gear command.

**Grip limit**: the most force a tyre can pass to the road before it
slides or spins: its friction coefficient μ times the load on it.

**HWFET**: the EPA Highway Fuel Economy Test cycle, 765 s of highway
driving, used for US fuel-economy ratings.

**Lap case**: a case of kind *Lap*. The car drives the model's *Race
Track* (its layout and laps are the case's own) as fast as its tyres,
motors and battery allow, as a lap simulation does, and the motors and
battery then drive that speed for the energy. The case's *Duration*, *Step*
and *Pacing* do not apply; the results are estimates.

**Layer Configurations**: the tab under the diagram that chooses which
kinds of wires are drawn (electrical, mechanical, signal links) and
whether live values show on the diagram.

**Live run**: a run slowed down to a pace you can watch (*Pacing* 1× to
30× in the case settings), so you can change values while it runs.

**Load sensitivity**: how much a tyre's friction coefficient falls as the
load on it grows, per kN. Real tyres grip a little less, per kilogram,
the harder they are pressed down.

**Load transfer**: the weight that moves to the rear wheels as the car
speeds up and to the front ones as it brakes. LightSim counts it when
the Vehicle has a *Centre of Gravity Height*.

**Loss map**: a table of the power a motor and its inverter lose, by speed
and torque. The engine's equivalent is its fuel map.

**Minimum SOC**: the lowest state of charge a battery may reach. Below it
the battery gives no more power, and the run warns.

**Monitor**: a part that only shows values: link signals to its inputs and
watch them in the *Monitors* panel while a run goes.

**Not valid**: a mark on a summary figure that the run's checks rule out,
with the reason, for example *cycle not followed*.

**OCV**: open-circuit voltage, the voltage a battery shows with no current
flowing. It depends on the state of charge.

**Output Power Limit**: the most power a battery may give at its
terminals, in kW; Formula Student allows 80 kW. With *Hold Power to
Limit* on, the battery holds the motors back to keep to it.

**Override**: a parameter value that applies to one case only. The model's
own value stays as it is for the other cases.

**P2 hybrid**: a parallel hybrid with its electric motor between the
engine's clutch and the gearbox, so the motor can drive the car with the
engine stopped.

**Parameter**: one value of a part, such as a Vehicle's mass or a
battery's capacity, with its unit. *Properties* shows them.

**Performance case**: a case of kind *Performance*, for 0-100 km/h and top
speed. The Driver holds full throttle until the car reaches the target
speed, and the run reports the time it took.

**PI controller**: a controller that acts on the gap between a target
and the actual value (P, proportional) and on how long the gap has lasted
(I, integral). The Driver is one; the **PID** block adds a third term
for how fast the gap changes (D, derivative).

**Port, pin**: a connection point on a part: a dot on its edge for wires,
or a signal input or output in *Data Bus Connections*.

**Power Check Window**: the time over which a battery's power is averaged
before it is checked against its *Output Power Limit*: 0.5 s in
Formula Student.

**Problems**: the tab that lists what the Data Checks found and what went
wrong in the latest run, each with a line on how to fix it.

**Profile**: a list of `time:speed` pairs typed into a Driving Task, such
as `0:0; 30:50; 120:50`, in s and km/h. The speed changes in a straight
line between the points.

**Recuperation**: braking with the motor running as a generator, which
charges the battery instead of heating the brakes.

**RMS power**: the root mean square of a power over time: the steady
power that would heat a battery or motor as much as the real, changing
one.

**Road load**: the force that holds the car back at a steady speed on a
flat road: rolling resistance plus air drag. It can also be given as the
coefficients A, B and C of a coast-down test: A + B·v + C·v².

**Rolling resistance**: the force a rolling tyre takes, about the same at
every speed: the rolling resistance coefficient times the load on the
tyre.

**Run**: one simulation of a case. Its status is *success*, *warning*,
*cancelled* or *failed*.

**Run status**: how a run ended: *success*, *warning*, *cancelled* or
*failed* ([what each means](reference/results.md#not-valid-and-the-run-status)).

**Sample Time**: how often a Script, PID or Lookup block runs, in s. At 0 it
runs at every solver step.

**Script**: a part that runs a Python function of your own at every step
(see the [Script API](reference/script-api.md)).

**Slip**: how much faster or slower a tyre turns than the road passes
under it, as a share. A little slip is how a tyre passes force; a lot
means it spins or locks.

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

**Supply limit**: a motor that gets less power than it asks for because
its battery or other source cannot give it; the summary's *time limited
by supply* counts it.

**Sweep, study**: runs of one case over a range of values of one
parameter. The study is the table of their results, saved with the
project.

**Target speed**: the speed the Driving Task asks for at each moment.

**Test mass**: the mass a car is tested at for a consumption figure: the
empty car plus a driver, a share of load and fluids, by the test's rules.

**Topology, diagram**: the drawing of the model's parts and wires in the
*Topology* panel.

**Traction Command**: the Driver's output that tells the motor how hard to
drive (above 0) or to brake as a generator (below 0), from -1 to 1.

**UDDS**: the EPA Urban Dynamometer Driving Schedule, 1,369 s of city
driving, used for US city fuel-economy ratings.

**Usable Capacity**: the energy a battery holds between full and empty,
in kWh.

**Vehicle Load Share**: the part of the car's weight a wheel carries, in %.
The shares of all wheels should add up to 100 %.

**Voltage Class**: the highest voltage a battery may reach, in V;
Formula Student allows 600 V. The run checks the battery against it.

**WLTC**: the Worldwide harmonised Light vehicles Test Cycle, used for
consumption and range ratings in the EU and elsewhere. Class 3b is the one
for most passenger cars: 1,800 s in four phases, Low to Extra High.
