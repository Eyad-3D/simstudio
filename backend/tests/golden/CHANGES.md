# Golden fixture changes

The fixtures in this folder freeze the bundled examples' results: the BEV
City Cycle and the hybrid Mixed Cycle, each at the shipped case step (a
10 ms solver step) and at a 5 ms solver step (the `__fine` files), both
recorded every second. `tests/test_golden.py` compares new runs with them:

- status and messages must be the same;
- each headline (summary) number must stay within 0.2 % of its value, and
  never less than a few steps of the summary's rounding (percentages such as
  SOC: ±0.02 points);
- each channel must pass through a tube around its stored curve: every
  stored point within 0.5 % of the channel's range of a new point at most
  one recorded step away;
- the shipped step's headline numbers must agree with the 5 ms step's
  within the same bands (the shipped step is converged).

A failing test prints a diff report: what moved, old → new, by how much and
against which band. `LIGHTSIM_GOLDEN_EXACT=1` demands identical results
instead (to 1e-6): use it for refactors that must not change anything. In
CI the reports of every run go to the job summary.

These tests ask whether behaviour changed, not whether it is right: that is
the plausibility tests' job (`tests/test_examples_plausible.py`).

## How to regenerate

Only for an intended behaviour change:

    cd backend
    python tests/update_golden.py                   # the diff report; writes nothing
    python tests/update_golden.py --reason "Title"  # rewrites every fixture

`--reason` is required. It becomes a new `## Title` entry at the end of
this file, with the headline numbers that moved (old → new) filled in:
write under it why they moved. Each fixture records its entry's title, and
`test_golden` fails when the entry is missing.

# Changes

## Recorded times match the state they hold (ENG-03)

Before, each recorded point held the state one case step after its time
stamp (distance read 9.95 m at "t = 0" when driving at 10 m/s), and every
run integrated one extra step (601 steps for a 600 s case). Now point 0 is
the initial state at t = 0, every later point is stored under the end time
of its step, and the run stops exactly at the case duration. Scripts also
see the battery's real initial SOC on their first step instead of 0.

- Every channel shifts one point later in time; point 0 now holds initial
  values (for example 0 A battery current, 0 m distance).
- bev-car City Cycle: energy delivered 1.552 -> 1.551 kWh, consumption
  20.39 -> 20.38 kWh/100 km; distance 7.292 km and final SOC 87.5 %
  unchanged.
- hybrid-car Mixed Cycle: fuel 19.34 -> 19.21 l/100 km, final SOC 51.71 ->
  51.46 %, distance 8.984 -> 8.979 km. The hybrid numbers are still
  dominated by the 1 s control rate.

## Controllers run every solver step (ENG-01)

Before, the Driving Task target, Script, PID, Lookup and Road Profile
blocks and gear selection were evaluated once per case step (1 s in both
demos) and held in between, and scripts read battery SOC, engine speed and
other state signals only once per case step. The demo results depended on
the case step (hybrid 19.34 l/100 km at 1 s vs 12.28 at 0.1 s). Now all of
them run at every solver step (10 ms), state signals are refreshed after
every solver step, and scripts and the PID get the solver step as dt. The
case step only sets how often results are stored; both demos give the same
numbers at 1 s as at 0.02 s.

- bev-car City Cycle: energy delivered 1.551 -> 1.479 kWh, recuperated
  0.066 -> 0.029 kWh, consumption 20.38 -> 19.88 kWh/100 km, final SOC
  87.50 -> 87.56 %; distance 7.292 km unchanged.
- hybrid-car Mixed Cycle: fuel 19.21 -> 7.76 l/100 km (0.553 kg), final
  SOC 51.46 -> 52.92 %, distance 8.979 -> 9.556 km (the car now follows the
  cycle). The drop against the earlier fine-step figure of 12.28 l/100 km
  comes from the entry above: the supervisory script no longer reads SOC 0
  on its first step, so it does not start the engine at t = 0 to charge the
  battery to 62 %; the engine now first starts at 268 s, when SOC falls
  below 48 %.

## E-Motor spin losses counted once (MOD-04)

The motor's loss map ("Power Loss (Motor + Inverter)") already holds the
losses of a motor spinning at zero torque, yet the drag table was also
subtracted from the shaft torque at all times, so a powered motor paid its
spin losses twice. Now the loss map covers every loss while the inverter is
powered, and the drag table applies only when the motor coasts unpowered
(a command of exactly 0, or no live supply), when it draws nothing. The
recorded Shaft Torque and Mechanical Power are now the net shaft values
(before: the torque before drag), and Losses is always electrical minus
shaft power.

- bev-car City Cycle: energy delivered 1.479 -> 1.361 kWh, recuperated
  0.029 -> 0.036 kWh, consumption 19.88 -> 18.16 kWh/100 km, final SOC
  87.56 -> 87.78 %; distance 7.292 km unchanged.
- hybrid-car Mixed Cycle: fuel 0.553 -> 0.540 kg, 7.76 -> 7.58 l/100 km,
  final SOC 52.92 -> 53.00 %, battery consumption row 2.49 -> 2.39
  kWh/100 km; distance 9.556 km unchanged.

## Source-limit handshake (ENG-02 / MOD-01)

Motors could draw power their source did not have (an empty battery, a
battery past its maximum-power point, a fuel cell over its maximum), and
recuperated power could vanish into a full battery, a charge limit or a
one-way DC-DC. Every solver step now starts with a handshake: each bus
states what its source can deliver and absorb, consumers and DC-DC
setpoints are served first, and motor torque is cut so the motors fit what
is left. A new summary row, "Electrical energy balance error", reports the
energy no source covered as a share of all energy through the buses.

- Neither demo reaches a source limit, so every channel and number is
  unchanged; both fixtures gain "Electrical energy balance error 0.0 %".

## Engine: brake-torque maps, fuel cut-off, rev limiter and CO₂ (MOD-05)

The engine subtracted its drag (friction) table even while fired, so it
fell 28 % short of its own full-load curve (59 kW peak instead of 82 kW),
burned map(speed, 0) with the pedal lifted, and ran on past the curve's
last speed with its torque held flat. The full-load curve and fuel map are
now brake (net) maps: fired, the engine gives throttle × full-load torque
and burns map(speed, torque); drag applies only when it is not fired. Zero
throttle above the new Fuel Cut-Off Re-Entry Speed (default 1,100 1/min)
cuts the fuel, the idle governor trims the fuel down to the drag torque
above idle, and above the full-load curve's last speed a rev limiter cuts
fuel and torque. Engine Torque now records the net shaft torque. The run
summary gains "CO₂ emissions" (g/km) from the fuel burnt and the tank's
new CO₂ factor (default 3.17 kg per kg, petrol).

- bev-car City Cycle: unchanged (no engine).
- hybrid-car Mixed Cycle: fuel 0.540 -> 0.459 kg, 7.58 -> 6.45 l/100 km,
  new CO₂ emissions 152.3 g/km; final SOC 53.00 -> 55.83 % (from 55 %),
  recuperated 0.606 -> 0.949 kWh, internal losses 0.0118 -> 0.0164 kWh;
  the battery "Consumption" row (2.39 kWh/100 km) is gone because the
  battery now ends above its start. Status success -> warning: from 544 s
  the car stops with the engine on and the clutch open, and the example's
  Hybrid Control Unit script holds throttle 0.3 there, which now revs the
  engine to its 6,000 1/min limiter (before, the drag held it near 4,900).
  That is 0.087 kg of the fuel. The script belongs to the example (CON-02
  rewrites it to stop the engine free-revving).

## Run verdict (VAL-02)

Both fixtures are unchanged: both demos follow their cycles, so the new
verdict adds no message. The status rules changed for other runs: a run
whose speed leaves the ±2 km/h, ±1 s band for more than 1 % of its
duration is at best "warning" ("Cycle not followed: ..."), and one that
covers under 5 % of the cycle's distance or records NaN "failed". Summary
values carry an optional notValid reason, which the fixtures do not store.

## E-Motor default loss and drag tables (MOD-04, review round 1)

With the spin losses counted once, the library's default E-Motor still
lost 2.6 kW at zero torque near 8,000 1/min, so the BEV example turned only
79.8 % of its battery power (net of the 2.5 kW auxiliary load) into road
load at a steady 100 km/h. The zero-torque column of "Power Loss (Motor +
Inverter)" is now 0.1 / 0.35 / 0.7 / 1.2 / 1.9 kW at 0 / 3,000 / 6,000 /
9,000 / 12,000 1/min (was 0.2 / 0.8 / 1.8 / 3.0 / 4.6), about 1.0 kW near
8,000 1/min; the loaded columns (100 N·m and up) are unchanged. "Drag Torque
(unpowered)" is now 0.7 / 0.8 / 1.0 / 1.2 N·m at 3,000 / 6,000 / 9,000 /
12,000 1/min (was 1.2 / 2.6 / 4.2 / 6.0), so a coasting, unpowered motor
never loses more than a powered one at zero torque (before, 7.5 kW against
4.6 kW at 12,000 1/min). These are generic values for a motor of this size
(background knowledge, not from a datasheet); the sourced maps of the
example rework (CON-03) and the library defaults (CON-14) replace them. The
BEV example at a steady 100 km/h is now 86.0 % battery-to-wheel.

- bev-car City Cycle: energy delivered 1.361 -> 1.279 kWh, recuperated
  0.036 -> 0.041 kWh, internal losses 0.0098 -> 0.0088 kWh, consumption
  18.16 -> 16.97 kWh/100 km, final SOC 87.78 -> 87.92 %; distance and
  status unchanged.
- hybrid-car Mixed Cycle: fuel 0.459 -> 0.450 kg, 6.45 -> 6.32 l/100 km,
  CO₂ emissions 152.3 -> 149.3 g/km, final SOC 55.83 -> 55.84 %,
  recuperated 0.949 -> 0.951 kWh; status still warning (rev limiter, see
  MOD-05).
- Any user model that keeps the default E-Motor maps moves the same way;
  models with their own loss or drag table are unchanged.

## Net effect of the engine-lane step 2 on the demos

| Demo | Number | Before step 2 | After |
|---|---|---|---|
| bev-car City Cycle | Consumption | 19.88 kWh/100 km | 16.97 kWh/100 km (MOD-04 18.16, default maps 16.97) |
| | energy delivered / recuperated | 1.479 / 0.029 kWh | 1.279 / 0.041 kWh |
| | final SOC | 87.56 % | 87.92 % |
| | status | success | success |
| hybrid-car Mixed Cycle | Fuel consumption | 7.76 l/100 km | 6.32 l/100 km (MOD-04 7.58, MOD-05 6.45, default maps 6.32) |
| | fuel used | 0.553 kg | 0.450 kg |
| | CO₂ emissions | — | 149.3 g/km (new) |
| | final SOC (starts at 55 %) | 52.92 % | 55.84 % |
| | battery Consumption row | 2.49 kWh/100 km | gone (the battery ends above its start) |
| | status | success | warning (rev limiter, see MOD-05) |
| both | Electrical energy balance error | — | 0.0 % (new) |

## Battery Electric Car rebuilt on the 2021 Cupra Born (CON-03)

The example is now a named compact electric car with FASTSim's values for
the 2021 Cupra Born 58 kWh (docs/data-register.csv DR-01): 1,927 kg (was
1,800), Cd 0.27 x 2.31 m² (0.28 x 2.2), rolling resistance 0.011 (0.012),
61 % of the weight on the driven axle (55 %), 0.25 kW auxiliaries (2.5 kW),
wheel radius 0.3488 m (0.33), a 62 kWh battery with a 4 % floor (60 kWh,
10 %), 98 % transmission efficiency (97 % x 98 %) and 98 % recuperation
weight (80 %). The motor has its own maps: 150 kW, 310 N·m, falling to zero
torque at its 16,000 1/min maximum speed, and a loss map calibrated to
FASTSim's motor efficiency curve (about 95 % peak). The final drive is 12.8
(9.7), so that the motor's maximum speed is reached at the car's 160 km/h.
The City Cycle case and its speed profile are unchanged; new cases "WLTC
Class 3b" and "WLTC, heating/air-con on" are not fixtures (their numbers are
checked by test_examples_plausible.py).

- bev-car City Cycle: consumption 16.97 -> 11.11 kWh/100 km, energy
  delivered 1.279 -> 0.881 kWh, recuperated 0.041 -> 0.071 kWh, internal
  losses 0.0088 -> 0.0057 kWh, final SOC 87.92 -> 88.68 % (of a larger
  battery, from the same 90 %); distance 7.292 km and status unchanged.
  The auxiliary load alone accounts for 5.14 of the 5.86 kWh/100 km drop
  (2.25 kW less for 600 s over 7.29 km); the higher driveline efficiency,
  the lower rolling resistance and the higher recuperation weight save more
  than the extra 127 kg costs.
- New headline numbers (1 s case step, verified converged: controllers run
  every 10 ms): WLTC class 3b 14.04 kWh/100 km at the battery, 18.88 with
  2.5 kW heating/air-con; full power 0-100 km/h 7.16 s, 0-60 mph 6.74 s,
  top speed 160.2 km/h at 15,723 1/min.

## P2 Hybrid Car rebuilt as a charge-sustaining hybrid (CON-02)

The example is now sized after the Hyundai Ioniq Hybrid Blue
(docs/data-register.csv DR-02): EPA test mass 1,474 kg and road load
(rolling resistance 0.0064, Cd·A 0.70 m², fitted to the EPA 2022 Test Car
List coefficients), final drive 3.38 from EPA's N/V ratio, a 77 kW engine
with a generic Atkinson-cycle fuel map (best 220 g/kWh, was 274-331 g/kWh
everywhere), a 32 kW / 170 N·m motor, a 1.56 kWh 240 V battery (was 12 kWh
at the library's 300-376 V) and a new 0.5 kW "12 V Loads" consumer. The
Hybrid Control Unit script is rewritten: electric launch, engine off at
standstill, when coasting and at low demand (minimum on/off times 8 / 4 s),
engine started by bringing it up to the input-shaft speed before the clutch
closes, load-point shifting around 55 % SOC (engine forced on below 51 %),
and a shift map with 6 km/h hysteresis. It reads two new inputs (engine and input-shaft speed, links
db-19 and db-20). Each case starts at the SOC the cycle ends with, as a
preconditioning drive would leave it (Mixed Cycle 52.5 %).

- hybrid-car Mixed Cycle: fuel 0.450 -> 0.198 kg, 6.32 -> 2.79 l/100 km,
  CO₂ 149.3 -> 65.8 g/km; SOC 55 -> 55.84 % before, 52.5 -> 52.5 % now;
  battery energy delivered 0.834 -> 0.202 kWh, recuperated 0.951 -> 0.205
  kWh, internal losses 0.0165 -> 0.0030 kWh; distance 9.556 km unchanged.
  Status warning -> success: the engine no longer runs declutched at the
  rev limiter at standstill (it is off whenever the car stops). One new
  channel, the 12 V Loads' power (60 -> 61 channels).
- New headline numbers (charge balanced, not fixtures; checked by
  test_examples_plausible.py): EPA city (UDDS) 2.67 l/100 km, EPA highway
  (HWFET) 3.13 l/100 km, against the real car's 2.91 and 2.94 in EPA's
  tests.

## Golden fixtures v2: tolerance bands, a 5 ms variant, readable diffs (VAL-18)

No behaviour change: the solver is the same, and the shipped-step fixtures
hold exactly the numbers of the previous ones (status, messages, every
summary value and every stored channel point). What changed is how they are
stored and compared (see the top of this file):

- The fixtures are now one line per message, summary row and channel, with
  one shared time grid, so a regeneration's diff shows what moved (the BEV
  file went from 106 to 28 KB, the hybrid's from 144 to 36 KB). Each names
  the entry here that produced it.
- New `__fine` fixtures run each case at a 5 ms solver step (case step
  5 ms, stored every 200th step, so on the same 1 s grid). Every headline
  number equals the shipped step's (10 ms solver step) except the hybrid's
  final SOC, 52.50 % against 52.49 %: the examples are converged, and a
  test keeps them so.
- The tests compare with tolerance bands instead of 1e-6 everywhere, and
  print a diff report when they fail; `update_golden.py` prints the same
  report and needs `--reason` to write.

## Recuperation held to what the supply takes, the rest reported (MOD-02)

- A motor whose regeneration command asks for more than its bus can take
  (a full or charge-limited battery, a fuel-cell-only bus, a bus behind a
  one-way DC-DC) was already held to what the bus takes (ENG-02); what it
  asked for beyond that is now reported in a new summary row, "<motor> —
  regeneration not recovered" (kWh). Neither example reaches such a limit,
  so the fixtures do not have the row.
- The Driver's blending checks the recuperation command it sends against
  what the motors' buses can take this step, on the motors' own loss maps,
  and reflects motor torque to the wheels through the drivetrain in the
  generating direction, as the mechanics do. Before, it multiplied by the
  efficiency, so regeneration braked 1/η² harder than planned (4 % in the
  BEV). The recuperation weight now applies to the motors' capability
  only, so a charge limit is used in full (before, 80 % of it by default).

- bev-car City Cycle: no headline number moved (consumption 11.11
  kWh/100 km, recuperated 0.071 kWh). The brake pedal is slightly lower
  while braking (0.0259 → 0.0247 at 320 s) and the friction torque holding
  the car at the end slightly higher (11.0 → 11.4 N·m per brake at
  600 s): 6 channels (8 at 5 ms) left their tubes.
- hybrid-car Mixed Cycle: identical. Its motor sits on its segment's
  reference axis, so its drivetrain efficiency factor is 1 and the new
  reflection changes nothing (its gearbox and final-drive losses are not
  applied to any torque: a separate, older issue).

| Fixture | Number | Old | New | Change |
|---|---|---|---|---|
| bev-car City Cycle (shipped step) | channels that moved | | 37 | 6 outside their tube |
| bev-car City Cycle (fine step) | channels that moved | | 39 | 8 outside their tube |

## Gear losses act on the power through each gear, wherever the driveline walk starts

The solver walks each rigid section of a driveline from whichever port a
joint asks for first, and applied gear, final-drive and shaft efficiencies
only to motor and engine torque on its way from the motor or engine to that
port. In the P2 Hybrid Car the clutch asks first, at the E-Motor's node, so
the gearbox (97 %) and final-drive (98 %) losses reached no torque: not the
motor's, and not the engine's, which comes in through the clutch and was
never charged a gear or differential loss at all. The BEV's walk starts at
the differential, so its final-drive loss was applied.

Now each rigid section is oriented towards its output (the input of the
differential or transfer case it drives, else a wheel, propeller or brake,
else a clutch), and every gear on the way passes on its efficiency times
the net torque through it, or asks 1/efficiency times when the power flows
back (recuperation, an engine dragged in fuel cut-off). Motor torque,
engine torque and the torque a clutch passes on all go through it, so the
losses act on the net power through each gear: when the engine charges
the battery through the motor, that power crosses no gear and loses
nothing there (the phantom braking of MOD-03). Gears between a
differential's output and its wheels now count too. The differential's
Torque A/B channels now show the torque through it, the engine's share
included (before: the motor's alone, with the differential's efficiency
counted twice).

- bev-car City Cycle: identical (the fixtures only name this entry).
- hybrid-car Mixed Cycle: fuel 2.79 -> 2.89 l/100 km (+3.6 %), final SOC
  52.5 -> 51.79 %: the case starts at the charge the cycle used to end with
  (the next entry balances it again). The battery now gives out more than
  it takes back, so a Consumption row appears (0.08 kWh/100 km). The
  differential's Torque A/B include the engine's torque (t = 10 s, engine
  driving and motor charging: -170 -> +190 N·m); the clutch, wheel and
  battery channels move with the engine's start times.
- Not fixtures, same start SOCs: EPA city (UDDS) 2.67 -> 2.96 l/100 km,
  EPA highway (HWFET) 3.13 -> 3.29 l/100 km. The city cycle rises most
  because the losses now apply both ways: to the traction energy and to
  the recuperated energy.

| Fixture | Number | Old | New | Change |
|---|---|---|---|---|
| hybrid-car Mixed Cycle (shipped step) | HV Battery — final SOC | 52.5 % | 51.79 % | -0.71 (-1.35 %) |
| hybrid-car Mixed Cycle (shipped step) | HV Battery — energy delivered | 0.202 kWh | 0.21 kWh | +0.008 (+3.96 %) |
| hybrid-car Mixed Cycle (shipped step) | HV Battery — energy recuperated | 0.205 kWh | 0.201 kWh | -0.004 (-1.95 %) |
| hybrid-car Mixed Cycle (shipped step) | HV Battery — internal losses | 0.003 kWh | 0.0031 kWh | +0.0001 (+3.33 %) |
| hybrid-car Mixed Cycle (shipped step) | Engine — fuel used | 0.198 kg | 0.206 kg | +0.008 (+4.04 %) |
| hybrid-car Mixed Cycle (shipped step) | Fuel consumption | 2.79 l/100km | 2.89 l/100km | +0.1 (+3.58 %) |
| hybrid-car Mixed Cycle (shipped step) | CO₂ emissions | 65.8 g/km | 68.2 g/km | +2.4 (+3.65 %) |
| hybrid-car Mixed Cycle (shipped step) | Consumption | — | 0.08 kWh/100km | new row |
| hybrid-car Mixed Cycle (shipped step) | channels that moved | | 55 | 39 outside their tube |
| hybrid-car Mixed Cycle (fine step) | HV Battery — final SOC | 52.49 % | 51.79 % | -0.7 (-1.33 %) |
| hybrid-car Mixed Cycle (fine step) | HV Battery — energy delivered | 0.202 kWh | 0.21 kWh | +0.008 (+3.96 %) |
| hybrid-car Mixed Cycle (fine step) | HV Battery — energy recuperated | 0.205 kWh | 0.201 kWh | -0.004 (-1.95 %) |
| hybrid-car Mixed Cycle (fine step) | HV Battery — internal losses | 0.003 kWh | 0.0031 kWh | +0.0001 (+3.33 %) |
| hybrid-car Mixed Cycle (fine step) | Engine — fuel used | 0.198 kg | 0.205 kg | +0.007 (+3.54 %) |
| hybrid-car Mixed Cycle (fine step) | Fuel consumption | 2.79 l/100km | 2.88 l/100km | +0.09 (+3.23 %) |
| hybrid-car Mixed Cycle (fine step) | CO₂ emissions | 65.8 g/km | 68.1 g/km | +2.3 (+3.50 %) |
| hybrid-car Mixed Cycle (fine step) | Consumption | — | 0.08 kWh/100km | new row |
| hybrid-car Mixed Cycle (fine step) | channels that moved | | 55 | 46 outside their tube |

## The hybrid's cases start at the charge they end with again

With the gear losses above, the P2 Hybrid Car's cycles no longer end at the
charge they start with, which its example card and the fuel figures assume.
Each case again starts at the charge the cycle ends with, as a
preconditioning drive would leave it: EPA city (UDDS) 56.42 -> 56.7 %, EPA
highway (HWFET) 59.14 -> 58.9 %, Mixed Cycle and its live copy 52.5 ->
51.79 % (found by running each case from the charge it last ended at
until the two agree within 0.01 %). The example card's figures follow.

- hybrid-car Mixed Cycle: fuel 2.89 -> 2.93 l/100 km, final SOC 51.79 %
  unchanged (the strategy holds it wherever the cycle starts); the battery
  again takes back what it gives out, so the Consumption row is gone.
- Charge-balanced headline numbers (not fixtures), against the 0.2.0
  figures before both entries: EPA city (UDDS) 2.67 -> 2.95 l/100 km
  (Ioniq Blue, EPA: 2.91), EPA highway (HWFET) 3.13 -> 3.30 l/100 km
  (EPA: 2.94), Mixed Cycle 2.79 -> 2.93 l/100 km; 32 engine starts on UDDS
  (was 31).

| Fixture | Number | Old | New | Change |
|---|---|---|---|---|
| hybrid-car Mixed Cycle (shipped step) | HV Battery — energy delivered | 0.21 kWh | 0.207 kWh | -0.003 (-1.43 %) |
| hybrid-car Mixed Cycle (shipped step) | HV Battery — energy recuperated | 0.201 kWh | 0.21 kWh | +0.009 (+4.48 %) |
| hybrid-car Mixed Cycle (shipped step) | HV Battery — internal losses | 0.0031 kWh | 0.0032 kWh | +0.0001 (+3.23 %) |
| hybrid-car Mixed Cycle (shipped step) | Engine — fuel used | 0.206 kg | 0.208 kg | +0.002 (+0.97 %) |
| hybrid-car Mixed Cycle (shipped step) | Consumption | 0.08 kWh/100km | — | row gone |
| hybrid-car Mixed Cycle (shipped step) | Fuel consumption | 2.89 l/100km | 2.93 l/100km | +0.04 (+1.38 %) |
| hybrid-car Mixed Cycle (shipped step) | CO₂ emissions | 68.2 g/km | 69.1 g/km | +0.9 (+1.32 %) |
| hybrid-car Mixed Cycle (shipped step) | channels that moved | | 49 | 20 outside their tube |
| hybrid-car Mixed Cycle (fine step) | HV Battery — energy delivered | 0.21 kWh | 0.207 kWh | -0.003 (-1.43 %) |
| hybrid-car Mixed Cycle (fine step) | HV Battery — energy recuperated | 0.201 kWh | 0.21 kWh | +0.009 (+4.48 %) |
| hybrid-car Mixed Cycle (fine step) | HV Battery — internal losses | 0.0031 kWh | 0.0032 kWh | +0.0001 (+3.23 %) |
| hybrid-car Mixed Cycle (fine step) | Engine — fuel used | 0.205 kg | 0.208 kg | +0.003 (+1.46 %) |
| hybrid-car Mixed Cycle (fine step) | Consumption | 0.08 kWh/100km | — | row gone |
| hybrid-car Mixed Cycle (fine step) | Fuel consumption | 2.88 l/100km | 2.92 l/100km | +0.04 (+1.39 %) |
| hybrid-car Mixed Cycle (fine step) | CO₂ emissions | 68.1 g/km | 69 g/km | +0.9 (+1.32 %) |
| hybrid-car Mixed Cycle (fine step) | channels that moved | | 49 | 20 outside their tube |

## Battery state of charge counts amp-hours (MOD-38)

Before, the battery's SOC was an energy count (OCV × I × dt over the
capacity in Wh), so a pack whose voltage falls as it empties reached 0 %
late: a 1C discharge took 3,611 s on the library OCV table instead of
3,600 s. Now the SOC counts the charge that flows (I × dt over the
capacity in Ah), as battery management systems and datasheets do, and the
OCV table is read at that SOC. A battery with no Charge Capacity (both
examples, and every project from 0.2.0) gets its amp-hours from its Usable
Capacity divided by the OCV table's SOC-weighted mean voltage (62 kWh /
345.0 V = 179.7 Ah for the BEV, 1.56 kWh / 238.6 V = 6.54 Ah for the
hybrid), so a full-to-empty discharge still gives out exactly the Usable
Capacity. Coulombic efficiency defaults to 100 %, so charging stores all
of its current as before.

- bev-car City Cycle: final SOC 88.68 -> 88.77 %; energy delivered,
  recuperated, internal losses and consumption (11.11 kWh/100 km)
  unchanged. Only the SOC and the terminal voltage it sets move.
- hybrid-car Mixed Cycle: final SOC 51.79 -> 51.84 % (from a 51.79 %
  start), fuel 0.208 -> 0.209 kg, 2.93 l/100 km unchanged, CO₂ 69.1 ->
  69.2 g/km. The engine switches off about 1 s later near t = 90 s,
  because the control script's thresholds now read the charge-based SOC;
  the other channels that left their tube follow the script's
  SOC-dependent torque split (most at t = 130 s).
- Headline numbers that are not fixtures: BEV WLTC final SOC 84.61 ->
  84.93 %, 14.04 kWh/100 km unchanged; hybrid EPA city (UDDS) 2.95 and
  highway (HWFET) 3.30 l/100 km unchanged, final SOC 56.70 -> 56.68 % and
  58.90 -> 58.84 % from their 56.7 and 58.9 % starts. The hybrid's cases
  now end within 0.06 points of the charge they start at (under 0.001 l of
  fuel), so their start SOCs are not re-balanced here; MOD-11 re-tunes
  the hybrid and re-balances them.

| Fixture | Number | Old | New | Change |
|---|---|---|---|---|
| bev-car City Cycle (shipped step) | HV Battery Pack — final SOC | 88.68 % | 88.77 % | +0.09 (+0.10 %) |
| bev-car City Cycle (shipped step) | channels that moved | | 3 | 2 outside their tube |
| bev-car City Cycle (fine step) | HV Battery Pack — final SOC | 88.68 % | 88.77 % | +0.09 (+0.10 %) |
| bev-car City Cycle (fine step) | channels that moved | | 3 | 2 outside their tube |
| hybrid-car Mixed Cycle (shipped step) | HV Battery — final SOC | 51.79 % | 51.84 % | +0.05 (+0.10 %) |
| hybrid-car Mixed Cycle (shipped step) | HV Battery — energy delivered | 0.207 kWh | 0.206 kWh | -0.001 (-0.48 %) |
| hybrid-car Mixed Cycle (shipped step) | Engine — fuel used | 0.208 kg | 0.209 kg | +0.001 (+0.48 %) |
| hybrid-car Mixed Cycle (shipped step) | CO₂ emissions | 69.1 g/km | 69.2 g/km | +0.1 (+0.14 %) |
| hybrid-car Mixed Cycle (shipped step) | channels that moved | | 49 | 16 outside their tube |
| hybrid-car Mixed Cycle (fine step) | HV Battery — final SOC | 51.79 % | 51.84 % | +0.05 (+0.10 %) |
| hybrid-car Mixed Cycle (fine step) | HV Battery — energy delivered | 0.207 kWh | 0.206 kWh | -0.001 (-0.48 %) |
| hybrid-car Mixed Cycle (fine step) | CO₂ emissions | 69 g/km | 69.1 g/km | +0.1 (+0.14 %) |
| hybrid-car Mixed Cycle (fine step) | channels that moved | | 46 | 13 outside their tube |

## Brake inertia 0.18 kg·m², motor maximum speed and map edges (MOD-18)

Before, the library's friction brake had a rotational inertia of 0.6 kg·m²,
three to four times that of a real 330 mm disc: a 10 kg disc gives
0.14 kg·m² solid and 0.18 as a ring from 100 to 165 mm radius (a background
estimate). Both examples use four default brakes, so each car carried
1.68 kg·m² too much at its wheels: when accelerating and braking, as if the
BEV were 14 kg and the hybrid 17 kg heavier (1.68 / r², r = 0.349 and
0.31 m). Now the default is 0.18 kg·m², the ring value, since a disc's mass
sits mostly in its friction ring. (With the solid-disc 0.14 the hybrid
example's first engine start moved from 6 s to 10 s at the shipped 10 ms
step but not at the 5 ms step, so the two steps no longer agreed: energy
delivered 0.210 against 0.206 kWh. From 0.15 kg·m² up both start it at
6 s: the example's start rule sits on a knife-edge there.) Less energy goes
into spinning the brakes up, and less comes back when braking:

- bev-car City Cycle: energy delivered 0.881 -> 0.880 kWh, recuperated
  0.071 -> 0.070 kWh; consumption 11.11 kWh/100 km and final SOC 88.77 %
  unchanged. The channels that left their tube are the wheel torque, force
  and slip and the brake torque, which no longer carry the brakes' extra
  inertia torque.
- hybrid-car Mixed Cycle: final SOC 51.84 -> 51.76 %, recuperated 0.210 ->
  0.209 kWh, fuel 0.209 -> 0.208 kg, CO₂ 69.2 -> 69.1 g/km at the shipped
  step; 2.93 l/100 km unchanged. The engine starts about 1 s later near
  t = 72 s, which moves most of the channels that left their tube. At the
  5 ms step: CO₂ 69.1 -> 68.9 g/km.
- Headline numbers that are not fixtures: BEV WLTC 14.04 -> 14.03 kWh/100 km
  (energy delivered 4.381 -> 4.368 kWh), with heating/air-con 18.88 ->
  18.87 kWh/100 km; hybrid EPA city (UDDS) 2.95 -> 2.94 and highway
  (HWFET) 3.30 -> 3.29 l/100 km, final SOC 56.68 -> 56.65 % and 58.84 ->
  58.79 %. Every example case is still a success.

The rest of MOD-18 changes nothing in the examples: with the old brake
inertia every fixture is identical to 1e-6 (`LIGHTSIM_GOLDEN_EXACT=1`). An
E-Motor now has a maximum speed (the last speed point of its full-load curve
unless set) and each table axis an outside-the-data setting (Error, Clamp or
Linear). The examples' motors stay below their maximum speed and no table is
read outside its data: the hybrid's engine fires below its full-load curve's
first speed (800 1/min) while it starts, and the new start-up rule reads that
point, which the flat hold did before. They use no voltage source, DC-DC
converter, fuel cell or default engine, whose defaults also changed (350 V,
350 V, a 396-250 V polarization curve, a 175 N·m peak and an 800 1/min
fuel-map row). No summary row or message is added: the rows for time
outside a table or above a maximum speed appear only when that happens.

| Fixture | Number | Old | New | Change |
|---|---|---|---|---|
| bev-car City Cycle (shipped step) | HV Battery Pack — energy delivered | 0.881 kWh | 0.88 kWh | -0.001 (-0.11 %) |
| bev-car City Cycle (shipped step) | HV Battery Pack — energy recuperated | 0.071 kWh | 0.07 kWh | -0.001 (-1.41 %) |
| bev-car City Cycle (shipped step) | channels that moved | | 43 | 12 outside their tube |
| bev-car City Cycle (fine step) | HV Battery Pack — energy delivered | 0.881 kWh | 0.88 kWh | -0.001 (-0.11 %) |
| bev-car City Cycle (fine step) | HV Battery Pack — energy recuperated | 0.071 kWh | 0.07 kWh | -0.001 (-1.41 %) |
| bev-car City Cycle (fine step) | channels that moved | | 43 | 12 outside their tube |
| hybrid-car Mixed Cycle (shipped step) | HV Battery — final SOC | 51.84 % | 51.76 % | -0.08 (-0.15 %) |
| hybrid-car Mixed Cycle (shipped step) | HV Battery — energy recuperated | 0.21 kWh | 0.209 kWh | -0.001 (-0.48 %) |
| hybrid-car Mixed Cycle (shipped step) | Engine — fuel used | 0.209 kg | 0.208 kg | -0.001 (-0.48 %) |
| hybrid-car Mixed Cycle (shipped step) | CO₂ emissions | 69.2 g/km | 69.1 g/km | -0.1 (-0.14 %) |
| hybrid-car Mixed Cycle (shipped step) | channels that moved | | 55 | 31 outside their tube |
| hybrid-car Mixed Cycle (fine step) | HV Battery — final SOC | 51.84 % | 51.76 % | -0.08 (-0.15 %) |
| hybrid-car Mixed Cycle (fine step) | HV Battery — energy recuperated | 0.21 kWh | 0.209 kWh | -0.001 (-0.48 %) |
| hybrid-car Mixed Cycle (fine step) | CO₂ emissions | 69.1 g/km | 68.9 g/km | -0.2 (-0.29 %) |
| hybrid-car Mixed Cycle (fine step) | channels that moved | | 55 | 31 outside their tube |
