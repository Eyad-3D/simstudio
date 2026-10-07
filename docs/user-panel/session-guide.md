# Panel session guide: 45 minutes (draft)

> Draft for the owner (BIZ-35). One guide for every seat type; pick three
> tasks from the list for the seat.

## Before the session

- Consent form signed; recording choice known.
- The participant has installed the release themselves (installing is
  part of what we learn; note any trouble).
- Pick three tasks, at least one from their own work. Send nothing that
  tells them how to do it.

## The script

| Minutes | What |
|---|---|
| 0–5 | Thanks; what we will do; "we are testing LightSim, not you"; ask them to think aloud. Start recording if agreed. |
| 5–10 | Since last time: what did you use LightSim for? What stopped you? |
| 10–35 | Three tasks, about 8 minutes each. Read the task aloud, then stay quiet. Help only after they are stuck for 2 minutes, and note it. |
| 35–40 | SUS questionnaire (below). |
| 40–45 | "What one change would make you use it more?" Anything else? Thanks; next date. |

## Tasks per seat type

Formula Student:
- F1. Run a 75 m acceleration test on your car (or the FS example with
  your mass and motor) and read the time and peak accumulator power.
- F2. Import or type a lap speed trace and read the net energy (kWh) for
  one endurance lap.
- F3. Compare two accumulator sizes and say which you would choose.
- F4. Make a slide-ready chart of the run you trust most.

Industry:
- I1. Build a battery electric car from typical values and run WLTC.
- I2. Run a sweep of one parameter and read the trend.
- I3. Export results for your usual tool (CSV) and open them there.
- I4. Find out how far you can trust a figure (Known issues, Validation).

IT or CAE gatekeeper:
- G1. Install for all users on a managed PC without admin rights for the
  user.
- G2. Find what LightSim does on the network and where it stores files.
- G3. Find the licence terms, the third-party notices and the security
  policy, and say what is missing for approval (the
  [checklist](it-gatekeeper-checklist.md)).

## Timing sheet (one row per task)

| Seat | Release | Task | Finished? (yes / with help / no) | Time (min:s) | Stuck at | Quote |
|---|---|---|---|---|---|---|
| FS-1 | 0.3.0 | F1 | | | | |

## System Usability Scale (SUS)

Answer each from 1 (strongly disagree) to 5 (strongly agree):

1. I think that I would like to use this system frequently.
2. I found the system unnecessarily complex.
3. I thought the system was easy to use.
4. I think that I would need the support of a technical person to be able
   to use this system.
5. I found the various functions in this system were well integrated.
6. I thought there was too much inconsistency in this system.
7. I would imagine that most people would learn to use this system very
   quickly.
8. I found the system very cumbersome to use.
9. I felt very confident using the system.
10. I needed to learn a lot of things before I could get going with this
    system.

Score: for odd questions subtract 1 from the answer; for even questions
subtract the answer from 5; add the ten results and multiply by 2.5
(0 to 100). Around 68 is average.

## Release summary (one page)

```text
LightSim [version] — user panel summary, [date]
Sessions: [n] (FS [n], industry [n], IT [n])
Tasks finished without help: [n] of [n]
Median time per task: F1 [..], F2 [..], …
SUS: median [..] (last release [..])
Top 3 problems (roadmap id, seats that hit it):
1.
2.
3.
What worked well:
Gatekeeper: what still blocks approval:
Proposed changes to the next release's plan:
```
