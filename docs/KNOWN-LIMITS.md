# LightSim: known issues and limits

LightSim 0.2.0 is an early version. You can build, run and inspect models,
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
- Last reviewed: 23 September 2026, for version 0.2.0. This page is updated
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
before 0.3); *Linear* extends the edge slope. Motor and engine speed and
torque axes and the fuel cell's current stop the run by default. When a run
reads a table outside its data, or a motor or engine goes above its maximum
speed, Messages says so once and the run summary lists for how long (as a
share of the run) and how far; these rows appear only when that happened.
What remains:

- Above its maximum speed a motor gives no regeneration either. A car with
  no friction brakes running downhill past it speeds up further than before.
- Battery SOC, motor voltage, drag-torque and Lookup tables hold their edge
  value by default. Set an axis to *Error* to be stopped there instead.
- Below the first speed of its full-load curve (starting, stalling) a fired
  engine gives that point's torque and burns that point's fuel: a start-up
  rule, not a model of starting.
- An engine's rev limiter is a hard cut, so it overshoots its limit by up to
  a solver step's acceleration (about 1 %); only more than 2 % above it
  counts as over speed.
- A run that went outside its data still ends as *success* if it followed
  its cycle: the run status does not judge these counters yet.
- A project from 0.2.0 whose loss or fuel map is narrower than its
  full-load map, or whose fuel map does not reach the full-load curve's
  first speed, now stops with an error where it used to hold the edge
  value. Data Checks warn about it before the run.

*Workaround:* read the summary rows and Messages after a run; where a run
stops, extend the table or set that axis to *Clamp* or *Linear*.
*Roadmap:* VAL-39 (judging the counters in the run status).

### A "success" checks the speed trace, not the physics

A run is a *success* when the vehicle stayed within ±2 km/h and ±1 s of its
target speed for all but 1 % of the run (at least 2 s), covered the cycle's
distance, and nothing raised a warning. It does not judge how long motors
and engines spent outside their maps (see above) or that the numbers are
plausible for a real vehicle, and the Data Checks all-clear does not vouch
for the results either. Also:

- An acceleration or top-speed test driven by a step in the target speed
  (for example `0:100; 600:100` from standstill) is outside that band while
  the car accelerates. It ends with a *Cycle not followed* warning, and its
  *Consumption* is marked *not valid*.
- A cancelled run ends as *warning*; there is no separate status for it. Its
  per-distance figures are marked *not valid: run cancelled at t = …*.
- The tolerance (1 % of the run, at least 2 s) is LightSim's own choice:
  test procedures such as WLTP set no allowance for a simulation.

*Workaround:* read the Messages panel and the *not valid* notes in the
summary table. Read step-target tests for their speeds and times, not their
consumption, or ramp the target up instead.
*Roadmap:* VAL-39, VAL-08.

### Only its internal resistance limits what a battery delivers

A battery delivers power up to its maximum-power point (the most its
internal resistance lets through: about 420 kW for the default pack at 90 %
charge) and takes back up to its *Max Charge Power*. There are no current or
voltage limits, fuel cells have no ramp rate, and DC-DC converters have no
power rating, so a model is never held back by these.

*Workaround:* check the battery's *Discharge Power* channel against what the
real pack or its management system allows, and reduce the motor's torque
map or add a limit in a Script if needed.
*Roadmap:* ENG-02 (follow-up).

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
minimum or an average, for example from the CSV export.

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
  the driveline shows the same value. In the P2 Hybrid Car example with a
  Shaft added between the engine and the clutch, at t = 281 s the engine
  delivers 20.9 kW and the motor takes 8.8 kW to charge the battery, and
  the Shaft and the Final Drive both show 12.1 kW. Read the *Mechanical
  Power* of each motor and engine instead. *Roadmap:* MOD-10.
- **Air density is fixed, and steep grades are overstated.** Air drag always
  uses 1.2 kg/m³: the Ambient block's temperature and pressure are ignored,
  and there is no wind. Real air is about 10 % denser at −7 °C, and about
  20 % thinner at 35 °C and 85 kPa (about 1,500 m altitude). The slope
  force uses the grade in % divided by 100 instead of the sine of the slope
  angle, and rolling resistance ignores the slope, so both are 0.5 % too high
  at a 10 % grade and 3 % at 25 %. To model cold or thin air, multiply the
  Vehicle's *Drag Coefficient (Cd)* by the real density divided by 1.2, and
  keep grades moderate. *Roadmap:* MOD-11.
- **Wheel loads do not shift when braking, accelerating or cornering.**
  Each wheel carries a fixed share of the vehicle weight (*Vehicle Load
  Share*); the shares of the connected wheels are scaled to add up to
  100 %, and Data Checks say when they had to be. *Roadmap:* MOD-16.
- **An engine behind a script-controlled clutch starts at rest.** A run that
  starts at speed starts every wheel, gear and motor at that speed, and an
  engine behind a closed clutch too; a clutch a Script controls counts as
  open at t = 0, so the engine behind it starts at rest. *Roadmap:* MOD-19.
- **Stiff settings can cause short wheel-spin spikes at launch.** This is a
  numerical effect of how the solver steps the tyres: a high tyre *Slip
  Stiffness*, a very light inertia or a strong clutch can push the solver
  past its stability limit, and nothing warns when that happens. Keep *Slip
  Stiffness* near its default, and after changing these settings plot the
  wheels' *Longitudinal Slip* at launch. *Roadmap:* ENG-09, ENG-14.
- **Fuel-cell hydrogen use is a fixed figure per kWh** (*Specific H₂
  Consumption*, 55 g/kWh by default), which overstates it at part load by up
  to about a third and understates it at full load.
  *Roadmap:* MOD-20.

### Live edits, charts, sweeps and export

- **Values between recorded points are not stored.** Charts keep the
  highest and lowest value of each stretch they thin for drawing, but with
  *Store every* above 1 the values in between recorded points are never
  stored, so a short spike or dip between them does not show. Use *Store
  every* 1 when peaks matter. *Roadmap:* RES-17, ENG-16.
- **Stored values are rounded.** Every stored value is rounded to 5 decimal
  places, so small values keep few digits (a tyre slip of 0.0018 keeps two).
  The summary rounds energies to 1 Wh (battery losses to 0.1 Wh), fuel to
  1 g and consumption to 0.01 per 100 km. CSV export has the same rounding. Treat smaller differences
  between runs as noise; to compare two close variants, lengthen the run
  (for example, repeat the cycle) so that the difference adds up.
  *Roadmap:* ENG-16.

## The examples

- **P2 Hybrid Car:** sized after the Hyundai Ioniq Hybrid, with its test
  mass and road load from EPA data, but its engine, motor and battery maps
  are generic, not the car's. With its charge-sustaining control script it
  uses about 3.0 l/100 km on the EPA city cycle and 3.3 on the highway
  cycle, against 2.91 and 2.94 for the real car in EPA's tests. The model
  has no cold start, engine warm-up or start-up fuel, and it counts the
  driveline drag that EPA's road-load coefficients already include a second
  time. Each case starts at the charge the cycle ends with (as a
  preconditioning drive would leave it), so the fuel figure needs no
  battery-charge correction; start it elsewhere and the figure includes the
  charge the strategy restores. *Roadmap:* CON-14 (sourced maps).
- **Battery Electric Car:** modelled on the 2021 Cupra Born with FASTSim's
  values; about 14 kWh/100 km on WLTC at the battery (a car of this class is
  rated about 15-16 kWh/100 km at the charging socket, charging losses
  included), 18.9 with heating or air-conditioning on (the 2.5 kW case).
  Its motor loss map is generic, not the car's measured map. The real car is
  rear-wheel drive and has an 11.5:1 reduction gear with an electronic
  160 km/h limit; the example drives the front axle (only the load share
  matters without weight transfer) and uses a 12.8 ratio so that the motor's
  maximum speed sets the 160 km/h. An E-Motor's *Maximum Speed* could now
  set that limit, but the example still sets it through the ratio.
  *Roadmap:* MOD-12.
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
  component is a placeholder (it does not set the air density either; see
  above), and thermal or fluid connections are ignored during a run.
  *Roadmap:* MOD-09.
- **Forward driving only.** No reverse, and no rolling back: a car on a steep
  hill stays put even with no brakes. *Roadmap:* MOD-21, ENG-21.
- **Longitudinal dynamics only.** No cornering and no weight transfer between
  axles. Tyre force rises with slip and then stays flat (no peak and drop).
  *Roadmap:* MOD-16.
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
- **Some colours are too faint in the dark theme.** The ribbon title, the
  Run button, the status bar's "backend connected" and "success" in the log
  fall short of the WCAG AA contrast minimum. Switch to the light theme if
  they are hard to read. *Roadmap:* GUI-02.
- **Unsigned installers.** Windows SmartScreen warns on first launch (choose
  *More info → Run anyway*). *Roadmap:* PLT-13.
- **No macOS version.** Builds exist for Windows 10/11 (x64) and Linux (x64)
  only. *Roadmap:* PLT-13.
- **No automatic updates yet.** Download a newer version from the GitHub
  Releases page and install it over the old one; your projects are kept.
  *Roadmap:* PLT-18.
- **Little help in the app.** There is no user manual, tutorial or
  explanation of individual parameters yet; the README's quick start is the
  only walk-through. *Roadmap:* LRN-04, LRN-05.
- **Licence.** LightSim is proprietary (`LICENSE`). The desktop app is free
  for evaluation, learning, research and other non-commercial use under its
  end-user licence agreement (`EULA.txt`, installed with the app);
  commercial use needs a separate licence from the owner. The owner sets
  these terms, and they may change in a later release. The open-source parts
  inside the app keep their own licences, listed in `THIRD-PARTY-NOTICES.txt`
  (*Help → Third-Party Notices*). *Roadmap:* BIZ-01, BIZ-03.
