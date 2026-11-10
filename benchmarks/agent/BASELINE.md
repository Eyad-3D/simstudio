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


---

# After the safety part: the same comparison, saves go through

Step 6, item 0 again, now that an agent's saves and runs no longer wait on a
person. The text above is the before and is left as it was; this part is the
after.

- **Date:** 6 November 2026.
- **LightSim:** commit `067accd` (branch `work/ai`; the baseline's was
  `b228609`). It is the safety part (saves and runs that wait on nobody, the
  command-line tool that saves, the tool problems of the baseline fixed) plus
  the fix of the bench's SAYS check (it now knows all 16 unit groups and
  takes only the library's unit for a quantity). No tool was changed for the
  run. The commit is also in
  `/home/eyada/scratch/judge-cache/ai/bench-runs/after-safety.commit`.
- **Run:** `python -m benchmarks.agent.compare --name after-safety --jobs 8`,
  the baseline's command: all stages, all eight settings, all 33 tasks.
  Before it, a trial in the same folder (so its tries are kept, not run
  twice): `--stage A` on five visible tasks, one of each kind
  (`question-loaded-ev`, `compare-battery-range`, `build-city-ev`,
  `repair-hatchback`, `write-speed-limiter`), 40 tries in MCP mode at all
  eight settings; then `--stage all --settings opus-high` on the same five
  tasks (stage C waits for the cheapest working setting), which ran the
  ceiling's second and third MCP tries on them (stage B, 10 tries) and its
  5 command-line tries. All 55 tries were solved, so there was no STOP.
  Claude Code 2.1.316, signed in to the owner's plan, no API key.
- **Plan limit:** the plan's five-hour limit stopped the run once. Four
  command-line tries (3 at the ceiling, 1 at Haiku low) were marked not run
  and were run again after the reset at 13:20 UTC. They are not scored as
  failures; their cost (0.91 USD) is in the total below.
- **One band widened after the run, and its tries scored again:**
  `question-fs-endurance` asks for the energy per lap in kWh and names no
  precision. Its band was 2 %, so 0.22 (the run's 0.2246 to two decimals,
  2.06 % off) failed, while 0.23 for the other reading's 0.2305 passed. The
  band is now 2.5 % (commit `dc8231f`; a change to the key alone, so the
  task keeps version 1 and its tries were not run again). The task's 26
  records were scored again into a copy of the run's records,
  `/home/eyada/scratch/judge-cache/ai/bench-runs/after-safety-rescored`
  (script and output in `/home/eyada/scratch/lane-ai/fix-rerun`,
  `rescore.py` and `rescore.out`). Five tries go from failed to solved: in
  MCP mode Haiku low try 2, medium try 1, high try 1 and Sonnet high try 3;
  on the command line Haiku low. None goes the other way. The figures
  below are the re-scored ones, and the text says where the 2 % band gave
  another.
- **Records:** `/home/eyada/scratch/judge-cache/ai/bench-runs/after-safety`
  (`report.md`, `report.json`, `tries/`, each try's transcript under `runs/`)
  and my reading of the tries in
  `/home/eyada/scratch/judge-cache/ai/after-safety-analysis`.
- **Held-out tasks:** as before, only their scores and costs are used. Every
  mistake below is counted on the 22 visible tasks, and every line quoted is
  from a visible task.
- **Models that answered:** `claude-haiku-5-5`, `claude-sonnet-5-5`,
  `claude-opus-5-5`, and only these. Every try was billed to the plan.

## The two answers

**1. The cheapest working setting: Haiku 5.5 at low effort.** It passes all
three tries on all 33 tasks (22 of 22 visible, 11 of 11 held out) at
**0.0063 USD a try**, 0.0063 USD a solve. The ceiling (Opus 5.5, high) passes
33 of 33 at 0.2524 USD a try: 40 times as much. **Sonnet 5.5 at medium
effort, the owner's expectation, was tested and works: 33 of 33 tasks on all
three tries at 0.0825 USD a try**, 13 times Haiku low's price. Every setting
but Sonnet 5.5 at high passes every task on every try (Sonnet high is 32 of
33: one try answered 0.234 kWh, which fits no reading). So Haiku low is the
cheapest by either rule: the report's ("as good as the ceiling", at most two
tasks below it) and every task on every try. Under the 2 % band the run
had, Haiku at all three efforts missed one try of the endurance question
for writing 0.22, and the cheapest setting with every task on every try was
Sonnet 5.5 at low (0.0753 USD a try). That turned on the band alone, not on
a mistake of the agent. Both statements are for the MCP server.

**2. Is any setting good enough for hands-off work? In MCP mode, yes: all
eight are.** The bar is pass^3 on at least 90 % of all tasks and of the
held-out ones. Every setting has 97 % or more of all tasks (32 or 33 of 33)
and 11 of 11 held out. Before, none had more than 13 of 33. In
command-line mode (once on each task, so no pass^3) only the ceiling was
shown to work: Opus 5.5 at high solved 31 of 33 (20 of 22 visible, 11 of 11
held out). Haiku 5.5 at low has no command-line figure here. The bench's own
fence (below) refused a command in 32 of its 33 tries, and every one of its
28 failed tries had a refused command, so its 5 of 33 measures the fence,
not Haiku. Once the prompt named the fence it solved 31 of 33, once on
each task, as many as the ceiling ("After the check" below); that is one
try a task outside the comparison, so it is not in the answers. The limits
of the claim: three tries a task,
33 tasks, briefs that say plainly what to build, and an agent that is given
the tools and skills.

## Do the saves and runs go through now (the trial's question)

Yes. In MCP mode all 456 tries on the 19 build, repair and write tasks made
a save, none was declined, and none failed; 397 of them (87 %) then ran the
case on the saved file. In command-line mode Opus saved with `lightsim edit`
and solved 18 of the 19 build, repair and write tasks (the miss is a wrong
wheel radius, below). No try at any setting was stopped by an approval, a
sandbox that could not be set up, or a tool that could not be reached. The
baseline's 312 refused tries are 0.

## Before and after, per setting

MCP server, three tries on each of the 33 tasks. "Before" is the re-scored
baseline above. Held-out tasks are counted in "all".

| Setting | pass^3 before (all / visible / held out) | pass^3 after (all / visible / held out) | Mean cost a try before, USD | Mean cost a try after, USD | Tool errors a try before / after |
|---|---|---|---|---|---|
| Haiku 5.5, low | 13 / 9 / 4 | 33 / 22 / 11 | 0.0059 | 0.0063 | 1.0 / 0.3 |
| Haiku 5.5, medium | 12 / 8 / 4 | 33 / 22 / 11 | 0.0067 | 0.0072 | 1.0 / 0.2 |
| Haiku 5.5, high | 13 / 9 / 4 | 33 / 22 / 11 | 0.0077 | 0.0089 | 0.9 / 0.2 |
| Sonnet 5.5, low | 13 / 9 / 4 | 33 / 22 / 11 | 0.0787 | 0.0753 | 1.2 / 0.2 |
| Sonnet 5.5, medium | 13 / 9 / 4 | 33 / 22 / 11 | 0.0886 | 0.0825 | 1.1 / 0.1 |
| Sonnet 5.5, high | 13 / 9 / 4 | 32 / 21 / 11 | 0.1104 | 0.1129 | 0.7 / 0.1 |
| Sonnet 5.5, extra-high | 12 / 9 / 3 | 33 / 22 / 11 | 0.1633 | 0.1646 | 0.9 / 0.1 |
| Opus 5.5, high (ceiling) | 13 / 9 / 4 | 33 / 22 / 11 | 0.2472 | 0.2524 | 0.8 / 0.0 |

By kind, pass^3: every setting scores 6 of 6 builds, 7 of 7 repairs and 6 of
6 writes (before: 0 of each). Questions: Sonnet 5.5 at high scores 7 of 8
(the endurance question), the others 8 of 8 (under the 2 % band Haiku at
all three efforts scored 7 of 8 as well). Comparisons: 6 of 6 everywhere.
Solved once: 33 of 33 everywhere (under the 2 % band Haiku 5.5 at medium and
at high had 32 of 33).

Command line, once on each task (the baseline's figures are for the same two
settings):

| Setting | Solved before (all / visible / held out) | Solved after (all / visible / held out) | Mean cost a try before, USD | Mean cost a try after, USD |
|---|---|---|---|---|
| Haiku 5.5, low | 2 / 0 / 2 | 5 / 4 / 1 | 0.0034 | 0.0039 |
| Opus 5.5, high | 13 / 9 / 4 | 31 / 20 / 11 | 0.1836 | 0.3054 |

The ceiling's command-line tries cost more: they now save and check the model
they built (17.1 tool calls a try, from 12.2). Haiku's row measures the
bench's fence, before and after; with the fence named in the prompt it
solved 31 of 33 ("After the check" below).

## The settings table (from the command's report)

From the report of the re-scored copy (`after-safety-rescored/report.md`;
the run's own `report.md` has the 2 % band). Mean tokens in / out / cache
read / cache write are per try. Costs are at API list price.

| Setting | Solved once (all / visible / held out) | pass^3 (all / visible / held out) | Mean cost a try, USD | Total, USD | Per solve, USD | Mean tokens in / out / cache read / cache write | Tool calls | Tool errors | Time, s |
|---|---|---|---|---|---|---|---|---|---|
| Haiku 5.5, low | 33/33 / 22/22 / 11/11 | 33/33 / 22/22 / 11/11 | 0.0063 | 0.63 | 0.0063 | 14 / 2,991 / 120,462 / 18,202 | 10.3 | 0.3 | 28 |
| Haiku 5.5, medium | 33/33 / 22/22 / 11/11 | 33/33 / 22/22 / 11/11 | 0.0072 | 0.72 | 0.0072 | 15 / 3,544 / 133,680 / 20,646 | 11.1 | 0.2 | 32 |
| Haiku 5.5, high | 33/33 / 22/22 / 11/11 | 33/33 / 22/22 / 11/11 | 0.0089 | 0.88 | 0.0089 | 16 / 4,895 / 164,565 / 24,043 | 12.4 | 0.2 | 39 |
| Sonnet 5.5, low | 33/33 / 22/22 / 11/11 | 33/33 / 22/22 / 11/11 | 0.0753 | 7.46 | 0.0753 | 11 / 1,369 / 72,773 / 13,584 | 6.5 | 0.2 | 33 |
| Sonnet 5.5, medium | 33/33 / 22/22 / 11/11 | 33/33 / 22/22 / 11/11 | 0.0825 | 8.17 | 0.0825 | 11 / 1,462 / 76,055 / 15,060 | 6.9 | 0.1 | 35 |
| Sonnet 5.5, high | 33/33 / 22/22 / 11/11 | 32/33 / 21/22 / 11/11 | 0.1129 | 11.18 | 0.1141 | 13 / 2,017 / 102,257 / 20,624 | 9.0 | 0.1 | 40 |
| Sonnet 5.5, extra-high | 33/33 / 22/22 / 11/11 | 33/33 / 22/22 / 11/11 | 0.1646 | 16.29 | 0.1646 | 15 / 3,577 / 155,153 / 28,317 | 12.2 | 0.1 | 57 |
| Opus 5.5, high | 33/33 / 22/22 / 11/11 | 33/33 / 22/22 / 11/11 | 0.2524 | 24.99 | 0.2524 | 16 / 2,948 / 140,715 / 20,654 | 10.6 | 0.0 | 50 |

Command line (stage C: Haiku 5.5 at low is the cheapest setting as good as
the ceiling in MCP mode, so it and the ceiling were run once on each task):

| Setting | Solved once (all / visible / held out) | Mean cost a try, USD | Total, USD | Per solve, USD | Mean tokens in / out / cache read / cache write | Tool calls | Tool errors | Time, s |
|---|---|---|---|---|---|---|---|---|
| Haiku 5.5, low | 5/33 / 4/22 / 1/11 | 0.0039 | 0.13 | 0.0260 | 14 / 2,435 / 90,044 / 9,074 | 6.5 | 1.9 | 25 |
| Opus 5.5, high | 31/33 / 20/22 / 11/11 | 0.3054 | 10.08 | 0.3251 | 32 / 4,699 / 254,390 / 20,055 | 17.1 | 4.1 | 106 |

## What it cost

- **The whole comparison: 81.43 USD at API list price**, on the plan (nothing
  was billed to an API account): 80.52 USD for the 858 scored tries (70.31
  USD for the 792 MCP tries, 10.21 USD for the 66 command-line ones) plus 0.91
  USD for the 4 tries the plan limit stopped. The command-line tool's own
  `total_cost_usd` adds up to the same 81.43 USD (price basis: list). The
  baseline was 77.66 USD. The trial's 55 tries are inside these figures.
  The try after the check (Haiku low on the command line, below) cost 0.33
  USD more, so 81.76 USD in all.
- **Tokens:** input 12,694, output 2,507,575, cache read 107,822,818, cache
  write 16,980,729.
- **Time:** 9.9 hours of tries; 4.9 hours from the first try's start to the
  last one's end, with the plan's limit reached once (the tries it cut were
  run again after the reset at 13:20 UTC).
- **What one try costs, per setting** (mean at list price, MCP, 33 tasks).
  An edit task (build, repair, write) costs two to three times a question or
  a comparison.

| Setting | All tasks | Build, repair, write | Question, compare | Baseline, all tasks |
|---|---|---|---|---|
| Haiku 5.5, low | 0.0063 | 0.0085 | 0.0034 | 0.0059 |
| Haiku 5.5, medium | 0.0072 | 0.0099 | 0.0037 | 0.0067 |
| Haiku 5.5, high | 0.0089 | 0.0119 | 0.0048 | 0.0077 |
| Sonnet 5.5, low | 0.0753 | 0.1050 | 0.0350 | 0.0787 |
| Sonnet 5.5, medium | 0.0825 | 0.1129 | 0.0412 | 0.0886 |
| Sonnet 5.5, high | 0.1129 | 0.1470 | 0.0667 | 0.1104 |
| Sonnet 5.5, extra-high | 0.1646 | 0.1987 | 0.1183 | 0.1633 |
| Opus 5.5, high | 0.2524 | 0.3122 | 0.1713 | 0.2472 |

The cost of a try barely moved (the baseline's edit tries had already spent
their calls before the refused save), but a try now ends in a solve, so the
cost per solve fell from 0.0149 to 0.0063 USD (Haiku low) and from 0.6275 to
0.2524 USD (Opus). More effort again cost more and bought nothing: Sonnet 5.5
at extra-high costs twice medium for the same 33 of 33.

## What agents still get wrong

On the 22 visible tasks only: 528 MCP tries and 44 command-line ones. Ranked
by the number of MCP tries each mistake touched. There were 62 tool errors in
59 of the 528 MCP tries (the baseline: 313 failed tries and many more
errors). The 5 tries that failed under the 2 % band are all on the endurance
question (item 8), and re-scored only one of them fails; every other
mistake below was recovered in the same try unless the table says it was not.
Mistakes in fewer than three tries are left out: a relative project path
(`car.json`), whose error is the bare word `car.json`, in two tries (Haiku
low and medium, `repair-case-override`), and one `model_edit` call whose JSON
did not parse (Sonnet medium).

| # | Mistake | Tries (calls) | Recovered | Setting |
|---|---|---|---|---|
| 1 | `add` with a `system` that does not exist | 28 (28) | 28 | Haiku 25 (low 9, medium 8, high 8), Sonnet extra-high 2, high 1 |
| 2 | A part read or edited by a name that is not a part | 10 (10) | 5 | Haiku 5 (medium 2, high 2, low 1), Sonnet 5 (extra-high 2, high 2, medium 1); none at Sonnet low or Opus |
| 3 | A tool called by the wrong name | 5 (6) | 5 | Haiku 3 (medium 2, high 1), Sonnet low 3 |
| 4 | A parameter named by a guess | 5 (5) | 5 | Sonnet low 2, Opus 2, Sonnet high 1 |
| 5 | A part type guessed | 4 (4) | 4 | Sonnet low 3, medium 1 (all on `build-add-brakes`) |
| 6 | `compare_runs` on runs of two files | 3 (3) | 0 through that tool | Haiku high, Sonnet medium, Sonnet extra-high, 1 each |
| 7 | A port guessed | 3 (3) | 3 | Sonnet low 2, medium 1 |
| 8 | The endurance energy to two decimals, failed by the 2 % band; one wrong energy | 5 failed tries; 1 fails re-scored | – | Haiku low, medium, high 1 each, Sonnet high 2 (one of them the wrong energy) |

Against the baseline's list: the missing `op` key (40 tries) is gone (0); the
refused save (312) is gone; `compare_runs` on two files went from 19 tries to
3; a tool called by the wrong name from 17 tries (22 calls) to 5 (6); a part
read by name from 16 tries (6 recovered of 17 calls) to 10 (5 recovered of
10); a parameter by a guess from 8 tries to 5. Item 1 is new, and the
safety part brought it: its schema for the `add` operation (`b750c16`)
added a `system` key and said nothing of what it takes. The baseline's
schema had no such key (its operations were bare objects): Haiku sent 857
`add` operations in the baseline, 456 of them in dry runs that reached the
checks, and none had a `system`. After, 68 of its 840 had one, and every
`add` with a `system` at any setting (71) failed. (Counts by
`/home/eyada/scratch/lane-ai/check-rerun/addbase.py` and `sysbase.py`.)

**1. `add` with a `system` that does not exist (28 tries, all recovered).**
The `add` operation takes an optional `system`, which had to be a system's
id, and the schema did not say so. Haiku fills it with a name (the car's,
which is the main system's name), and the reply listed nothing to choose
from. The agent drops the key and the call goes through, so it costs one
call a try. Fixed after the run (commit `5bde110`): the schema says the key
is the system's id or name and to leave it out for the main system, a name
is taken, and an unknown one lists the systems.
- `build-add-brakes.mcp.haiku-high.1`: `model_edit` with `"op": "add",
  "type": "mech.brake", "label": "Brake FL", "system": "Battery Electric Car"`
  returns "Operation 1 (add): No system 'Battery Electric Car'. Nothing was
  changed."

**2. A part read or edited by a name that is not a part (10 tries; 5
recovered).** The error now lists the parts (closest first) and says whether
the name is a case or a part type, which the baseline's error did not. What
is left is mostly a part the agent is about to add (`Brake`), so the list
cannot help, and a resource address given as a part.
- `build-add-brakes.mcp.haiku-medium.2`: `element_read` with element `Brake`
  returns "No part 'Brake' in this project. Name a part by its id or exact
  label; parts (closest first): el-battery ("HV Battery Pack",
  battery.generic), el-driver ("Driver", driver.driver) ..."
- `build-city-ev.mcp.haiku-high.2`: `element_read` with element
  `lightsim://components`.

**3. A tool called by the wrong name (5 tries, 6 calls).** Down from 22 calls
in 17 tries. Two kinds: a bare name (`element_read`, twice in one Sonnet low
try) and the resource tools of Claude Code, which have no prefix, called with
the server's prefix (`mcp__lightsim__ReadMcpResourceTool`: Haiku 3, Sonnet
low 1).
- `build-fs-powertrain.mcp.sonnet-low.3`: "Error: No such tool available:
  element_read".
- `build-fs-powertrain.mcp.sonnet-low.1`: "Error: No such tool available:
  mcp__lightsim__ReadMcpResourceTool".

**4. A parameter named by a guess (5 tries, recovered).** The reply lists the
real keys. `repair-wont-start.mcp.opus-high.1`: "A HV Battery Pack has no
parameter 'soc_min_pct' (its parameters: pack_model, capacity_kWh,
capacity_Ah ..." and `repair-case-override.mcp.sonnet-low.3`: `power_kw` for
`power_kW`.

**5. A part type guessed (4 tries, recovered).** `build-add-brakes.mcp.sonnet-low.2`
adds `brake.brake` and `build-add-brakes.mcp.sonnet-low.3` adds `brake`: "No
library part 'brake' (lightsim://components lists them). Nothing was
changed."

**6. `compare_runs` on runs of two files (3 tries; not recovered through that
tool).** The message now says the tool compares two runs of one project; the
agents compared the two summaries by hand and solved the task anyway.
`compare-bev-gearing.mcp.haiku-high.2`: `compare_runs` with project
`sedan-a.json` and the run of the other file returns "No run
'ai-20261106-092151-84cd39' of this project (recent runs:
ai-20261106-092151-b3247d). compare_runs compares two runs of this one
project; it cannot compare runs of two different project files."

**7. A port guessed (3 tries, recovered).** `build-fs-powertrain.mcp.sonnet-low.2`:
"Operation 14 (connect): 'Driver' has no port 'sig_traction' (its ports:
sig_target_in, sig_speed_in, sig_traction_cmd, ...). Nothing was changed."

**8. The endurance energy to two decimals (4 tries, a band too narrow) and
one wrong energy (1 try), all on `question-fs-endurance`; not a tool
fault.** It is the only task any MCP try failed. Four tries gave 0.22 kWh,
the run's 0.2246 to two decimals: Haiku low (try 2), medium (try 1), high
(try 1) and Sonnet high (try 3). That is 2.06 % off the key 0.22462, outside
the 2 % band the task had, while 0.23 for the other reading's 0.2305 passed.
The brief names no precision, so these answers are right, not the agents'
mistake: the band is now 2.5 % and they are solved in the re-scored
records. Sonnet high's try 2 gave 0.234 with 20.56 km, which fits no
reading, and is still failed. The band was not changed in the middle of the
comparison, only after it.
- `question-fs-endurance.mcp.haiku-low.2`: `ANSWER: 20.0; 0.22`.

**Command-line mode** (44 visible tries):
- **The bench's own fence.** The runner allows the agent only the `lightsim`
  command, and any command with another command in it (`echo`, `head`, a
  `for` loop, `python3`, `grep`, `cd`) is refused. That hit 42 of the 44
  tries, 61 times (Haiku low 31 in 21 tries, Opus 30 in 21 tries), and the
  runner's guard refused a path outside the folder or the `ai` command in 14
  more calls (Opus 11, Haiku 3). Both are the bench's, not LightSim's, and
  are counted apart. **Every one of Haiku's 29 command-line tries that
  failed under the 2 % band (19 visible) had a refused command**, and all but
  one stopped there: they answered "unavailable" or "unresolved", or asked
  for the command to be allowed. `compare-fs-aero.cli.haiku-low.1` ends
  `ANSWER: unavailable; not determined (autocross runs were not
  permitted)`. The exception is `question-fs-endurance.cli.haiku-low.1`,
  which went on after its refusal and answered `20.56; 0.22`: failed for the
  rounding (item 8), solved re-scored. Opus works around the fence. Haiku's
  score in this mode is a measure of the fence, and is kept out of the
  answers. The prompt did not say what the fence refuses; it does now
  (commit `dc8231f`, below).
- **LightSim's own: `lightsim edit` does not say which keys an operation
  takes.** Its help gives one example. `connect` takes `from` and `to`,
  `add_port` takes `element`, `name` and `direction`, and none of it is in the
  command's help. Opus found the keys by sending each operation with only
  its name and reading the error: 13 of the 26 tries that used `lightsim
  edit` did (11 Opus, 2 Haiku), and 3 sent `{"op":"help"}`. For example
  `build-add-brakes.cli.opus-high.1` sends `--op '{"op":"connect"}'` and gets
  "lightsim: Operation 2 (connect): Name a port as 'element:port', not ''."
  (exit 3, nothing changed). An agent on the MCP server finds the list in the
  tool's schema. This is the one LightSim fault in command-line mode that the
  run shows: about one extra call per operation.
- **The answers Opus got wrong (2 tries, its own mistakes).**
  `build-sedan-abc.cli.opus-high.1` set the wheel radius to 0.34434 m, the
  brief says 0.334. `question-fs-endurance.cli.opus-high.1` answered
  `20.56; 0.231`, an energy 2.8 % off the key with the distance of the other
  reading.
- **A time series.** `question-peak-current.cli.opus-high.1` took 26 tool
  calls (19 of them runs) and 0.50 USD to find a peak current and a pack
  voltage, and answered `177.9; 353.4` for the keys 177.69 and 353.47.

### The mistakes by setting, in short

| Setting | Most typical mistakes |
|---|---|
| Haiku 5.5 (all efforts) | An unknown `system` on `add` (25 of 28 tries), a part read by a name that is not one (5 tries), the endurance energy to two decimals (failed by the old band). In command-line mode it mostly stopped at the first refused command, which the prompt did not warn of; with the fence named it solved 31 of 33. By far the cheapest. |
| Sonnet 5.5, low and medium | Guessed part types and ports and bare tool names, mostly at low. Low is 33 of 33 at 0.0753 USD a try. |
| Sonnet 5.5, high and extra-high | One or two unknown systems and parts read by name each; at high the endurance energy to two decimals once and a wrong one (0.234) once. Costs 1.4 to 2 times medium for the same score. |
| Opus 5.5, high | A parameter named by a guess (2 tries) and nothing else; 0.25 USD a try. In command-line mode, the fence (30 refusals) and edit keys learned by trying. |

## After the check: the command line with the fence named

- **Question:** once the prompt says what the fence refuses (commit
  `dc8231f`), is Haiku 5.5 at low's command-line score still decided by the
  fence? The run's records cannot say: 32 of its 33 tries there met a
  refusal.
- **Run:** Haiku 5.5 at low, command-line mode, once on each of the 33
  tasks, through the runner's `run_try` on `dc8231f`: the run's model,
  effort, tools and checker, with the new prompt line. Script
  `/home/eyada/scratch/lane-ai/fix-rerun/cli_prompt_try.py`, records
  `/home/eyada/scratch/judge-cache/ai/fix-rerun/cli-prompt`. Five tries lost
  the network or gave no start event; they were marked not run and run
  again, not scored. The script had no `__main__` guard, so its checker could
  not run a model with a Script block (the sandbox's worker imports the
  script again, and the reference's run failed as well). The tries were
  scored again from the files they left by a script that has the guard
  (`cli_prompt_rescore.py`, into `results-rescored.jsonl`): 7 go from failed
  to solved, none the other way.
- **Result: 31 of 33 solved (21 of 22 visible, 10 of 11 held out)**, against
  5 of 33 in the run. The fence refused one command in one try
  (`write-speed-zone`, solved), against 48 in 32 tries. 0.0090 USD a try and
  14.6 tool calls a try (the run: 0.0039 and 6.5): the tries do the work
  instead of stopping. That is the ceiling's command-line figure (Opus 5.5
  at high, 31 of 33 at 0.3054 USD a try) at 3 % of its price. So the fence
  no longer decides Haiku's command-line score. It is one try a task, not
  the comparison, so the tables above keep the run's figures.
- **What it still gets wrong** (the 22 visible tries):
  - The one visible miss, `question-peak-current`, wants a peak battery
    current and a lowest pack voltage. They are in the run's channels, not
    its KPIs, and `lightsim run` prints the KPIs only; `--out` writes a file
    that the fenced shell cannot read. Haiku said so and answered
    `unavailable` rather than guess. (Opus found them in the run with 19
    runs of the case.) A LightSim gap: item 5 below.
  - The runner's folder guard refused a path outside the folder or the `ai`
    command 13 times in 10 tries, all recovered.
  - `lightsim edit` keys learned by trying, as Opus did in the run (item 3
    below): an empty `connect` in 7 tries, an operation that does not exist
    (`help`, `list`, `?`) in 5.
  - A part type guessed (`battery`) in 5 tries, all recovered.
- **Cost:** 0.33 USD at API list price (0.30 on the 33 scored tries, 0.03
  on the 5 not run); tokens in 1,060, out 167,300, cache read 10,077,496,
  cache write 716,365. Billed to the plan, only `claude-haiku-5-5`
  answered, Claude Code 2.1.316, no API key.

## What this means for the later parts of step 6

- The baseline's large item is closed: saves go through at every setting in
  MCP mode and, for the ceiling, in command-line mode. The settings are no
  longer told apart by the tools, only by price. The target the baseline
  set, Sonnet 5.5 at medium, passes every task on every try, and so does
  Haiku 5.5 at low at a thirteenth of the price (with the endurance band
  that takes two decimals; the 2 % band failed it once).
- Tool defects the run shows, written down for the later parts (no tool was
  changed for the run; item 1 was fixed after it):
  1. `add` with an unknown `system` (28 tries): the key the safety part put
     in the schema said nothing of what it takes. Fixed in `5bde110`: it
     says so, a system's name is taken, and the error lists the systems.
  2. `element_read` and `model_edit` on a name that is not a part: say so
     when it is a part the task is about to add (10 tries, half not
     recovered).
  3. `lightsim edit` help: list the operations and the keys each takes, as
     the MCP schema does (13 tries probed for them).
  4. A relative project path in an MCP tool: the error is the bare path
     (`car.json`), two tries.
  5. `lightsim run` prints a run's KPIs only: a channel's peak or lowest
     value needs the result file, which an agent with only `lightsim`
     cannot read (`question-peak-current`, "After the check"). A way to
     print a channel's minimum, maximum and mean would close it.
- This bench no longer separates the settings: every one solves almost
  everything. A comparison that tells them apart needs tasks that Haiku 5.5 at
  low does not already solve. The command-line prompt now says what the
  fence refuses (`dc8231f`), and with it Haiku 5.5 at low solved 31 of 33
  once: the next comparison runs with that prompt, so its Haiku row
  measures Haiku, not the fence.

# After the live part: the edit tasks again

Step 6, item 4 (an agent's change shows in the open window and is one undo
step, with a line in the project's history) changes how every save is
written: under a lock the app and an agent share, with a history line beside
the file, a reason the tool takes, and a re-apply when someone else saved in
between. This run asks whether agents still succeed with it.

- **Date:** 6 November 2026.
- **LightSim:** commit `6c6d27d` (branch `work/ai`): the history and the
  shared lock (`b777661`) and the window's side (`6c6d27d`), with the
  command-line prompt that names the fence (`dc8231f`). Also in
  `/home/eyada/scratch/lane-ai/live/bench.commit`.
- **Run:** the 19 build, repair and write tasks (6 of them held out), 3 tries
  each, in MCP and command-line mode, at the cheapest working setting
  above (Haiku 5.5, low) and at the owner's expectation (Sonnet 5.5,
  medium): 228 tries. The bench's own `Comparison` (its records, checker,
  not-run retries and report) with these specs, which its stages do not make
  (they give the command line one try a task). Script
  `/home/eyada/scratch/lane-ai/live/edit_rerun.py`. Claude Code 2.1.316,
  signed in to the owner's plan, no API key, no budget limit.
- **Not run:** three command-line tries (1 at Haiku low, 2 at Sonnet medium)
  gave no result and were run again; they are not scored, and their cost is
  in the total.
- **One scoring fault, mine, and its tries scored again:** the script had no
  `__main__` guard (the same fault as "After the check"), so the checker
  could not run a model with a Script block: the sandbox's worker imports the
  script again. It hit one task, `repair-hybrid-wiring`, whose 12 tries were
  all scored failed, the reference's run failing too. The agents' own runs
  were not touched. Every try was scored again from the files it left by a
  script with the guard (`rescore.py`, output `rescore.out`), into
  `/home/eyada/scratch/judge-cache/ai/bench-runs/after-live-rescored`: the 12
  go from failed to solved, none the other way. The figures below are the
  re-scored ones.
- **Records:** `/home/eyada/scratch/judge-cache/ai/bench-runs/after-live`
  (`report.md`, `report.json`, `tries/`, each try's transcript and files under
  `runs/`). Models that answered: `claude-haiku-5-5`, `claude-sonnet-5-5`, and
  only these; every try billed to the plan.

## The answer: agents still succeed

| Setting, mode | pass^3 (all / visible / held out) | Solved on the first try | Solved in some try | Mean cost a try, USD | Total, USD |
|---|---|---|---|---|---|
| Haiku 5.5, low, MCP | 19/19 / 13/13 / 6/6 | 19/19 | 19/19 | 0.0086 | 0.49 |
| Sonnet 5.5, medium, MCP | 19/19 / 13/13 / 6/6 | 19/19 | 19/19 | 0.1136 | 6.48 |
| Haiku 5.5, low, command line | 18/19 / 12/13 / 6/6 | 19/19 | 19/19 | 0.0161 | 0.92 |
| Sonnet 5.5, medium, command line | 17/19 / 12/13 / 5/6 | 18/19 | 19/19 | 0.1490 | 8.49 |

(This table first had one column, "Solved once", with the first try's
figure: 18/19 for Sonnet 5.5, medium, on the command line. Every task was
solved in at least one of its three tries; `build-sedan-abc` failed its
first two and passed the third. The report script now names that column
"Solved on the first try".)

In MCP mode both settings pass every try of every edit task, as they did
after the safety part. On the command line, which had only one try a task
before, Haiku 5.5 at low passes 18 of 19 on all three tries and Sonnet 5.5
at medium 17 of 19. Haiku 5.5 at low stays the cheapest setting that
works, at 0.0086 USD a try in MCP mode, 13 times less than Sonnet 5.5 at
medium.

The four failed tries, none of them a save that did not go through:

- `build-sedan-abc.cli.sonnet-medium.1` and `.2` set the wheel radius to
  0.34435 m for a brief that gives a rolling radius of 13.15 in (0.334 m):
  they divided it by the Wheel's 0.97 rolling radius factor and said they
  could not confirm from the part descriptions what the field means. The
  same miss as Opus after the safety part. `lightsim parts propulsion.wheel`
  shows `radius_m [m] Wheel Radius` and, with `--json`, key, label, unit and
  type: not the description ("The tyre's rolling radius ...") that the MCP
  tools give. A LightSim gap: item 6 below.
- `write-limp-home.cli.haiku-low.2` labelled its Script block "Limit Home",
  where the brief asks for "Limp Home".
- One held-out try at Sonnet 5.5, medium, on the command line (held out:
  only its score is used).

## What the agents left in the history

Every try's project has its history (`<file>-backups/history.jsonl`), one
line per save. Counted over the files of the 57 scored tries a setting and
mode, and the not-run tries' (script
`/home/eyada/scratch/lane-ai/live/history_stats.py`, output
`history_stats.out`):

| Mode, setting | Agent saves | With a reason | Client recorded |
|---|---|---|---|
| MCP, Haiku 5.5 low | 62 | 62 | `claude-code` (all) |
| MCP, Sonnet 5.5 medium | 66 | 66 | `claude-code` (all) |
| Command line, Haiku 5.5 low | 327 | 133 | `command line` 308, set with `--client` 19 |
| Command line, Sonnet 5.5 medium | 234 | 42 | `command line` (all) |

Every MCP save has its reason (`model_edit` asks for one) and a diff an undo
can use. On the command line `--reason` may be left out, and most saves
leave it out; 100 and 43 of the saves wrote a new file (a copy to try
something on), which has no earlier version, so no diff.

## What it cost

**16.49 USD at API list price** for the 228 scored tries and the 3 not run
(0.11 USD of it on those); the command-line tool's own figure is 16.49 USD.
Haiku 5.5 at low 1.41 USD, Sonnet 5.5 at medium 14.97 USD. Tokens: input
6,558, output 1,031,376, cache read 64,485,348, cache write 5,582,873. Time:
5.0 h of tries, 1.0 h from the first try's start to the last one's end.

## Tool defect for the later parts

6. `lightsim parts <type>` lists a part's parameters with key, unit and
   label only. The description, which the MCP tools give and which says what
   a value means (the Wheel's `radius_m` is the rolling radius), is not
   printed, in text or in `--json`. Two Sonnet tries here and one Opus try
   after the safety part guessed at it on the command line and set the wrong
   radius.
