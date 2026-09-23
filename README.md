# SimStudio — Vehicle System Simulation

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
| **Topology builder** | Drag components from the searchable library tree onto a React Flow canvas, connect ports (kind-checked), pan/zoom, multi-select, delete, undo/redo (Ctrl+Z / Ctrl+Y), minimap toggle |
| **Component library** | Declarative catalog in `backend/app/library/components.json` with mandatory units on every parameter (dimensionless = `-`) and first-class lookup tables (`table1d` / `table2d`, dict-keyed by the independent variable) |
| **Dynamic solver** | Causal multi-pass solver with real states: vehicle speed integrates from net tire force, per-wheel speeds with a longitudinal slip tire model, battery SOC from an equivalent-circuit model, semi-implicit Euler with internal sub-steps of at most 10 ms. Results currently depend on the case time step (see [Known limits](docs/KNOWN-LIMITS.md#results-depend-on-the-case-time-step)); how the time step is used is being reworked |
| **Differential** | Locked/unlocked with genuinely different dynamics: unlocked = equal torque split with free output speeds (one wheel on ice spins up), locked = common speed with grip-dependent emergent torque split |
| **Driver** | Separate Driver component (speed-following PI): wire a target-speed profile and the Vehicle's speed into it; braking blends recuperation (motor generator quadrant, battery charge limit) before friction brakes |
| **Live simulation** | Runs stream over a WebSocket: progress + all channels update live, the solver can be paced against real time (Pacing selector), cancelled, and scalar parameters (e.g. driver PI gains) can be edited mid-run from the Properties panel |
| **Monitors** | Display-only Monitor component: add named signal inputs, wire anything into them, get live readout cards + sparklines in the Monitors panel |
| **Scripting** | Script (Function) component: user-written Python `step(t, dt, inputs, state, params)` with named per-instance ports — for hybrid control strategies, custom recuperation logic, signal math |
| **Maps** | E-Motor with voltage-dependent full-load torque map, power-loss map, drag torque; battery OCV(SOC) table — all edited in table grids in the Properties panel |
| **Data Checks** | Pre-run validation: reference integrity, port-kind mismatches, parameter ranges, table data, script compilation, driveline solvability (delegated to the solver's model extraction) |
| **Results** | Dedicated full-page Results workspace (own ribbon tab): channel picker grouped per element, multi-channel time-series **chart or table view** that fills in live during the run, summary table (SOC, energy, recuperation, distance, consumption), CSV export |
| **Electrical** | Two-terminal components: every electrical element has explicit positive (+, red) and negative (−, blue) pins; the solver balances power on the supply rail with the negative terminals as the return (wire to Ground, or leave implicit) |
| **Canvas** | Signal/data-bus wiring is edited in the Data Bus panel; an optional dashed overlay draws those links on the canvas (the *signal* layer in Layer Configurations, off by default); background-grid toggle; double-click an element for a modal parameter dialog; Shift+click a pin to move it around the node |
| **Persistence** | Save/load projects on the backend (single JSON file per project), plus browser Export / Import |
| **UI shell** | Ribbon tabs act as full-page workspaces (Home = topology + panels; Results = its own page); light/dark theme (persisted); dockable & resizable panels (Dockview); status bar with live progress |

![Results view](docs/doc-results.png)
![Data bus connections](docs/doc-databus.png)

## Install the desktop app

SimStudio ships as a normal desktop application — download, install, launch.
Python and Node are **not** required: the simulation engine is bundled inside.

| Platform | Download |
|---|---|
| Windows 10/11 (x64) | `SimStudio-Setup-<version>.exe` |
| Linux (x64) | `SimStudio-<version>-x86_64.AppImage` or `SimStudio-<version>-amd64.deb` |

No release has been published yet, so for now the installers come from the
**Build desktop app** workflow: on the repository's **Actions** tab, open the
latest successful *Build desktop app* run on `main` and download the
`simstudio-windows` or `simstudio-linux` artifact (a zip with the installers
and their `.sha256` checksums; you need to be signed in to GitHub, and
artifacts expire after 90 days). On Linux, mark the AppImage executable once
(`chmod +x SimStudio-*.AppImage`) and run it.

> These builds are unsigned, so Windows SmartScreen warns on first launch —
> choose *More info → Run anyway*, or sign them with your own certificate
> before distributing.

The **Battery Electric Car** example loads on first launch: HV Battery Pack →
HV Bus → (Power Consumer, E-Motor) → Final Drive → Differential → Node FL/FR →
Brake + Wheel per corner (rear corners unpowered), with a Vehicle body, a
Driver element, a target-speed Driving Task (labelled *Vehicle Task*), and
Vehicle/BMS monitors. A **P2 Hybrid Car** example (engine, clutch, gearbox,
HCU script) is available via Open. Press **Run** — pick the *City Cycle
(live, 10×)* case to watch it stream in real time and tune parameters (try
the P and I gains on the Driver, or lock the Differential) while it runs.

### Where your work is saved

Projects are stored one JSON file each, in your own user folder, so they
survive reinstalls and upgrades. **File → Open Projects Folder** opens it.

| Platform | Location |
|---|---|
| Windows | `%APPDATA%\SimStudio\projects` |
| Linux | `~/.config/SimStudio/projects` |

The examples are copied in on first launch only — delete one and it stays
deleted.

## Building the app from source

One command builds the UI, freezes the backend, and produces an installer for
whichever OS you run it on. Requires Python ≥ 3.11 and Node ≥ 20.

```bash
./scripts/build-desktop.sh          # Linux (macOS is untested)
.\scripts\build-desktop.ps1         # Windows
```

Installers land in `desktop/release/`. Add `--dir` (or `-Dir`) for just the
unpacked app, which is much faster while iterating. Before packaging, the
build writes `THIRD-PARTY-NOTICES.txt` and `sbom.cdx.json`, and stops if
anything it would ship is under a licence SimStudio does not allow (see
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

In this mode projects are read from and written to `backend/projects/`, not
your user folder. If the backend is not running the UI still works from
bundled data (topology editing only); save / checks / simulation are disabled
and a warning appears in Messages.

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
npm run build               # type-check + production build
```

Two smoke tests exercise what unit tests cannot — they run the *built*
artefacts rather than the source (Node ≥ 22, for its built-in WebSocket):

```bash
node scripts/smoke-backend.mjs    # the frozen executable: library, seeding,
                                  # a REST run and a live WebSocket run
xvfb-run -a node scripts/smoke-app.mjs \
  desktop/release/linux-unpacked/simstudio   # the packaged app reaches its engine
```

These exist because a frozen build can be missing a module that every unit
test passes without — that is exactly how a broken live-run WebSocket nearly
shipped.

### Versioning

The root `VERSION` file is the single source of truth. `scripts/sync-version.mjs`
copies it into the npm manifests (electron-builder and Vite each insist on
reading their own `package.json`), and the backend reports it at
`/api/health`. CI fails if they drift.

```bash
node scripts/sync-version.mjs           # write VERSION into the manifests
node scripts/sync-version.mjs --check   # what CI runs
```

### Third-party licences

The app ships other people's open-source code: the UI's npm packages,
Electron, and everything PyInstaller freezes into the engine (Python
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
python -m PyInstaller --noconfirm --distpath dist --workpath build simstudio-backend.spec
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
| `ci.yml` | every push and PR (~3 min) | backend lint + tests, frontend lint + typecheck + build, version-sync check, third-party licence check |
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
  │    ├─ domains.py               domain slaves: control → gear → driver → mechanics + vehicle → electrical
  │    └─ core.py                  simulate(): runs the master, records channels, streams progress
  ├─ app/validation.py             "Data Checks" pre-run validation
  ├─ app/storage.py                one JSON file per project
  ├─ app/paths.py                  bundled vs. user-writable location resolution
  ├─ app/server.py                 entrypoint the desktop shell launches
  ├─ simstudio-backend.spec        PyInstaller recipe for the frozen backend
  └─ projects/                     example projects (bev-car.json, hybrid-car.json)

desktop/   Electron shell
  ├─ src/main.js                   starts the backend on a stable loopback port,
  │                                waits for /api/health, then opens the window
  ├─ src/loading.html              splash shown while the engine starts
  └─ electron-builder.yml          installer definitions (NSIS / AppImage / deb)
```

In the packaged app the backend serves the built UI as well as the API, so the
window talks to a single local origin and the frontend's relative `/api` calls
— including the live-simulation WebSocket — work unchanged. Nothing is exposed
off the machine: the server binds to 127.0.0.1. Each installation picks a free
port on first launch and keeps it (choosing a new one only if another program
takes it), so the UI's saved layout and settings persist across restarts.

### API

| Method & path | Purpose |
|---|---|
| `GET /api/library` | Component definitions |
| `GET /api/projects` | List saved projects |
| `GET/PUT/DELETE /api/projects/{id}` | Load / save / delete a project |
| `POST /api/validate` | Run Data Checks on a project payload |
| `POST /api/simulate` | Validate + solve one case synchronously |
| `WS /api/simulate/run` | Live run: client sends `start`, then optional `set_param` / `cancel`; server streams `step` / `message` events and a final `done` with the full result |

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

Each recorded step evaluates signal sources and Script blocks (topological
order over the signal graph, one-step delay on loops), then runs internal
sub-steps (≤ 10 ms, semi-implicit Euler):

1. **Driver** — PI on target vs. actual speed → traction command ∈ [−1, 1]
   and brake command, with capability-aware regen blending (motor Q4 map ×
   recuperation weight, battery max charge power, fade-out near standstill).
2. **Mechanics** — motor torque from the voltage-dependent full-load map,
   reflected through the gear chain (direction-aware efficiencies) into the
   differential; per-segment equations of motion (2×2 coupled mass matrix
   for an open diff, merged inertia when locked); brakes with proper
   static-friction standstill hold; tire slip term integrated implicitly
   (it is numerically stiff at low speed).
3. **Vehicle** — net tire force − aero − rolling − grade integrates speed
   and distance.
4. **Electrical** — motor electrical power = mechanical + loss map; buses
   solved in dependency order (DC-DC bridges); battery equivalent circuit
   (OCV(SOC) table, R0, optional RC pair) solved closed-form per sub-step;
   SOC integrates; the terminal voltage feeds next step's motor map.

Because the drive-cycle target, Script, PID and Lookup blocks and the gear
choice are evaluated only once per recorded step, results depend on the case
time step. This is being reworked; until then see
[Known limits](docs/KNOWN-LIMITS.md#results-depend-on-the-case-time-step).

Live `set_param` messages apply at recording-step boundaries; structural
parameters (ratios, inertias, code, table axes) take effect on the next run
and say so in Messages.

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

SimStudio is proprietary software: Copyright © 2026 Eyad Abualkhair, all
rights reserved (see [`LICENSE`](LICENSE)). The desktop app is free to use for
evaluation, learning, research and other non-commercial purposes under the
[End-User Licence Agreement](EULA.txt); commercial use needs a separate
licence. Third-party components keep their own licences; they are listed,
with their licence texts, in `THIRD-PARTY-NOTICES.txt`.
