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
