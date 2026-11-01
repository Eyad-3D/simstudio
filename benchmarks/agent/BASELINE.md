# The baseline: what agents achieve with today's tools

Step 6, item 0. The question is the owner's: which is the cheapest model and
effort level that drives LightSim successfully? This is the answer on
LightSim as it is, before any step 6 change to the tools.

- **Date:** 31 October to 1 November 2026.
- **LightSim:** commit `b228609` (branch `work/ai`). No tool was changed for
  the run.
- **Run:** `python -m benchmarks.agent.compare --name baseline --jobs 8`,
  stages A, B and C, in the way `README.md` describes. Claude Code 2.1.316,
  signed in to the owner's plan, no API key. It ran in two sittings: 824 tries
  from 22:03 to 23:07 UTC on 31 October (then the plan's five-hour limit
  stopped it, and WSL shut down), and the last 34 tries from 11:12 to 11:17 UTC
  on 1 November, after the same command resumed it. The 14 tries the limit
  had stopped were run again, not scored as failures.
- **Records:** `/home/eyada/scratch/judge-cache/ai/bench-runs/baseline`
  (`report.md`, `report.json`, `tries/`, and each try's transcript under
  `runs/`). They stay outside the repository; the transcript lines quoted
  below are from there, named by try.
- **Models that answered:** `claude-haiku-5-5`, `claude-sonnet-5-5`,
  `claude-opus-5-5`, and only these. No setting was refused or changed, and
  every try was billed to the plan.

## The two answers

**1. The cheapest working setting: none. No setting drives LightSim
successfully today.** Even the ceiling (Opus 5.5, high) passes all three
tries on only 12 of the 33 tasks, and on 4 of the 11 held-out ones; hands-off
work needs 90 % of both. The cheapest setting that is as good as the ceiling
is **Haiku 5.5 at low effort: 13 of 33 tasks, 0.0059 USD a try**, against
12 of 33 and 0.2472 USD for the ceiling (42 times as much). Sonnet 5.5 at
medium effort, the owner's expectation, was tested and is as good as the
ceiling too (13 of 33), at 0.0886 USD a try, 15 times Haiku low's price. The
settings cannot be told apart on today's tools. The tasks they can solve are
the same 13 questions and comparisons, and the gaps of one task between them
are the `question-fs-endurance` wording described below, not skill.

**2. Is any setting good enough for hands-off work? No.** The best, Haiku 5.5
at low effort, has 13 of 33 tasks (39 %) and 4 of 11 held out (36 %).
All 19 tasks that build, repair or write a model score 0 at every setting,
the ceiling's included, and the cause is not the agents (next section).
That gap is what the later changes of step 6 must close, with Sonnet 5.5 at
medium as the target.

## Why: the tools ask, and a headless agent is refused

Today's MCP server asks the user before it saves an edit (`model_edit` with
`dry_run` false) and before it runs a project that has Script blocks
(`run_case`). A headless agent has nobody to ask: `claude -p` declines at
once, and the tool answers "The user did not confirm, so nothing was done."

This explains **480 of the 494 failed MCP tries (97 %)**:

- all 456 failed tries on build, repair and write tasks (8 settings, 3 tries,
  19 tasks): every one ended at a refused save, and no try ever saved a file;
- all 24 tries on `question-hybrid-load`, whose P2 hybrid has Script blocks, so
  `run_case` asked and was refused.

The other 14 failures are wrong answers: 13 on `question-fs-endurance` and
one on `compare-fs-gearing` (below).

The agents behaved well. In all 480 tries the agent ended by saying that the
save or the run had been declined and that nothing was done (read by its
words, not by a judge). In 447 of the 456 edit tries it
first checked its work with a dry run, which is allowed. Typical lines:

- `repair-hatchback.mcp.sonnet-medium.1`: `model_edit` with `"dry_run": false`
  returns "The user did not confirm, so nothing was done." The agent writes
  "I haven't saved anything: both attempts to write to car.json were declined
  at the confirmation prompt, so the file is unchanged."
- `build-add-brakes.mcp.opus-high.1`: "I prepared the brake changes and tested
  them in a dry run, but when I asked to save, the confirmation was declined.
  The file is still exactly as it was, and I won't try again without your
  go-ahead."
- `question-hybrid-load.mcp.sonnet-medium.1`: "LightSim asked for
  confirmation before running "Mixed Cycle", because the model contains a
  Script block (the Hybrid Control Unit). The request was declined, so nothing
  ran. I haven't made up any numbers."

**What would the same tries have scored if the save had been allowed?** An
estimate, not a measurement. For each of the 456 refused edit tries, the
checker was run on the starting files plus the last edit the agent had
prepared (the call that was refused, or its last clean dry run). Unedited
starting files fail the checker on all 19 tasks (the control), so the passes
are real. **453 of the 456 pass.** The three that fail are one mistake, in
`write-regen-gate` (Sonnet 5.5 at low effort twice, at medium once): the
Script compares the Vehicle's speed with `10.0 / 3.6`, taking the signal for
m/s, but it is in km/h, so the gate opens at 2.8 km/h and consumption comes out
2.2 % low (10.605 against 10.838 kWh/100 km). So, of the 494 failed tries, 453
(92 %) were lost only to the refusal.

If saves had been allowed, the estimated count of tasks with three passed
tries would have been:

| Setting | Tasks (of 33) | Held out (of 11) | Also missing, besides hybrid-load |
|---|---|---|---|
| Haiku 5.5, low | 32 | 10 | |
| Haiku 5.5, medium | 31 | 10 | fs-endurance |
| Haiku 5.5, high | 31 | 10 | fs-endurance |
| Sonnet 5.5, low | 31 | 9 | regen-gate |
| Sonnet 5.5, medium | 31 | 9 | regen-gate |
| Sonnet 5.5, high | 31 | 10 | fs-endurance |
| Sonnet 5.5, extra-high | 30 | 9 | fs-endurance, fs-gearing |
| Opus 5.5, high | 31 | 10 | fs-endurance |

(`question-hybrid-load` is counted as missing everywhere: its run was refused,
so nothing says what the agents would have answered. In the command-line
tries it runs, and Opus solved it.) Read this table as "the tools, not the
models, stood in the way", and not as a result: the agents were never seen
to verify a saved model, a try needs three passes, and two held-out misses
would put Sonnet at medium under the 90 % line again.

## The settings

MCP server, stages A and B: three tries on each of the 33 tasks, 99 tries a
setting. "Solved once" is the first try; pass^3 needs all three. Costs are at
API list price (the runner's figure equals the tool's own `total_cost_usd`
to the cent).

| Setting | Solved once (all / visible / held out) | pass^3 (all / visible / held out) | Mean cost a try, USD | Total, USD | Per solve, USD | Tool calls | Tool errors | Time, s |
|---|---|---|---|---|---|---|---|---|
| Haiku 5.5, low | 13 / 9 / 4 | 13 / 9 / 4 | 0.0059 | 0.58 | 0.0149 | 9.5 | 1.0 | 24 |
| Haiku 5.5, medium | 12 / 8 / 4 | 12 / 8 / 4 | 0.0067 | 0.66 | 0.0183 | 10.2 | 1.0 | 27 |
| Haiku 5.5, high | 12 / 8 / 4 | 12 / 8 / 4 | 0.0077 | 0.77 | 0.0213 | 11.2 | 0.9 | 33 |
| Sonnet 5.5, low | 13 / 9 / 4 | 13 / 9 / 4 | 0.0787 | 7.79 | 0.1998 | 6.4 | 1.2 | 21 |
| Sonnet 5.5, medium | 13 / 9 / 4 | 13 / 9 / 4 | 0.0886 | 8.77 | 0.2248 | 7.0 | 1.1 | 24 |
| Sonnet 5.5, high | 12 / 8 / 4 | 12 / 8 / 4 | 0.1104 | 10.93 | 0.2953 | 8.5 | 0.7 | 25 |
| Sonnet 5.5, extra-high | 12 / 9 / 3 | 11 / 8 / 3 | 0.1633 | 16.16 | 0.4490 | 11.9 | 0.9 | 38 |
| Opus 5.5, high (ceiling) | 12 / 8 / 4 | 12 / 8 / 4 | 0.2472 | 24.47 | 0.6798 | 10.3 | 0.8 | 43 |

By kind, pass^3 (8 question tasks, 6 compare tasks; every setting scores 0 of 6
builds, 0 of 7 repairs and 0 of 6 writes):

| Setting | question | compare |
|---|---|---|
| Haiku 5.5, low | 7 | 6 |
| Haiku 5.5, medium | 6 | 6 |
| Haiku 5.5, high | 6 | 6 |
| Sonnet 5.5, low | 7 | 6 |
| Sonnet 5.5, medium | 7 | 6 |
| Sonnet 5.5, high | 6 | 6 |
| Sonnet 5.5, extra-high | 6 | 5 |
| Opus 5.5, high | 6 | 6 |

Command line, stage C (once on each task, for the cheapest setting as good as
the ceiling and for the ceiling; no pass^3):

| Setting | Solved once (all / visible / held out) | Mean cost a try, USD | Total, USD | Tool calls | Tool errors | Time, s |
|---|---|---|---|---|---|---|
| Haiku 5.5, low | 2 / 0 / 2 | 0.0034 | 0.11 | 5.5 | 1.5 | 19 |
| Opus 5.5, high | 12 / 8 / 4 | 0.1836 | 6.06 | 12.2 | 1.8 | 61 |

The `lightsim` command has no command that saves an edited model, so the 19
build, repair and write tasks cannot pass in this mode whatever the agent
does: 38 of the 52 command-line failures. The other 14 are wrong answers
(below). Opus solved all 12 questions and comparisons except two; Haiku at
low effort solved two.

## What it cost

- **The whole comparison: 77.66 USD at API list price**, on the plan (nothing
  was billed to an API account). That is 858 scored tries: 70.13 USD for the
  792 MCP tries, 6.17 USD for the 66 command-line tries, plus 1.36 USD for
  the 14 tries a plan limit stopped and that were run again.
- **Tokens:** input 11,382, output 2,557,894, cache read 79,536,213, cache
  write 16,501,410.
- **Time:** 7.3 hours of tries; the first try started at 22:03 on 31 October
  and the last ended at 11:17 on 1 November (13.2 hours, 12 of them waiting
  for the plan's limit to reset).
- **What one try costs, per setting** (mean at list price, MCP). A question or
  a comparison costs a third to two thirds of an edit try, which is longer. Every
  edit try stopped at the refused save: once saves work, the agent will verify
  the saved model and a try will cost more.

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
times medium and scored two tasks fewer.

## What agents got wrong

Ranked by the number of MCP tries it touched (792 tries unless said), then by
what it cost. "Setting" is what differs by model or effort. The first item is
a design choice of the tools, not an agent mistake; the rest are the
mistakes. Most were recovered by the next call, but each cost a call, and in
a longer task a weak model that is not recovered stops there.

**1. The save and the Script run are refused (480 tries, 60 at every
setting).** The cause, the effect and the lines are in the section above.
Setting: Sonnet 5.5 at low and medium effort ask again with the same call
after the refusal in 17 and 12 of their 60 tries (Haiku 1 to 3, Sonnet high 2,
extra-high 4, Opus 1); each repeats a call that cannot succeed.
`repair-hatchback.mcp.sonnet-medium.1` sends the same `dry_run: false` call
twice and is declined twice.

**2. An operation has no `op` key (64 tries; 62 recovered on the next
call).** `model_edit` takes a list of free-form objects. Its description names
the operations (set, add, connect...) but not that the key is `op`, and the
schema is `{"type": "object"}`, so agents put the name in as a key or as
`type`. Setting: mostly Haiku (low 14, medium 19, high 14), then Sonnet low 9,
medium 4, high 4; none for extra-high or Opus.
- `repair-hatchback.mcp.haiku-low.3` sends `{"set": "Vehicle", "param":
  "mass_kg", "value": 1927}` and gets "Operation 1 (None): Unknown 'op': use
  set, add, remove, connect, disconnect, add_port, add_case or set_case.
  Nothing was changed."
- `build-add-brakes.mcp.haiku-medium.1` sends `{"add": true, "type":
  "mech.brake", "label": "Brake FL"}`; `build-add-brakes.mcp.haiku-low.3`
  sends `{"type": "mech.brake", "label": "Brake FL"}`; same reply.

**3. `compare_runs` on runs of two files (26 tries, none recovered through
that tool).** All 26 are on compare tasks, where the agent runs the same case
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
- Setting: Sonnet medium 9, extra-high 6, Opus 4, low 3, Haiku high 2 and
  medium 2.

**4. A parameter is named by its label or a guess, not its key (45 calls in
34 tries; 38 recovered).** Setting: every setting; by call, Haiku low 13, Sonnet
medium 7, Sonnet low 9, Sonnet high 5, Haiku medium 4, Opus 4, Sonnet
extra-high 3. The reply lists the real keys, but a weak model does not always pick
the right one.
- `repair-fs-rules.mcp.haiku-low.2`: `"param": "Output Power Limit (0 =
  none)"` returns "A HV Battery Pack has no parameter 'Output Power Limit (0 =
  none)' (its parameters: pack_model, capacity_kWh, ..., output_power_limit_kW,
  ...)". The label comes from the part's own display text.
- `repair-motor-limits.mcp.haiku-low.1`: `"param": "Maximum Speed"` for the
  E-Motor (`max_speed_rpm`), `"Transmission Ratio"` for the Final Drive
  (`ratio`), `"Wheel Radius"` for the Wheel (`radius_m`).
- `repair-wont-start.mcp.opus-high.2` and `.3`: `"Minimum SOC"` for
  `min_soc_pct`.

**5. A tool is called without its server prefix (35 calls in 27 tries; 33
recovered).** Claude Code names the tools `mcp__lightsim__run_case`; the
agent called `run_case`, `results_query`, `lightsim_overview` or
`element_read`, and the harness replies "No such tool available". The skills and LightSim's own messages name tools without it ("start one
with run_case"). Setting: Sonnet
low 11, extra-high 11, medium 6, Opus 4, Haiku 2, Sonnet high 1.
- `compare-battery-range.mcp.sonnet-low.3`: "Error: No such tool available:
  run_case".
- `build-hill-van.mcp.opus-high.1`: "Error: No such tool available:
  lightsim_overview".

**6. A part is read by name instead of id (19 calls in 18 tries; only 6
recovered).** `element_read` takes an element id; agents give a label
(`Battery`, `Wheel`), a type (`signal.driving_task`) or a case id (`case-1`).
The reply is "No part 'Battery' in this project: lightsim_overview lists
them." and a weak model often gives up reading the part. Setting: Haiku 12 of
the 19 calls (low, medium, high 4 each), Sonnet medium 3, extra-high 2, high 1,
Opus 1.
- `build-city-ev.mcp.haiku-medium.1`: `element_read` with element `Battery`.

**7. A part type or a port is guessed (20 tries, all recovered).** Types:
`add` of `Brake`, `Climate Control` gets "No library part 'Brake'
(lightsim://components lists them)" (9 tries, all Sonnet low or medium: 6 and
3). Ports: `Ground` has `t1` to `t3` and the agent connected `t4` or "Terminal
4"; a mechanical node has `f1` to `f4` and the agent used `flange2` (11
tries: Sonnet low 4, medium 3, Haiku low 2, Opus 2).
- `build-add-climate.mcp.sonnet-low.1`: "No library part 'Climate Control'".
- `build-add-climate.mcp.haiku-low.2`: "Operation 6 (connect): 'Ground' has
  no port 't4' (its ports: t1, t2, t3). Nothing was changed."

**8. The wrong distance for "before the accumulator is empty"
(`question-fs-endurance`, 13 of 24 failed).** The key is the run's own
"Distance driven", 20.557 km: the run stops when the car can no longer drive
the lap. The more careful agents read "empty" as the moment the charge
reaches its minimum, 5 %, and answered 20.0 km. Setting (3 tries each):
solved 3 at Haiku low, Sonnet low and Sonnet medium; 1 at Sonnet high and
extra-high; 0 at Haiku medium, Haiku high and Opus. So more effort scored
worse. LightSim helps the confusion along: the run's status is "failed" and
marks the energy-per-lap, lap-time and consumption rows "not valid", and the
`lightsim-read-not-valid-flags` skill says never to report such a number on
its own, so the careful agents build their own figures from the channels.
- `question-fs-endurance.mcp.opus-high.1`: "Charge hits 5 % at t ≈ 1262.6 s,
  when the car's distance reads 20,023 m ... The summary's "Distance driven" of
  20.56 km counts that rolling." It ends `ANSWER: 20.0; 0.229` (key 20.557;
  0.22462).
- `question-fs-endurance.mcp.haiku-low.1`: ends `ANSWER: 20.56; 0.225`
  (solved).

This failure is the task's wording ("how far does the car get before the
accumulator is empty") as much as the agents. It changes the ranking of the
settings by one task (it is why Haiku low has 13 and the ceiling 12), and the
bench keeps the task as it is. The task wording should be made exact before
the next comparison.

**9. An agent overrides LightSim's own figure (`compare-fs-gearing`, 1 try).**
`compare-fs-gearing.mcp.sonnet-xhigh.1` checked the summary's time to 75 m
against the trace, found the solver stops one step after the trace crosses
75 m, and reported its own 3.99 s (key 3.767 s): "I found nothing in the
documentation that defines this key result, so I reported the time I could
verify from the trace." (The agent found no definition of that result row.)

**10. A unit slip in a Script (3 of 24 tries on `write-regen-gate`, found
only in the estimate above).** Sonnet 5.5 at low effort twice and at medium
once took the Vehicle's speed signal for m/s (`V_MIN = 10.0 / 3.6  # 10 km/h
in m/s`, `write-regen-gate.mcp.sonnet-low.1`); it is km/h. The other 21 tries got it right.

**11. Other single mistakes (under 5 tries each).** A results query before any
run (3 tries, `question-hybrid-load`, because the run was refused);
`results_query` with a bare word for the channel list, which is not JSON (2
tries, Haiku medium and high); `set_case` with nothing to change (3 tries:
Opus, Sonnet low, Sonnet medium); a `null` for a number (3 tries: Haiku high,
Opus twice).

**12. Command-line mode** (66 tries):
- **No way to save.** The `lightsim` command cannot save an edit, so all 38 edit
  tries failed; the agents say so. The edit tasks are not measurable in this
  mode today.
- **Cannot read a time series.** To get a peak current (`question-peak-current`,
  `question-peak-power`) `lightsim run` prints only the summary. Opus
  recovered by bisection: `question-peak-current.cli.opus-high.1`: "Can't read
  the result files with only `lightsim`, so I'll bisect" (27 tool calls, correct);
  `question-peak-power.cli.opus-high.1` took 31 calls and ended 2 % off (7065
  against 6923.5).
- **Shell commands are denied.** In 62 of the 66 tries Claude Code's own
  permission rules (the runner allows only the `lightsim` command) refused a
  command, 84 times in all: loops over commands, `; echo "exit $?"`, writing a
  file with a heredoc, `python3 -c`, `grep | awk`. Opus works around it. Haiku 5.5
  at low effort did not: in 12 of 12 failed answer tries it was denied once and
  then answered "unavailable", "unknown" or "unresolved", or gave one number
  where two were asked. `question-loaded-ev.cli.haiku-low.1`:
  "The permission system denied the one command that runs the WLTC Class 3b case,
  so I have no consumption or regen figures to report." I did not look into the
  exact rule that denies these; it is a property of headless Claude Code, and
  it would hit any hands-off agent that uses the shell.
- **Out-of-folder commands.** The runner's guard refused a path outside the
  folder, or the `ai` command, in 23 tries (Opus 20, Haiku 3); it is the
  bench's own fence, not a LightSim fault.

### The mistakes by setting, in short

| Setting | Most typical mistakes |
|---|---|
| Haiku 5.5 (all efforts) | Operation without `op` (47 of 64), part read by label (12 of 19 calls), a non-JSON channel list; in CLI mode it stops at the first denied command. Cheapest by far. |
| Sonnet 5.5, low and medium | Part types guessed (all 9), the refused save asked again (17 and 12 of 60), the `m/s` slip in a Script (3). Bare tool names at low (11). |
| Sonnet 5.5, high and extra-high | The fewest tool errors at high (0.7 a try); at extra-high the most bare tool names (11) and, with Opus, the careful reading that fails `question-fs-endurance`; one override of a LightSim figure. Costs 1.2 to 1.8 times medium for no gain. |
| Opus 5.5, high | Few tool errors, but the same careful reading of `question-fs-endurance` (0 of 3), a bare `lightsim_overview` (4 calls), compare_runs on two files (4). |

## What this means for step 6

- Remove the per-run approvals (the owner's decision), made safe by design:
  this alone moves the estimate from 13 tasks to about 31 for every setting.
  It is the first item, and nothing else on the list is as large.
- Then the small ones, in the order above: say in the `model_edit` description
  that each operation has an `op` key and give the schema its list of
  operations; have the errors name the right key, id or port (and say that
  runs of two projects cannot be compared); make the tool names the agent sees
  match the names in the docs.
- Fix the `question-fs-endurance` wording before the next comparison, and
  decide whether LightSim's own "Distance driven" of a run that stopped on a
  minimum charge is a figure an agent may report.
- Run the comparison again once the saves work: the cost per try and the order
  of the settings will change, because the agents will then verify what they
  saved.
