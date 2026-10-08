# Command-line tool

The `lightsim` command runs, checks and reads LightSim models from a
terminal, a script or a continuous-integration (CI) pipeline, without
opening the app. It runs the model inside its own process: no window, no
server, nothing on the network. The [Python API](python-api.md) does the
same from Python.

## Where it is

- **From the repository**: in `backend/`, with Python 3.11 or newer and
  `pip install -r requirements.txt`, run `python -m lightsim …`.
- **In the desktop app**: the engine program takes the same commands.
  On Windows it is `resources\backend\lightsim-backend.exe`
  in the folder LightSim is installed in; on Linux,
  `resources/backend/lightsim-backend` inside the
  installed app. Run it as `lightsim-backend run …`.

The examples below write `lightsim`; use whichever of these you have.

## Commands

| Command | Does |
|---|---|
| `lightsim run PROJECT [--case NAME]` | Runs a case (the first if none is named) and prints its summary |
| `lightsim check PROJECT` | Runs the Data Checks (the *Problems* list). `--strict` fails on warnings too |
| `lightsim export RUN -o FILE` | Writes a stored run (`.json.gz` from the app's runs folder, or `.json`) as CSV, MAT, JSON or Parquet |
| `lightsim show PROJECT` | Lists the project's cases and parts, with their ids |
| `lightsim params PROJECT [--part LABEL]` | Lists the parameter values, as `Part.key = value unit` |
| `lightsim parts [TYPE]` | Lists the part types of the library, or one type's parameters and ports |
| `lightsim examples` | Lists the examples that come with LightSim |
| `lightsim schema [NAME] [--out DIR]` | Prints or writes the JSON Schemas of the [file formats](../../spec/README.md) |
| `lightsim notebook PROJECT [-o FILE]` | Writes a Jupyter notebook that runs the project and plots its results |
| `lightsim ai …` | Shows or changes what AI assistants may do: [AI access](#ai-access) |
| `lightsim version` | The version of LightSim, its Python API and its file formats |

`PROJECT` is a project file, or an example id (`bev-car`, `hybrid-car`,
`fs-electric`). Every command takes `--json` to print JSON instead of
text, and `-h` for its options.

`run` also takes:

| Option | Does |
|---|---|
| `--case NAME`, `-c` | The case, by name or id |
| `--all-cases` | Runs every case; put `{case}` in each `--out` name |
| `--out FILE`, `-o` | Writes the results: `.csv`, `.mat` (MATLAB, Octave), `.json` or `.parquet` (Parquet needs `pip install pyarrow`, so the Python package's `lightsim` writes it and the app's own engine does not); repeat for several |
| `--set PART.KEY=VALUE` | Changes a value for this run only, with its unit: `--set "Vehicle.mass_kg=1.9 t"`. It also replaces the case's own value, if the case sets one |
| `--no-check` | Skips the Data Checks |
| `--time-limit S` | Stops the run after S seconds of wall-clock time |

The files' contents are described in
[Runs and results](../../spec/results.md#exports).

## Exit codes

| Code | Means |
|---|---|
| 0 | Done: the checks passed, or the run is a *success* with every figure valid |
| 1 | The Data Checks found an error (`check`, or `run` before it started) |
| 2 | The run finished but is not valid: *warning*, *failed* or *cancelled*, or a figure is marked not valid |
| 3 | A usage or file error: an unknown option, or a file, case, part or unit that does not exist or fit |

## Examples

```text
lightsim run bev-car --case "City Cycle"
lightsim run my-car.json --all-cases --out "results/{case}.csv"
lightsim run my-car.json --case WLTC --set "Vehicle.mass_kg=1900 kg" --json
lightsim check my-car.json --strict
lightsim export runs/my-car/run-1.json.gz -o run-1.mat
```

In a CI pipeline, a failing check or an invalid run stops the job:

```text
lightsim check my-car.json && lightsim run my-car.json --all-cases --out "out/{case}.json"
```

## AI access

AI assistants (through LightSim's MCP server) reach your projects only
under rules you set. They start **off**.

| Command | Does |
|---|---|
| `lightsim ai status` | Shows the rules |
| `lightsim ai on`, `lightsim ai off` | Turns AI access on or off |
| `lightsim ai allow FOLDER`, `… disallow FOLDER` | Lets AI tools see the projects in a folder, or stops it. They see nothing else (the examples excepted) |
| `lightsim ai block PROJECT`, `… unblock PROJECT` | Hides one project from AI tools, whatever folder it is in (sets `"noAi": true` in the file) |
| `lightsim ai trust PROJECT`, `… untrust PROJECT` | Lets AI tools run this project's Script blocks. Changing a script, or a value of a Script block, takes the trust away, also when only one case sets it |
| `lightsim ai log [-n N]` | The last calls AI tools made |

What an AI tool can do once access is on:

- **Read** an allowed project, check it, and read its results.
- **Run** a case. A run stops after 300 s. A project with Script blocks
  (Python code that runs on your computer) runs only after you trust it,
  and each run asks you first. On Windows the Script sandbox is weaker
  (see [Known issues](../../KNOWN-LIMITS.md)), so trust only scripts you
  have read.
- **Change** a project only after you confirm each change.

Labels, descriptions and script text from your projects reach the AI as
quoted data, not as instructions. Every call, allowed or refused, is
written to `ai-audit.jsonl` next to the settings file (`ai-access.json`
in `%APPDATA%\LightSim` on Windows, `~/.config/LightSim` on Linux). The
log never leaves your computer.
