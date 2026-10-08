# LightSim: known issues and limits

LightSim is still an early version. You can build, run and inspect models,
but the component physics are simplified, **only the energy use of four
electric cars on EPA's city and highway cycles has been compared with
official test results** (within 9 %, see [What is validated](VALIDATION-STATUS.md)),
and some results are known to be wrong.
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
  closing clutch can ring* and *Stiff settings* below), and only a Data
  Checks note says so:
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
- The two automatic hand calculations under each run's expected values
  (VAL-35) are bounds, not predictions: the top speed one reads the gear
  ratio from the run at its fastest point, and the energy one sums the
  road load over the recorded points (1 s by default) and allows 3 % for
  that. Runs with a combustion engine skip the energy check.

*Workaround:* read the Messages panel and the *not valid* notes in the
summary table.
*Roadmap:* VAL-08; CON-06 (rolling starts, ends at a speed or a charge
level); MOD-16 (a tyre whose force drops past its peak, so wheelspin
costs time); RES-38 (time per limiting regime and peak slip).

### Battery limits are a battery management system's tables, not cell physics

A battery delivers power up to its maximum-power point (the most its
resistance lets through: about 420 kW for the default pack at 90 %
charge), or up to its *Output Power Limit* when one is set, and takes back
up to its *Max Charge Power*. Since 0.3 it can also hold the current and
voltage as a battery management system does: with *Defined By: Pack
values*, a *Max Discharge Current*, *Max Charge Current* and minimum and
maximum pack voltage (0 = none, as in every existing model); with
*Defined By: Cells*, the cells' continuous and peak currents and their
minimum and maximum voltage, for the weakest series group. These are the
limits a BMS keeps (fidelity L1), with these simplifications:

- The resistance comes from tables (pulse length, SOC, temperature), not an
  electrochemical model; the default tables are estimates. The pulse length
  is the time the current has flowed one way, so the step after the current
  reverses still uses the old pulse's resistance.
- The cells are at the first Ambient's temperature: they do not warm up
  under load (MOD-09). There is no hysteresis and no second RC pair
  (MOD-51), and no ageing.
- A cell's voltage limit is met at the start of each 10 ms step; the
  open-circuit voltage then falls a little over the step, so a cell can end
  it a few millivolts past its limit (at most 2 mV in the tests).
- The 2, 10 and 30 s power-limit channels are the state of power for a
  pulse starting from the present state; the handshake holds the motors to
  the limit of the pulse going on (the 2 s values at a pulse's start).
- No pulse test has been compared with a published cell's data yet (that
  needs a licence-clear cell data set, CON-19).

Fuel cells have no ramp rate, and DC-DC converters have no power rating.

The Output Power Limit is ideal: it holds the terminal power (volts × amps)
exactly at every solver step, with none of a real limiter's lag or
overshoot, and it limits discharge only. Its check averages the solver
step's power (10 ms) over the *Power Check Window*, with nothing before
t = 0, not the samples of a competition's energy meter; a run that starts
at speed was already drawing power before t = 0. FSAE's rule that 100 ms
over the limit is a violation is not counted on its own (a window of 0
gives a check at least as strict). The *Voltage Class* check compares the
highest terminal voltage of a solver step, not a 500 ms average.

*Workaround:* for a real pack, fit the resistance tables to a pulse test
(HPPC) of its cells.
*Roadmap:* MOD-09 (cell temperature), MOD-51 (2RC and hysteresis), ENG-02
(follow-up).

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
  7.5-8 % faster than the real car's. Calibrate the grip and the downforce
  against a lap your car has driven (**Calibrate lap**, VAL-38) before
  trusting a lap time.
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
*Roadmap:* VAL-38 (a published check on a real logged lap), MOD-34 (a dynamic
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

- The endurance energy study varies only the capacity and the Output
  Power Limit of the first battery; it runs one endurance for each pair, a
  4 × 4 grid in about 3 min, and a larger pack keeps the car's mass (add
  the cells' mass to the Vehicle yourself). Grid studies of other
  parameters are STU-06's work.

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

### The Traction Control block is a simple slip loop

The *Traction Control* block (MOD-45) limits one demand for all the
motors it feeds, from the larger of two wheels' slip:

- It reads the slip one solver step late, so at the 10 ms step only low
  gains are stable (its defaults); at a 1-2 ms step (case Step 0.002 s)
  higher gains hold the slip within 0.005 of the target.
- It has no feed-forward from the tyres' load and grip, and no limit per
  motor: for hub motors, add one block for each motor and wire each its
  own wheel.
- LightSim's tyres keep their grip however much they slip, so holding the
  slip at the peak gains no time yet (MOD-16), and wet grip is the μ you
  set.

*Workaround:* run launches at a 2 ms step; tune Kp and Ki on the Slip
channel.
*Roadmap:* MOD-45 (feed-forward from the wheel loads, the tyre's peak slip
once MOD-16 exists), MOD-16.

### Lap mode calibration fits two numbers on one lap

**Calibrate lap** (VAL-38) fits only a grip factor and the CzA:

- Grip and downforce trade off on one lap, so the two values are not
  reliable on their own; the check lap's errors are.
- The track comes from the lateral acceleration over the speed squared:
  no GPS position, elevation or track width, and lap mode's other limits
  apply (ideal driver, no transients).
- It searches a grid (grip 0.6-1.5, CzA 0-5 m²) and takes about 30 s;
  values outside it are not found.
- It has been checked on LightSim's own laps only. No accuracy is claimed
  for a real car until a documented logged lap, with a licence that lets
  LightSim publish the result, has been used.

*Workaround:* fit on a lap with both slow and fast corners, check on a lap
from another session, and compare the energy error too.
*Roadmap:* VAL-38 (a published check on a real log), STD-35 (tracks from
GPS).

### Signal units are checked, not converted

Data Checks warn when a signal wire joins two different units (VAL-17),
such as a battery's *SOC* in % (0-100) into an input that expects 0-1, or a
vehicle speed into a rotational speed. Only ports with a unit are judged:
a Script's ports have one when you pick it next to the port's name, a PID's
inputs when you set its *Setpoint & Feedback Unit*, a Lookup Table's when
you set *Input X Unit* and *Input Y Unit*; a port left at *Not set* (No
Unit) is not checked. The motors', brakes', engine's and clutch's commands
are 0-1 (Fraction) in the library. Nothing is converted: the number on the
wire arrives as it is, and the run gives no message.

*Workaround:* set the units of your Script, PID and Lookup inputs, and read
the warnings in Data Checks; divide by 100 in a Script where a block expects
0-1.
*Roadmap:* automatic conversion on wires is not planned yet.

### An example's stored result holds a few signals

An example opened from **Open** or the *Start* page shows its stored
results (CON-15), named *Stored result*: the summary, the expected values
and seven comparison signals (vehicle speed and target, battery SOC and
power, motor speed and torque, engine fuel rate) every second. The other
channels appear only after **Run**. The stored runs are not saved with the
project and are not kept when LightSim restarts; the run lists call them
*not stored on disk*.

*Workaround:* press **Run** for every channel.

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
- **The energy breakdown leaves out inertia in the gears and reads the
  flows at the step's start.** Each run lists every part's energy in, out,
  lost and stored (*energy* in the run result; the parts' *Losses*, *Input
  Power*, *Braking Power* and *Slip Losses* channels). A gear's power is the
  motors', engines' and clutches' power reaching it, so the torque that
  speeds up the driveline's own inertia shows as the *Rotating parts*' store,
  not as a flow through each gear; a locked differential splits by what each
  side carried. The flows between parts are worked out separately, so their
  books together close only to within the *Energy balance residual* (0.01 %
  on the Battery Electric Car's City Cycle, 0.02 % on the hybrid's Mixed
  Cycle, 0.24 % on its UDDS, 0.39 % on the Formula Student car's 75 m acceleration, with its
  wheels spinning). Lap cases book the electrical parts per part, and the
  mechanics (road load, brakes, gears) as one Vehicle entry from the lap's
  own energy pass; they have no residual row (see *Lap energy balance
  error*). *Roadmap:* MOD-03 (gear losses with inertia), VAL-03 (energy
  audit table).
- **Resized machines follow simple scaling rules.** An E-Motor's *Speed
  Scale* treats the machine as rewound, with each point's loss that of the
  matching point of the original, as if through an ideal gear: a real
  faster-running rewind loses more in its iron at the higher frequency.
  The *Torque Scale* scales every loss with the active length, so end
  windings and bearings (which do not grow with it) are over-scaled for
  long machines. The Engine Scale keeps the fuel use per kWh; small engines
  really lose a little more to heat. Data Checks give the resized machine's
  peak torque, speed and power, but no chart of the scaled map beside the
  original yet. Use 50–200 %; beyond it, use the other machine's own maps.
  *Roadmap:* MOD-12 (e-drive upgrade), MOD-13 (engine Willans line).
- **A tyre code gives estimates, not the tyre's data.** The tyre estimates
  a Tyre Code fills in are the same for every size: Rill's guess for a
  passenger-car tyre on a dry road, scaled by its load index, with no speed
  rating, pressure, compound or wear. Racing and Formula Student tyres grip
  more (μ 1.4-1.7) and their codes carry no load index, so only their
  radius is filled in. The overload check uses the static load standing
  still, not the load transfer while braking or cornering. *Workaround:*
  replace the estimates with your tyre's test data. *Roadmap:* MOD-16
  (tyre model beyond μ and its load).
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
- **Stiff settings make runs slower, and some effects remain.** Before a
  run, LightSim checks whether the tyres' *Slip Stiffness* or a
  propeller-type load is too stiff for its 10 ms solver step and, if so,
  uses a smaller step (down to 0.5 ms) and says so in Data Checks and
  *Messages* (ENG-14): a *Slip Stiffness* of 20 or 30 runs at 5 ms, 100 at
  1.43 ms, 300 at 0.5 ms, each that many times slower (the FS Electric
  example, at 20, runs at 5 ms). Beyond that the run warns that the value
  is too stiff. The limit comes from the solver's stability grid (a launch
  and a car held braked, `backend/tests/test_numerics.py`): stable up to
  a gain of 2.94 on its scale, unstable from 3.92; LightSim keeps it at 3
  or less, so other manoeuvres may still differ. A value edited during a
  live run is not checked. A strong clutch on a light shaft is only noted
  (see *A closing clutch can ring* below): its step is not reduced. Below
  the limit, the step still shows at launch and at a stop. A car braked to a stop can also
  creep with the brake fully applied: under 0.2 km/h at the default *Slip
  Stiffness* of 10, about 2 km/h at 30 and up to 23 km/h at 300, at the
  10 ms solver step. When a car pulls away from rest faster than
  μ × 0.5 m/s ÷ (*Slip Stiffness* × step), 5 m/s² at the defaults and the
  10 ms step, its undriven wheels ring (their tyre force changes sign from
  one step to the next) until it reaches 0.7-2.3 m/s (measured on a 300 kg
  Formula Student car at μ 1-1.6); at a Formula Student launch this moves
  the 75 m time by about 0.4 %. *Roadmap:* ENG-09.
- **A closing clutch can ring at the 10 ms step.** While a clutch slips by
  more than 0.5 rad/s the solver passes its full torque for the whole
  step, and at 10 ms that overshoots the lock-up: the shaft on either side
  can swing by several hundred 1/min from one step to the next (up to
  770 1/min in the P2 Hybrid Car) for a few steps, now and then for a
  second or two, before the clutch locks. Energy is still conserved, but
  the fuel it costs follows the step: the P2 Hybrid Car's EPA city figure
  reads 2.838 l/100 km at the shipped 10 ms step, 0.017 (0.6 %) above a
  2.5 ms run (2.821), with the same engine starts; its highway and Mixed
  Cycle figures are about 0.004 above. Data Checks note a clutch whose
  torque can change its slip by more than 20 times that band (10 rad/s) in one
  step (the P2 Hybrid Car's: 65 rad/s), but the step is not made smaller
  for it. *Roadmap:* ENG-09.
- **Fuel-cell hydrogen use is a fixed figure per kWh** (*Specific H₂
  Consumption*, 55 g/kWh by default), which overstates it at part load by up
  to about a third and understates it at full load.
  *Roadmap:* MOD-20.

### Energy, duty and limit reports

- **The gears, clutches and spinning parts are one row, worked out from
  what is left.** The *Energy* view measures each motor, engine, battery,
  DC-DC converter, consumer, friction brake, propeller and tyre, and the
  car's road load and speed, but no part of the driveline between them
  reports its own losses yet. Their row is the motors' and engines' shaft
  energy less what the wheels, brakes and propellers took, so it also holds
  the clutches' slip and the change in the spinning parts' speed, and it
  cannot show a mistake in the driveline. *Roadmap:* MOD-10.
- **The books sample every fourth solver step.** The energy, the duty and
  the limit band add up every fourth solver step (40 ms at the usual
  10 ms), weighted by the time since the last, so a run is about 5 %
  slower (measured: 5.5 % on the Battery Electric Car, 1.8 % on the P2
  Hybrid Car). What sampling misses shows in *Not accounted for*: at most
  0.14 % on the examples' cycles (the hybrid's EPA city cycle), 0.48 % on
  the Formula Student car's 75 m acceleration, where the tyres spin hard;
  a state shorter than 40 ms can be missed by the limit band. *Roadmap:* ENG-16.
- **Fuel energy uses one heating value.** The Sankey's fuel energy is the
  fuel burnt × 43 MJ/kg, a petrol value (background knowledge, not checked
  against a source); hydrogen uses 33.3 kWh/kg. A diesel or other fuel's
  engine losses are off by the difference in heating value. *Roadmap:*
  RES-22.
- **A wheel's In and Out are net.** The table gives each wheel the energy
  its shaft gave it less what braking took, and the same for the car, so a
  wheel that drove and braked shows the difference; its *Lost* (the tyre's
  slip) is complete.
- **A motor's current is its DC current.** The *Duty* view's *DC current*
  is the motor's electrical power over its bus voltage; the current in the
  motor's windings (phase current), which sets the inverter's sizing, is
  not modelled. *Time above* comes from the stored points, not the solver
  steps. *Roadmap:* RES-39.
- **The limit band is a rule of thumb.** Each step is named by the first
  state that applies, in a fixed order: braking, tyre grip, set power
  limit, battery or supply, motor or engine, coasting, demand met. *Motor
  or engine* means the driver asked for at least 99.9 % of the torque, a
  motor was within 2 % of its maximum speed, or an engine gave 99 % of its
  full-load torque; a control script that asks for less than full torque
  never shows it. The band is drawn only on a time axis. *Roadmap:* RES-38.
- **Change marks compare with the run's stored model.** The dots and the
  *These results are from before …* note compare the model on screen with
  the copy the run kept: a run stored before 0.2.0 kept none and gets no
  marks, edits made while a run was going are not counted, and a part or
  wire you removed is counted in the note but has nothing to carry a dot.
  *Roadmap:* UX-41.

### Live edits, charts, sweeps and export

- **Sweeps run side by side, but single runs do not.** A sweep's runs go
  in worker processes, one per processor core less one (ENG-05; LightSim
  counts logical processors, so on a computer with hyper-threading it may
  start more than its physical cores less one), and fewer when half the
  computer's memory would not hold them (250 MB each, plus 512 MB for a
  model with Script blocks). Each worker starts the engine afresh for a
  sweep (about a second) and builds the model again for every point. A
  sweep's runs do not draw live, and they are stored on disk and read back
  when the sweep ends, so a sweep larger than 20 points shows only its 20
  newest runs in *Results* (its study table keeps every point). A single
  run (**Run**) still goes in the app's engine process. A study varies one
  parameter at a time from the app; the engine's `/api/studies` takes any
  list of points. *Roadmap:* PLT-09 (single runs in a worker), STU-06
  (studies of several parameters), AI-12 and PLT-25 (the command line and
  clusters).
- **Peaks between recorded points are stored but not drawn.** Since 0.3
  every stored point also keeps each channel's lowest, highest and
  time-averaged value since the point before it, taken at every solver
  step (ENG-16), so a regeneration burst between two points is in the run:
  with *Store every* 10 on the Battery Electric Car's City Cycle, the
  battery's recorded points go down to −9.56 kW and its stored lowest
  value to −11.19 kW, the same as every solver step. The *Results* charts,
  cursors and CSV export use the recorded points only and do not show
  these values yet; runs stored by earlier versions do not have them.
  Keeping them makes a run about 15 % slower (none are kept, at no cost,
  when each point is one solver step: a *Step* of 0.01 s or less and
  *Store every* 1) and its file about 5 times larger, so the runs of a
  parameter sweep keep their recorded points only. *Roadmap:* RES-17.
- **The Results page shows at most 3 decimals.** Since 0.3, stored values
  and summary numbers keep full precision (ENG-16): one more kilogram on
  the Battery Electric Car changes its City Cycle's consumption and final
  SOC, and the energies equal the solver steps' sum to 1e-6. The *Results*
  page shows a number with at most 3 decimals, so a change smaller than
  that reads +0.000 against the baseline; the run's file, its CSV and
  .mat export and the study tables' CSV have every digit. *~ 0* now marks only runs stored by
  earlier versions, which kept 5 decimals (2 to 4 in the summary).
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

- **Efficient Electric Sedan:** a Tesla Model 3 RWD class car with EPA's
  test mass, road load, rated power and gearing, but the Battery Electric
  Car's motor maps scaled to 192 kW, not the car's own (more efficient)
  motor, and a 54 kWh battery from FASTSim's file, which gives no source.
  Its city figure comes out about 2 % better and its highway figure about
  4 % worse than EPA's tests of the car. Only one of the seven
  question-led examples the roadmap plans ships so far (gear ratios,
  diesel against petrol, a two-wheeler, a truck, a gear-by-battery study
  and control recipes are still to come). *Roadmap:* CON-07.
- **P2 Hybrid Car:** sized after the Hyundai Ioniq Hybrid, with its test
  mass and road load from EPA data (EPA's own coefficients A/B/C, with the
  axle's losses counted once), but its engine, motor and battery maps
  are generic, not the car's. With its charge-sustaining control script it
  uses about 2.84 l/100 km on the EPA city cycle and 3.24 on the highway
  cycle, against 2.91 and 2.94 for the real car in EPA's tests. The model
  has no cold start, engine warm-up or start-up fuel, so its city figure
  reads below EPA's, whose city test starts cold; on the highway, with its
  generic maps, it stays about 10 % above. Each case starts at the charge
  the cycle ends with (as a preconditioning drive would leave it), and its
  cases run charge-balanced (see below), so started at any charge they
  give the same fuel figure. With *Charge balance* off, the summary's
  *Fuel consumption, charge-corrected* estimates the balanced figure from
  one run by counting the battery's energy at the engine's average
  efficiency. *Roadmap:* CON-14 (sourced maps).
- **Battery Electric Car:** modelled on the 2021 Cupra Born with FASTSim's
  values; about 14 kWh/100 km on WLTC at the battery and 16.3 at the
  charging socket with the default 86 % charger efficiency (a car of this
  class is rated about 15-16 kWh/100 km at the socket), 18.9 at the battery
  with heating or air-conditioning on (the 2.5 kW case). The socket figure
  rests on one charger efficiency for every charge; a real charger's
  efficiency changes with its power and the battery's temperature.
  Its motor loss map is generic, not the car's measured map. The real car is
  rear-wheel drive and has an 11.5:1 reduction gear with an electronic
  160 km/h limit; the example drives the front axle (its CG height is 0,
  so its loads do not shift and only the load share matters) and uses a
  12.8 ratio so that the motor's maximum speed sets the 160 km/h. An
  E-Motor's *Maximum Speed* could now set that limit, but the example
  still sets it through the ratio.
  *Roadmap:* MOD-12.
- **FS Electric (generic):** a typical Formula Student electric car, not a
  real one: replace its values with your car's. Its 75 m time (3.75 s from
  the start line) sits in the faster half of FS Czech Republic 2025's
  3.51-6.44 s: the tyres keep their grip however much they slip (no peak
  and drop), so wheelspin at the launch costs no time, and nothing limits
  it (the example has no traction control; 49 % of the run is at the
  tyres' grip limit). The *Traction Control* block (MOD-45) holds the
  slip near a target, but cannot gain time until the tyres lose grip past
  their peak (MOD-16).
  So its 80 kW is reached at 0.37 s, while the rear wheels still spin: a
  car with traction control reaches it later. Its tyres' *Slip Stiffness*
  of 20 (not 10) makes LightSim run it at a 5 ms step instead of 10 ms
  (ENG-14).
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
  own cases hardly depend on it. The 500 A current limit (EV 2.2.2) is
  not set in the example (its cases stay under 160 A; the battery's preset
  sets it), and its pack is defined by pack values, so the cells' own limits
  are not checked:
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
  change and do not affect motors or engines. The Ambient component sets
  the air density (see above), the Climate Control's outside temperature
  and the cell temperature of a battery built from cells, and thermal or
  fluid connections are ignored during a run.
  *Roadmap:* MOD-09.
- **Heating and air-conditioning are a steady-state estimate.** The
  Climate Control draws the power its demand table gives at the outside
  temperature from the first second: there is no cabin that warms up or
  cools down, so a cold start's first minutes (when a heater runs at
  5-7 kW) are missing and short trips use too little. Its default table
  is an estimate for a compact car, not measured data, and it ignores the
  sun's angle, humidity, speed and the number of people inside.
  *Workaround:* fit the demand table to logged heater and
  air-conditioning power for your car. *Roadmap:* MOD-46 (cabin model).
- **Forward driving only.** No reverse, and no rolling back: a car on a steep
  hill stays put even with no brakes. *Roadmap:* MOD-21, ENG-21.
- **Drive cycles are longitudinal only.** A drive cycle, performance or
  acceleration case does not corner: weight shifts between the axles but
  not from side to side. Lap cases corner, as a quasi-steady-state
  estimate (see *Lap mode is a quasi-steady-state estimate*). Tyre force
  rises with slip and then stays flat (no peak and drop). *Roadmap:*
  MOD-16, MOD-34.
- **Charge balancing repeats the whole run.** A hybrid's cycle case is run
  again from the charge its battery ended with until the battery's stored
  energy changes by less than 1 % of the fuel's energy (ENG-33): started
  at 30, 50 or 70 %, the P2 Hybrid Car's Mixed Cycle gives 2.878 l/100 km
  in 2 runs, against 2.8777 at its hand-set start. Each extra run takes as
  long as the first, and the *Results* page shows only the last run (the
  others are listed in *Messages*, not kept in the run history). A live run
  shows the first run as it goes; later runs show only when they finish. A
  parameter changed during a live run stops the balancing. The 1 % is
  SAE J1711's criterion as we know it (background knowledge, not checked
  against the standard), and the fuel's energy comes from the fuel tank's
  *Lower Heating Value* (43 MJ/kg unless set). When the charge does not
  settle in 5 runs, the run is a *warning* and the summary adds the fuel
  figure corrected to no change of charge, from a straight line through
  the runs. A model with several batteries balances each from its own end
  charge. *Roadmap:* ENG-33 (the extra runs kept in the run history).
- **A simple driver.** The Driver is a PI speed follower: it does not look
  ahead along the cycle or shift gears; gear and clutch logic comes from
  Script blocks. *Roadmap:* MOD-14.
- **A speed against distance has no stops.** A Driving Task whose *Profile
  Axis* is *Distance* gives the Driver the target speed at the distance
  the car has driven. A point of 0 km/h stops the car there for good (Data
  Checks warn): there is no stop with a waiting time, and a profile that
  starts at 0 km/h never sets off, so start it at a small speed (for
  example 5 km/h). The Driver does not brake ahead of a slower point; it
  follows the target where the car is, so it reaches a slower point a
  little late: on the Battery Electric Car at 100 km/h it starts braking
  2.9 m after the profile starts to slow at its own 1,927 kg and 4.7 m
  after at 2,500 kg. The case's *Duration* is
  the time limit of a run that ends after a number of *Laps*. The trace is
  judged against distance: each point's band spans the target's lowest and
  highest value within the distance the car covers in ±1 s at the target
  speed (at least ±2 m), widened by ±2 km/h. *Roadmap:* MOD-14 (stops and
  look-ahead), CON-11 (cycle files with a distance column).
- **Structural limits.** One battery or voltage source per electrical bus;
  DC-DC converters work in one direction only; one differential and one
  E-Motor per driveline (several independent drivelines, such as dual-motor
  all-wheel drive as two axles, work). Sub-system containers are for
  organising only: physical connections cannot cross a container boundary
  (signals can, through the Data Bus).
- **Unfinished parts of the app.** The Optimization tab is hidden until it is
  implemented, and the canvas bookmark tool is disabled. *Roadmap:* STU-04.

### Files in and out

- **The .mat export is checked with SciPy and GNU Octave, not with MATLAB
  itself yet.** The tests read every exported channel back through
  SciPy's `loadmat` with the same values and units, and the files load in
  Octave 8.4. Reading them in MATLAB, and `lightsim_run.m`, are a manual
  check before each release; text with characters beyond ASCII (N·m, °C)
  is stored as UTF-16, which MATLAB documents but which that check must
  confirm. *Roadmap:* STD-09.
- **No Parquet or HDF5 export yet.** Results go out as .mat, CSV and the
  run card (JSON); Parquet comes with the Python package (AI-02).
  *Roadmap:* STD-09.
- **`lightsim_run.m` is not installed with the app.** Copy it from the
  `matlab` folder of LightSim's source. With the AppImage, give it the
  engine's path (the engine lives inside the AppImage; extract it with
  `--appimage-extract`). *Roadmap:* STD-09, AI-02.
- **The table import reads values, not formulas or formats.** From an
  `.xlsx` file it takes the value Excel saved with each formula; a
  workbook saved by a program that does not store those values (some
  scripts that write Excel files) gives empty cells. Dates and times are
  read as Excel's day numbers. Old `.xls` and OpenDocument `.ods` files
  must be saved as `.xlsx` or CSV first. *Roadmap:* STD-10.
- **A unit LightSim does not know is refused.** The import converts the
  common units of speed, rotational speed, torque, power, energy,
  voltage, current, charge, mass, mass flow, distance, time, temperature,
  force, pressure, curvature and resistance. Other units (for example
  kg·m² written as g·cm²) must be converted in the file first. A speed or
  grade without a unit is guessed from its values, and the preview asks
  you to confirm. *Roadmap:* STD-16.
- **The parameter sheet does not hold scripts, case values or when a value
  changed.** Script blocks stay in the project; a case's own values
  (*Cases & Parameters*) are not in the sheet; the *Source* and *Notes*
  columns are for your team and are not read back. *Roadmap:* STD-36,
  UX-24.

## Using and installing the app

- **Stored runs have a disk budget.** Finished runs are kept on disk with
  their project, up to 500 MB per project and 2 GB in all; past that the
  oldest are deleted (runs of projects that were never saved go first), and
  the app shows at most the 20 newest. Export runs you need to keep for
  good to CSV or as a MATLAB .mat file. *Roadmap:* RES-02, RES-09.
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
    any file, starting programs and making or accepting TCP connections.
    Not blocked: other network traffic (UDP) and local sockets. On older
    Linux, only the memory cap and a limit that stops it writing data into
    files apply; it could still delete files.
  - *Windows:* the process has the memory cap, ends when LightSim ends,
    cannot start programs and runs at Windows' low integrity level, so it
    cannot change your files. It can still read files and use the network:
    blocking that needs a more locked-down kind of process (an
    AppContainer), not built yet.
  - *macOS:* Apple's sandbox stops the process writing files, starting
    programs and using the network; it can still read files. This is
    tested on GitHub's Mac runners only, as there is no Mac release yet.

  Scripts that came with a project from another computer run only after
  you have seen their code and chosen **Run scripts**
  ([how](help/how-to/open-a-project-with-scripts.md)). All this makes a
  harmful script much harder to write, not impossible: approve scripts only
  from people you trust. *Roadmap:* PLT-35 (Windows network and file
  reading).
- **FMU blocks are a first version.** An FMU block (a model from another
  tool, see [Use a model from another tool](help/how-to/use-an-fmu.md))
  runs Co-Simulation FMUs of FMI 2.0 and 3.0 only. Not yet:
  - Model Exchange FMUs (they need LightSim's solver), FMI 1.0 FMUs, and
    FMUs with source code only: LightSim does not compile them.
  - Variables that are not single numbers: arrays, text, binary data and
    clocks cannot be pins. Integers and on/off values pass as numbers.
  - Units: a pin passes the number as it is, in the FMU's unit, as for any
    signal (see *Signal units are not checked*).
  - Changes during a live run: start values apply when the run starts.
  - Iteration: values pass once per communication step, so a signal loop
    through an FMU and back arrives one step late, as between Script
    blocks.
  - Lap cases do not run signal blocks, FMUs included.

  The FMU file is not saved inside the project yet: LightSim keeps a copy in
  your LightSim folder (`fmus`, beside your projects) and the project
  points at it. On another computer, import the FMU again. *Roadmap:*
  STD-02.
- **FMU support is an optional pack.** It needs FMPy (BSD-2-Clause) and its
  NumPy, lxml, attrs and lark. The desktop installers do not include it
  yet; without it, Data Checks say so and every other model runs. *Roadmap:*
  STD-01.
- **FMUs run in a separate, locked-down process, but how locked-down
  depends on your system.** An FMU is compiled code from another company or
  tool. LightSim runs it only after you allow it on your computer (once per
  FMU file), and never inside the engine: each FMU block gets a process of
  its own that the engine stops if a step takes longer than 30 s, with a
  2 GB memory cap. A crash ends that process and the run, not LightSim.
  - *Linux 5.13 or newer:* the process can read only the FMU's own files
    and the system libraries, writes no files and makes no TCP connections.
    Not blocked: UDP and local sockets.
  - *Windows:* the process has the memory cap and ends when LightSim ends,
    but nothing stops it reading or writing your files or using the
    network.
  - *Older Linux:* only the memory cap and a limit that stops it writing
    data into files apply.

  Allow FMUs only from people you trust. *Roadmap:* PLT-02.
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
- **Weather presets set only the air.** The Ambient's presets (cold,
  standard, hot and sunny, high altitude) set its temperature and pressure,
  so only the air density follows them: no heating or air-conditioning
  load, no sun, no cold battery or engine, and engine power does not fall
  with altitude. The electric car's winter and hot-day cases add a fixed
  2.5 kW load instead. Hourly weather files cannot be loaded.
  *Roadmap:* MOD-41, MOD-09, CON-30 (second step).
- **Templates: slots are names only.** A template's slots say which part
  plays which role, but swapping a slot's part for another while keeping
  its wiring, a shared signal naming convention across templates (so one
  control script runs on several), two-motor, series-hybrid, fuel-cell,
  petrol and two-wheeler templates, and a picture per template are not
  there yet. *Roadmap:* CON-18 (follow-up), UX-33, UX-16.
- **Value sources stop at the project file.** The sources and confidence
  levels recorded for a part's values (CON-13) are saved with the project
  but are not yet listed in result exports or run reports, carry no
  uncertainty (±) a study could sample, and the Formula Student example's
  values have none recorded yet. *Roadmap:* RES-14, STU-23, VAL-37.
- **Vehicle tests leave out a few.** *Vehicle tests* has no hill start, no
  range test that drives a battery down to empty over repeated cycles (the
  summary's *Range at this consumption* estimates it from one cycle), and
  no elasticity test held in one gear. A fuel-cell car's consumption is
  its battery's share only, and it gets no range. *Roadmap:* CON-06
  (follow-up), STU-39.
- **US label estimate from two cycles only.** *Simulations → US label*
  uses EPA's derived two-cycle method. The five-cycle tests (US06, SC03 at
  35 °C, a cold FTP at −7 °C) need heat and climate models LightSim does
  not have, and plug-in hybrids (charge-depleting runs and utility
  factors) are not covered. Nor are fuel-cell cars: a model with a fuel
  cell or a voltage source is refused, since EPA's hydrogen rule (a
  kilogram counted as a gallon) is not built in. *Roadmap:* CON-21.
- **27 standard drive cycles, no files of your own.** The Driving Task's
  *Drive Cycle* list has the WLTC (classes 1 to 3b, their city cycles and
  phases), NEDC, the EPA cycles, two motorcycle cycles and a long-haul truck
  route. Cycle files of your own (CSV, Excel, a logged lap against
  distance) cannot be added to the list yet: type or paste their points
  into the Profile. Japan's JC08 and WLTC, China's CLTC and the Artemis
  cycles are not included (CLTC and Artemis may never be, because their
  terms do not allow LightSim to ship them). Only the long-haul route
  carries a road grade. The FTP-75 and the motorcycle FTP leave out the
  real test's 10-minute soak, and nothing models a cold start. A project that names a drive cycle, opened in
  0.2.0, drives the typed profile instead, with no warning. *Roadmap:*
  CON-34, STD-10, STD-35, PLT-07.
- **Few starting points.** The *Start* page offers the examples that come
  with LightSim and a blank project. Ready-made starting points for other
  layouts (two motors, a fuel-cell car) and templates that ask a few
  questions first are not there yet: start from the example closest to
  your car and change it. *Roadmap:* CON-18, PLT-32.
- **Project files outside the projects folder: what is missing.**
  `.lightsim` files open from anywhere, but:
  - The AppImage does not register the file type, so a double-click does
    not open LightSim there; use **File → Open…**, or the .deb package. No
    macOS version yet.
  - Runs next to a `.lightsim` file count against the 500 MB per-project
    budget but not the 2 GB total: delete runs you no longer need.
  - LightSim notices a change on disk by looking every 4 s while its window
    is in front, not at once.
  - **Save As** a project that was already saved makes a copy with an id of
    its own; its runs and studies stay with the original.
  - Runs and backups always go next to the file; keeping them in the app's
    own folder instead is not a setting yet.
  *Roadmap:* PLT-33.
- **Attached files are kept, not used yet.** A project can carry files
  (Project → Attached): they are copied into its resources folder, travel
  with Save As, **Export** (a `.lightsim.zip`) and **Import**, and Data
  Checks report one that is missing or changed. No part reads an attached
  FMU, AI model or data file yet. A file is one level deep (no folders
  inside resources) and at most 1 GB. *Roadmap:* STD-02, STD-08.
- **The trust question for attached files protects runs in the app only.**
  Before the first run of a project with attached FMUs, AI models or
  programs, LightSim asks whether you trust it, and remembers the answer by
  a fingerprint of those files (a changed file asks again). The question is
  asked by the app's window; the engine itself does not refuse to run them
  (an FMU block runs only an FMU you allowed on this computer). Script
  blocks are not part of this question: their code is shown for you to
  approve, and the engine refuses Script code you have not approved (see
  the scripts entry above). *Roadmap:* STD-02, PLT-02.
- **Study tables are keyed by the figure's name.** A study's results table
  names each column by the summary figure's label; a figure renamed in a
  later version starts a new column. *Roadmap:* PLT-34.
- **Scripting LightSim from Python or a terminal: early version.** The
  `lightsim` Python package and command-line tool (see *Python API* and
  *Command-line tool* in the help) run from the `backend/` folder of the
  repository, as the desktop engine's `lightsim-backend run …`, or as a
  wheel you build with `scripts/build-wheel.py`; it is not on PyPI yet.
  *Roadmap:* AI-02, AI-07.
- **AI access is set from the command line only.** AI assistants are off
  until you turn them on with `lightsim ai on` and allow folders with
  `lightsim ai allow`; there is no *Settings → AI access* page in the app
  yet, and no switch in the app to hide one project from AI tools (use
  `lightsim ai block <file>`). On Windows, a run of a trusted project with
  Script blocks opens a private connection on 127.0.0.1 between the engine
  and its script process for a moment; on Linux it uses no network at all.
  *Roadmap:* AI-01.
- **Unsigned installers, until the owner buys a certificate.** The build
  can sign every Windows file, but only once the owner has a code-signing
  certificate. Until then Windows SmartScreen warns on first launch (choose
  *More info → Run anyway*), company antivirus may treat the unsigned
  engine (`lightsim-backend.exe`) with suspicion, and IT cannot allow
  LightSim by its publisher. Each build is scanned with Microsoft Defender,
  which finds nothing. *Roadmap:* PLT-32.
- **No macOS version yet.** The build makes and tests a Mac version for
  Apple silicon, but it is published only once it can be signed and
  notarised by Apple, which needs the owner's paid Apple Developer account.
  Until then there are builds for Windows 10/11 (x64) and Linux (x64) only.
  There is no build for Intel Macs. *Roadmap:* PLT-13.
- **Updates need a yes, and some installs only point to the download.**
  LightSim checks for updates only after you agree (it asks the first time
  it opens). The .deb package, MSI installs and unsigned Windows builds do
  not install updates themselves: they open the download page. There is
  one update channel (no beta) and no way back to an older version from the
  app. *Roadmap:* PLT-18, PLT-31.
- **The policy file has no Group Policy template.** IT fixes settings with a
  `policy.json` file ([how](help/how-to/deploy-for-it.md)), not through
  Group Policy's administrative templates (ADMX) or the registry. Its `ai`,
  `aiProviders` and `licenceFile` keys change nothing yet. *Roadmap:*
  PLT-36.
- **The help is a first draft.** F1, or the **?** menu at the top right,
  opens LightSim's help in a panel inside the app, served from your
  computer. If a page does not match what the app shows, the app is right.
  The numbers the tutorials, lessons and example pages quote are checked
  against the app by the automatic tests; the words around them and the
  click paths are not, and there are no pictures of the app beyond the
  quick start's. The help is not online yet. The Formula Student lessons
  cannot read a lap trace from your logger or lap simulator (roadmap
  STD-35) and cannot export a design-review pack (RES-14); they say what
  to do instead. *Roadmap:* LRN-04 (follow-up: online), LRN-12 (replaying
  the tutorials' clicks in the browser tests).
- **The first-steps tour is short.** It points at the screen's main parts
  only; it does not walk you through a run, and its steps are not checked
  against a band. The step bar's *Set values* ticks on any change of a
  part's value. Automated browsers get neither the tour nor the bar.
  *Roadmap:* UX-26, LRN-07.
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
- **The AI connection is new and partly provisional.** The MCP server
  follows the 28 July 2026 revision as the official MCP SDK (version 2.3)
  implements it; its Tasks support (long runs) uses the task shapes of the
  2025-11-25 revision, because the Tasks extension's own messages were not
  available to check against, so an AI app may run cases without a task
  and wait for them. Whether a project with Script blocks may run on
  Windows is set for the whole connection (`--trust-scripts`), not per
  project; the folders an assistant may see are set when it connects, not
  in LightSim's settings; and the only way to hide one project is
  `"noAI": true` in its file. A change an assistant saves to a project
  that is open in LightSim does not show there until you open it again,
  and a save from the app over it is refused as a conflict (the earlier
  version stays in *Restore…*). Connect AI writes the AI apps' settings
  files where those apps kept them in 2026; an app that moves its file
  needs an update to LightSim. On the Linux AppImage the AI app can start
  LightSim only while LightSim is open (install the .deb instead). The
  skill pack has not yet been measured with an AI benchmark. *Roadmap:*
  AI-01 (settings, trusted projects), AI-04 (edits in an open window),
  AI-16 (benchmark).
- **Licence.** LightSim is proprietary (`LICENSE`). The desktop app is free
  for evaluation, learning, research and other non-commercial use under its
  end-user licence agreement (`EULA.txt`, installed with the app);
  commercial use needs a separate licence from the owner. The owner sets
  these terms, and they may change in a later release. The open-source parts
  inside the app keep their own licences, listed in `THIRD-PARTY-NOTICES.txt`
  (*Help → Third-Party Notices*). *Roadmap:* BIZ-01, BIZ-03.
- **Some licence questions have no clear answer yet.** The licence
  agreement does not define "non-commercial", so it does not say clearly
  whether a sponsored Formula Student team, a thesis written at a
  company or an industry-funded university project is free. It allows
  installs on "your own computers" only, so not on university lab PCs or
  a company's software portal, and it has no trial for companies. Licence
  questions go to a public GitHub issue: there is no private address
  yet. A [draft FAQ](licensing/licence-faq.md) answers 20 cases and marks
  the unclear ones *Ask*; a [proposed revision](licensing/EULA-proposal.md)
  awaits the owner's approval. *Roadmap:* BIZ-29, BIZ-30.
