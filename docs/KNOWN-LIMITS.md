# SimStudio: known issues and limits

SimStudio 0.1.0 is an early version. You can build, run and inspect models,
but the component physics are simplified, **nothing has been validated
against measured vehicles yet**, and some results are known to be wrong.
This page lists what we know, what you can do about it today, and which
roadmap item tracks the fix.

Use SimStudio to learn the workflow and to compare variants of one model
with each other. Do not use its absolute numbers (consumption, range, top
speed, acceleration) for decisions about a real vehicle yet.

- *Roadmap* gives the ID of the item on the SimStudio development roadmap
  that tracks the fix, so the release notes can say when it is resolved.
- *Being fixed* means the work is under way for an upcoming release. Until
  the release notes say it is done, the problem and the workaround apply.
- Last reviewed: 23 September 2026, for version 0.1.0. This page is updated
  with every release.

## Results that can be wrong today

### Results depend on the case time step

The drive-cycle target, Script, PID and Lookup blocks and the gear choice are
evaluated only once per case time step (*Step (s)* in the Cases & Parameters
panel). Both examples use 1 s, and at that step the controllers react late.
The P2 Hybrid Car example reports 19.34 l/100 km at 1 s and 12.28 l/100 km
at 0.1 s, and at 1 s it misses its speed trace and drives only 8.98 of the
cycle's 9.56 km. The Battery Electric Car uses 5-22 % more energy on
standard test cycles at 1 s than at 0.1 s.

*Workaround:* set *Step (s)* to 0.1 and *Store every* to 10. That still
stores one result point per second, so results stay the same size; a run
takes at most about twice as long.
*Roadmap:* ENG-01 (being fixed), VAL-04 (warning), CON-01 (examples).

### Motors and engines can run past the end of their maps without a warning

Maps are extended flat beyond their last point (the edge value is held), and
motors and engines have no maximum-speed limit. The Battery Electric Car
example with a 300 km/h target reaches 257 km/h with its motor at
20,476 rpm, on a torque map that ends at 12,000 rpm, and the run is still
reported as a success.

*Workaround:* plot the motor and engine speed and compare it with the last
speed point of their maps; keep target speeds within what the real vehicle
can do.
*Roadmap:* MOD-18.

### A battery or fuel cell at its limit still delivers full power

When a battery reaches its minimum state of charge or its power limit, or a
fuel cell its current limit, the motor keeps getting the power it asks for,
so the car drives on energy that does not exist. Braking energy that the
battery cannot accept (full battery, charge-power limit, a DC-DC converter
or fuel-cell-only bus) disappears, and *energy recuperated* can be
overstated. For example, the Battery Electric Car example started at 10.2 %
charge completes its cycle and reports 1.57 kWh/100 km, 13 times too low.

*Workaround:* watch the battery's state of charge and power channels and the
Messages panel. If the charge reaches its minimum or a power-limit warning
appears, do not use the energy and consumption figures of that run. Do not
start runs at 100 % charge.
*Roadmap:* ENG-02 (being fixed), MOD-01, MOD-02, VAL-03.

### "Success" does not mean the car followed the cycle

A run is reported as a success even when the vehicle missed its speed trace,
covered no distance (for example because the motor is not connected), or
kept driving after the battery was empty. Data Checks can also report
*All data checks passed* for a model that cannot drive, such as one without
a battery, motor or differential.

*Workaround:* after each run, plot the vehicle speed together with the
target speed, and compare *Distance driven* with the cycle's length.
*Roadmap:* VAL-02 and VAL-01 (both being fixed).

### Result time stamps are one step early

Each stored value is labelled with the time at the start of the step that
produced it, and a run simulates one extra step (a 600 s case integrates
601 s). At a 1 s step, a car moving at 36 km/h shows about 10 m of distance
at t = 0.

*Workaround:* use a small time step (see above); the shift is one step.
*Roadmap:* ENG-03 (being fixed).

### Component models with known errors

- **E-Motor losses are counted twice.** No-load losses are in the loss map
  and are subtracted again as drag torque, so electric drives look less
  efficient than they are. If your loss map already contains the no-load
  losses, set the E-Motor's *Drag Torque* table to zero.
  *Roadmap:* MOD-04 (being fixed).
- **The combustion engine is short of power and keeps burning fuel when
  coasting.** Friction is subtracted even at full throttle (the default
  engine delivers about 59 of its nominal 82 kW), and there is no fuel
  cut-off when the driver lifts off. There is no CO2 output.
  *Roadmap:* MOD-05 (being fixed).
- **Gear losses are applied per motor or engine, not to the power actually
  flowing through each gear.** When a motor and an engine push against each
  other (hybrids), this creates phantom braking. *Roadmap:* MOD-03.
- **Wheel load shares are not checked.** Each wheel's share of the vehicle
  weight (*Vehicle Load Share*) is typed in by hand; if the shares do not add
  up to 100 %, tyre grip and rolling resistance are wrong. Make them add up
  to 100 %.
  *Roadmap:* MOD-06.
- **An initial speed only spins the wheels.** A car that starts at speed has
  its motor at 0 rpm and heavy tyre slip in the first instant. Start runs
  from standstill. *Roadmap:* MOD-19.
- **Stiff tyres can cause short wheel-spin spikes at launch.** This is a
  numerical effect of how the solver steps the tyres. Keep *Slip Stiffness*
  near its default. *Roadmap:* ENG-09.
- **Fuel-cell hydrogen use is a fixed figure per kWh** (*Specific H₂
  Consumption*, 55 g/kWh by default), which overstates it at part load by up
  to about a third and understates it at full load.
  *Roadmap:* MOD-20.

### Live edits, charts, sweeps and export

- **A live parameter edit is undone at the next gear shift** in models with a
  gearbox. Set the value before the run instead (as a case override).
  *Roadmap:* ENG-04.
- **Charts can hide short peaks.** Signals with more than 2,000 points are
  thinned for drawing, and *Store every* above 1 does not record the values
  in between, so short spikes and dips may not show. Use *Store every* 1
  when peaks matter, and export to CSV for the full data. *Roadmap:* RES-01
  (being fixed), RES-17.
- **Stopped or failed sweep points are plotted as if they were results.**
  Check each run's status before reading a sweep. *Roadmap:* STU-02 (being
  fixed).
- **CSV export does not quote fields**, so an element label that contains a
  comma shifts the columns. Avoid commas in labels. *Roadmap:* STD-03.

## The examples

- **P2 Hybrid Car:** its fuel figure is not realistic. It reports
  19.3 l/100 km at the shipped 1 s step and 12.3 l/100 km at 0.1 s, while a
  comparable real hybrid uses about 3 l/100 km on the US EPA city test. In
  the shipped case its control script never switches the engine off.
  *Roadmap:* CON-02 (being fixed).
- **Battery Electric Car:** about 20-25 kWh/100 km on standard test cycles
  (WLTC, UDDS, HWFET, US06), measured at the battery even at a 0.1 s step,
  where comparable real cars use roughly 10-16 kWh/100 km. Part of that is the
  default 2.5 kW auxiliary load (heating or air-conditioning level), about
  28 % of the City Cycle energy; set the Power Consumer's *Constant Power
  Draw* to about 0.3 kW for a mild-weather figure. *Roadmap:* CON-03 (being
  fixed), CON-14.
- **Updated examples do not reach existing installations.** Examples are
  copied into your projects folder on first launch only. To get the current
  version, close SimStudio, delete the example's file and the hidden
  `.seeded` file in the projects folder (*File → Open Projects Folder*),
  and start SimStudio again. Your own projects are not touched, but
  examples you deleted earlier come back as well.
  *Roadmap:* CON-10.

## Not modelled yet

- **No heat or cooling.** There is no thermal solver: temperatures do not
  change and do not affect batteries, motors or engines. The Ambient
  component is a placeholder, and thermal or fluid connections are ignored
  during a run. *Roadmap:* MOD-09.
- **Forward driving only.** No reverse, and no rolling back: a car on a steep
  hill stays put even with no brakes. *Roadmap:* MOD-21.
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

- **Results are not kept.** Runs live only in the open window: reloading,
  restarting or opening another project discards them, and only the last 20
  runs are kept. Export what you need to CSV. *Roadmap:* RES-02 (being
  fixed).
- **Saving is being reworked.** Today a save can drop resized node sizes and
  moved pin positions, and a crash during a save can damage the file. Keep an
  exported copy (*Export*) of important projects. *Roadmap:* PLT-01, PLT-04
  (being fixed).
- **Security hardening is in progress.** A project's Script blocks run
  Python code on your computer, and the local engine that runs them is being
  hardened. Until that ships, open projects only from people you trust.
  *Roadmap:* PLT-02, PLT-03 (being fixed).
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
- **Licence.** SimStudio is proprietary (`LICENSE`). The desktop app is free
  for evaluation, learning, research and other non-commercial use under its
  end-user licence agreement (`EULA.txt`, installed with the app);
  commercial use needs a separate licence from the owner. The owner sets
  these terms, and they may change in a later release. The open-source parts
  inside the app keep their own licences, listed in `THIRD-PARTY-NOTICES.txt`
  (*Help → Third-Party Notices*). *Roadmap:* BIZ-01, BIZ-03.
