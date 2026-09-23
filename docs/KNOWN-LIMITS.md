# LightSim: known issues and limits

LightSim 0.1.0 is an early version. You can build, run and inspect models,
but the component physics are simplified, **nothing has been validated
against measured vehicles yet**, and some results are known to be wrong.
This page lists what we know, what you can do about it today, and which
roadmap item tracks the fix.

Use LightSim to learn the workflow and to compare variants of one model
with each other. Do not use its absolute numbers (consumption, range, top
speed, acceleration) for decisions about a real vehicle yet.

- *Roadmap* gives the ID of the item on the LightSim development roadmap
  that tracks the fix, so the release notes can say when it is resolved.
- *Being fixed* means the work is under way for an upcoming release. Until
  the release notes say it is done, the problem and the workaround apply.
- Last reviewed: 23 September 2026, for version 0.1.0. This page is updated
  with every release.

## Results that can be wrong today

### Motors can run past the end of their maps without a warning

Maps are extended flat beyond their last point (the edge value is held), and
E-Motors have no maximum-speed limit. With the library's default E-Motor
map, a car with a 300 km/h target reaches 267 km/h with its motor at
21,333 rpm, on a torque map that ends at 12,000 rpm; the run ends with a
warning only because the car could not keep up with the target, and nothing
mentions the motor speed. The Battery Electric Car example no longer does
this: its motor's full-load curve falls to zero at the motor's 16,000 rpm,
so the car tops out at 160 km/h. Combustion engines are held at the last
speed of their full-load curve by a rev limiter, which says so in Messages.

*Workaround:* end a motor's full-load curve with zero torque at its maximum
speed, as the examples do; plot the motor speed and compare it with the last
speed point of its maps; keep target speeds within what the real vehicle can
do.
*Roadmap:* MOD-18.

### A "success" checks the speed trace, not the physics

A run is a *success* when the vehicle stayed within ±2 km/h and ±1 s of its
target speed for all but 1 % of the run (at least 2 s), covered the cycle's
distance, and nothing raised a warning. It does not check that motors stayed
within their maps (see above) or that the numbers are plausible for a real
vehicle, and the Data Checks all-clear does not vouch for the results
either. Also:

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
*Roadmap:* MOD-18, VAL-08.

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
yet. Motor and engine speed also read 0 at t = 0 when a run starts at speed.
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
- **Gear losses are applied per motor or engine, not to the power actually
  flowing through each gear.** When a motor and an engine push against each
  other (hybrids), this creates phantom braking. *Roadmap:* MOD-03.
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
- **An initial speed only spins the wheels.** A car that starts at speed has
  its motor at 0 rpm and heavy tyre slip in the first instant. Start runs
  from standstill. *Roadmap:* MOD-19.
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
- **CSV export does not quote fields**, so an element label that contains a
  comma shifts the columns. Avoid commas in labels. *Roadmap:* STD-03.

## The examples

- **P2 Hybrid Car:** sized after the Hyundai Ioniq Hybrid, with its test
  mass and road load from EPA data, but its engine, motor and battery maps
  are generic, not the car's. With its charge-sustaining control script it
  uses about 2.7 l/100 km on the EPA city cycle and 3.1 on the highway
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
  maximum speed sets the 160 km/h, because LightSim has no speed limiter.
  *Roadmap:* MOD-18 (maximum-speed limit), MOD-12.
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
- **Scripts run in a locked-down worker process — with limits that differ by
  platform.** A project's Script blocks are held two ways. First, as before,
  the engine checks them: Data Checks only compile a script, never run it, and
  during a run a script gets only `math`, an allow-listed set of builtins, and
  no way to name files, processes or Python's internals. Second, and new in
  this release, the run's scripts execute in a *separate* worker process that
  drops the privileges it does not need before any script runs, and the engine
  kills that worker if a step overruns the 2 s limit — so an endless loop the
  in-process check cannot stop (for example one written in C, `list(iter(int,
  1))`) now fails the run within the limit, and a runaway allocation hits a
  memory cap instead of the machine. What the worker guarantees depends on the
  operating system, and we would rather state it plainly than overclaim:
  - *Linux.* No filesystem access and no outgoing/listening TCP connections
    (enforced by the kernel's Landlock, on Linux 5.13 and newer); a memory cap,
    a file-size limit of zero (nothing can be written to a file), a CPU-time
    backstop and a small open-file limit; the environment is cleared, and all
    inherited file descriptors bar the engine link are closed. Not covered:
    UDP and Unix-domain sockets, and, on kernels older than 5.13 (no Landlock),
    the filesystem/network block is absent — only the memory/file-size/CPU
    limits, the closed descriptors and the in-process restriction apply there.
  - *Windows.* The worker is put in a Job object that caps its memory and dies
    with the engine. The Windows standard library offers no portable way to
    block filesystem or network calls from inside a process, so on Windows the
    real protections are that memory cap, the engine's kill-on-overrun, and the
    in-process restriction above — not an OS-enforced filesystem/network wall.
  Either way this is a strong second layer, not a perfect jail, and the engine
  still answers only the LightSim window. Open projects only from people you
  trust. *Roadmap:* PLT-02 (for a hostile-input server, still escalate to a
  real sandbox such as gVisor or a WASM runner).
- **Runs with Script blocks take a little longer than in 0.1.0.**
  Controllers and scripts now run every 10 ms, so a model with scripts
  does more work per second of driving; and, new in this release, each
  script step makes a short round trip to the worker process. On a test
  machine the P2 Hybrid Car's Mixed Cycle takes about 11 s (against 9.7 s
  when scripts ran in-process, and 7.9 s in 0.1.0); the worker adds under
  15 %, and to keep that latency low the worker briefly busy-waits between
  steps, so a scripted run uses a little more CPU. The Battery Electric Car
  (no Script blocks) starts no worker and takes about as long as before. A
  coarser case step does not make a run faster: the solver steps every
  10 ms whatever it is. *Roadmap:* ENG-10.
- **Some colours are too faint in the dark theme.** The ribbon title, the
  Run button, the status bar's "backend connected" and "success" in the log
  fall short of the WCAG AA contrast minimum. Switch to the light theme if
  they are hard to read. *Roadmap:* GUI-02.
- **Unsigned installers.** Windows SmartScreen warns on first launch (choose
  *More info → Run anyway*). *Roadmap:* PLT-13.
- **No macOS version.** Builds exist for Windows 10/11 (x64) and Linux (x64)
  only. *Roadmap:* PLT-13.
- **No published release and no automatic updates yet.** Installers come from
  the *Build desktop app* workflow on GitHub; install a newer build by hand.
  *Roadmap:* PLT-18.
- **Little help in the app.** There is no user manual, tutorial or
  explanation of individual parameters yet, and the README screenshots show
  an older version. *Roadmap:* LRN-04, LRN-05, LRN-03 (being fixed).
- **Licence.** LightSim is proprietary (`LICENSE`). The desktop app is free
  for evaluation, learning, research and other non-commercial use under its
  end-user licence agreement (`EULA.txt`, installed with the app);
  commercial use needs a separate licence from the owner. The owner sets
  these terms, and they may change in a later release. The open-source parts
  inside the app keep their own licences, listed in `THIRD-PARTY-NOTICES.txt`
  (*Help → Third-Party Notices*). *Roadmap:* BIZ-01, BIZ-03.
