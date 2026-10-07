# LightSim: known issues and limits

LightSim is still an early version. You can build, run and inspect models,
but the component physics are simplified, **nothing has been validated
against measured vehicles yet**, and some results are known to be wrong.
This page lists what we know, what you can do about it today, and which
roadmap item tracks the fix.

Use LightSim to learn the workflow and to compare variants of one model
with each other. Do not use its absolute numbers (consumption, range, top
speed, acceleration) for decisions about a real vehicle yet. [What is
validated](VALIDATION-STATUS.md) says what the automatic tests check and
how the example cars compare with real ones.

- *Roadmap* gives the ID of the item on the LightSim development roadmap
  that tracks the fix, so the release notes can say when it is resolved.
- *Being fixed* means the work is under way for an upcoming release. Until
  the release notes say it is done, the problem and the workaround apply.
- Last reviewed: 28 September 2026, for version 0.2.0. This page is updated
  with every release.

## Results that can be wrong today

### Machines and maps at the edge of their data

Since 0.3 an E-Motor has a *Maximum Speed* (left at 0, the last speed point
of its full-load curve): its drive torque falls to zero over the last 2 %
below it, and above it the inverter is off. A car with a 300 km/h target
now tops out at 152 km/h with its motor at 11,921 of 12,000 rpm, and
Messages names the maximum speed. Each table axis has an *outside the data*
setting, shown in the table editor of the parameter dialog: *Error* stops
the run with a message naming the table, the axis, the value, the data's
range and the time; *Clamp* holds the edge value (what every table did
before 0.3); *Linear* extends the edge slope. The speed and torque axes of
the motor's full-load and loss maps and the engine's fuel map, and the fuel
cell's current, stop the run by default. When a run
reads a table outside its data, or a motor or engine goes above its maximum
speed, Messages says so once and the run summary lists for how long (as a
share of the run) and how far; these rows appear only when that happened.
When a motor, engine, battery or fuel cell spends longer there than 1 % of
the run (at least 2 s, the allowance of the speed trace), the run ends as
*warning* with a message naming the part, how far past and for how long,
and its per-distance figures are marked *not valid*. What remains:

- Above its maximum speed a motor gives no regeneration either. A car with
  no friction brakes running downhill past it speeds up further than before.
- Battery SOC, motor voltage, drag-torque and Lookup tables hold their edge
  value by default. Set an axis to *Error* to be stopped there instead.
- Below the first speed of its full-load curve (starting, stalling) a fired
  engine gives that point's torque and burns that point's fuel: a start-up
  rule, not a model of starting.
- A motor's or engine's speed limit acts from the solver step after the one
  that reached it, so the machine's own drive can carry it past by up to a
  step's acceleration (the default engine's hard rev limiter: about 1 %; a
  light machine further). While it falls back from there it is not counted
  as over speed; only what drives it higher is.
- Lookup blocks are not judged: their table is a controller's schedule,
  and its held edge value is what the controller asks for. Only their
  summary rows say that the table was left.
- A project from 0.2.0 whose loss or fuel map is narrower than its
  full-load map, whose fuel map does not reach the full-load curve's first
  speed, whose motor full-load map starts above 0 1/min, or whose fuel cell
  has a Maximum Current beyond its polarization curve, now stops with an
  error where it used to hold the edge value. Data Checks warn about it
  before the run.

*Workaround:* read the summary rows and Messages after a run; where a run
stops, extend the table or set that axis to *Clamp* or *Linear*.

### A "success" checks the trace and the data edges, not plausibility

A run is a *success* when the vehicle stayed within ±2 km/h and ±1 s of its
target speed for all but 1 % of the run (at least 2 s), covered the cycle's
distance, no motor, engine, battery or fuel cell spent longer than that
outside its data or above its maximum speed (see above), and nothing raised
a warning. It does not judge that the numbers are plausible for a real
vehicle, and the Data Checks all-clear does not vouch for the results
either. Also:

- A case of kind *Performance* (Cases & Parameters → Kind) is not judged
  on that band: the Driver holds full throttle until the car first
  reaches the target (its PI holds the target after that), and the
  run reports its *Maximum speed* and the *Time to* the target's highest
  value, timed from t = 0. That is a standing start only: there is no
  rolling start, no time between two speeds (such as 80-120 km/h) and no
  second timed speed in one run. An acceleration case can end at a
  distance, but no case can end at a speed or a charge level.
- A case of kind *Acceleration* (or Simulations → *Acceleration test*,
  which adds a 75 m case staged 0.30 m behind the start line with a 25 s
  limit) is not judged on the band either: the Driver holds full throttle
  for the whole run and reads no target. The run ends at the end of the
  solver step that crosses the line, so its channels go up to one solver
  step (at most 10 ms, about 0.3 m) past it; the time and the speed at the
  line are read inside that step. The case duration is the time limit,
  counted from rest (so it includes the drive up to the start line): a car
  that has not reached the line by then gets a warning and no time, so the
  time's *pass* can never read *fail*. FS Rules 2026
  v1.1 (FSG) D 9.2.1 disqualifies runs over 25 s in driverless runs only
  (a manual run is capped by the scoring); FSUK and FSAE may differ, and
  their staging distance was not checked. The results are estimates: the
  wheel loads shift only with a Centre of Gravity Height set (one step
  late, see below), the tyre's grip depends on its load only through a
  Wheel's *Load Sensitivity*, and nothing limits wheelspin, so when the driven wheels are at their grip
  limit the tyre still gives μ times its load but the battery power and
  energy include the power that spins the wheels. The time at the grip
  limit counts driven wheels only; there is no peak-slip figure. The time
  and that share follow the solver step when the driveline rings (see *A
  closing clutch can ring* and *Stiff settings* below), and nothing warns:
  at the 10 ms step the P2 Hybrid Car's slipping clutch rings in second
  gear and pushes its driven wheels to the grip limit both ways, so its
  75 m takes 6.474 s against 6.344 s at 1 ms (2 % slower) and it is at
  the grip limit 7.3 % of the run against 0 %; a *Step* of 0.002 s gives
  6.345 s and 0 %. A too-stiff tyre can even beat a slip-free car: the
  Battery Electric Car at a *Slip Stiffness* of 1000 gives 5.015 s,
  faster than a slip-free point mass's 5.172 s. Without a
  Driver, or with a Script between the Driver and the motors, full
  throttle is only what that model makes of an accelerator pedal of 1:
  Data Checks do not check it.
- A motor held back by its battery or fuel cell, or regeneration that a
  full or charge-limited battery refuses, makes the run a *warning* at the
  first touch, however brief; the figures stay valid, because the limit is
  part of the model. The summary says how long each motor was limited and
  how much regeneration was not recovered.
- The tolerance (1 % of the run, at least 2 s) is LightSim's own choice:
  test procedures such as WLTP set no allowance for a simulation.

*Workaround:* read the Messages panel and the *not valid* notes in the
summary table.
*Roadmap:* VAL-08; CON-06 (rolling starts, ends at a speed or a charge
level); MOD-16 (a tyre whose force drops past its peak, so wheelspin
costs time); RES-38 (time per limiting regime and peak slip).

### A battery has a power limit but no current limit

A battery delivers power up to its maximum-power point (the most its
internal resistance lets through: about 420 kW for the default pack at 90 %
charge), or up to its *Output Power Limit* when one is set, and takes back
up to its *Max Charge Power*. There are no current limits: the 500 A limit
of Formula Student (FS Rules 2026 v1.1 (FSG) EV 2.2.2, FSUK 2026 EV2.3.1) is
not modelled or checked. Fuel cells have no ramp rate, and DC-DC converters
have no power rating.

The Output Power Limit is ideal: it holds the terminal power (volts × amps)
exactly at every solver step, with none of a real limiter's lag or
overshoot, and it limits discharge only. Its check averages the solver
step's power (10 ms) over the *Power Check Window*, with nothing before
t = 0, not the samples of a competition's energy meter; a run that starts
at speed was already drawing power before t = 0. FSAE's rule that 100 ms
over the limit is a violation is not counted on its own (a window of 0
gives a check at least as strict). The *Voltage Class* check compares the
highest terminal voltage of a solver step, not a 500 ms average.

*Workaround:* check the battery's *Current* channel against what the real
pack or its management system allows, and reduce the motor's torque map or
add a limit in a Script if needed.
*Roadmap:* MOD-08 (current limits), ENG-02 (follow-up).

### Lap mode is a quasi-steady-state estimate

A case of kind *Lap* finds the fastest speed along the Race Track's line
about every metre, then drives that speed through the model's motors,
gears, brakes and battery. It is not a driving simulation:

- The driver is ideal: at the tyres' limit on the given line everywhere,
  with no transients, suspension, yaw, tyre slip or slip-angle drag, and
  an ideal brake balance (every tyre brakes at its limit). Regeneration
  comes first when braking, up to the Driver's *Recuperation Weight* and
  the driven wheels' braking grip; the drive cycles' Driver does not hold
  regeneration to the grip. Lap times are usually optimistic: a user of
  OpenLAP, a similar point-mass lap simulator, found its F1 example lap
  7.5-8 % faster than the real car's. Calibrate μ, μ_y and the downforce against a lap your
  car has driven before trusting a lap time.
- The car follows the line as drawn, with no track width or racing line.
  Where the curvature changes sign within a metre, as at the skidpad's
  crossover, the speed rises by up to 3 % at that point: a real car cannot
  turn from one circle into the other that quickly.
- E-Motor cars only: a lap case refuses Combustion Engines and Clutches on
  the wheels, holds Gearboxes in their gear and commands every E-Motor
  itself, with one demand for all, so Scripts or controllers between the
  Driver and the motors (torque vectoring, traction control) do nothing.
  With E-Motors on both axles, the driven wheels' grip is used together,
  as if the torque went to whichever axle has grip to spare: a car whose
  axles' torque does not match their grip can be slower than lap mode
  says (with a 6.5 front and a 4.0 rear final drive a 600 m straight
  took 3.7 % longer in the time domain, against 1.6 % with equal final
  drives).
- The powertrain's limit is read at each lap's start, with the battery's
  charge and voltage then. Within a lap the motors can fall short of the
  speed where the battery's voltage sags more than expected; the *Lap
  energy balance error* shows it, and above 0.5 % the *Energy per lap* is
  marked not valid, and so are the lap times when the motors gave less
  than the speed asked for (or the battery reached its minimum SOC).
  The Output Power Limit is held at every point; the check window's
  average is not used to let short peaks through.
- Sideways load transfer is shared between the axles as the static weight
  is (no roll stiffness or anti-roll bars), and each axle's wheels are
  taken in pairs across the car, the same way in left and right corners:
  give the left and right wheels the same load share.
- Driving a lap's speed as a drive cycle gives other energy figures: the
  drive cycles' Driver has no brake balance or ABS and can lock the driven
  wheels when braking hard (a Formula Student-sized car driving its
  Autocross lap's speed as a drive cycle used 62 % more energy and
  recuperated 30 Wh instead of 110 Wh).
- Live edits during a lap case reach the motors, gears and battery at
  once, but the lap's speed, with the edited tyres, Vehicle and brakes,
  is solved again only at the next lap's start.
- A Custom track's curvature is used as entered: a logged lateral
  acceleration / speed² is noisy and should be smoothed first. Data Checks
  refuse a curvature above 0.5 1/m (a 2 m radius).
- The Autocross, Skidpad and Acceleration 75 m layouts are LightSim's own
  drawings after FS Rules 2026 v1.1 (FSG) D 4.1, D 5.1.1, D 6.1 and D 7.1,
  not official layouts; FSUK and FSAE may differ, check the current
  season's rules.

*Workaround:* compare lap cases with each other rather than with a stop
watch, and check the *Time limited by* rows and the Race Track's *Limit*
channel for what holds the car back.
*Roadmap:* VAL-12 (calibration against a logged lap), MOD-34 (a dynamic
lap model), STD-35 (tracks from GPS or OpenStreetMap), CON-11 (driving a lap's speed as a drive cycle), MOD-08 (state
of power), MOD-09 (heat over an endurance).

### Formula Student points are estimates

The *FS event* rows and the *Formula Student points* table score a run
with the formulas of FS Rules 2026 v1.1 (FSG) D 9. They are not official
results:

- Only FSG 2026 scoring is built in. FSUK and FSAE use other maximum
  points and formulas, and a season's rules can change them.
- The points need the other teams' results: the fastest time and the most
  efficient energy, which you type in. No competition's results come with
  LightSim.
- Each event is one run, with no penalties (cones, off-course, flags) and
  no second driver or second run.
- The rule checks are simplified: the current and, without the battery's
  Formula Student preset, the power are checked at their highest over a
  solver step, not as a 500 ms average (stricter than D 10.4.1); the
  voltage check takes the open-circuit voltage at full charge or the
  highest terminal voltage. Any breach scores the event 0, where the rules
  take away only the fastest run.
- The endurance's driver change is a stop at the end of the lap at half
  distance and a start from rest on the next one; the event time leaves
  out that restart lap whole, but keeps the braking into the stop. The
  3 min stop itself is not simulated (no battery recovery or cooling).
- The skidpad time is the mean of the two circles of one lap of LightSim's
  Skidpad layout; the rules time a second lap on each circle.

*Workaround:* compare points between versions of your car, with the same
references, rather than with a competition's results.
*Roadmap:* MOD-43 (more competitions' scoring), MOD-44 (endurance energy
strategy), MOD-09 (heat over an endurance).

### Lift-and-coast is a simple driver strategy

A Race Track's *Lift-and-Coast* and *Energy Target* (MOD-44) save energy
only by coasting before the braking points:

- The coast is a share of each stretch of acceleration that ends in
  braking, from no slower than half the braking speed; it does not
  regenerate while coasting and never lifts in corners.
- The *Energy Target* picks each lap's share from an estimate of the
  lap's energy that it corrects lap by lap. It met targets within 0.6 %
  on the FS example; a target beyond what full lift-and-coast saves is
  missed with a warning. It does not lower the power cap for you.
- Battery temperature is not part of the strategy (no heat model yet).

*Workaround:* combine it with the battery's *Output Power Limit*, and sweep
both to find the pace you want.
*Roadmap:* MOD-44 (power-cap search, regeneration level), MOD-09 (heat).

### Imported laps are drive cycles against time

**Import lap** (STD-35) makes a lap from a logger or lap simulator into a
drive cycle:

- The Driving Task follows speed against time, so a trace against
  distance is turned into time from its speed; driving it against
  distance is ENG-34's work.
- The logger layouts (MoTeC i2, AiM Race Studio, OpenLAP, TUM
  laptime-simulation) are LightSim's reading of those tools, not checked
  against teams' files.
- Only one speed column is read: grade, elevation, GPS position and
  lateral acceleration are not, so a lap on a hill is driven flat, and a
  track cannot yet be built from the GPS trace.
- Laps are split only by a lap number column, not by a GPS start line.
- The trace is not smoothed: a spike in the speed is driven as it is
  (LightSim warns about changes faster than 2.5 g). The drive cycles'
  Driver has no brake balance, so hard braking can lock the driven wheels
  (see *Lap mode is a quasi-steady-state estimate*).
- The case keeps the speed trace, not the file; the file's SHA-256 is
  written to *Messages* only. Saved import settings (presets of your own)
  are not offered yet.

*Workaround:* pick the columns by hand when a layout does not find them,
and smooth a noisy speed in a spreadsheet first.
*Roadmap:* STD-35 (GPS start line, grade, saved presets), ENG-34 (driving
against distance), STD-02 (keeping the file in the project), STD-07
(measured data).

### Signal units are not checked

Data Checks now report two signals wired into the same input (UX-37), but
units are not checked: a battery's *SOC* output is in % (0-100), and a
Script, Lookup or PID block that expects 0-1 gets 0-100 without a warning.

*Workaround:* in the Data Bus panel, check that the units at both ends of
each link match; divide percentages by 100 where a block expects 0-1.
*Roadmap:* VAL-17.

### Computed values read 0 in the first result point

Point 0 of every run is the initial state at t = 0. Vehicle speed, distance,
state of charge, tank levels and the Constant and Driving Task outputs are
right there, but values that blocks, the Driver or the physics compute
(pedal and traction commands, Script, PID and Lookup outputs, motor and
engine torque, powers, road grade) read 0, because nothing has computed them
yet.
At later points these values come from the last solver step before the
point, at most 10 ms earlier.

*Workaround:* leave out point 0 of computed channels when you take a
minimum or an average, for example from the CSV export, or put the
*Results* chart's cursor A after t = 0.

### Component models with known errors

- **A declutched engine with any throttle runs to its rev limiter.** The
  engine has no speed governor above idle: with the clutch open, any
  throttle above 0 revs it up to the last speed of its full-load curve,
  where it runs on the rev limiter at high fuel flow. Control scripts
  should set the throttle to 0, or switch the engine off, while the clutch
  is open, as the P2 Hybrid Car example's script does.
  Turbo lag, restart cost and warm-up are not modelled either.
  *Roadmap:* MOD-13.
- **Gear losses leave out inertia.** Each gear's loss now acts on the net
  power through it, in both directions, but the torque that accelerates the
  driveline's own inertia is not part of that net, and a locked clutch's
  torque is taken from the previous 10 ms step. *Roadmap:* MOD-03.
- **Shaft and Final Drive power is the total of all motors and engines.**
  Their *Transmitted Power* channel shows the summed mechanical power of
  every motor and engine on the driveline, not the power through that part:
  gear and clutch losses are left out, and every Shaft and Final Drive on
  the driveline shows the same value. In the P2 Hybrid Car example's Mixed
  Cycle with a Shaft added between the engine and the clutch, at t = 281 s
  the engine delivers 9.8 kW and the motor takes 0.6 kW to charge the
  battery, and the Shaft and the Final Drive both show 9.2 kW. Read the *Mechanical
  Power* of each motor and engine instead. *Roadmap:* MOD-10.
- **The air is dry, still and the same along the road.** Air drag uses the
  density the Ambient block's temperature and pressure give (1.204 kg/m³,
  20 °C and 101.325 kPa, without an Ambient), but not the road's altitude
  (a 30 m climb makes it 0.35 % thinner), humidity (damp air is up to about
  1.6 % thinner at 30 °C) or wind. The Ambient holds one temperature and
  pressure for the run: a case value or a sweep changes them between runs,
  a live edit during one, but there is no temperature over time or
  distance. For a road at altitude, set the Ambient's pressure.
  *Roadmap:* MOD-56 (wind), MOD-57 and CON-11 (altitude along the road),
  MOD-09 (temperature over time).
- **Road-load coefficients: the driveline's share is all or nothing.** With
  *Coefficients Include Driveline Losses* ticked, the final drives,
  differentials and transfer cases run lossless, as if the coefficients held
  their whole loss; a coast-down holds only their spin losses, so this leaves
  out a little. There is no estimate of the driveline's share from target
  minus dyno-set coefficients (EPA ALPHA's road-load adjustment), and no
  test-mass mode (equivalent test weight, × 1.015 for a two-wheel-drive
  dynamometer): enter the test mass as the Vehicle Mass. *Roadmap:* MOD-03
  (zero-load gear drag), VAL-05 (EPA reference tests).
- **Wheel loads shift one step late, and only between the axles.** With a
  Vehicle *Centre of Gravity Height* and each Wheel's *Axle* set, load
  moves between the axles by m·(a + g·sin θ)·h/L when the car accelerates,
  brakes or stands on a slope, and a *Downforce Area* adds ½·ρ·CzA·v² split
  by the *Front Aero Balance*. The acceleration a is the previous solver step's,
  10 ms behind: exact while it is steady, off by its change over one step
  while it changes (0.6-4 % where an FS car's motor reaches its power
  limit, the whole transfer in a launch's first step). With the driven
  front wheels spinning, the lag feeds back with a gain of μ × h/L per
  step: at a real car's 0.2-0.4 the loads settle within a few steps, at
  0.9 they still ring after 0.4 s, and above 1 the acceleration swings
  from step to step and the run can warn of front wheels lifting that
  would not. Drag is taken to act at ground height and moves no load (at
  90 km/h an FS car's drag would move about 90 N, 5 % of its rear axle
  load). An axle that would carry
  less than nothing carries nothing and the run warns: the car does not
  pitch, wheelie or tip over, and there is no suspension. In a drive cycle
  load does not shift from side to side (a lap case shifts it), and grip
  depends on load only with a Wheel's *Load Sensitivity* set. The static
  split is the *Vehicle Load Share*; the shares
  of the connected wheels are scaled to add up to 100 %, and Data Checks
  say when they had to be. Wheels are on the Front axle unless set to
  Rear: set the rear wheels before giving a CG height (Data Checks say
  so). *Roadmap:* MOD-16 (a tyre model beyond μ and its load
  sensitivity), MOD-34 (pitch and suspension).
- **An engine behind a script-controlled clutch starts at rest.** A run that
  starts at speed starts every wheel, gear and motor at that speed, and an
  engine behind a closed clutch too; a clutch a Script controls counts as
  open at t = 0, so the engine behind it starts at rest. *Roadmap:* MOD-19.
- **Stiff settings can cause short wheel-spin spikes at launch.** This is a
  numerical effect of how the solver steps the tyres: a high tyre *Slip
  Stiffness*, a very light inertia or a strong clutch can push the solver
  past its stability limit, and nothing warns when that happens. Keep *Slip
  Stiffness* near its default, and after changing these settings plot the
  wheels' *Longitudinal Slip* at launch. A car braked to a stop can also
  creep with the brake fully applied: under 0.2 km/h at the default *Slip
  Stiffness* of 10, about 2 km/h at 30 and up to 23 km/h at 300, at the
  10 ms solver step. When a car pulls away from rest faster than
  μ × 0.5 m/s ÷ (*Slip Stiffness* × step), 5 m/s² at the defaults and the
  10 ms step, its undriven wheels ring (their tyre force changes sign from
  one step to the next) until it reaches 0.7-2.3 m/s (measured on a 300 kg
  Formula Student car at μ 1-1.6); at a Formula Student launch this moves
  the 75 m time by about 0.4 %. *Roadmap:* ENG-09, ENG-14.
- **A closing clutch can ring at the 10 ms step.** While a clutch slips by
  more than 0.5 rad/s the solver passes its full torque for the whole
  step, and at 10 ms that overshoots the lock-up: the shaft on either side
  can swing by several hundred 1/min from one step to the next (up to
  770 1/min in the P2 Hybrid Car) for a few steps, now and then for a
  second or two, before the clutch locks. Energy is still conserved, but
  the fuel it costs follows the step: the P2 Hybrid Car's EPA city figure
  reads 2.838 l/100 km at the shipped 10 ms step, 0.017 (0.6 %) above a
  2.5 ms run (2.821), with the same engine starts; its highway and Mixed
  Cycle figures are about 0.004 above. *Roadmap:* ENG-09.
- **Fuel-cell hydrogen use is a fixed figure per kWh** (*Specific H₂
  Consumption*, 55 g/kWh by default), which overstates it at part load by up
  to about a third and understates it at full load.
  *Roadmap:* MOD-20.

### Live edits, charts, sweeps and export

- **Values between recorded points are not stored.** Charts draw every
  stored point (the highest and lowest of each pixel column), and zooming
  in shows each one, but with *Store every* above 1 the values in between
  recorded points are never stored, so a short spike or dip between them
  does not show. Use *Store every* 1 when peaks matter. *Roadmap:* RES-17,
  ENG-16.
- **Stored values are rounded.** Every stored value is rounded to 5 decimal
  places, so small values keep few digits (a tyre slip of 0.0018 keeps two).
  The summary rounds energies to 1 Wh (battery losses to 0.1 Wh), fuel to
  1 g and consumption to 0.01 per 100 km. CSV export has the same rounding. Treat smaller differences
  between runs as noise; to compare two close variants, lengthen the run
  (for example, repeat the cycle) so that the difference adds up. The
  *Results* page marks a change against the baseline run that is no larger
  than one step of the stored rounding as *~ 0*. It reads that step from
  the stored digits, so where both values end in 0 (0.07 kWh stored for
  0.070) it takes the step 10 times larger and a change of up to 10 real
  steps can read *~ 0*.
  *Roadmap:* ENG-16.
- **Cursor integrals come from the recorded points.** The *Results*
  chart's cursors integrate the stored points with the trapezoid rule, so
  they differ a little from the summary's energies, which add up every
  solver step: on the Battery Electric Car's City Cycle (a point every
  1 s), the battery's power integrates to 0.8145 kWh over the run, and the
  summary's energy delivered less energy recuperated is 0.811 kWh (0.4 %
  less). Set the case's *Step* smaller and *Store every* to 1 when the two
  must agree.
  On the distance axis, the points of a stop share one distance: the up
  and down arrows then step through them while the line stands still, and
  an overlaid run that stopped where a line crosses it is read at the
  start or the end of its stop.
- **Long runs with many lines zoom less smoothly.** On a 1-hour run
  (36,001 points) with 7 channels ticked and the baseline drawn faint as
  well (14 lines), the mouse wheel zooms at 60 frames a second most of the
  time, but about one step in 20 takes two frames (30-40 ms). Untick *Draw
  the baseline faint on the chart*, or tick fewer channels, to zoom
  smoothly.

## The examples

- **P2 Hybrid Car:** sized after the Hyundai Ioniq Hybrid, with its test
  mass and road load from EPA data (EPA's own coefficients A/B/C, with the
  axle's losses counted once), but its engine, motor and battery maps
  are generic, not the car's. With its charge-sustaining control script it
  uses about 2.84 l/100 km on the EPA city cycle and 3.24 on the highway
  cycle, against 2.91 and 2.94 for the real car in EPA's tests. The model
  has no cold start, engine warm-up or start-up fuel, so its city figure
  reads below EPA's, whose city test starts cold; on the highway, with its
  generic maps, it stays about 10 % above. Each case starts at the charge
  the cycle ends with (as a preconditioning drive would leave it), so the
  fuel figure needs no battery-charge correction; start it elsewhere and
  the figure includes the charge the strategy restores. *Roadmap:* CON-14
  (sourced maps).
- **Battery Electric Car:** modelled on the 2021 Cupra Born with FASTSim's
  values; about 14 kWh/100 km on WLTC at the battery (a car of this class is
  rated about 15-16 kWh/100 km at the charging socket, charging losses
  included), 18.9 with heating or air-conditioning on (the 2.5 kW case).
  Its motor loss map is generic, not the car's measured map. The real car is
  rear-wheel drive and has an 11.5:1 reduction gear with an electronic
  160 km/h limit; the example drives the front axle (its CG height is 0,
  so its loads do not shift and only the load share matters) and uses a
  12.8 ratio so that the motor's maximum speed sets the 160 km/h. An
  E-Motor's *Maximum Speed* could now set that limit, but the example
  still sets it through the ratio.
  *Roadmap:* MOD-12.
- **FS Electric (generic):** a typical Formula Student electric car, not a
  real one: replace its values with your car's. Its 75 m time (3.74 s from
  the start line) sits in the faster half of FS Czech Republic 2025's
  3.51-6.44 s: the tyres keep their grip however much they slip (no peak
  and drop), so wheelspin at the launch costs no time, and nothing limits
  it (no traction control; 48 % of the run is at the tyres' grip limit).
  So its 80 kW is reached at 0.2 s, while the rear wheels still spin: a
  car with traction control reaches it later. At its 10 ms step the front
  wheels' slip and force channels ring at the launch and read high for
  about 2 s (the example's *Slip Stiffness* is 20, not 10): plot them at a
  1-2 ms step (the 75 m time moves 0.3 %).
  The *Endurance energy* case is lap mode at the tyres' limit in every
  corner and under every braking, with no lift-and-coast and no driver
  change (FS Rules 2026 v1.1 (FSG) D 7.5.4's 3 min); its energy follows the
  Output Power Limit the case sets, 30 kW: 20 kW gives 4.14 kWh net, 40 kW
  6.33 kWh with 7.9 % SOC left, and at 45 kW the pack runs out before the
  last lap is done. The E-Motor's generator torque is held to 40 % for
  drive cycles you add: their Driver has no brake balance or ABS, and at
  full generator torque, braking hard from 100 km/h locks the rear wheels
  and turns them backwards (at 40 % they still lock from about 45-60 km/h
  in a hard stop, and turn backwards just before the car stops, which FS
  Rules 2026 v1.1 (FSG) EV 2.2.4 forbids; at 20 % they lock only below
  walking pace).
  Lap mode holds regeneration to the rear tyres' grip, so the example's
  own cases hardly depend on it. The 500 A current limit (EV 2.2.2) is not
  checked (the cases stay under 160 A), nor are the cells' own limits:
  recuperating into the full pack raises its cells to about 4.3 V (594 V),
  which a real accumulator management system would not allow. A
  two-motor variant is not shipped.
  *Roadmap:* MOD-16 (tyre peak and drop), MOD-08 (pack from cells, current limit), CON-18 (templates,
  two-motor variant).
- **Runs made on an example stay with the copy you ran.** An example opens
  as an unsaved copy, and its runs are stored with that copy: they are
  listed while it stays open, also after a restart, but opening the example
  again from the Open menu starts a new copy with no runs listed. Save the
  copy (*Home → Save*) to keep it and its runs as a project of your own;
  runs of copies never saved are the first deleted when stored runs reach
  their disk budget.

## Not modelled yet

- **No heat or cooling.** There is no thermal solver: temperatures do not
  change and do not affect batteries, motors or engines. The Ambient
  component sets only the air density (see above), and thermal or fluid
  connections are ignored during a run.
  *Roadmap:* MOD-09.
- **Forward driving only.** No reverse, and no rolling back: a car on a steep
  hill stays put even with no brakes. *Roadmap:* MOD-21, ENG-21.
- **Drive cycles are longitudinal only.** A drive cycle, performance or
  acceleration case does not corner: weight shifts between the axles but
  not from side to side. Lap cases corner, as a quasi-steady-state
  estimate (see *Lap mode is a quasi-steady-state estimate*). Tyre force
  rises with slip and then stays flat (no peak and drop). *Roadmap:*
  MOD-16, MOD-34.
- **A simple driver.** The Driver is a PI speed follower: it does not look
  ahead along the cycle or shift gears; gear and clutch logic comes from
  Script blocks. *Roadmap:* MOD-14.
- **Structural limits.** One battery or voltage source per electrical bus;
  DC-DC converters work in one direction only; one differential and one
  E-Motor per driveline (several independent drivelines, such as dual-motor
  all-wheel drive as two axles, work). Sub-system containers are for
  organising only: physical connections cannot cross a container boundary
  (signals can, through the Data Bus).
- **Unfinished parts of the app.** The Optimization tab is hidden until it is
  implemented, and the canvas bookmark tool is disabled. *Roadmap:* STU-04.

## Using and installing the app

- **Stored runs have a disk budget.** Finished runs are kept on disk with
  their project, up to 500 MB per project and 2 GB in all; past that the
  oldest are deleted (runs of projects that were never saved go first), and
  the app shows at most the 20 newest. Export runs you need to keep for
  good to CSV. *Roadmap:* RES-02, RES-09.
- **Scripts run in a separate, locked-down process, but how locked-down
  depends on your system.** Script blocks hold Python code that comes with
  the project. LightSim checks that code first: Data Checks only compile it,
  never run it, and during a run a script can use `math` and a short list of
  basic functions, and cannot name files, programs or Python's internals.
  Since 0.2.0 scripts also run in a process of their own, apart from the
  engine. If a script step takes longer than 2 s, the engine stops that
  process and the run fails, even for loops the first check cannot
  interrupt; a script that asks for too much memory hits a 512 MB cap.
  - *Linux 5.13 or newer:* the system also stops that process from opening
    any file and from making or accepting TCP connections. Not blocked:
    other network traffic (UDP) and local sockets. On older Linux, only the
    memory cap and a limit that stops it writing data into files apply; it
    could still delete files.
  - *Windows:* the process has the memory cap and ends when LightSim ends,
    but Windows has no simple way to block its files or network, so there
    the first check is the main protection.

  This makes a harmful script much harder to write, not impossible. Open
  projects only from people you trust. *Roadmap:* PLT-02.
- **Runs with Script blocks take longer than in 0.1.0.** Controllers and
  scripts now run every 10 ms, and every script step is a round trip to the
  script process. On a test machine the P2 Hybrid Car's Mixed Cycle takes
  about 11 s, against 7.9 s in 0.1.0; the Battery Electric Car has no Script
  blocks and takes about as long as before. On computers with 4 or more
  processor cores, the script process keeps a second core busy while a run
  goes at full speed, which makes that run about 20 % faster; a paced (live)
  run hardly uses it. A coarser case step does not make a run faster: the
  solver steps every 10 ms whatever it is. *Roadmap:* ENG-10.
- **Part names can overlap when the diagram is zoomed out.** Names keep
  their 11 px on screen however far you zoom out, so neighbours' names run
  into each other: on the Battery Electric Car, with Windows' font, one
  pair at 53 %, seven at 39 % and all of them at the 15 % minimum; a wider
  font, as on Linux, overlaps sooner. Zoom in, or use the minimap to find
  a part. With a bottom panel open, two of the lowest names are cut off in
  the smallest window (1024 × 700), and in a 1366 × 768 window when the
  panel keeps the taller height an earlier build saved, because automatic
  fits stop at 50 %: close the panel, zoom out or choose *Reset UI*.
  *Roadmap:* GUI-10.
- **A few small marks are still faint.** The warning badge on a part, and
  the pin outlines and polarity marks in the dark theme, fall short of the
  WCAG contrast minimum. *Roadmap:* GUI-14.
- **Run warnings find their part by its name.** The Problems list shows the
  latest run's warnings and errors, and a row selects the part whose name
  the message quotes. A part renamed after the run is missed, parts that
  share a name are all selected, and some messages name no part (*Cycle
  not followed*). A run's rows stay until the next run, even once the
  model is fixed. *Roadmap:* VAL-10.
- **Signals are linked one at a time.** Data Bus Connections has no
  "connect to all Brakes" or "connect by matching names" yet, and signals
  are not drawn on the diagram: pick each input's source in its row (two
  clicks). In a 1366 × 768 window the bottom panel shows two rows at a
  time; drag its top edge up to see more. *Roadmap:* UX-15 (follow-up),
  UX-11.
- **Three standard drive cycles.** The Driving Task's *Drive Cycle* list
  has WLTC class 3b, EPA city (UDDS) and EPA highway (HWFET). Other cycles
  (NEDC, FTP-75, US06, the WLTC of other classes) and cycle files of your
  own are not in it yet: type or paste their points into the Profile. A
  cycle has no grade. A project that names a drive cycle, opened in 0.2.0,
  drives the typed profile instead, with no warning. *Roadmap:* CON-04,
  CON-11, PLT-07.
- **Few starting points.** The *Start* page offers the examples that come
  with LightSim and a blank project. Ready-made starting points for other
  layouts (two motors, a fuel-cell car) and templates that ask a few
  questions first are not there yet: start from the example closest to
  your car and change it. *Recent projects* lists projects saved in
  LightSim's projects folder only. *Roadmap:* CON-18, PLT-32.
- **Unsigned installers.** Windows SmartScreen warns on first launch (choose
  *More info → Run anyway*). *Roadmap:* PLT-13.
- **No macOS version.** Builds exist for Windows 10/11 (x64) and Linux (x64)
  only. *Roadmap:* PLT-13.
- **No automatic updates yet.** Download a newer version from the GitHub
  Releases page and install it over the old one; your projects are kept.
  *Roadmap:* PLT-18.
- **The help is a first draft.** F1, or **?** at the top right, opens
  LightSim's help in your web browser, served from your computer: two
  tutorials, how-to guides, a page for every part in the library, and the
  documents that come with each release. It is new: if a page does not
  match what the app shows, the app is right. There are no pictures of the
  app beyond the quick start's, the help opens outside the app window, and
  it is not online yet. *Roadmap:* LRN-09 (help inside the app), LRN-04
  (follow-up: online).
- **The parameter texts are first drafts.** Rest the pointer on a
  parameter, or move to it with Tab, to see what it is, its usual values
  and where to find the real number; each part's help page lists the same
  texts. They follow what the solver does, but the usual values come from
  general knowledge and no vehicle engineer has reviewed them yet: check a
  value that matters against its source. *Roadmap:* LRN-05 (review),
  CON-13 (where a value came from).
- **Limits are checked one parameter at a time.** Data Checks and the form
  check each number against its own limits only: a PID's Output Minimum
  above its Output Maximum, or a Default Gear past the last gear, is not
  flagged. A sweep's From and To are not checked as you type; a point
  outside the limits fails when it runs, with the Data Check's reason. A
  case's own value out of range stops the runs of every case, not only
  its own. An acceleration case's Distance, Start line and Reference time
  turn red as you type but are not Data Checks: a run ignores a value
  outside them (no finish line, a 0 m start line, no reference gap).
  Properties does not mark a value that differs from the library's default
  and cannot reset it. *Roadmap:* UX-38 (the mark and reset).
- **Licence.** LightSim is proprietary (`LICENSE`). The desktop app is free
  for evaluation, learning, research and other non-commercial use under its
  end-user licence agreement (`EULA.txt`, installed with the app);
  commercial use needs a separate licence from the owner. The owner sets
  these terms, and they may change in a later release. The open-source parts
  inside the app keep their own licences, listed in `THIRD-PARTY-NOTICES.txt`
  (*Help → Third-Party Notices*). *Roadmap:* BIZ-01, BIZ-03.
