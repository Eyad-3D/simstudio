# The baseline: what agents achieve with today's tools

Step 6, item 0. The question is the owner's: which is the cheapest model and
effort level that drives LightSim successfully? This is the answer on
LightSim as it is, before any step 6 change to the tools.

- **Date:** 31 October to 1 November 2026.
- **LightSim:** commit `b228609` (branch `work/ai`). No tool was changed for
  the run.
- **Run:** `python -m benchmarks.agent.compare --name baseline --jobs 8`,
  stages A, B and C, in the way `README.md` describes. Claude Code 2.1.316,
  signed in to the owner's plan, no API key. The plan's five-hour limit
  stopped it twice: 8 tries at 22:06 UTC on 31 October (the limit reset at
  22:10 and the run went on), and 6 tries at 23:07 (the limit reset at 03:10
  on 1 November); WSL then shut down while the runner waited. The same
  command resumed it at 11:12, and the last try ended at 11:17. The 14 tries
  the limit had stopped were run again, not scored as failures.
- **Scoring again:** the checker of `question-fs-endurance` failed a right
  answer (below). Its key now takes both readings of the question, and that
  task's tries were scored again by hand from their records, with
  `agent.check` on each try's files and final reply, into a copy of the
  run's folder (`README.md` says when and how). Every figure here is the
  re-scored one.
- **Records:** `/home/eyada/scratch/judge-cache/ai/bench-runs/baseline` (as
  run: `report.md`, `report.json`, `tries/`, and each try's transcript
  under `runs/`) and `baseline-rescored` beside it (the records scored again
  and their report). They stay outside the repository; the transcript lines
  quoted below are from there, named by try.
- **Held-out tasks:** only their scores are given here (the held-out columns,
  and the costs). Every mistake below is counted on the 22 visible tasks
  only, and every line quoted is from a visible task, so the step 6 changes
  are not fitted to the held-out ones (`README.md`, "The held-out set").
- **Models that answered:** `claude-haiku-5-5`, `claude-sonnet-5-5`,
  `claude-opus-5-5`, and only these. No setting was refused or changed, and
  every try was billed to the plan.

## The two answers

**1. The cheapest working setting: none. No setting drives LightSim
successfully today.** Even the ceiling (Opus 5.5, high) passes all three
tries on only 13 of the 33 tasks, and on 4 of the 11 held-out ones; hands-off
work needs 90 % of both. The cheapest setting that is as good as the ceiling
is **Haiku 5.5 at low effort: 13 of 33 tasks, 0.0059 USD a try**, against
13 of 33 and 0.2472 USD for the ceiling (42 times as much). Sonnet 5.5 at
medium effort, the owner's expectation, was tested and is as good as the
ceiling too (13 of 33), at 0.0886 USD a try, 15 times Haiku low's price. The
settings cannot be told apart on today's tools: six of the eight solve the
same 13 tasks, all of them questions and comparisons. Haiku 5.5 at medium
misses one of them (one try answered 0.22 kWh for 0.2246, 2.1 % off, outside
the 2 % band), and Sonnet 5.5 at extra-high misses a held-out comparison.

**2. Is any setting good enough for hands-off work? No.** The best have 13
of 33 tasks (39 %) and 4 of 11 held out (36 %). All 19 tasks that build,
repair or write a model score 0 at every setting, the ceiling's included,
and on the visible ones the cause is not the agents (next section). That
gap is what the later changes of step 6 must close, with Sonnet 5.5 at
medium as the target.

## Why: the tools ask, and a headless agent is refused

Today's MCP server asks the user before it saves an edit (`model_edit` with
`dry_run` false) and before it runs a project that has Script blocks
(`run_case`). A headless agent has nobody to ask: `claude -p` declines at
once, and the tool answers "The user did not confirm, so nothing was done."

On the 22 visible tasks, 313 of the 528 MCP tries failed. **312 of them
(all but one) are the 13 visible build, repair and write tasks** (8
settings, 3 tries), and every one of the 312 was refused a save; no try
ever saved a file. The one other failure is the rounded answer of Haiku 5.5
at medium on `question-fs-endurance` (above).

The agents behaved well. In 311 of the 312 refused tries the agent's last
reply says that the save was declined and nothing was saved (read by its
words, not by a judge). In 268 of them the agent first checked its edit
with a clean dry run, which is allowed, before it asked to save; 306 made a
clean dry run at some point. Typical lines:

- `repair-hatchback.mcp.sonnet-medium.1`: `model_edit` with `"dry_run": false`
  returns "The user did not confirm, so nothing was done." The agent writes
  "I haven't saved anything: both attempts to write to car.json were declined
  at the confirmation prompt, so the file is unchanged."
- `build-add-brakes.mcp.opus-high.1`: "I prepared the brake changes and tested
  them in a dry run, but when I asked to save, the confirmation was declined.
  The file is still exactly as it was, and I won't try again without your
  go-ahead."

**What would the same tries have scored if the save had been allowed?** An
estimate, not a measurement, on the visible tasks only. For each of the 312
refused edit tries, the checker was run on the starting files plus the last
edit the agent had prepared (the call that was refused, or its last clean
dry run). Unedited starting files fail the checker (the validator's blank
solution, for every task), so the passes are real. **All 312 pass.** With
saves allowed, every setting would then have passed all three tries on all
22 visible tasks, except Haiku 5.5 at medium (21: its rounded answer).

Read this as "the tools, not the models, stood in the way", and not as a
result: the agents were never seen to verify a saved model, and the
estimate says nothing of the held-out tasks, which are judged on their
scores alone.

## The settings

MCP server, stages A and B: three tries on each of the 33 tasks, 99 tries a
setting. "Solved once" is the first try; pass^3 needs all three. Costs are at
API list price (the runner's figure equals the tool's own `total_cost_usd`
to the cent).

| Setting | Solved once (all / visible / held out) | pass^3 (all / visible / held out) | Mean cost a try, USD | Total, USD | Per solve, USD | Tool calls | Tool errors | Time, s |
|---|---|---|---|---|---|---|---|---|
| Haiku 5.5, low | 13 / 9 / 4 | 13 / 9 / 4 | 0.0059 | 0.58 | 0.0149 | 9.5 | 1.0 | 24 |
| Haiku 5.5, medium | 12 / 8 / 4 | 12 / 8 / 4 | 0.0067 | 0.66 | 0.0173 | 10.2 | 1.0 | 27 |
| Haiku 5.5, high | 13 / 9 / 4 | 13 / 9 / 4 | 0.0077 | 0.77 | 0.0197 | 11.2 | 0.9 | 33 |
| Sonnet 5.5, low | 13 / 9 / 4 | 13 / 9 / 4 | 0.0787 | 7.79 | 0.1998 | 6.4 | 1.2 | 21 |
| Sonnet 5.5, medium | 13 / 9 / 4 | 13 / 9 / 4 | 0.0886 | 8.77 | 0.2248 | 7.0 | 1.1 | 24 |
| Sonnet 5.5, high | 13 / 9 / 4 | 13 / 9 / 4 | 0.1104 | 10.93 | 0.2802 | 8.5 | 0.7 | 25 |
| Sonnet 5.5, extra-high | 12 / 9 / 3 | 12 / 9 / 3 | 0.1633 | 16.16 | 0.4254 | 11.9 | 0.9 | 38 |
| Opus 5.5, high (ceiling) | 13 / 9 / 4 | 13 / 9 / 4 | 0.2472 | 24.47 | 0.6275 | 10.3 | 0.8 | 43 |

By kind, pass^3 (8 question tasks, 6 compare tasks; every setting scores 0 of 6
builds, 0 of 7 repairs and 0 of 6 writes):

| Setting | question | compare |
|---|---|---|
| Haiku 5.5, low | 7 | 6 |
| Haiku 5.5, medium | 6 | 6 |
| Haiku 5.5, high | 7 | 6 |
| Sonnet 5.5, low | 7 | 6 |
| Sonnet 5.5, medium | 7 | 6 |
| Sonnet 5.5, high | 7 | 6 |
| Sonnet 5.5, extra-high | 7 | 5 |
| Opus 5.5, high | 7 | 6 |

Command line, stage C (once on each task, for the cheapest setting as good as
the ceiling and for the ceiling; no pass^3):

| Setting | Solved once (all / visible / held out) | Mean cost a try, USD | Total, USD | Tool calls | Tool errors | Time, s |
|---|---|---|---|---|---|---|
| Haiku 5.5, low | 2 / 0 / 2 | 0.0034 | 0.11 | 5.5 | 1.5 | 19 |
| Opus 5.5, high | 13 / 9 / 4 | 0.1836 | 6.06 | 12.2 | 1.8 | 61 |

The `lightsim` command has no command that saves an edited model, so the 19
build, repair and write tasks cannot pass in this mode whatever the agent
does: 38 of the 51 command-line failures. Opus solved 13 of the 14
questions and comparisons; Haiku at low effort solved two.

## What it cost

- **The whole comparison: 77.66 USD at API list price**, on the plan (nothing
  was billed to an API account). That is 858 scored tries: 70.13 USD for the
  792 MCP tries, 6.17 USD for the 66 command-line tries, plus 1.36 USD for
  the 14 tries a plan limit stopped and that were run again. Scoring again
  ran no try and cost nothing.
- **Tokens:** input 11,382, output 2,557,894, cache read 79,536,213, cache
  write 16,501,410.
- **Time:** 7.3 hours of tries, and 13.2 hours from the first try's start
  (22:03 on 31 October) to the last one's end (11:17 on 1 November): about 4
  of them waiting for the plan's limit to reset (23:07 to 03:10), and about 8
  with the machine down before the run was resumed.
- **What one try costs, per setting** (mean at list price, MCP, all 33
  tasks). A question or a comparison costs a third to two thirds of an edit
  try, which is longer. Every edit try stopped at the refused save: once
  saves work, the agent will verify the saved model and a try will cost more.

| Setting | All tasks | Build, repair, write | Question, compare |
|---|---|---|---|
| Haiku 5.5, low | 0.0059 | 0.0078 | 0.0032 |
| Haiku 5.5, medium | 0.0067 | 0.0089 | 0.0036 |
| Haiku 5.5, high | 0.0077 | 0.0101 | 0.0046 |
| Sonnet 5.5, low | 0.0787 | 0.1097 | 0.0366 |
| Sonnet 5.5, medium | 0.0886 | 0.1208 | 0.0448 |
| Sonnet 5.5, high | 0.1104 | 0.1402 | 0.0699 |
| Sonnet 5.5, extra-high | 0.1633 | 0.1881 | 0.1296 |
| Opus 5.5, high | 0.2472 | 0.2878 | 0.1921 |

More effort cost more and bought nothing: Sonnet 5.5 at extra-high costs 1.8
times medium and scored one task fewer.

## What agents got wrong

On the 22 visible tasks only: 528 MCP tries and 44 command-line ones. Ranked
by the number of MCP tries it touched, then by what it cost. "Setting" is
what differs by model or effort. "Recovered" means a later call to the same
tool in that try went through. The first item is a design choice of the
tools, not an agent mistake; the rest are the mistakes. Most were recovered,
but each cost a call, and in a longer task a weak model that is not
recovered stops there. Mistakes in fewer than three tries are left out.

**1. The save is refused (312 tries, 39 at every setting).** The cause, the
effect and the lines are in the section above. Setting: Sonnet 5.5 at low
and medium effort send the same call again after the refusal in 9 of their
39 tries each, and ask to save again in some other way in one more each;
Haiku 1 to 2, Sonnet high 2, extra-high 3, Opus none. Each repeats a call
that cannot succeed. `repair-hatchback.mcp.sonnet-medium.1` sends the same
`dry_run: false` call twice and is declined twice.

**2. An operation has no `op` key (40 tries; 39 recovered).** `model_edit`
takes a list of free-form objects. Its description names the operations
(set, add, connect...) but not that the key is `op`, and the schema is
`{"type": "object"}`, so agents put the name in as a key or as `type`.
Setting: mostly Haiku (low 10, medium 12, high 9), then Sonnet low 5,
medium 2, high 2; none for extra-high or Opus.
- `repair-hatchback.mcp.haiku-low.3` sends `{"set": "Vehicle", "param":
  "mass_kg", "value": 1927}` and gets "Operation 1 (None): Unknown 'op': use
  set, add, remove, connect, disconnect, add_port, add_case or set_case.
  Nothing was changed."
- `build-add-brakes.mcp.haiku-medium.1` sends `{"add": true, "type":
  "mech.brake", "label": "Brake FL"}`; `build-add-brakes.mcp.haiku-low.3`
  sends `{"type": "mech.brake", "label": "Brake FL"}`; same reply.

**3. `compare_runs` on runs of two files (19 tries, none recovered through
that tool).** All 19 are on compare tasks, where the agent runs the same case
on two project files (`car-a.json`, `car-b.json`) and compares the two run
ids. The tool compares runs of one project, and says only "No run ... of this
project (recent runs: ...)", naming a run that does exist. The agents then
compared the two summaries by hand, and still solved the tasks, but each lost
a call.
- `compare-battery-range.mcp.sonnet-low.1`: `compare_runs` with project
  `car-a.json`, `run_a` `ai-20261031-221221-ebf457`, `run_b`
  `ai-20261031-221221-1fc025` (the run of `car-b.json`) returns "No run
  'ai-20261031-221221-1fc025' of this project (recent runs:
  ai-20261031-221221-ebf457)."
- Setting: Sonnet medium 7, low 3, extra-high 3, Opus 3, Haiku high 2 and
  medium 1.

**4. A tool is called without its server prefix (22 calls in 17 tries; 16
recovered).** Claude Code names the tools `mcp__lightsim__run_case`; the
agent called `run_case`, `results_query`, `lightsim_overview` or
`element_read`, and the harness replies "No such tool available". The
skills and LightSim's own messages name tools without it ("start one with
run_case"). Setting, by call: Sonnet low 9, extra-high 6, Opus 3, Sonnet
medium 2, Haiku high 1 and medium 1.
- `compare-battery-range.mcp.sonnet-low.3`: "Error: No such tool available:
  run_case".

**5. A part is read by name instead of id (17 calls in 16 tries; only 6
recovered).** `element_read` takes an element id; agents give a label
(`Battery`, `Wheel`), a type (`signal.driving_task`) or a case id (`case-1`).
The reply is "No part 'Battery' in this project: lightsim_overview lists
them." and a weak model often gives up reading the part. Setting, by call:
Haiku 11 of the 17 (low 3, medium 4, high 4), Sonnet medium 3, high 1,
extra-high 1, Opus 1.
- `build-city-ev.mcp.haiku-medium.1`: `element_read` with element `Battery`.

**6. A parameter is named by its label or a guess, not its key (8 tries; 5
recovered).** The reply lists the real keys, but a model does not always
pick the right one. Setting: Sonnet low 3, Opus 2, Haiku low 1, Sonnet
medium 1, high 1.
- `repair-wont-start.mcp.opus-high.2` and `.3`: `"Minimum SOC"` for
  `min_soc_pct`.

**7. A part type or a port is guessed (5 tries, all recovered).** `add` of
`Brake` gets "No library part 'Brake' (lightsim://components lists them).
Nothing was changed." (4 tries: Sonnet low 3, medium 1, all on
`build-add-brakes`); `build-add-brakes.mcp.sonnet-low.1` connects a
mechanical node's `flange2` and gets "'Node FL' has no port 'flange2' (its
ports: f1, f2, f3, f4, sig_speed)".

**Not an agent mistake: the `question-fs-endurance` checker.** The task asks
how far the car gets "before the accumulator is empty". The key was the
run's own "Distance driven", 20.557 km within 1 %. But LightSim's own
message says the accumulator reached its minimum charge (5 %) at
t = 1263 s, where the car's distance channel reads 20.023 km, and the car
then coasts on to 20.557 km. Both readings are right; the careful agents
gave the second, and 13 MCP tries (Opus 3 of 3, Haiku medium and high 3 of
3 each, Sonnet high 2, extra-high 2) and the Opus command-line try were
failed for it. The key now takes both readings (`also` in the task file;
a test works the figures out from the run). An answer must fit one of them
as a whole: the run's 20.557 km with the run's own energy per lap, or
20.023 km with either the run's energy per lap or the net energy over the
laps up to there, 0.2305 kWh. The tries were scored again: 12 of the 13
MCP failures and the command-line one now pass. The one left is the
rounded energy of Haiku 5.5 at medium.
- `question-fs-endurance.mcp.opus-high.1`: "Charge hits 5 % at t ≈ 1262.6 s,
  when the car's distance reads 20,023 m ... The summary's "Distance driven" of
  20.56 km counts that rolling." It ends `ANSWER: 20.0; 0.229`, which now
  passes.

The energy key is still the run's own Energy per lap, 0.22462 kWh, which
LightSim marks not valid because the run stopped before the event ended.
It is kept: it is the battery's net energy (delivered less recuperated,
4.717 kWh) over the 21 laps the car drove, which is what the question asks;
the flag is about the unfinished event, not about that number.

**Command-line mode** (44 visible tries):
- **No way to save.** The `lightsim` command cannot save an edit, so all 26
  visible edit tries failed; the agents say so. The edit tasks are not
  measurable in this mode today.
- **Cannot read a time series.** To get a peak current (`question-peak-current`)
  `lightsim run` prints only the summary. Opus recovered by bisection:
  `question-peak-current.cli.opus-high.1`: "Can't read the result files with
  only `lightsim`, so I'll bisect" (27 tool calls, correct).
- **Shell commands are refused, by the bench's own fence.** The runner
  allows the agent only the `lightsim` command (`--allowedTools
  "Bash(lightsim),Bash(lightsim *)"`, with `--permission-mode dontAsk`), so
  any command with something else in it is refused. That happened in 42 of
  the 44 tries, 57 times (Haiku low 31, Opus 26), and every refused command
  had another command in it: `echo` (46), a `for` loop (22), `head` (16),
  `sed` (9), `python3` (9), `grep` (7), a `cat` heredoc, `cd`. Commands
  made only of `lightsim` calls, even several joined by `;`, were allowed.
  Opus works around the fence. Haiku 5.5 at low effort did not: all 9 of
  its failed answer tries had a refused command, after which it answered
  "unavailable", "unknown" or "unresolved" (8 tries) or gave no answer and
  asked to be allowed to run the case (1). `question-loaded-ev.cli.haiku-low.1`: "The
  permission system denied the one command that runs the WLTC Class 3b
  case, so I have no consumption or regen figures to report." This is a
  property of the bench, not of LightSim; an agent given a wider shell
  would not meet it.
- **Out-of-folder commands.** The runner's guard refused a path outside the
  folder, or the `ai` command, in 15 tries (Opus 13, Haiku 2); it is the
  bench's own fence too, not a LightSim fault.

### The mistakes by setting, in short

| Setting | Most typical mistakes |
|---|---|
| Haiku 5.5 (all efforts) | Operation without `op` (31 of 40 tries), part read by label (11 of 17 calls). In command-line mode it stops at the first refused command. Cheapest by far. |
| Sonnet 5.5, low and medium | The refused save sent again (9 of 39 tries each), part types guessed (all 4), bare tool names at low (9 calls), compare_runs on two files at medium (7). |
| Sonnet 5.5, high and extra-high | The fewest tool errors at high (0.7 a try); at extra-high bare tool names (6 calls). Costs 1.2 to 1.8 times medium for no gain. |
| Opus 5.5, high | Few tool errors: a bare tool name, compare_runs on two files and a parameter by its label, 2 to 3 tries each. |

## What this means for step 6

- Remove the per-run approvals (the owner's decision), made safe by design:
  on the visible tasks this alone moves the estimate from 8 or 9 of 22
  tasks to all 22 for every setting but one (21). It is the first item, and nothing else
  on the list is as large.
- Then the small ones, in the order above: say in the `model_edit` description
  that each operation has an `op` key and give the schema its list of
  operations; have the errors name the right key, id or port (and say that
  runs of two projects cannot be compared); make the tool names the agent sees
  match the names in the docs.
- Run the comparison again once the saves work: the cost per try and the order
  of the settings will change, because the agents will then verify what they
  saved.
