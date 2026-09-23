# Golden fixture changes

The fixtures in this folder freeze the bundled demo results. They are only
regenerated (`python tests/update_golden.py`) for an intended behaviour
change, and each regeneration is recorded here with the reason and how the
headline numbers moved.

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
