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
