---
name: lightsim-explain-a-result
description: Explain a LightSim result to a student in plain words - where the energy went, why one variant uses more than another, what a channel shows. Use when a user asks why a number is what it is, or what a run means.
license: See README.md of the LightSim skill pack
metadata:
  version: "0.3.0"
  app: LightSim 0.3.0
---

# Explain a result

## Build the explanation from the run, not from memory

1. Read the run: `results_query` without channels gives every channel's
   minimum, maximum, mean and end value, and the summary rows.
2. Follow the energy: battery energy delivered, minus energy recuperated,
   gives the net; the motor's *Losses* channel and the battery's
   *internal losses* row show where some of it went; the rest pushed the
   car against drag, rolling resistance and the auxiliary loads.
3. For "why is B different from A", use `compare_runs`: it lists what
   changed in the model and how each result moved. Explain the biggest
   change first, with its cause.
4. For "what happens at t = …", use `results_query` with a time window
   (`t_from`, `t_to`) and the channels of the parts involved.

## Say it plainly

- Short sentences; explain each technical term the first time ("state of
  charge (SOC), how full the battery is").
- Numbers with units and the case they came from: "11.1 kWh/100 km on
  the City Cycle".
- Give the physical reason: air drag grows with the square of speed, so
  a highway cycle uses more energy per kilometre than a city cycle at the
  same mass; braking energy can come back through recuperation, which a
  city cycle gives more chances for.
- Quote any *not valid* note and the run's status
  (`lightsim-read-not-valid-flags`).
- End with what the model cannot tell (`lightsim-what-it-cannot-do`),
  above all that the results are not validated against measured vehicles.
