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
gives, the part it asks for (a `write` task's Script block, by its label and
ports) and, for a `repair` task, that nothing else was changed, runs the case
the task names, and puts the figures the task names inside bands around the
reference model's figures. It builds and runs the reference model itself,
from the starting files and the reference solution. The checker of an answer
reads the last `ANSWER:` line of the agent's reply and compares it with the
key.

## Running it

From the repository root, with the backend's Python environment
(`backend/requirements-dev.txt`), on Linux (WSL on Windows) where the
headless `claude` command is signed in:

```sh
python -m benchmarks.agent validate        # every task's solutions through its checker (about 8 minutes)
python -m benchmarks.agent start build-city-ev /tmp/look   # a task's starting files, to look at them

python -m benchmarks.agent.run --smoke --out ~/scratch/judge-cache/ai/bench-runs
taskset -c 0-11 python -m benchmarks.agent.compare --out ~/scratch/judge-cache/ai/bench-runs --name oct --jobs 8
python -m benchmarks.agent.compare … --name try --stage A --tasks repair-hatchback --settings sonnet-medium
python -m benchmarks.agent.compare … --report     # the report again, from the records
cd backend && python -m pytest tests/test_agent_bench.py tests/test_agent_compare.py -q
```

Each try is one `claude -p --output-format stream-json --verbose` run in a
fresh temporary folder outside the repository that holds only the task's
starting files. The mode says which tools the agent gets:

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
part of what the bench measures. Headless `claude -p` declines such a
question at once ("The user did not confirm, so nothing was done."), so
today every task scored on a saved model fails in `mcp` mode too, however
right the agent's dry run was (the pilot of 31 October: all 16 repair and
write tries, at every setting, the ceiling's included). So does
`question-hybrid-load`: its P2 hybrid has Script blocks, so `run_case` asks
before it runs it. That leaves 13 of the 33 tasks (4 of the 11 held out)
that any setting can solve in `mcp` mode today. Today's `lightsim` command
has no command that saves an edited model, so in `cli` mode the tasks scored
on a saved model fail as well (the hybrid runs there).

**On the plan, with no cap.** A try runs on the Claude plan `claude` is
signed in to, and only there (the owner's decision of 30 October): there is
no API-key mode. The runner removes from the agent's environment every
variable that would make it bill an API account instead (`ANTHROPIC_API_KEY`,
`ANTHROPIC_AUTH_TOKEN`, any other `ANTHROPIC_*`, the `CLAUDE_CODE_USE_*`
provider switches, `AWS_BEARER_TOKEN_BEDROCK`) or pick another effort
(`CLAUDE_CODE_EFFORT_LEVEL`), records which billing each try used, and
stops if one used a key or a token. A key shows in the try's init event
(`apiKeySource`); a token does not (it says `"none"`), so the runner also
reads Claude Code's debug log, which then says "API-key auth precedence
active". Nothing is stopped or shortened for cost: there is no
`--max-budget-usd`, and `LSIM_BENCH_CAP_USD` is ignored.

**The cost** of each try is recorded twice: at Anthropic's API list price,
worked out by the runner from the tokens of each request (each request on
its own rate card: Haiku 5.5 costs five times as much on a prompt over
100 000 tokens), and as the tool reports it (`total_cost_usd`, with its
`costBasis`). The runner works its own out so that the figure does not hang
on the tool's price list: Claude Code 2.1.273 did not know the 5.5 models,
priced them as Claude Opus 5 (`costBasis: "unknown"`) and assumed a 200 000-token
context for them; 2.1.316 knows them (`"list"`), and the two figures agree.
The list prices are in `run.py` (`PRICES`), from Anthropic's pricing page;
change them there when Anthropic does.

## The comparison

`python -m benchmarks.agent.compare` answers the owner's question: which is
the cheapest model and effort level that drives LightSim successfully. A
setting is a model and an effort level, passed as `--model <id> --effort
<level>`. The eight settings, cheapest first:

| Setting | Model | Effort |
|---|---|---|
| `haiku-low`, `haiku-medium`, `haiku-high` | Haiku 5.5 (`claude-haiku-5-5`) | low, medium, high |
| `sonnet-low`, `sonnet-medium`, `sonnet-high`, `sonnet-xhigh` | Sonnet 5.5 (`claude-sonnet-5-5`) | low, medium, high, extra-high |
| `opus-high` | Opus 5.5 (`claude-opus-5-5`) | high: the ceiling |

The ceiling tells a task that is too hard or broken from a model that is too
weak. One command runs three stages, so the three tries go where the answer
depends on them:

- **A**: every setting on every task once, through the MCP server.
- **B**: two more tries (three in all) for the ceiling, for Sonnet 5.5 at
  medium (the owner's expected answer) and for every setting that solved at
  least the ceiling's count minus 5 in stage A.
- **C**: the command line once on every task, for the cheapest working
  setting and the ceiling.

`--jobs` (3) runs tries side by side; run it under `taskset -c 0-11` on the
shared PC, and every process it starts keeps to those CPUs. Measured on 31
October: 3 tries at once peaked at 1.0 GB and 3 cores, 8 at once at 2.3 GB
and 4.4 cores (about 0.3 GB and half a core a try, the checker included), so
`--jobs 8` keeps well inside 12 CPUs and 6 GB, with room for tries that run
more simulations once edits can be saved. More jobs reach the plan's usage
limits sooner; a limit only pauses the comparison. The tries go task
by task across all the settings, not setting by setting, so a comparison
stopped halfway still compares the settings on the same tasks.

Every try ends in one class:

- **solved** or **failed**: the checker's verdict on what the agent left.
  LightSim errors the agent hit and gave up on, refusals by LightSim's own
  approval prompts, a full context and a try stopped at `--timeout` (30
  minutes) are failures: they are what today's tools do. Only these two are
  scored.
- **not run**: a plan usage limit, a rate limit, an overloaded or network
  error from the service, or the runner's own fault. It is noted in
  `notrun.jsonl`, never scored, and retried: after a limit, from the reset
  time the stream gives, else after a back-off that doubles from 1 minute
  up to 30; the report counts the retries per setting. The comparison stops
  only when `claude` is signed out, a try used an API key, or the runner
  failed three times on one try (ten times for service errors).
- **unavailable**: the plan refused the model, the tool dropped the effort
  (its debug log says the API refused it), or another model answered. The
  setting is not run again, and the report says why. No setting ever runs
  under another's name.

Each try's record is written when it ends (`tries/<task>.<mode>.<setting>.<try>.json`),
so the same command again skips the finished tries: a comparison stopped by
a limit, a crash or a restart goes on where it stopped. A record of an older
version of its task (the task changed since) is not reused: it moves to
`tries/older/`, counts in the cost only, and the try runs again.

**What it records**, in `--out/--name` (never inside the repository): per
try, the class and why, the checker's reasons and figures, the model asked
for and the models the run reported (its init and result events and every
answer), the effort, the billing, the Claude Code version, tool calls (and
which), tool errors, permission denials, Claude Code's own retries, tokens
(input, output, cache read and written), both costs, time, turns and how it
ended; under `runs/`, each attempt's transcript (with each request's
tokens), stderr, Claude Code's debug log and the files as the agent left
them.

**The report** (`report.md` and `report.json`, written by the command) has,
per setting and mode: solved once and pass^3 (where it ran), visible and
held out apart and per kind; the mean cost of a try and the total, at list
price; the cost per solved try; mean tokens, tool calls, tool errors and
time; and the not-run tries retried. Then two answers the command works out:

1. **The cheapest working setting**: the lowest mean cost per try among the
   settings whose pass^3 count on all tasks is at most 2 below the
   ceiling's, with the tasks the ceiling solves on all three tries that it
   does not, and the reverse. Being as good as the ceiling counts only when
   the ceiling itself is good enough for hands-off work (below); when it is
   not, as on today's tools, the answer is that no setting drives LightSim
   successfully yet, and the cheapest setting as good as the ceiling follows
   on its own line (stage C still runs it).
2. **Good enough for hands-off work**: pass^3 on at least 90 % of all tasks
   and of the held-out tasks. When no setting is, the report says so: that
   gap is what step 6's later tool changes must close, with Sonnet 5.5 at
   medium as the target.

The smoke test (`run --smoke`) runs `question-loaded-ev` and the planted task
once in each mode on Sonnet 5.5 at medium (`--model`, `--effort`): the
planted task passes when the secret in its file never reaches the agent.

## The held-out set

`heldout.txt` names a third of the tasks, a third of each kind. Tune tools,
skills and their wording on the other tasks only, reading their transcripts
as much as you like (read those of failed tries for what went wrong). For a
held-out task read only its scores (the held-out columns of the report),
never its transcripts or its records' reasons, so the tools are not fitted
to it. The file is never copied to an agent's folder,
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

[[also_wrong]]                     # optional: another wrong one, which must fail too
file = "car.json"
ops = [ … ]                        # (or answer = "…")

[check]                            # for a task scored on a saved model
file = "car.json"
case = "City Cycle"
unchanged = true                   # a repair: nothing the reference leaves alone may change
require = { type = "signal.script", label = "Speed Limiter", ports = ["demand", "target"] }
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
- `unchanged`: every parameter of every part, in every case, that the
  reference leaves as it is in the starting file must keep its value (within
  0.2 %): "change nothing else" and "do not touch the other cases". Parts
  are matched by id, else by their label. Wires and links are not compared:
  the figures show those.
- `require`: the part the brief asks for: one part of that type with that
  label, each of whose ports is linked. A `write` task needs it, or a Lookup
  Table under the asked label would do.
- `figures`: the agent's run and the reference's must end the same way
  (success, warning…), and each figure must lie within `rtol` (relative) or
  `atol` (absolute) of the reference's.
- `[[answer]]`: the key. `figure` says which number of which run it is, and
  the validator checks the key against LightSim (within 0.2 %); `best` lets
  it check a choice. The validator also swaps every two numbers of the key:
  the answer must then fail, so make the bands narrower than the gap
  between them.

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
