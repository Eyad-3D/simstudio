# LightSim — Vehicle System Simulation

A desktop app for simulating vehicle energy use, range and powertrains. It
runs entirely on your computer and works offline: build a system topology
from a component library, wire elements together (including signal /
data-bus connections), run a dynamic simulation, watch it live, and
inspect results — all in a dockable-panel UI.

> **Status:** early version. The workflow works, but the component physics
> are simplified, nothing has been validated against measured vehicles yet,
> and some results are known to be wrong. Read
> [Known issues and limits](docs/KNOWN-LIMITS.md) before relying on a number.

![Topology editor](docs/doc-topology.png)

## Features

| Area | What works |
|---|---|
| **Topology builder** | Drag components from the searchable library tree onto a React Flow canvas, or add one with Enter, a double-click or click-to-place; connect ports (kind-checked), pan/zoom, multi-select, delete, undo/redo (Ctrl+Z / Ctrl+Y), minimap toggle |
| **Component library** | Declarative catalog in `backend/app/library/components.json` with mandatory units on every parameter (dimensionless = `-`) and first-class lookup tables (`table1d` / `table2d`, dict-keyed by the independent variable) |
| **Dynamic solver** | Causal multi-pass solver with real states: vehicle speed integrates from net tire force, per-wheel speeds with a longitudinal slip tire model, battery SOC from an equivalent-circuit model, semi-implicit Euler with solver steps of at most 10 ms. Controllers, scripts, the drive cycle, gear choice and the physics all run at every solver step (Script, PID and Lookup blocks can be given a slower *Sample Time*), and motors are held to what their battery, fuel cell or voltage source can supply. The case time step only sets how often results are stored (see [Solver](#solver)) |
| **Differential** | Locked/unlocked with genuinely different dynamics: unlocked = equal torque split with free output speeds (one wheel on ice spins up), locked = common speed with grip-dependent emergent torque split |
| **Driver** | Separate Driver component (speed-following PI): wire a target-speed profile and the Vehicle's speed into it; braking blends recuperation (motor generator quadrant, what the battery or other source can take back) before friction brakes |
| **Live simulation** | Runs stream over a WebSocket: progress + all channels update live, the solver can be paced against real time (Pacing selector), cancelled, and scalar parameters (e.g. driver PI gains) can be edited mid-run from the Properties panel |
| **Monitors** | Display-only Monitor component: add named signal inputs, wire anything into them, get live readout cards + sparklines in the Monitors panel |
| **Scripting** | Script (Function) component: user-written Python `step(t, dt, inputs, state, params)` with named per-instance ports — for hybrid control strategies, custom recuperation logic, signal math. `step()` is called every solver step with `dt` = that step (0.01 s unless the case time step is shorter), or at the block's *Sample Time* with `dt` = the Sample Time when that is longer; wired inputs are fresh on every call. Scripts get `math`, `clamp()` and `interp()` and a small set of builtins; other imports, file access, class definitions and dunder attributes are refused, and each call must return within 2 s |
| **Maps** | E-Motor with voltage-dependent full-load torque map, power-loss map and unpowered drag torque; combustion engine full-load curve, fuel map and unfired drag torque; battery OCV(SOC) table — all edited in table grids in the Properties panel |
| **Data Checks** | Pre-run validation: reference integrity, port-kind mismatches, parameter ranges, table data, drive-cycle and road-profile entries (an entry that is not an `x:value` pair of numbers, or points out of order, is an error; a repeated x is a warning), Sample Times (negative or not a finite number is an error, above 0.1 s a warning), script compilation (compile only — script code never runs during checks), driveline solvability (delegated to the solver's model extraction). Errors that block the run when the model cannot drive: an E-Motor with no power source, a motor or engine that reaches no wheel, an open differential with a free output, a missing command or target-speed signal, a speed demand that reaches no motor or engine, two signals wired into one input. Warnings for parts the solver would leave out (unconnected, or an input that silently reads 0) and for implausible values (vehicle mass, battery size, auxiliary load, final-drive ratio, wheel load shares that do not add up to 100 %, which the solver scales to 100 % (a 0 % total is an error), a battery that starts empty). An all-clear says what was checked; it does not vouch for the results |
| **Results** | Dedicated full-page Results workspace (own ribbon tab): channel picker grouped per element, multi-channel time-series **chart or table view** that fills in live during the run, summary table (SOC, energy, recuperation, distance, consumption, fuel and CO₂ per km, electrical energy balance error, time a motor was held back by its supply, regeneration a motor's supply could not take) with a *not valid* note on figures the run's checks rule out (see [Run status](#run-status-and-not-valid-figures)), CSV export. Each run keeps a copy of the model and case settings it ran with, the app version and the parameters edited while it ran; *Run info* (ⓘ next to the run picker) shows them and opens that model again as an unsaved copy. Point 0 is the initial state at t = 0, each later point holds the state at its own time, and the run ends exactly at the case duration |
| **Parameter studies** | Per-case parameter overrides and one-parameter sweeps (Cases & Parameters panel). Each sweep is saved with the project as a study: what was swept on which case, and its results table with a row per point (value, status, run, every summary value; CSV download), kept after its runs leave the Results history |
| **Electrical** | Two-terminal components: every electrical element has explicit positive (+, red) and negative (−, blue) pins; the solver balances power on the supply rail with the negative terminals as the return (wire to Ground, or leave implicit) |
| **Canvas** | Signal/data-bus wiring is edited in the Data Bus panel; an optional dashed overlay draws those links on the canvas (the *signal* layer in Layer Configurations, off by default); background-grid toggle; double-click an element for a modal parameter dialog; Shift+click a pin to move it to the next side of its node, Shift+drag a pin to slide it anywhere along the node's edges (Shift+drag elsewhere draws a selection box) |
| **Persistence** | Save/load projects on the backend (single JSON file per project), plus browser Export / Import; the last 20 saved versions of each project can be restored as a copy (Project → Restore…) |
| **UI shell** | Ribbon tabs act as full-page workspaces (Home = topology + panels; Results = its own page); light/dark theme (persisted); dockable & resizable panels (Dockview) around a large diagram, with Messages, Data Checks, layers, Data Bus and Signal Plot in a collapsible bottom tray (double-click a tab to maximise its group); status bar with live progress |

![Results view](docs/doc-results.png)
![Data bus connections](docs/doc-databus.png)

## Install the desktop app

LightSim ships as a normal desktop application — download, install, launch.
Python and Node are **not** required: the simulation engine is bundled inside.

| Platform | Download |
|---|---|
| Windows 10/11 (x64) | `LightSim-Setup-<version>.exe` |
| Linux (x64) | `LightSim-<version>-x86_64.AppImage` or `LightSim-<version>-amd64.deb` |

No release has been published yet, so for now the installers come from the
**Build desktop app** workflow: on the repository's **Actions** tab, open the
latest successful *Build desktop app* run on `main` and download the
`lightsim-windows` or `lightsim-linux` artifact (a zip with the installers
and their `.sha256` checksums; you need to be signed in to GitHub, and
artifacts expire after 90 days). On Linux, mark the AppImage executable once
(`chmod +x LightSim-*.AppImage`) and run it.

> These builds are unsigned, so Windows SmartScreen warns on first launch —
> choose *More info → Run anyway*, or sign them with your own certificate
> before distributing.

The **Battery Electric Car** example loads on first launch: HV Battery Pack →
HV Bus → (Power Consumer, E-Motor) → Final Drive → Differential → Node FL/FR →
Brake + Wheel per corner (rear corners unpowered), with a Vehicle body, a
Driver element, a target-speed Driving Task (labelled *Vehicle Task*), and
Vehicle/BMS monitors. It is modelled on the 2021 Cupra Born (values from
FASTSim's vehicle file) and has WLTC cases next to the quick *City Cycle*. A
**P2 Hybrid Car** example (engine, clutch, gearbox, HCU script), sized after
the Hyundai Ioniq Hybrid with EPA road-load data and run on the EPA city and
highway cycles, is available via Open. Each example's entry in the Open menu
lists the results to expect. Press **Run** — pick the *City Cycle
(live, 10×)* case to watch it stream in real time and tune parameters (try
the P and I gains on the Driver, or lock the Differential) while it runs.
An example opens as an unsaved copy: change it freely, and **Save** keeps
your copy as a new project of your own.

### Where your work is saved

Projects are stored one JSON file each, in your own user folder, so they
survive reinstalls and upgrades. **File → Open Projects Folder** opens it.

| Platform | Location |
|---|---|
| Windows | `%APPDATA%\LightSim\projects` |
| Linux | `~/.config/LightSim/projects` |

The examples are not copied there: they are part of the app, read-only,
and every update brings the current ones. The Open menu lists them under
*Examples*, apart from *Your projects*. An example opens as an unsaved copy
with an id of its own, so saving it makes a new project (and its runs and
backups are its own) and never changes the example. To take an example out
of the menu, hide it (the eye icon next to it); *Restore hidden examples*
lists it again. Copies of the examples that earlier versions put in your
projects folder stay there as your own projects, as you left them; an
example whose copy you had deleted starts out hidden.

A save replaces the file in one step, so a crash or a full disk mid-save never
leaves a half-written project, and the version it replaced is kept next to it
as `<id>.json.bak`. If the file changed after you opened it (saved from a
second window, or edited by another program), Save asks before overwriting it.

Every save also keeps the version it replaced in the hidden `.backups/<id>/`
folder, the last 20 per project. **Project → Restore…** lists them by the
time each was saved and opens the one you pick as an unsaved copy: the
project file is left as it is, and saving the copy makes a new project.

## Building the app from source

One command builds the UI, freezes the backend, and produces an installer for
whichever OS you run it on. Requires Python ≥ 3.11 and Node ≥ 20.

```bash
./scripts/build-desktop.sh          # Linux (macOS is not supported)
.\scripts\build-desktop.ps1         # Windows
```

Installers land in `desktop/release/`. Add `--dir` (or `-Dir`) for just the
unpacked app, which is much faster while iterating. Before packaging, the
build writes `THIRD-PARTY-NOTICES.txt` and `sbom.cdx.json`, and stops if
anything it would ship is under a licence LightSim does not allow (see
[Third-party licences](#third-party-licences)).

Neither half cross-compiles — the frozen Python backend and the Electron
package are both platform-specific — so `.github/workflows/desktop-build.yml`
builds Windows and Linux on GitHub's runners. Push a `v*` tag to draft a
release with the installers attached.

## Developing

Two processes: a FastAPI backend (solver, validation, library, persistence)
and a Vite dev server (UI). The Vite server proxies `/api` (HTTP and
WebSocket) to the backend.

```bash
# 1. backend  (Python ≥ 3.11)
cd backend
pip install -r requirements.txt
python -m uvicorn app.main:app --reload --port 8000

# 2. frontend (Node ≥ 20)
cd frontend
npm install
npm run dev            # → http://localhost:5173
```

In this mode your projects (with their runs and backups) are saved in
`backend/dev-projects/`, which git ignores, not in your user folder. The
examples are read from `backend/projects/`, which the engine never writes (the
golden tests read them too): to change an example, edit its file there.
Projects that earlier versions saved into `backend/projects/` now show up as
examples; move them to `backend/dev-projects/`. If the backend is not running
the UI still works from bundled data (topology editing only); save / checks /
simulation are disabled and a warning appears in Messages.

Building the frontend once (`cd frontend && npm run build`) also lets the
backend serve the whole app at `:8000` with no Vite process. To run the
Electron shell against your working tree, do that and then `cd desktop && npm
install && npm start` — the shell prefers the frozen backend in
`backend/dist/` when present and otherwise falls back to `python3` on PATH, so
no packaging step is needed while developing.

### Tests and checks

```bash
cd backend
pip install -r requirements-dev.txt ruff
python -m pytest tests/     # maps, dynamics, differential modes, scripts, API + WS
ruff check .                # lint (config in backend/pyproject.toml)

cd ../frontend
npm run lint                # ESLint (config in frontend/eslint.config.js)
npm test                    # unit tests (Vitest)
npm run build               # type-check + production build
npx playwright install chromium   # once
npm run test:e2e            # browser + accessibility tests of the built UI
```

The browser tests start the engine themselves with `python3` (set
`LIGHTSIM_PYTHON` to use another interpreter; it needs the backend's
requirements) and a throw-away projects folder. They include screenshot
comparisons: after a deliberate UI change, regenerate the baselines on CI
(Actions → CI → Run workflow, tick *Regenerate the screenshot baselines*),
since renders from another machine do not match the runner's.

Two smoke tests exercise what unit tests cannot — they run the *built*
artefacts rather than the source (Node ≥ 22, for its built-in WebSocket):

```bash
node scripts/smoke-backend.mjs    # the frozen executable: library, examples,
                                  # a REST run and a live WebSocket run
xvfb-run -a node scripts/smoke-app.mjs \
  desktop/release/linux-unpacked/lightsim   # the packaged app reaches its engine
```

These exist because a frozen build can be missing a module that every unit
test passes without — that is exactly how a broken live-run WebSocket nearly
shipped.

### Versioning

The root `VERSION` file is the single source of truth. `scripts/sync-version.mjs`
copies it into the npm manifests (electron-builder and Vite each insist on
reading their own `package.json`), and the backend reports it at
`/api/health`. CI fails if they drift, and also when
[docs/KNOWN-LIMITS.md](docs/KNOWN-LIMITS.md) was last reviewed for another
version: review the page for each release and update its *Last reviewed*
line (the script checks that line but never rewrites it).

```bash
node scripts/sync-version.mjs           # write VERSION into the manifests
node scripts/sync-version.mjs --check   # what CI runs
```

### Third-party licences

The app ships other people's open-source code: the UI's npm packages,
Electron and any runtime dependencies of the desktop shell, and everything
PyInstaller freezes into the engine (Python
packages, the Python runtime and native libraries such as OpenSSL).
`scripts/third-party-notices.py` lists all of it, with the licence texts, in
`THIRD-PARTY-NOTICES.txt` (**Help → Third-Party Notices** in the app), and as
a CycloneDX software bill of materials in `sbom.cdx.json`; the installers
include both. It fails if any of it is under a licence that
[`scripts/licenses/allowed.txt`](scripts/licenses/allowed.txt) does not
allow, or under a licence it cannot determine. GPL, AGPL, SSPL and EUPL are
always refused; the two GPL-licensed parts that do ship come with an
exception that permits it (the PyInstaller bootloader and, on Linux, the GCC
runtime library). What is built into Electron itself (Chromium, Node.js) is
not checked here: Electron ships those notices as `LICENSES.chromium.html`
next to the executable.

The desktop builds (the workflow and both build scripts) write both files
afresh before packaging, and the CI licence check runs on every pull request
and every push to `main`. To regenerate them by hand, and commit the result
when the dependencies change:

```bash
cd backend
pip install -r requirements-build.txt   # includes pip-licenses
python -m PyInstaller --noconfirm --distpath dist --workpath build lightsim-backend.spec
cd ..
npm ci --prefix frontend && npm ci --prefix desktop   # desktop/ pins the npm licence reader
python scripts/third-party-notices.py   # add --check to only check
```

If the check fails on a package whose metadata names no licence the script
can read, or names several without an SPDX expression (the script then
requires all of them, since the list does not say whether they are a
choice), check its licence file and record it in
`scripts/licenses/clarifications.json`. A native library it does not know
goes in `scripts/licenses/bundled-runtime.json`. Adding a licence to
`allowed.txt` is a licensing decision for the owner, not a build fix.

### Continuous integration

| Workflow | Runs on | Does |
|---|---|---|
| `ci.yml` | every push and PR (~3 min; the browser tests ~6 min, in parallel) | backend lint + tests, frontend lint + unit tests + typecheck + build, browser + accessibility tests, version-sync check, third-party licence check |
| `desktop-build.yml` | `main`, `v*` tags, manual, and PRs touching packaging (~12 min) | builds Windows and Linux installers (with their third-party notices), smoke-tests both the frozen backend and the packaged app, uploads artefacts with SHA-256 checksums |

Push a `v*` tag to draft a release with the installers attached. Dependabot
opens grouped dependency PRs monthly.

## Architecture

```
frontend/  React 19 + TypeScript + Vite
  ├─ Dockview        dockable panel shell (library / canvas / properties / bottom tabs)
  ├─ React Flow      topology canvas with custom element nodes & kind-colored edges
  ├─ Zustand         project graph, selection, undo history, live run state, results
  ├─ Recharts        results time-series charts (live-updating)
  └─ Tailwind CSS    dense engineering-tool styling

backend/   Python + FastAPI
  ├─ app/main.py                   HTTP + WebSocket API (FastAPI app)
  ├─ app/library/components.json   declarative component catalog (ports, params, maps)
  ├─ app/schemas.py                pydantic models mirroring the shared JSON data model
  ├─ app/solver/                   causal multi-pass solver package
  │    ├─ maps.py                  shared table parsing + 1D/2D interpolation
  │    ├─ profiles.py              driving-task profile parsing
  │    ├─ network.py               model extraction: rigid segments, buses, signal routes
  │    ├─ scripting.py             Script component compile/run
  │    ├─ runtime.py               shared constants, signal routing, small linear solver
  │    ├─ slave.py                 FMI-style co-simulation slave interface
  │    ├─ master.py                co-simulation master: steps the slaves on a shared grid
  │    ├─ domains.py               domain slaves: control → gear → source limits → driver → mechanics + vehicle → electrical
  │    ├─ verdict.py               run verdict: speed trace vs. target, distance, non-finite values
  │    └─ core.py                  simulate(): runs the master, records channels, streams progress
  ├─ app/validation.py             "Data Checks" pre-run validation
  ├─ app/storage.py                one JSON file per project
  ├─ app/paths.py                  bundled vs. user-writable location resolution
  ├─ app/security.py               Host / Origin / launch-token checks on every request
  ├─ app/server.py                 entrypoint the desktop shell launches
  ├─ lightsim-backend.spec         PyInstaller recipe for the frozen backend
  └─ projects/                     example projects (bev-car.json, hybrid-car.json)

desktop/   Electron shell
  ├─ src/main.js                   starts the backend on a stable loopback port
  │                                with a per-launch token, waits for
  │                                /api/health, then opens the window
  ├─ src/loading.html              splash shown while the engine starts
  └─ electron-builder.yml          installer definitions (NSIS / AppImage / deb)
```

In the packaged app the backend serves the built UI as well as the API, so the
window talks to a single local origin and the frontend's relative `/api` calls
— including the live-simulation WebSocket — work unchanged. Nothing is exposed
off the machine: the server binds to 127.0.0.1. Each installation picks a free
port on first launch and keeps it (choosing a new one only if another program
takes it), so the UI's saved layout and settings persist across restarts.

Web pages the user visits can still reach a loopback port, so the engine
answers only requests addressed to `127.0.0.1` / `localhost` (no DNS
rebinding), refuses any request whose `Origin` is not its own (the Vite dev
server's is allowed in development), and in the desktop app requires a
random per-launch token: the shell passes it to the engine in
`LIGHTSIM_TOKEN`, and the window receives it as an HttpOnly, SameSite=Strict
cookie on its first page load. Without `LIGHTSIM_TOKEN` (development) there
is no token check. To reach a development engine through another host name
(e.g. a forwarded port), list it in `LIGHTSIM_ALLOWED_HOSTS` (comma-separated).

### API

| Method & path | Purpose |
|---|---|
| `GET /api/library` | Component definitions |
| `GET /api/projects` | List your saved projects |
| `GET/PUT/DELETE /api/projects/{id}` | Load / save / delete a project. GET adds the file's `revision` (also sent as the `ETag`); a PUT with `If-Match: "<revision>"` is refused with 409 if the file changed since, and `If-None-Match: *` refuses to replace an existing project |
| `GET /api/examples` | List the examples shipped with the app, each with `hidden` (hidden from the Open menu) |
| `GET /api/examples/{id}` | Load an example as shipped (read-only: no `revision`; the UI opens it as a copy with a new id) |
| `POST /api/examples/{id}/hide`, `POST /api/examples/restore` | Hide an example from the Open menu / list every hidden one again |
| `POST /api/validate` | Run Data Checks on a project payload |
| `POST /api/simulate` | Validate + solve one case synchronously |
| `WS /api/simulate/run` | Live run: client sends `start`, then optional `set_param` / `cancel`; server streams `step` / `message` events and a final `done` with the full result |

A result (`SimResult`) has a `status` of `success`, `warning` or `failed`, its
`messages`, the recorded `channels` and a `summary` of `SummaryValue`s
(`label`, `value`, `unit`). A summary value that the run's checks rule out
also carries `notValid`, the reason as text (for example
`"cycle not followed"`); it is absent or `null` otherwise.

## Data model

A project is a single JSON document (see `backend/projects/bev-car.json`):
`Project → SystemNode[] (hierarchical) → ElementInstance[] + Connection[]`,
plus project-level `DataBusConnection[]` and `SimCase[]`. Component types are
referenced by id and resolved against the library, so parameters live as
sparse overrides on the instance. Table parameters are dicts keyed by the
independent variable (`{"1500": 345.6, …}`; 2D maps nest one level, e.g.
voltage → speed → torque). Elements of components with `allowDynamicPorts`
(Script, Monitor) carry per-instance named signal ports in `dynamicPorts`.

## Solver

The solver steps with semi-implicit Euler at a fixed solver step of at most
10 ms: each case time step (*Step (s)* in Cases & Parameters) is split into
equal solver steps. Every solver step runs, in this order:

1. **Control** — signal sources (Constant, Driving Task, Road Profile) and
   blocks (Script, PID, Lookup) in topological order over the signal graph,
   with a one-solver-step delay on loops. A Script, PID or Lookup whose
   *Sample Time* is longer than the solver step runs only at multiples of
   it and holds its outputs in between.
2. **Gear** — each gearbox reads its Gear Select signal; a shift rebuilds
   the driveline at that step.
3. **Source limits** — each electrical bus with its battery, fuel cell or
   voltage source, plus the buses DC-DC converters feed from it, states what
   the source can deliver and take back over the step. A battery delivers up
   to its maximum-power point and takes back up to its max charge power,
   never past its minimum SOC or 100 %; a fuel cell delivers up to its power
   at maximum current, and nothing once its hydrogen tank is empty; a voltage
   source has no limit. A fuel cell, and a bus fed through a one-way DC-DC,
   take nothing back. Power Consumers and DC-DC setpoints are served first
   and are cut back, with a message, when the source cannot carry them; the
   motors share what is left.
4. **Driver** — PI on target vs. actual speed → traction command ∈ [−1, 1]
   and brake command, with capability-aware regen blending (motor Q4 map ×
   recuperation weight, what the sources can take back this step, fade-out
   near standstill); the friction brakes take the rest of the braking.
5. **Mechanics** — before any torque is applied, each motor gets an
   electrical-power window: motors that draw share the room in proportion
   to what they ask for, and a motor's torque is cut until its electrical
   power fits (a message names the limit, and the summary shows how long
   the motor was held back). Motor torque comes from the voltage-dependent
   full-load map, engine torque from the throttle and the full-load curve;
   both are reflected through the gear chain (direction-aware
   efficiencies) into the differential; per-segment equations of motion
   (2×2 coupled mass matrix for an open diff, merged inertia when locked);
   brakes with proper static-friction standstill hold; tire slip term
   integrated implicitly (it is numerically stiff at low speed).
6. **Vehicle** — net tire force − aero − rolling − grade integrates speed
   and distance.
7. **Electrical** — motor electrical power = shaft power + loss map; buses
   solved in dependency order (DC-DC bridges); battery equivalent circuit
   (OCV(SOC) table, R0, optional RC pair) solved closed-form per step; SOC
   integrates; the terminal voltage feeds the next step's motor map.
8. The state signals wired into block inputs (SOC, speeds, torques, tank
   level …) are refreshed, so the next control pass reads current values.

Results are recorded at the end of each case time step: point 0 is the
initial state at t = 0, every later point holds the state at its own time,
and a last, shorter step makes the run end exactly at the case duration.
*Store every* keeps every Nth of those points. Changing the case time step
does not change the results (the solver step stays at 10 ms whenever the
case step is a multiple of 10 ms), only how many points are stored and when
live edits apply. At point 0, values that blocks, the Driver or the physics
compute (commands, torques, powers) read 0, because nothing has computed
them yet; at later points they come from the last solver step before the
point.

Live `set_param` messages apply at the start of the next case time step
and stay in force through gear shifts; structural parameters (ratios,
inertias, code, table axes) take effect on the next run and say so in
Messages.

### Motors, engines and fuel

- **E-Motor** — the *Power Loss (Motor + Inverter)* map holds every loss
  while the inverter is powered, spin losses at zero torque included, so
  electrical power = shaft torque × speed + map loss. *Drag Torque
  (unpowered)* applies only when the motor coasts unpowered: a command of
  exactly 0, no live supply, or a source that cannot cover even the spin
  loss. It then draws nothing. *Shaft Torque* and *Mechanical Power* are
  net shaft values, and *Losses* is electrical minus shaft power. The
  library defaults are generic values for a motor of this size, not data
  for a real one: about 1 kW of spin loss near 8,000 1/min, with the
  unpowered drag below it at every speed.
- **Combustion Engine** — the full-load curve and fuel map are brake (net)
  values, as on a datasheet: fired, the engine gives throttle × full-load
  torque and burns map(speed, torque). *Drag Torque (unfired)* applies only
  when it is not fired: switched off, out of fuel, in overrun fuel cut-off
  (zero throttle above the *Fuel Cut-Off Re-Entry Speed*, 1,100 1/min by
  default), or above the full-load curve's last speed, where a rev limiter
  cuts fuel and torque and says so in Messages. At zero throttle below the
  re-entry speed an idle governor holds idle speed: below idle it adds
  torque, above it trims the fuel down to the drag torque, with fuel
  falling linearly from map(speed, 0) to 0. There is no speed governor
  above idle, so a declutched engine given any throttle above 0 runs up to
  its rev limiter: control scripts should set the throttle to 0, or switch
  the engine off, while the clutch is open. *Engine Torque* is the net
  shaft torque.
- **Fuel Tank** — its density turns the fuel burnt into litres, and its
  *CO₂ per kg of Fuel* (3.17 by default, for petrol) gives the *CO₂
  emissions* row of the summary. Without a tank, 0.745 kg/l and 3.17 are
  used.

### Run status and not-valid figures

Each run ends as *success*, *warning* or *failed*:

- **failed** — an error was raised, for example: the model could not be
  built, a script failed, the vehicle covered less than 5 % of the distance
  its target speed asks for, or a result went NaN or infinite. When no
  distance has been covered after 60 s, a warning already says so while the
  run goes on.
- **warning** — a warning was raised, or the run was cancelled. This
  includes *Cycle not followed*: the vehicle speed was outside ±2 km/h and
  ±1 s of the target for more than 1 % of the run (at least 2 s). The trace
  is checked every 0.1 s of simulated time, whatever the case time step. A
  step in the target (for example `0:100; 600:100` from standstill) is
  outside that band while the car accelerates, so acceleration and
  top-speed tests end with this warning.
- **success** — neither of the above.

Summary figures that a failed check makes meaningless are marked *not
valid*, with the reason, in the results table:

- Consumption, Fuel consumption and CO₂ emissions when the cycle was not
  followed ("cycle not followed") or the run was cancelled ("run cancelled
  at t = …");
- Consumption once the battery reached its minimum SOC, and the fuel
  figures once the tank ran empty;
- battery, energy and consumption figures when the *Electrical energy
  balance error* (energy that no source supplied or absorbed, as a share
  of all the energy through the buses) is above 0.1 %;
- every figure except the simulated duration when a result went NaN or
  infinite ("the solution broke down").

A success means the car followed its target and nothing warned. It does not
mean the numbers match a real vehicle: see
[Known limits](docs/KNOWN-LIMITS.md).

## Known limitations

[docs/KNOWN-LIMITS.md](docs/KNOWN-LIMITS.md) is the maintained list,
including the open bugs that change results and how to work around them; the
desktop app installs a copy (**Help → Known Limits**). The structural limits
in short:

- One differential and one E-Motor per driveline subgraph (multiple
  independent drivelines — e.g. dual-motor AWD as two axles — work).
- One battery or voltage source per electrical bus; DC-DC is unidirectional.
- Forward driving only (no reverse), no thermal/fluid solving.
- Sub-system containers are organizational: physical connections cannot cross
  a container boundary (signals can, via the Data Bus).
- The canvas bookmark tool is disabled, and the Optimization tab is hidden
  until it is implemented. (Parameters — per-case overrides and sweeps —
  works.)

## License

LightSim is proprietary software: Copyright © 2026 Eyad Abualkhair, all
rights reserved (see [`LICENSE`](LICENSE)). The desktop app is free to use for
evaluation, learning, research and other non-commercial purposes under the
[End-User Licence Agreement](EULA.txt); commercial use needs a separate
licence. Third-party components keep their own licences; they are listed,
with their licence texts, in `THIRD-PARTY-NOTICES.txt`.
