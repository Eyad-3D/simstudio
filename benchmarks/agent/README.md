# The agent test bench

What an AI agent achieves with LightSim's tools, measured the way the
makers of agent tools measure it: real modelling tasks, each scored by a
hidden checker that uses LightSim itself, tried several times by Claude
through the tools an outside agent has (the MCP server, the `lightsim`
command). Step 6 of the plan changes the tools only where this bench says
they got better on the visible tasks and no worse on the held-out ones.

Five kinds of task:

| Kind | The agent… | Scored on |
|---|---|---|
| `build` | builds a car from a written brief | the model it saved |
| `repair` | fixes a broken model | the model it saved |
| `question` | answers a question about results | its answer |
| `compare` | compares two designs and picks one | its answer |
| `write` | writes a part as a Script block | the model it saved |

A part written as equation text is a `write` task too, once the new engine
runs it; today's engine has only Script blocks.

The checker of a saved model loads it, runs its Data Checks (no errors, and
no more warnings than the reference model has), checks the values the brief
gives, runs the case the task names, and puts the figures the task names
inside bands around the reference model's figures. It builds and runs the
reference model itself, from the starting files and the reference solution.
The checker of an answer reads the last `ANSWER:` line of the agent's reply
and compares it with the key.

## Running it

From the repository root, with the backend's Python environment
(`backend/requirements-dev.txt`), on Linux (WSL on Windows) where the
headless `claude` command is signed in:

```sh
python -m benchmarks.agent validate        # every task's solutions through its checker (about 2 minutes)
python -m benchmarks.agent start build-city-ev /tmp/look   # a task's starting files, to look at them

export LSIM_BENCH_CAP_USD=2                # required: the most the run may spend, USD at list price
python -m benchmarks.agent.run --smoke --out ~/scratch/judge-cache/ai/bench-runs
python -m benchmarks.agent.run --mode mcp,cli --tries 3 --out ~/scratch/judge-cache/ai/bench-runs
python -m benchmarks.agent.run --tasks repair-hatchback --mode mcp --tries 1 --model sonnet --out …
python -m benchmarks.agent.run --set visible …   # or heldout; the default is all
python -m benchmarks.agent.run --auth api …      # by ANTHROPIC_API_KEY instead of the Claude plan
cd backend && python -m pytest tests/test_agent_bench.py -q
```

Each try is one `claude -p --output-format stream-json --verbose` run in a
fresh temporary folder outside the repository that holds only the task's
starting files. `--mode` says which tools the agent gets:

- `mcp`: LightSim's MCP server from this checkout, over stdio, allowed to
  see that folder and the examples, is the agent's only tool (with the two
  tools that read the server's resources, where its skills are).
- `cli`: the shell, allowed only the `lightsim` command, and that command
  refuses any path outside the folder and the `ai` command (`guard.py`).

Either way every other tool is left out and denied, Claude Code reads none
of the user's settings files, so none of their hooks, plugins or skills,
and no other MCP server (`--restricted`, `--strict-mcp-config`), and
whatever would ask for a permission is refused (`--permission-mode
dontAsk`, `--permission-prompts none`). The runner never skips permission
checks. So the agent cannot read the checkers, the answer keys,
`heldout.txt` or anything else in the repository; the smoke test checks
this with a planted task that asks for them (`smoke/`). Keep no
`~/.claude/CLAUDE.md` on the bench machine: these switches do not say they
leave it out.

During a try nobody answers questions: not the agent's, and not LightSim's
own (today `model_edit` asks the user before it saves, and `run_case`
before it runs a project with Script blocks). What the agent does then is
part of what the bench measures. Today's `lightsim` command has no command
that saves an edited model, so in `cli` mode the tasks scored on a saved
model fail until it has one.

**The cap.** `LSIM_BENCH_CAP_USD` is required. The runner adds up the cost
each try reports (`total_cost_usd`: USD at list price, also on a Claude
plan), stops before the next try once the sum reaches the cap, and gives
each try only what is left of it (`--max-budget-usd`). A try stopped at
`--timeout` (30 minutes) reports no cost, so it counts as all that was
left, and the run stops there. `--auth session` (the default) runs on the
Claude plan `claude` is signed in to and keeps `ANTHROPIC_API_KEY` away
from it; `--auth api` needs the key set. The run stops if a try used the
other account.

**What it records**, in a new folder under `--out` (never inside the
repository): `results.jsonl`, a line per try with success and the
checker's reasons, tool calls (and which), tool errors, permission
denials, tokens (input, output, cache read and written), cost, time,
turns, how the run ended, the model, and the transcript's path; the
transcript (`<task>-<mode>-<auth>-<try>.jsonl`, every message and tool
call) and its stderr; the files as the agent left them
(`…-files/`); and `summary.md` and `summary.json`: per task and mode the
tries, passes, pass^k (1 when all k tries passed, – when the cap cut the
tries short) and the means. Read the transcripts of failed tries for what
went wrong.

The smoke test (`--smoke`, with a cap of 2 USD) runs `question-loaded-ev`
once in each mode (and by API when `ANTHROPIC_API_KEY` is set), the
planted task once in each mode (it passes when the secret in its file
never reaches the agent), then sets the cap just above what was spent and
checks that it stops the run before the next try.

## The held-out set

`heldout.txt` names a third of the tasks, a third of each kind. Tune tools,
skills and their wording on the other tasks only, reading their transcripts
as much as you like. For a held-out task read only its scores (`--set
heldout`, or the held-out column of a summary), never its transcripts, so the
tools are not fitted to it. The file is never copied to an agent's folder,
and the agent cannot read the repository. When you add tasks, keep the list a
third of them and of each kind (the validator and the tests check both).

The bench has 33 tasks: at least six of each kind, 11 of them held out. They
start from the four bundled examples (a compact hatchback, an efficient
sedan, a Formula Student car, a P2 hybrid) or from a blank project, and set
traps an engineer would meet: a value in the wrong unit (Wh for kWh, lbf for
N, a fraction for a percent, centimetres for metres), a connection that is
missing, a value that only one case overrides, a part used outside its data
(a motor past its map, a heat pump below its working range), a design that
wins on one figure and loses on another (the quicker lap that uses more
energy, the longer range that costs more per 100 km, the lighter car that
does not finish), and a Script that must read a signal in the unit LightSim
keeps it in.

## A task file

One TOML file per task in `tasks/`, named by its id:

```toml
id = "repair-hatchback"            # = the file name
kind = "repair"                    # build, repair, question, compare or write
version = 1                        # raised whenever the task changes
title = "Repair a car model a colleague broke"
statement = """…"""                # all the agent is told, with the file names and the answer format

[[start]]                          # each starting file, derived when a try starts
file = "car.json"
from = "example:bev-car"           # a bundled example, or "blank" (an empty project)
name = "Compact hatchback"         # optional: the project's name
ops = [ … ]                        # model_edit operations (app/ai/edits.py) that make it

[reference]                        # the reference solution: these operations on that file
file = "car.json"
ops = [ … ]

[wrong]                            # a plausible wrong solution: must fail
file = "car.json"
base = "reference"                 # optional: the reference, then these operations
ops = [ … ]
answer = "ANSWER: 14.05; 1.10"     # for an answer task: a wrong answer

[[also_right]]                     # optional: another right solution, which must pass too
file = "car.json"
ops = [ … ]

[check]                            # for a task scored on a saved model
file = "car.json"
case = "City Cycle"
params = [ { type = "vehicle.body", key = "mass_kg", value = 1350, rtol = 0.002 } ]
figures = [
  { key = "consumption_kwh_per_100km", rtol = 0.01 },     # a summary number by stable key
  { key = "*.final_soc_pct", atol = 0.1 },                # one part's, whatever its id
  { channel = "vehicle.body:sig_speed", stat = "max", atol = 1.5 },  # max, min, mean or end
]

[[answer]]                         # for an answer task: one table per ;-separated field
value = 15.625
rtol = 0.02
figure = { file = "car.json", case = "WLTC Class 3b", key = "consumption_kwh_per_100km" }

[[answer]]                         # a choice between files
choice = "B"
options = { A = "car-a.json", B = "car-b.json" }
best = { case = "Acceleration 75 m", key = "accel_time_s", pick = "min" }
```

- `params`: every part of that type must have the value (the case's own,
  else the part's, else the library default), within `rtol` or `atol`, or
  exactly for text.
- `figures`: the agent's run and the reference's must end the same way
  (success, warning…), and each figure must lie within `rtol` (relative) or
  `atol` (absolute) of the reference's.
- `[[answer]]`: the key. `figure` says which number of which run it is, and
  the validator checks the key against LightSim (within 0.2 %); `best` lets
  it check a choice.

Starting files are never stored: only operations on the bundled examples
(`backend/projects/`) or on a blank project. A task file stays small.

## Adding a task

1. Pick the kind, and a question an engineer would really ask.
2. Write `[[start]]`: an example, or a blank project, and the operations
   that make the starting file (a fault to repair, a variant to ask about).
   Change something, so the answer is not in the docs or the example.
   Look at it with `python -m benchmarks.agent start <id> <folder>`.
3. Write the statement in plain words: what the files are, what to do,
   where to save, and for an answer the exact `ANSWER:` line.
4. Write the reference solution and a wrong one that a careless agent might
   give (a fault missed, the unloaded car's figure, the other design).
5. For a model task, pick a short case and the figures that show the task
   was done; make the bands wide enough for any right way of doing it and
   narrow enough to fail the wrong one. When there is more than one right
   way to build it, add it as `[[also_right]]`.
6. For an answer task, run the reference and put the figures in the key
   (the validator says what LightSim computes when they differ).
7. `python -m benchmarks.agent validate <id>` until it says ok; then
   `cd backend && python -m pytest tests/test_agent_bench.py -q`.
8. Keep `heldout.txt` a third of the tasks and of each kind (`python -m
   benchmarks.agent validate` says when it is not).
