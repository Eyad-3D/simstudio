---
name: lightsim-verify-a-model
description: Check a LightSim model before trusting or reporting its numbers - Data Checks, a smoke run, plausibility targets and the run's own verdict. Use after building or editing a model and before giving the user any result.
license: See README.md of the LightSim skill pack
metadata:
  version: "0.3.0"
  app: LightSim 0.3.0
---

# Verify a model

Do these in order. Stop and fix at the first step that fails.

1. **Data Checks** (`run_checks`). No errors. Read every warning and say
   why it is acceptable, or fix it.
2. **Smoke run** of the shortest drive-cycle case (`run_case`). The status
   must be *success*. A *warning* or *failed* run, or any summary row with
   a *not valid* note, is a finding to fix or report, not a result
   (see `lightsim-read-not-valid-flags`).
3. **Physics sanity** (`results_query`):
   - the vehicle speed follows the target (compare the Driver's target with
     Vehicle Speed);
   - the battery's SOC falls while driving and rises when braking;
   - the motor's speed stays below its maximum, its torque within its
     full-load curve;
   - the *Electrical energy balance error* row is close to 0 %.
4. **Targets**: compare with what is known about the vehicle class, and
   say how far off it is:
   - compact BEV on WLTC at the battery: about 13-16 kWh/100 km (the
     Battery Electric Car example: about 14);
   - compact hybrid on the EPA city cycle: about 2.8-3.5 l/100 km (the P2
     Hybrid Car example: 2.84, against EPA's 2.91);
   - a 0-100 km/h time and a top speed from the maker's data sheet.
5. **Change one thing and check the direction**: more mass or drag must
   raise consumption; a higher final drive ratio must quicken 0-100 km/h.
   `compare_runs` shows the differences and the change of every result.

## When you report

- Give each number with its unit, the case it came from, and the run's
  status.
- Copy any *not valid* note next to the number.
- Say that LightSim's results are not validated against measured vehicles
  and are best used to compare variants of one model
  (`lightsim-what-it-cannot-do`).
