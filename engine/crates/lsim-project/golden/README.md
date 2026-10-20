# Golden comparison (DESIGN.md 12.3)

Every time-domain case of every example project (`backend/projects/*.json`:
14 cases of 4 projects; the lap cases stay in today's lap solver) runs on
the new engine and is compared, figure by figure and channel by channel,
with today's engine. `results.md` has every table, `results.json` the same
rows as data; this page has the method, the summary and the triage of
every difference outside its band.

## Method

**References.** `golden/reference.py` runs each case on today's engine
twice: at its normal step (10 ms) and at a tenth of it (1 ms). The
references here come from today's engine with its physics fixes
(`fix/engine-physics` at ffa5bb3: gear shifts keep the driveline's
momentum, energy books close to round-off, a coasting car stops, the
clutch is dry friction), made on 9 October 2026. Script blocks without a
Sample Time keep today's 10 ms tick in the 1 ms run too, so both runs drive
the same controller and differ only in the integration.

```sh
cd backend   # of today's engine
LIGHTSIM_SCRIPT_TRUST=off python ../engine/crates/lsim-project/golden/reference.py REF_DIR
cd ../engine
LSIM_PYTHON=$(which python) cargo run --release -p lsim-project --example golden -- REF_DIR OUT_DIR
```

**Bands.** A figure's band is the larger of twice the difference between
today's two runs and a floor: 0.1 % of the value for energies (kWh, J),
0.5 % for everything else (times, distances, consumptions, SOC); the new
engine must be within the band of today's 1 ms value. A channel is
compared by the RMS of its difference to today's 1 ms run over every
output point but the first (today records it before its first step, when
the signals its blocks publish still read 0), against twice the RMS
difference between today's two runs, at least 0.5 % of the channel's RMS.

**The new engine.** Each case is imported (`lsim_project::import_case`),
prepared, compiled and run at rtol 1e-6 (atol 1e-8) on IDA with the energy
books on; the figures are computed from its channels by today's
definitions (`core.py`, `labfig.py`, `verdict.py`). A Script block runs
on lsim-script's native runner when its code is in the runner's subset
(the app's runner's results bit for bit, DESIGN.md §9), otherwise on
today's own script runner in a Python process; `LSIM_SCRIPTS=python`
forces Python (`LSIM_PYTHON` runs today's figures, and the fall-back).
The Traction Control block runs today's rule in Rust. A
gear shift, which the gearbox declares a rigid engagement, keeps the
momentum of everything the gears tie together (the run loop's impulse
projection, DESIGN.md 8.2), and that loss is the gearbox's; the tyres,
whose forces are bounded by their grip, pass the momentum on to the
vehicle over the time their slip takes to relax, which the integrator
follows (today's engine relaxes a gripping tyre's slip at once: below).
A condition on time alone (`time >= c`) is an exact time event. Not
compared: today's "Energy
balance residual" and "Electrical energy balance error", which check
today's integrator (the new engine's energy books report their own
closure, below).

## Summary

Release build, one case after another on one machine (4 cores). "As the
gears engaged" is the kinetic energy the gear shifts' rigid engagements
took, the gearbox's, with today's gearbox "gear shifts" term at 1 ms
beside it; the tyres pass no impulse, and what their slip loses as it
relaxes after a shift is in their own slip losses (below).

| case | figures inside | channels inside | energy books: closure | gear shifts | as the gears engaged, kWh (today's gearbox term) | steps | events | run, s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| aero-bev/case-hwfet | 10/10 | 63/67 | 6.4e-12 | — | — | 81983 | 17 | 0.91 |
| aero-bev/case-udds | 14/14 | 63/67 | 4.1e-10 | — | — | 174670 | 257 | 1.67 |
| aero-bev/case-wltc | 18/18 | 63/67 | 3.1e-11 | — | — | 224108 | 122 | 1.98 |
| bev-car/case-city | 10/11 | 65/70 | 6.6e-10 | — | — | 4047 | 17 | 0.06 |
| bev-car/case-city-live | 10/11 | 65/70 | 6.6e-10 | — | — | 4047 | 17 | 0.07 |
| bev-car/case-wltc | 19/19 | 65/70 | 2.8e-10 | — | — | 213215 | 122 | 2.11 |
| bev-car/case-wltc-hvac | 19/19 | 65/70 | 2.0e-11 | — | — | 213911 | 122 | 2.10 |
| bev-car/case-wltc-summer | 20/20 | 65/70 | 1.1e-11 | — | — | 212949 | 122 | 2.19 |
| bev-car/case-wltc-winter | 20/20 | 65/70 | 6.5e-11 | — | — | 213829 | 122 | 2.06 |
| fs-electric/case-accel-75m | 19/19 | 65/65 | 8.0e-11 | — | — | 1062 | 14 | 0.06 |
| hybrid-car/case-hwfet | 9/9 | 88/88 | 1.8e-12 | 16 | 0.0002062 (0.0003367) | 1918237 | 77005 | 26.95 |
| hybrid-car/case-mixed | 9/9 | 82/88 | 7.2e-13 | 10 | 0.0001447 (0.0002622) | 571511 | 56218 | 9.69 |
| hybrid-car/case-mixed-live | 9/9 | 82/88 | 7.2e-13 | 10 | 0.0001447 (0.0002622) | 571511 | 56218 | 9.70 |
| hybrid-car/case-udds | 13/13 | 88/88 | 1.2e-11 | 104 | 0.0016634 (0.0035028) | 3022291 | 118072 | 42.28 |

199 of the 201 figures are inside their bands (194 before work package
4's second round, below), and 984 of 1038 channels (949 after its fourth
round, 952 before its second). The October 2026 triage below went through
every difference outside its band, case by case: 2 figures and 54
channels, before and after alike, because each is today's engine's error
(class b) and none the new engine's (a) or the band's (c), so nothing was
fixed or re-banded. They all have one cause, which shows in two ways: the
new engine's gears lose their efficiency on the torque their teeth carry
and report that torque; today's engine applies the efficiency to, and
reports, the torque of the motors and clutches above the gear pushed
through the gears, the inertias in between (a motor's rotor, the gears'
own) not taken out. The two figures are the
Battery Electric Car's recuperated energy on the city cycle (city and
city-live), at 1.0039 × band (1.0013 × before IDAS's order fix of 16
October), 1.0044 × converged; the next figure is at
0.44 × band. The output points where an event happens, which put 22 of
the hybrid's channels outside after the fourth round, are compared as
today's engine records them, before its step (the run keeps both sides:
below). The energy books close to 6.6e-10 of their throughput or better
in every case (1.5e-9 before IDAS's order fix; the hybrid's to about
1e-11), each stored energy's rate exact. Since work package 4's twelfth
round (16 October) the formula student car's accumulator, held at its
output power limit, switches 1.7e-7 s earlier (a mode on an iteration
variable, met again where the integrator located it a little early:
DESIGN.md 8.2), its figures moved by at most 6e-4 × band, every other
case the same to the last digit. Since the Driver's anti-windup became
back-calculation (20 October, the last section) the hybrid's UDDS has
moved by at most 1.6e-5 × band, every other case the same to the last
digit.
Every figure moved by at most 1.5e-3 × band since the fourth round, most
by less than 5e-4, and none in the sixth or the seventh; with the
engine's own mathematical library (October 2026, the last section) 143
moved, by at most 4.5e-4 × band.

## Triage

### Every difference outside its band (October 2026)

The references were made again for this triage with today's engine as it
is on the integration branch (`fix/engine-physics` with one change to how
a brake's stop is booked, which moves no channel the comparison reads):
every figure and channel of the comparison came out as before, to the
digits `results.md` shows.

**Counts.** Before: 199 of 201 figures, 984 of 1038 channels. After: the
same. Class (a), the new engine wrong: none. Class (b), today's engine
wrong: both figures and all 54 channels. Class (c), the band wrong: none.

**The cause.** Today's engine lumps each rigid stretch of a driveline into
one inertia and applies a gear's efficiency to the net torque its sources
send through it (`through_gears` in `domains.py`); it reports a gear's
power as the sources' power reaching it (`book_gears`) and a
differential's output torque as the sources' torque through the gears
(`torque_above`), half to each side. The torque that speeds up the rotor
and the gears above the mesh never reaches the teeth, but today's engine
both loses the efficiency on it and reports it as carried. The new
engine's gears take the torque at the mesh, after the inertias before it
(the motor's rotor and the gear's own input side), lose the efficiency on
that (`blocks/driveline.rs`: `tau_x`) and report it; a differential's
output torques are its half-shafts' torques. The premise is the usual one
for a gear's efficiency (Modelica's `LossyGear` alike): the mesh's
friction grows with the load on the teeth, and an inertia's torque never
reaches them. A catalogue efficiency is one steady-state number, which
also lumps in losses that do not depend on the load; under that premise
today's rule is the less faithful one, and that is what class (b) means
here: today's engine is wrong against the model the project describes,
and the new engine's answers are the more accurate ones.

**The evidence.**

1. *Today's rule reproduces today's channels and, applied to the new
   engine's run, brings every channel outside inside.*
   `golden/definitions.py` rebuilds today's gear channels from a run's own
   sources (the motors' and clutches' torque and speed, the gear in use)
   by today's rule. On today's 1 ms runs it reproduces today's channels to
   0.016 × band or better (to round-off on the electric cars' torques);
   on the new engine's runs it puts each of the 54 channels at 0.002 to
   0.17 × band (the table below): what is left is a small fraction of the
   band, against 0.005 to 0.016 for today's own run.
2. *The exact answer.* `lsim-project/tests/gear_loss.rs`: a motor (a
   constant torque on its rotor) through the library's final drive (ratio
   12.8, efficiency 0.9) to a load, from rest. Newton on the input side
   gives the teeth's torque, T − (J_rotor + J_in)·i·α, and the loss on it;
   the new engine's speed, teeth torque and loss rate are that exact
   answer to 1e-8 (this shows the block does what its law says; the law
   itself rests on the premise above). Today's rule gives another answer
   (the speed 11.951 against 12.007 rad/s, the loss 1529.8 against 1472.0
   J). With nothing
   on the gear's output but its own output side, the teeth carry 0.246
   N·m of the motor's 100 and the gear loses 89 J over 2 s; today's rule
   would lose 32 655 J in a gear that passes almost nothing.
3. *The wheels' momentum.* An output flange's torque must be what the
   wheel behind it needs: the tyre's force × radius plus the wheel's and
   the brake disc's inertia × their acceleration. Over the output points
   that accelerate by more than 1 rad/s² without braking (the acceleration
   from the 1 s output grid), the new engine's differential torque meets
   it to within that estimate (least squares −0.003 kg·m² × the
   acceleration on the BEV's WLTC, within ±0.27 at every point; +0.007 on
   the hybrid's mixed cycle), as it must, its output torque being the
   half-shaft's. Today's differential torque is above the new engine's by
   4.51 kg·m² × the acceleration on the BEV's WLTC and 4.52 on its city
   cycle (least squares): the rotor and the gears above the differential,
   4.52 kg·m² at the wheels' speed. Against its own wheel channels today's
   misses by 5.4 and 5.6 kg·m² × the acceleration: the extra 0.9 to 1.1,
   about the wheel's and the disc's 0.995 kg·m², is a difference in
   today's wheel torque channel (−0.94 and −1.08 × the acceleration
   against the new engine's, inside that channel's band at 0.18 and 0.08
   ×) that this triage does not explain.
4. *With the inertias above the gears gone, everything agrees.* The BEV's
   city cycle with the motor's rotor and the gears' inertias set to 1e-6
   kg·m² (where the two rules coincide), on both engines: 11 of 11
   figures and 70 of 70 channels inside, the recuperated energy at
   −4.3e-6 kWh (0.058 × band). (The variants of this item and the next
   are `backend/projects/bev-car.json` with those parameters changed and
   only its city case, run through `reference.py` and the golden runner;
   they are not kept in the repository.)
5. *The figure's whole difference is the rule.* With the rotor's and the
   final drive's input inertia moved to its output side (reflected through
   the ratio: 9.031 kg·m²), the new engine's final drive takes its
   efficiency on the motor's whole torque, as today's does; today's
   engine, which lumps them anyway, moves by 1.5e-7 kWh (0.002 × band).
   The new engine's recuperated energy then lands at −4.3e-6 kWh (0.054 ×
   band; −4.1e-6 converged), the final drive's power and losses inside;
   only the differential's torques and power stay outside, by what the
   channel reports. So the rule moves the figure by +8.34e-5 kWh at
   convergence (0.12 % of it), a little more than its band, 7.89e-5 kWh.

**The intended difference, re-checked.** *bev-car city and city-live:
energy recuperated, +1.0039 × band (7.918e-5 kWh against a 7.888e-5 kWh
band; +1.0013 × before IDAS's order fix); converged (rtol 1e-7, 1e-8,
1e-9: 1.0041, 1.0044, 1.0044 × band), +7.923e-5 kWh.* By (5) the rule is the whole difference, so under the
premise above the new engine's figure is the more accurate: braking, the
rotor gives up its kinetic energy to the generator without passing the
teeth, and today's engine takes the final drive's 2 % off that too, so it
recuperates less. Over the cycle the final drive loses about 2.1e-4 kWh
less (0.016843 against 0.017050 kWh, its loss channel integrated over the
1 s output grid; today's own book says 0.017003 kWh, so the difference is
known to about a quarter), which the battery's energies move by together
(delivered −1.1e-4 kWh, recuperated +7.9e-5 kWh). (At rtol 1e-10 IDA's
error test failed ten times at t = 120 s, a breakpoint of the cycle,
where the iteration variable `der(el_final_drive.w_out)` changes its
slope; the run restarts IDA there now, and stops at 300.125 s, where no
step passes the error test on `el_hvbus.t3.i`: DESIGN.md 8.1, not
investigated further here.)

**What else moves.** The same channels on the hybrid's HWFET and UDDS,
the BEV's other cases and the acceleration test differ the same way but
lie inside their bands, which today's own step error makes wider there
(the acceleration test's differential torque at 0.46 × band, 0.03 under
today's rule).

**Every channel still outside** (× band as compared, and with today's
rule applied to the new engine's run by `definitions.py`). "Loss" marks
the channels the loss rule moves (the final drive's on the BEV, the
gearbox's on the hybrid); the rest differ by what the channel reports
(the aero BEV's and the hybrid's axle gears run at 100 %: their road-load
coefficients hold the axle's drag).

| case | channel | × band | class | cause | × band, today's rule on the new run | × band, today's rule on today's run |
|---|---|---:|---|---|---:|---:|
| aero-bev/case-hwfet | el-final-drive:sig_power | 3.03 | b | reports the sources' torque or power | 0.015 | 0.005 |
| aero-bev/case-hwfet | el-diff:sig_power | 3.06 | b | reports the sources' torque or power | 0.015 | 0.005 |
| aero-bev/case-hwfet | el-diff:sig_torque_a | 3.51 | b | reports the sources' torque or power | 0.002 | 0.000 |
| aero-bev/case-hwfet | el-diff:sig_torque_b | 3.51 | b | reports the sources' torque or power | 0.002 | 0.000 |
| aero-bev/case-udds | el-final-drive:sig_power | 3.75 | b | reports the sources' torque or power | 0.048 | 0.016 |
| aero-bev/case-udds | el-diff:sig_power | 3.78 | b | reports the sources' torque or power | 0.048 | 0.016 |
| aero-bev/case-udds | el-diff:sig_torque_a | 2.95 | b | reports the sources' torque or power | 0.054 | 0.000 |
| aero-bev/case-udds | el-diff:sig_torque_b | 2.95 | b | reports the sources' torque or power | 0.054 | 0.000 |
| aero-bev/case-wltc | el-final-drive:sig_power | 3.42 | b | reports the sources' torque or power | 0.025 | 0.008 |
| aero-bev/case-wltc | el-diff:sig_power | 3.45 | b | reports the sources' torque or power | 0.025 | 0.008 |
| aero-bev/case-wltc | el-diff:sig_torque_a | 3.91 | b | reports the sources' torque or power | 0.034 | 0.000 |
| aero-bev/case-wltc | el-diff:sig_torque_b | 3.91 | b | reports the sources' torque or power | 0.034 | 0.000 |
| bev-car/case-city-live | el-final-drive:sig_power | 4.66 | b | reports the sources' torque or power | 0.089 | 0.002 |
| bev-car/case-city-live | el-final-drive:sig_losses | 4.70 | b | loss on the sources' torque | 0.090 | 0.002 |
| bev-car/case-city-live | el-diff:sig_power | 4.76 | b | reports the sources' torque or power | 0.091 | 0.002 |
| bev-car/case-city-live | el-diff:sig_torque_a | 5.59 | b | reports the sources' torque or power | 0.113 | 0.000 |
| bev-car/case-city-live | el-diff:sig_torque_b | 5.59 | b | reports the sources' torque or power | 0.113 | 0.000 |
| bev-car/case-city | el-final-drive:sig_power | 4.66 | b | reports the sources' torque or power | 0.089 | 0.002 |
| bev-car/case-city | el-final-drive:sig_losses | 4.70 | b | loss on the sources' torque | 0.090 | 0.002 |
| bev-car/case-city | el-diff:sig_power | 4.76 | b | reports the sources' torque or power | 0.091 | 0.002 |
| bev-car/case-city | el-diff:sig_torque_a | 5.59 | b | reports the sources' torque or power | 0.113 | 0.000 |
| bev-car/case-city | el-diff:sig_torque_b | 5.59 | b | reports the sources' torque or power | 0.113 | 0.000 |
| bev-car/case-wltc-hvac | el-final-drive:sig_power | 5.95 | b | reports the sources' torque or power | 0.106 | 0.008 |
| bev-car/case-wltc-hvac | el-final-drive:sig_losses | 5.97 | b | loss on the sources' torque | 0.107 | 0.008 |
| bev-car/case-wltc-hvac | el-diff:sig_power | 6.04 | b | reports the sources' torque or power | 0.107 | 0.008 |
| bev-car/case-wltc-hvac | el-diff:sig_torque_a | 7.03 | b | reports the sources' torque or power | 0.146 | 0.000 |
| bev-car/case-wltc-hvac | el-diff:sig_torque_b | 7.03 | b | reports the sources' torque or power | 0.146 | 0.000 |
| bev-car/case-wltc-summer | el-final-drive:sig_power | 6.00 | b | reports the sources' torque or power | 0.106 | 0.008 |
| bev-car/case-wltc-summer | el-final-drive:sig_losses | 6.03 | b | loss on the sources' torque | 0.106 | 0.008 |
| bev-car/case-wltc-summer | el-diff:sig_power | 6.09 | b | reports the sources' torque or power | 0.107 | 0.008 |
| bev-car/case-wltc-summer | el-diff:sig_torque_a | 7.04 | b | reports the sources' torque or power | 0.146 | 0.000 |
| bev-car/case-wltc-summer | el-diff:sig_torque_b | 7.04 | b | reports the sources' torque or power | 0.146 | 0.000 |
| bev-car/case-wltc-winter | el-final-drive:sig_power | 5.83 | b | reports the sources' torque or power | 0.103 | 0.008 |
| bev-car/case-wltc-winter | el-final-drive:sig_losses | 5.87 | b | loss on the sources' torque | 0.103 | 0.008 |
| bev-car/case-wltc-winter | el-diff:sig_power | 5.92 | b | reports the sources' torque or power | 0.104 | 0.008 |
| bev-car/case-wltc-winter | el-diff:sig_torque_a | 7.00 | b | reports the sources' torque or power | 0.145 | 0.000 |
| bev-car/case-wltc-winter | el-diff:sig_torque_b | 7.00 | b | reports the sources' torque or power | 0.145 | 0.000 |
| bev-car/case-wltc | el-final-drive:sig_power | 5.95 | b | reports the sources' torque or power | 0.106 | 0.008 |
| bev-car/case-wltc | el-final-drive:sig_losses | 5.97 | b | loss on the sources' torque | 0.107 | 0.008 |
| bev-car/case-wltc | el-diff:sig_power | 6.04 | b | reports the sources' torque or power | 0.107 | 0.008 |
| bev-car/case-wltc | el-diff:sig_torque_a | 7.03 | b | reports the sources' torque or power | 0.146 | 0.000 |
| bev-car/case-wltc | el-diff:sig_torque_b | 7.03 | b | reports the sources' torque or power | 0.146 | 0.000 |
| hybrid-car/case-mixed-live | el-gearbox:sig_power | 1.15 | b | reports the sources' torque or power | 0.046 | 0.004 |
| hybrid-car/case-mixed-live | el-gearbox:sig_losses | 1.16 | b | loss on the sources' torque | 0.046 | 0.004 |
| hybrid-car/case-mixed-live | el-fd:sig_power | 1.34 | b | reports the sources' torque or power | 0.046 | 0.004 |
| hybrid-car/case-mixed-live | el-diff:sig_power | 1.36 | b | reports the sources' torque or power | 0.046 | 0.004 |
| hybrid-car/case-mixed-live | el-diff:sig_torque_a | 5.46 | b | reports the sources' torque or power | 0.169 | 0.005 |
| hybrid-car/case-mixed-live | el-diff:sig_torque_b | 5.46 | b | reports the sources' torque or power | 0.169 | 0.005 |
| hybrid-car/case-mixed | el-gearbox:sig_power | 1.15 | b | reports the sources' torque or power | 0.046 | 0.004 |
| hybrid-car/case-mixed | el-gearbox:sig_losses | 1.16 | b | loss on the sources' torque | 0.046 | 0.004 |
| hybrid-car/case-mixed | el-fd:sig_power | 1.34 | b | reports the sources' torque or power | 0.046 | 0.004 |
| hybrid-car/case-mixed | el-diff:sig_power | 1.36 | b | reports the sources' torque or power | 0.046 | 0.004 |
| hybrid-car/case-mixed | el-diff:sig_torque_a | 5.46 | b | reports the sources' torque or power | 0.169 | 0.005 |
| hybrid-car/case-mixed | el-diff:sig_torque_b | 5.46 | b | reports the sources' torque or power | 0.169 | 0.005 |

**Found on the way (work package 4's), and fixed there.** The
exact-answer test above first ran its final drive on IDA (the gear's input
inertia makes the model a DAE) and took 9 336 steps for a 2 s spin-up at
rtol 1e-6, 93 000 at 1e-8, 930 000 at 1e-10: the states grow exactly
linearly, IDA stayed at order 1, and with the energy books under error
control the books' quadratures (whose integrals grow as t²) set the step,
steps growing as 1/√rtol, the loss's book 2.6e-4, 2.6e-5 and 2.6e-6 of
itself off (as √rtol); without that error control the run took 89 steps
and every book was 1.32 × too large (the stored energies' integrals
included, against their exact change). The cause was IDAS's order
selection: it decided an order decrease on the states' errors alone, both
zero on states exactly polynomial in time (`0 <= 0.5 · 0`), and its
quadratures' test kept that decision although their errors said
otherwise. The vendored IDAS decides it on the two together now
(`lsim-sundials-sys`, DESIGN.md 8.1): the test holds the loss's rate
exactly and its book to 1e-10 at rtol 1e-10 (it is 6e-15 off). With the
fix every golden case moves by about the tolerance, each figure and
channel on the side of its band it was (DESIGN.md 8.1); the summary
above and `results.md` are of the run with it (16 October 2026).

### Resolved: an event at an output time

*After the fourth round: 22 of the hybrid's HWFET and UDDS channels
outside their bands (16 and 19 × band at most, the driven wheels' slip
losses 49 ×), earlier the clutch's and the gear commands' channels at
the same points. Now: all inside.*

The Hybrid Control Unit's 10 ms ticks fall on the output grid (1 s), and
some of its decisions land exactly on an output point: a shift at t = 57
s on the HWFET and at 699 s and 966 s on the UDDS, a clutch command at
682 s on the HWFET. An output point at an event showed only the values
just after it (the gears engaged, the clutch slipping as the engine
behind it keeps its speed, the driven tyres' slip not yet relaxed);
today's engine records that point before its step. The run now keeps
both sides of an event at an output time, as Modelica tools write two
rows at that time to their result files (`SimResult::left_limits` beside
`SimResult::values`, which still hold the value just after it), and the
comparison takes the side today's engine records.

### Intended: a tyre passes no impulse

*No compared figure leaves its band. The gearbox's "gear shifts" term:
today 0.003503 kWh on the UDDS (1 ms), the new engine's gears 0.001663
kWh; HWFET 0.000337 against 0.000206; mixed 0.000262 against 0.000145.
The rest of what today books at a shift the new engine's tyres lose in
their slip over the milliseconds after it. Channels at the output points
where a shift happens: below.*

A tyre's force is bounded by its grip (μ × its load), inside its grip as
at it, so it passes no impulse in zero time. At a shift the gears'
rigid engagement moves the wheels' speed (the gearbox's loss); the
tyres' slip then relaxes through their own law, inside the grip over
its relaxation time (0.2 to 7 ms on the hybrid) or sliding at the grip
(half of the hybrid's shifts leave the front tyres past it, 2 m/s of
slip at launch), and the integrator follows that. Today's engine relaxes
the slip of every tyre that gripped before the shift at once, keeping
its slip velocity through the impulse, and books the impulse times the
slip before the shift to the tyre (negative on a downshift while
driving). The new engine did so too until the review of its third round
showed it is not the limit of a resolved tyre. The evidence
(`lsim-solve/tests/run_loop.rs`):

* a tyre with a grip limit, F = clamp(k κ, ±4000 N), on a 7 → 12
  downshift: relaxed at once, the vehicle stayed 0.196 m/s apart from
  the run with the slip integrated 50 ms after the shift at k = 2e5,
  2e6 and 2e7 N·s/m alike (the motor at 737 against 611 rad/s); the
  tyre is far past its grip once the gears have engaged and slides
  (`a_tyre_past_its_grip_after_the_rigid_stage_passes_no_impulse`);
* a car with a motor on each axle and the library's tyre law, against
  the same car fully resolved (its gear mesh a stiff damper, nothing
  projected): as the mesh stiffens tenfold, the new engine's speeds over
  the whole transient come tenfold closer (4.7e-4 m/s at 1000
  N·m·s/rad), and so do the gear's and each tyre's losses; relaxing the
  tyres at once while inside their grip stays 0.67 m/s and 22 J apart,
  as the tyre's slip takes about 25 ms to relax there
  (`a_two_axle_shift_matches_the_fully_resolved_car`).

Today's engine has the error; its figures move little with it (at most
0.17 × band here).

### Resolved: gear shifts keep the momentum

*Before work package 4's second round: hybrid-car mixed and mixed-live,
battery energy delivered −1.44 × band, recuperated +1.84 × band; UDDS,
battery internal losses +3.12 × band. Now: +0.01, +0.08 and +0.11 × band.*

At a shift the new engine re-solved the speeds with the states held, so
everything between the clutch and the wheels jumped to the new ratio at
the wheels' speed, and the vehicle got none of that momentum. The gearbox
now declares that a change of its selected ratio is a rigid engagement,
and the run loop projects the states there (DESIGN.md 8.2): the momentum
of everything the gears tie together is kept, and an engine behind a
slipping clutch keeps its speed, as in today's engine (for two inertias
`J_in`, `J_out` and the new ratio `r`, `w_out⁺ = (J_out·w_out⁻ +
r·J_in·w_in⁻)/(J_out + r²·J_in)`); the tyres then pass the momentum on
to the vehicle as their slip relaxes (above). No `reinit` is needed, as
the inertias on each side are the model's and not the gearbox's;
nothing but a declared engagement starts a projection, and what a
`reinit` sets stays. The benchmarks' rotational gear change
(`mech_gear_change` in `tests/exact.rs`) runs to 5e-16 with its loss
exact.

## Today's known errors and the references

The comparison was first made against today's engine before its fixes;
the fixed engine moved its own figures by: battery-electric consumption
by up to −0.08 % (energy books that close), hybrid fuel −0.1 to −0.5 %
(shifts that keep momentum, a dry clutch), the acceleration test's figures
by less than 0.04 %. Today's "Energy balance residual" fell from
0.0005–0.03 % to below 1e-9 %. Against the fixed engine no band had to be
widened for a known error of today's engine.

## Run loop: work package 4's second to ninth rounds

The golden comparison found these in the run loop; each is fixed on
`wp4/solver` with a regression test (`lsim-solve/tests/run_loop.rs`):

* A `when` condition made true by a sample tick's new outputs fires at
  the tick (its value before the event is the one before the tick).
* Mode changes a tick or a time event makes at its instant are scheduled,
  not an event storm: the harness's allowance (`golden::storm_allowance`)
  is gone.
* A `when` condition already true at the start fires as in Modelica:
  nothing at the start, then each time it becomes true again.
* Event iteration re-checks the conditions with the iteration variables
  solved for the new discrete values.
* A tick a few ulps before the end time is that instant (no "tout too
  close to t0").
* A restart hands IDA its derivatives in full and sizes its first step
  from the second derivative (on a sample-and-hold DAE 2.2 steps a tick,
  11.4 before). `suppress_algebraic_error` would take the hybrid's first
  100 s from 230 234 steps to 62 458, but the error on the exact-answer
  suite's DAEs grows up to 8×: it stays off.
* A condition on time alone is an exact time event; gear shifts keep the
  momentum (above).

The review of the second round found more, each fixed with the
reviewer's test (and more):

* Going on with the integration's history after a slight tick (a light
  restart) accumulates an error beyond the tolerance over a long run (57
  tolerance units on a 10 000-tick ramp at rtol 1e-6): it is opt-in now
  (`SolverOptions::light_restarts`). A tick whose outputs reach nothing
  the integrator integrates or watches still goes on without a restart,
  exactly.
* The projection started wherever a stored energy depended on a discrete
  value, a `reinit`'s jump included (a bouncing ball fell through the
  floor): now only at a declared engagement, leaving what a `reinit` set;
  with exact derivatives instead of finite differences; with event
  iteration going on from the moved states (a condition the jump crosses
  fires at the shift); with the loss split in two physical stages (the
  fourth round then found that a tyre passes no impulse at all: above).
* A timer re-armed at its own instant took its relation's value just
  after the old time; `time > t_last` with `t_last := time` never
  switched; a time crossing at the instant SUNDIALS reports a root was
  lost. The right limit now holds only while the crossing's time is still
  now, and what is scheduled at a root's instant joins it.
* A strict `when x > 0` from x = 0 fires as x leaves zero (Modelica).
  With it the Battery Electric Car's friction now registers a backward
  creep of 1e-5 s at the start of each cycle (one event more), which
  moves no figure.
* Alias start values that disagree are told, and the choice no longer
  depends on the order the parts are listed in.

The review of the third round found two more, each fixed with the
reviewer's test (and more):

* The settle loop (iterate, project, iterate on) stopped after 51
  projections at one instant without a word, the last ratio change
  applied but never projected (a shift logic that upshifts again
  whenever a shift leaves the motor at 100 rad/s or more: gear 53 at
  97.96 rad/s instead of gear 132 at 450.42). A cascade longer than
  `SolverOptions::max_event_iterations` now stops the run with an event
  storm naming the engagement and the conditions; raised, the limit lets
  the whole cascade run exactly.
* The tyres' stage relaxed a tyre's slip at once whenever it had gripped
  before the shift, even far past its grip after the gears engaged: a
  tyre passes no impulse at all now (above), and links a model declares
  are judged after the rigid stage.
* `Integrator::consistent_z` fails by default (a DAE backend cannot skip
  it silently), and the run report counts the ticks that reach nothing
  integrated (`inert_ticks`) apart from the opt-in light restarts.

The review of the fourth round found the energy books' closure down to
1.6e-6 (from 2.7e-8) and asked for both sides of an event at an output
time:

* The books took each stored energy's rate by a finite difference whose
  step collapsed whenever an entry of the state passed zero while moving
  (the engine's speed at each start): its round-off grows as the stored
  energy over that step, and a full fuel tank stores some 1.7e9 J. The
  error depended on the step sequence, which the fourth round's tyre
  transients had reshuffled (on the HWFET 10.5, 1.6, −0.8 and 1.9 J for
  rtol 1, 0.999, 1.001 and 0.99 × 1e-6). The rates are now exact
  (forward-mode differentiation through the assignments; on IDA the
  states' rate the model's own x', not IDA's y'): every case closes to
  1.5e-9 or better, and the runs are 5–31 % faster, the smooth
  integrand letting the steps grow.
* Those longer steps then let a car at rest step over a condition: the
  Battery Electric Car in winter stood 21 s at a start, one step spanning
  its motor's switch-on and switch-off. Tables read along time (the
  cycle's target speed) now have their breakpoints as stop times.
* An output time at an event records both sides (above).

The review of the fifth round found three older defects and asked for
two smaller changes, each fixed with a regression test:

* Two sampled blocks ticking together at an output time inside a step:
  the output point showed the state between the two ticks (one block's
  new output beside the other's old one). Every block due at an instant
  now ticks in one event.
* A declared stored energy or loss that read a table made the books NaN
  (they had no tables), and the default run, its books under error
  control, stopped at t = 0. The books read the model's own
  interpolants, the rates through them exact.
* A condition on an explicit function of time was stepped over while
  nothing integrated moved (`sin(2π time / 10) > 0.95`: x(100) = 0
  against 10.108, without a warning). Its sign changes are found ahead
  without integrating and reached exactly; a condition that mixes time
  and states stops the integrator at the extrema of its terms in time;
  what cannot be searched is said in a warning.
* A zero direction has a zero rate in the exact derivatives (`sqrt` at
  zero: no `inf · 0`); the left limits keep only the channels the event
  changed (on the hybrid's UDDS 1.33 MB instead of 4.18 MB); the
  breakpoints of a table whose position moves are recomputed after a
  discrete change, not kept as stale stops.

The review of the sixth round found one regression and asked for more:

* Blocks ticking at one instant read the iteration variables as they
  were before the blocks ahead of them changed their outputs (a block
  reading the battery's current as the motor controller changes its
  torque at the same tick). They are now solved again between the ticks
  when a block reads them; a block's computed input read through its
  chain at a tick where the integrator stopped is evaluated there.
* A condition that mixes time and states could still miss a pulse
  inside a monotone stretch of its time term (`sin(time) > x` with x
  ramping), silently: every step is now checked along the integrator's
  dense output.
* Integer powers are enclosed with a proven bound, the platform's libm
  is tested against correctly rounded references, a model that does not
  give the tables the run reads does not start, and books computed
  without error control are flagged.

The eighth round checks the driving cycle's conditions too, as the
review decided: the breakpoint stops keep a cycle's target linear within
a step, but the speed it is compared with curves, and two crossings can
fall inside one step (a linear table of time against a curving state,
above it for 0.3 s inside one step, fired 0 times; now once). On the
golden models these are the BEV's and the aero BEV's driver command
against its limits (two conditions) and whether the motor's spin losses,
read from its 2-D map, fit its supply window (two), and the hybrid's
driver command against its limits (two):

* A cheap test first: a certificate, a time window and a box of the
  states over which the condition's enclosure excludes zero, clears a
  step whose ends and middle lie inside the box (a few comparisons per
  state, one sample of the dense output per step). Only where none holds
  is the step fitted along the dense output and searched. The
  certificates clear all but 7 051 of 862 704 condition-steps of the
  BEV's WLTC and all but 717 of 1 145 426 of the hybrid's mixed cycle.
* 2-D tables are enclosed by their cells' polynomials, fitted to the
  model's own interpolant; each branch of an `if` with the variables its
  condition compares bounded as it says there.
* The motor's "running" flag, which moves only where a `noEvent`
  comparison flips, is left to root finding, as `noEvent` asks.
* The crossing root finding locates at a step's end, a little after the
  exact change, is not taken for a pulse (the first scan of the BEV's
  WLTC cut 7 steps that way).

The ninth round makes both checks rigorous on SUNDIALS (the golden runs'
integrator). They read the polynomial the integrator interpolates with
(CVODE's Nordsieck array, IDA's divided differences, diffsol's
interpolant through six Chebyshev points, exact for its order up to a
round-off it carries, which on diffsol is an estimate) instead of a fit,
and a step is cleared only when a bound on each state over the whole
step, not three samples of it, lies inside the certificate's box: a
state that swings by k √3 / 36 inside a step, back at its value at the
step's ends and middle, passed the old check with a box of half its
swing. The certificates now clear all but 6 386 of 862 704
condition-steps of the BEV's WLTC and all but 677 of 1 145 426 of the
hybrid's mixed cycle. Adams' order is capped at 7. The run takes the
tables' breakpoints from the data the model interpolates, and refuses to
start when anything it evaluates outside the compiled code (now also the
impulse projection's chain and links, and the time events' instants)
reads a table the model does not give.

Run time, the base (`engine/stage1` at 5581482) against the second round
(fd361a0), both release builds, one case after another on the same
machine:

| case | steps before | steps now | run before, s | run now, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3818875 | 3025084 | 101.4 | 85.7 |
| hybrid-car/case-hwfet | 2526264 | 1926891 | 68.5 | 52.7 |
| hybrid-car/case-mixed | 952522 | 561987 | 29.1 | 20.4 |
| bev-car/case-wltc | 240968 | 237694 | 3.68 | 3.26 |
| aero-bev/case-wltc | 249549 | 248324 | 3.23 | 3.33 |
| bev-car/case-city | 4254 | 4572 | 0.07 | 0.08 |
| fs-electric/case-accel-75m | 1156 | 1058 | 0.11 | 0.11 |

The third round (from the review) against the second, timed the same
way, alternating the two builds case by case, two runs each (the mean,
then each run):

| case | steps, second round | steps, third | run, second round, s | run, third, s | change |
|---|---:|---:|---:|---:|---:|
| hybrid-car/case-udds | 3025084 | 3031936 | 85.82 (86.23, 85.41) | 85.06 (84.59, 85.53) | −0.9 % |
| hybrid-car/case-hwfet | 1926891 | 1927283 | 54.86 (55.04, 54.68) | 53.60 (53.62, 53.58) | −2.3 % |
| hybrid-car/case-mixed | 561987 | 576470 | 23.30 (23.26, 23.33) | 22.75 (22.82, 22.68) | −2.3 % |
| bev-car/case-wltc | 237694 | 237694 | 3.54 (3.50, 3.59) | 3.65 (3.83, 3.47) | +3.0 % |
| aero-bev/case-wltc | 248324 | 248324 | 3.57 (3.52, 3.63) | 3.46 (3.66, 3.25) | −3.3 % |
| bev-car/case-city | 4572 | 4572 | 0.09 (0.10, 0.09) | 0.08 (0.08, 0.08) | −12 % |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.11 (0.11, 0.11) | 0.12 (0.12, 0.11) | +5 % |

Making light restarts opt-in costs the hybrid steps (2.6 % more on the
mixed cycle), but a tick whose outputs reach nothing integrated keeps
the step, and the projection runs only at a shift and without finite
differences: the hybrid's runs are 1–2 % faster, the electric cars'
within the runs' spread (their steps are the same).

The fourth round against the third, one run each of the full comparison
(`results.md`), one case after another:

| case | steps, third round | steps, fourth | run, third round, s | run, fourth, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3031936 | 3032825 | 83.70 | 83.86 |
| hybrid-car/case-hwfet | 1927283 | 1927517 | 53.83 | 53.36 |
| hybrid-car/case-mixed | 576470 | 577834 | 22.36 | 23.32 |
| hybrid-car/case-mixed-live | 576470 | 577834 | 21.95 | 22.15 |
| bev-car/case-wltc | 237694 | 237694 | 3.49 | 3.48 |
| aero-bev/case-wltc | 248324 | 248324 | 3.24 | 3.40 |
| bev-car/case-city | 4572 | 4572 | 0.07 | 0.07 |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.09 | 0.09 |

The tyres' transients after the hybrid's shifts, now integrated, cost
about 900 steps on the UDDS (0.03 %) and 1 400 on the mixed cycle
(0.24 %); the run times move by −1 % to +4 %, within what single runs
spread (mixed and mixed-live are the same computation: 23.32 and 22.15
s); the electric cars, which do not shift, take the same steps.

The fifth round (exact stored-energy rates, the breakpoint stops, both
sides of events) against the fourth, measured the same way:

| case | steps, fourth round | steps, fifth | run, fourth round, s | run, fifth, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3032825 | 3023217 | 83.86 | 76.53 |
| hybrid-car/case-hwfet | 1927517 | 1918254 | 53.36 | 46.70 |
| hybrid-car/case-mixed | 577834 | 572709 | 23.32 | 21.86 |
| hybrid-car/case-mixed-live | 577834 | 572709 | 22.15 | 21.12 |
| bev-car/case-wltc | 237694 | 215669 | 3.48 | 2.53 |
| aero-bev/case-wltc | 248324 | 223735 | 3.40 | 2.34 |
| bev-car/case-city | 4572 | 3999 | 0.07 | 0.09 |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.09 | 0.08 |

The exact rates replace four evaluations of every channel per step, and
the smooth integrand lets the quadratures' error control take longer
steps (the electric cars 9–10 % fewer); the breakpoint stops add a stop
per second of a driving cycle, without a restart.

The sixth round (every block due at an instant ticks in one event, the
books read the model's tables, conditions on explicit functions of time
searched ahead, left limits of the changed channels only) against the
fifth, measured the same way: every figure, channel, closure and step
count is the same to the last digit. None of the example projects has a
condition on a function of time other than a driving cycle's table read
along time, whose breakpoints are stops already (its terms are monotone
between them), no stored energy or loss that reads a table, and no two
blocks ticking together inside a step. The run times move within what
single runs spread (181.6 s in all before, 181.9 s now):

| case | steps, fifth round | steps, sixth | run, fifth round, s | run, sixth, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3023217 | 3023217 | 76.53 | 76.11 |
| hybrid-car/case-hwfet | 1918254 | 1918254 | 46.70 | 47.03 |
| hybrid-car/case-mixed | 572709 | 572709 | 21.86 | 21.28 |
| hybrid-car/case-mixed-live | 572709 | 572709 | 21.12 | 20.97 |
| bev-car/case-wltc | 215669 | 215669 | 2.53 | 2.63 |
| aero-bev/case-wltc | 223735 | 223735 | 2.34 | 2.33 |
| bev-car/case-city | 3999 | 3999 | 0.09 | 0.06 |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.08 | 0.09 |

The seventh round (iteration variables solved again between the ticks
of one instant, mixed conditions checked along every step, a run that
does not start without the tables it reads, books without error control
flagged) against the sixth: every figure, channel, closure and step
count is the same to the last digit again (the hybrid's one sampled
block ticks alone; none of the models has a mixed condition). The full
comparison took 191.1 s against 181.9 s, the hybrid's runs 4–10 % longer;
timed again alternating the two builds, two runs each, the hybrid's UDDS
took 75.74 and 75.04 s at the sixth round and 76.80 and 76.18 s at the
seventh, its HWFET 48.00 and 47.61 s against 46.90 and 47.09 s: the
difference is the machine's, within ±2 %.

| case | steps, sixth round | steps, seventh | run, sixth round, s | run, seventh, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3023217 | 3023217 | 76.11 | 80.89 |
| hybrid-car/case-hwfet | 1918254 | 1918254 | 47.03 | 51.50 |
| hybrid-car/case-mixed | 572709 | 572709 | 21.28 | 21.41 |
| hybrid-car/case-mixed-live | 572709 | 572709 | 20.97 | 21.52 |
| bev-car/case-wltc | 215669 | 215669 | 2.63 | 2.52 |
| aero-bev/case-wltc | 223735 | 223735 | 2.33 | 2.38 |
| bev-car/case-city | 3999 | 3999 | 0.06 | 0.06 |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.09 | 0.09 |

The eighth round (the driving cycle's conditions checked on every step,
behind certificates) against the seventh: every figure, channel, closure
and step count is the same to the last digit, and no run warns. The full
comparison took 195.6 s against 191.1 s. Timed again alternating the
two builds, two runs each, the hybrid's UDDS took 82.16 and 75.95 s at
the seventh round and 79.84 and 76.38 s at the eighth, its HWFET 50.81
and 50.76 s against 50.22 and 51.08 s, its mixed cycle 23.08 and 22.79 s
against 22.05 and 23.16 s, the BEV's WLTC 2.63 and 2.68 s against 2.80
and 2.65 s, the aero BEV's 2.46 and 2.67 s against 2.46 and 2.72 s: the
machine's ±4 % hides the difference. Counted in instructions (callgrind),
the checks take 0.55 % of the hybrid's mixed cycle, a quarter of it the
one dense-output sample per step, and 3.1 % of the BEV's WLTC, whose
steps cost a tenth of the hybrid's and which checks four conditions, two
of them the motor's chains of 45 and 49 assignments.

| case | steps, seventh round | steps, eighth | run, seventh round, s | run, eighth, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3023217 | 3023217 | 80.89 | 82.14 |
| hybrid-car/case-hwfet | 1918254 | 1918254 | 51.50 | 52.30 |
| hybrid-car/case-mixed | 572709 | 572709 | 21.41 | 21.87 |
| hybrid-car/case-mixed-live | 572709 | 572709 | 21.52 | 22.47 |
| bev-car/case-wltc | 215669 | 215669 | 2.52 | 2.96 |
| aero-bev/case-wltc | 223735 | 223735 | 2.38 | 2.62 |
| bev-car/case-city | 3999 | 3999 | 0.06 | 0.07 |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.09 | 0.09 |

The ninth round (both checks on the integrator's own polynomial, bounded
rigorously; the tables' breakpoints from the model; the start-up check of
every table the run reads) against the eighth: each case's output is the
same byte for byte, so every figure, channel, closure and step count is
the same to the last digit, and no run warns. The full comparison took
201.4 s against 195.6 s. Timed again alternating the two builds, two
runs each, the hybrid's UDDS took 86.58 and 82.33 s at the eighth round
and 84.68 and 85.46 s at the ninth, its HWFET 56.56 and 55.86 s against
53.64 and 53.41 s, its mixed cycle 23.08 and 24.39 s against 23.66 and
23.14 s, the BEV's WLTC 2.92 and 2.74 s against 2.98 and 2.85 s, the aero
BEV's 3.01 and 2.53 s against 2.94 and 2.65 s: the machine's noise hides
the difference. Counted in instructions (callgrind), the checks take
3.33 % of the BEV's WLTC against 3.10 % at the eighth round, and 0.64 %
of the hybrid's mixed cycle against 0.55 %; lsim-project's `scan_share`
example puts the BEV's at 4.8 % of its run time (the median of five
runs).

| case | steps, eighth round | steps, ninth | run, eighth round, s | run, ninth, s |
|---|---:|---:|---:|---:|
| hybrid-car/case-udds | 3023217 | 3023217 | 82.14 | 81.86 |
| hybrid-car/case-hwfet | 1918254 | 1918254 | 52.30 | 55.46 |
| hybrid-car/case-mixed | 572709 | 572709 | 21.87 | 23.74 |
| hybrid-car/case-mixed-live | 572709 | 572709 | 22.47 | 22.70 |
| bev-car/case-wltc | 215669 | 215669 | 2.96 | 2.80 |
| aero-bev/case-wltc | 223735 | 223735 | 2.62 | 2.65 |
| bev-car/case-city | 3999 | 3999 | 0.07 | 0.07 |
| fs-electric/case-accel-75m | 1058 | 1058 | 0.09 | 0.10 |

After the review of the ninth round (an automatic Adams → BDF switch
made before the next step, so the step that asked for it keeps its dense
output; diffsol's dense-output bound called an estimate and its end node
placed at the state's own time; a warning where a model gives a table's
values but not its breakpoints): each case's output is again the same
byte for byte, and no run warns. The full comparison took 208.0 s.

Measured with the Python host (before the native runner), the hybrid's
run included the Script block's round trip to Python at every 10 ms tick
(137 000 on the UDDS), and most of its remaining steps
follow the tyres' slip, which settles within about 1e-4 s after each
change of torque the controller makes: the error test on the iteration
variables needs them.

## The engine's own mathematical library (October 2026)

Every transcendental function the new engine evaluates now comes from its
own library, CORE-MATH's correctly rounded functions (`lsim-math`,
DESIGN.md 7.1), instead of the platform's (glibc here): the models'
library calls in the interpreter, the generated code, the tapes, the
kernels and the energy books, the interval enclosures (now widened by the
one ulp correct rounding proves, 2 before), and SUNDIALS' own `pow` in
CVODE's and IDA's step-size and order selection. The results are the same
on every platform and CPU, and they move in the last digits where glibc's
were not correctly rounded.

Against the ninth round (627dd01), the full comparison run the same way:

* **Figures**: 143 of the 201 moved, by at most 4.5e-4 × band (the aero
  BEV's battery losses on the UDDS), the median by 4e-6 × band. None left
  or entered its band: 199 of 201 inside, the two outside still the BEV's
  city recuperation, whose case did not move at all.
* **Channels**: 984 of 1038 inside, as before. The largest move is the
  hybrid's clutch losses on the HWFET, 0.125 × band: a channel whose RMS
  is 6e-10 kW against a band of 1e-9 kW, round-off around a loss that is
  zero there (today's engine has 1e-15 kW); then the same channel on the
  mixed cycle, 0.018 × band; every other channel by at most 1.6e-4 ×
  band.
* **Steps and events**: within 0.9 % (the aero BEV's HWFET 82 027 →
  82 663, the hybrid's mixed cycle 572 709 → 567 850, its UDDS 3 023 217
  → 3 023 612 steps and 118 094 → 118 064 events); the energy books still
  close to 1.5e-9 of the throughput or better.
* **Where the moves come from**, the change made in three steps and the
  comparison run after each: the models' own library calls moved the
  BEV's four WLTC cases and the hybrid's (the aero BEV, the city cycle and
  the acceleration test call no function whose result glibc rounds
  differently); the enclosures' one ulp moved nothing (they decide only
  where a condition is searched along a step); SUNDIALS' `pow` moved every
  case whose step sequence it changed, the aero BEV's included. The BEV's
  city cycle and the acceleration test are byte-identical throughout.
* **Without FMA**: with `LSIM_MATH_NO_FMA=1`, the library's build for CPUs
  without the instruction, every case writes the same bytes.

Run times, the ninth round's build (627dd01) against this one (1bb2173),
release builds, one case at a time on a quiet machine, the two builds
alternating (seconds; the steps are the same in every run of a build):

| case | steps, before | steps, now | run before, s | run now, s |
|---|---:|---:|---|---|
| bev-car/case-wltc | 215669 | 214672 | 3.15, 3.03, 3.96, 2.90, 3.10, 2.68 (median 3.06) | 2.80, 2.71, 3.54, 2.97, 2.97, 2.67 (median 2.88) |
| hybrid-car/case-mixed | 572709 | 567850 | 24.59, 25.36, 23.58 (median 24.59) | 24.74, 24.52, 24.34 (median 24.52) |
| hybrid-car/case-udds | 3023217 | 3023612 | 94.60, 93.07, 97.67, 84.07, 89.47 (median 93.07) | 98.33, 100.33, 102.10, 84.97, 89.72 (median 98.33) |

The UDDS's last two pairs ran the new build first (+1.1 %, +0.3 %), the
first three ran it second (+4 % to +8 %): within this machine's noise, but
for perhaps a few per cent. Counted in instructions (callgrind), which do
not depend on the machine's load, the BEV's WLTC takes 0.4 % fewer
(22.58 against 22.67 billion) and the hybrid's mixed cycle 0.5 % fewer
(93.01 against 93.43 billion, the Script block's Python process not
counted): fewer steps, while the library's share stays at 1.9 % on the
BEV and goes from 1.6 % to 2.4 % on the hybrid, whose `log` and `pow`
cost more per call than glibc's and `atan` less. The full comparison took
226.1 s of runs (208.0 s after the ninth round's review, measured on
another day). With the library's build for CPUs without FMA the BEV's
WLTC takes 18 % longer and the aero BEV's 15 % (3 pairs each).

## The Driver's anti-windup (20 October 2026)

The library Driver's integral is no longer frozen while its command is
held at a limit (a mode a heavy car slid along, stopping its run as an
event storm); it is pulled back by the command's excess, as `LimPID`'s
back-calculation does (DESIGN.md 12.2, `blocks/vehicle.rs`). The E-Motor
no longer solves its supply's share while its inverter is off. Against
the record before (cb76842):

* **No golden car is held at its Driver's limit.** A build with the
  frozen integral's mode taken out and nothing put in its place (der(I) =
  err) gives every case to the last digit. Inside the limits the new
  equation's term is exactly 0, so the residuals are the same to the bit.
* **One case moved: the hybrid's UDDS**, 12 of its 13 figures and 73 of
  its 88 channels, by at most 1.6e-5 × band (its bag 1 fuel
  consumption), none across its band; steps 3 022 879 → 3 022 291, events
  118 074 → 118 072, the books closed to 1.2e-11. Its first difference is
  a clutch event 2.7e-10 s earlier at 163.05 s. The integral's equation
  now reads the command before and after its limits, and through them the
  integral itself: the Newton matrix has entries it did not have, and its
  solves round differently, which the hybrid's 118 000 events carry on
  (the same term written as a dead zone, which reads only the command
  before its limits, gives the same bits as written). Every other case is
  the same to the last digit; the E-Motor's change moves none.
* **Against today's engine**: 199 of 201 figures and 984 of 1038
  channels inside their bands, as before, each on its side.

Run times, the record's build (cb76842) and this one, release builds, the
whole comparison twice, the two builds alternating, CPU time (the machine
shared with another build): 103.0 and 102.7 s before, 101.0 and 102.7 s
now; the hybrid's UDDS 44.11 and 43.59 s before, 42.28 and 42.41 s now
(its fewer steps).

