# LightSim yardstick: lightsim-py (2026-10-08)

Engine **lightsim-py** 0.2.0 (its code as of commit `61c34e0`; benchmarks run from `26c8321`), default solver step 10 ms. Machine: Intel(R) Xeon(R) Processor @ 2.80GHz, 4 CPUs, load average at the start 0.33, Python 3.11.15.

Targets (benchmarks/targets.toml): a signal within 0.0001 of its scale, an event within 0.1 ms + 1e-05 of its time, an energy term within 0.0001 and the energy balance within 1e-06 of the energy scale; at least 1000x real time in full dynamic simulation and about 1e+06x on the WLTC in a fast mode.

## Reference problems

Errors are shares of each quantity's scale (see benchmarks/README.md). *Order* is the observed order of convergence of the worst signal error between the default step and half of it: about 1 for a first-order integrator, near 0 when the error comes from how the model is built rather than from the step.

| Problem | Expressed as | Worst signal | Worst event | Worst energy | Energy balance | Target | Wall, s | x real time | Steps | Order |
|---|---|---|---|---|---|---|---|---|---|---|
| batt_cc_rc | as stated | 5.83e-05 | - | 1.02e-06 | 4.18e-15 | pass | 4.32 | 139 | 60000 | 1.00 |
| batt_cp_rc | as stated | 3.08e-05 | - | 6.65e-06 | 1.61e-13 | pass | 0.73 | 163 | 12000 | 1.00 |
| batt_voltage_limit | as stated | 4.08e-05 | 1.50e-05 | 6.22e-06 | 2.15e-15 | miss | 3.22 | 124 | 40000 | 1.00 |
| elec_rc_step | cannot be expressed | | | | | | | | | |
| elec_rl_step | cannot be expressed | | | | | | | | | |
| mech_clutch_lockup | as stated | 1.16e+00 | 1.49e-15 | 2.25e-03 | 2.15e-03 | miss | 0.02 | 88 | 200 | 7.44 |
| mech_gear_change | rotational load | 4.34e-01 | - | 1.05e+00 | 1.33e-03 | miss | 0.07 | 108 | 800 | 0.00 |
| mech_gear_change | vehicle load | 8.03e-03 | - | 1.58e-02 | 3.74e-04 | miss | 0.64 | 13 | 5600 | 0.00 |
| mech_inertia_coastdown | as stated | 6.12e-05 | 9.90e-05 | 1.33e-04 | 1.60e-04 | miss | 0.58 | 105 | 6100 | 1.00 |
| motor_dc_spinup | cannot be expressed | | | | | | | | | |
| motor_dc_spinup_l0 | as stated | 5.13e-02 | 2.52e-02 | 1.07e-04 | 1.27e-02 | miss | 0.01 | 116 | 150 | 1.04 |
| therm_lumped_mass | cannot be expressed | | | | | | | | | |
| therm_two_masses | cannot be expressed | | | | | | | | | |
| veh_coastdown | as stated | 3.65e-03 | never reached | 2.38e-05 | 2.24e-13 | miss | 0.70 | 316 | 22000 | 0.00 |
| veh_constant_power | as stated | 9.85e-04 | 2.07e-03 | 1.74e-03 | 1.76e-03 | miss | 1.00 | 15 | 10500 | -0.27 |

Today's engine cannot express:

- **elec_rc_step** (RC circuit: DC-link precharge from a voltage step): no resistor or capacitor part; the battery's RC pair cannot be charged from a voltage source (an electrical bus has exactly one source).
- **elec_rl_step** (RL circuit: contactor coil energised from a voltage step): no inductor or resistor part.
- **motor_dc_spinup** (DC motor spinning up from a voltage step (with winding inductance)): no winding inductance: the E-Motor is a torque-demand map model with no electrical state (the L = 0 case, motor_dc_spinup_l0, is expressed).
- **therm_lumped_mass** (Lumped thermal mass heated, then cooling): no thermal model (no heat capacity or conductance parts; MOD-09).
- **therm_two_masses** (Two coupled thermal masses: battery cells on a cooled plate): no thermal model (no heat capacity or conductance parts; MOD-09).

### batt_cc_rc: Battery (OCV + R0 + one RC pair) at constant current [as stated]

Expressed as: Battery (pack values, linear OCV table, R0, one RC pair) feeding a Power Consumer whose demand a Lookup sets to I x the terminal voltage of the step before. Solver step 10 ms, 60000 steps, 4.316 s wall (139x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| V | signal | 2.716e-04 at 31.5 s | 8.170e-05 | 7.07e-07 | 3.84e-02 | - | - | yes | 3.53e-07 |
| SOC | signal | 1.388e-07 at 600 s | 1.388e-07 | 1.54e-07 | 9.00e-05 | - | - | yes | 7.72e-08 |
| I | signal | 2.842e-13 at 599.5 s | 1.243e-13 | 2.37e-15 | 1.20e-02 | - | - | yes | 3.55e-15 |
| v1 | signal | 2.799e-04 at 31.5 s | 8.508e-05 | 5.83e-05 | 4.80e-04 | - | - | yes | 2.92e-05 |
| E_chem | energy | 2.160e+01 | - | 7.81e-07 | 2.76e+03 | 2.764800e+07 | 2.764802e+07 | yes | 3.91e-07 |
| E_terminal | energy | 2.825e+01 | - | 1.02e-06 | 2.76e+03 | 2.662848e+07 | 2.662851e+07 | yes | 5.11e-07 |
| E_R0 | energy | - | - | - | 2.76e+03 | 6.912000e+05 | - | not given | - |
| E_R1 | energy | - | - | - | 2.76e+03 | 3.196800e+05 | - | not given | - |
| E_C1 | energy | - | - | - | 2.76e+03 | 8.640000e+03 | - | not given | - |
| E_internal | energy | 5.033e+00 | - | 1.82e-07 | 2.76e+03 | 1.019520e+06 | 1.019515e+06 | yes | 9.10e-08 |
| energy balance | closure | | | 4.18e-15 | 1e-06 | | | yes | |

### batt_cp_rc: Battery (OCV + R0 + one RC pair) at constant power [as stated]

Expressed as: Battery (flat OCV) feeding a Power Consumer at a constant 60 kW. Solver step 10 ms, 12000 steps, 0.734 s wall (163x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| I | signal | 5.357e-03 at 0.1 s | 2.267e-03 | 3.08e-05 | 1.74e-02 | - | - | yes | 1.54e-05 |
| V | signal | 2.734e-03 at 20.8 s | 1.578e-03 | 7.35e-06 | 3.72e-02 | - | - | yes | 3.68e-06 |
| SOC | signal | 6.255e-07 at 120 s | 4.900e-07 | 6.95e-07 | 9.00e-05 | - | - | yes | 3.48e-07 |
| E_chem | energy | 5.196e+01 | - | 6.65e-06 | 7.82e+02 | 7.818932e+06 | 7.818880e+06 | yes | 3.42e-06 |
| E_terminal | energy | 1.263e-06 | - | 1.62e-13 | 7.82e+02 | 7.200000e+06 | 7.200000e+06 | yes | 3.23e-13 |
| E_R0 | energy | - | - | - | 7.82e+02 | 1.764632e+05 | - | not given | - |
| E_R1 | energy | - | - | - | 7.82e+02 | 3.975178e+05 | - | not given | - |
| E_C1 | energy | - | - | - | 7.82e+02 | 4.495059e+04 | - | not given | - |
| E_internal | energy | 5.134e+01 | - | 6.57e-06 | 7.82e+02 | 6.189316e+05 | 6.188802e+05 | yes | 3.28e-06 |
| energy balance | closure | | | 1.61e-13 | 1e-06 | | | yes | |

### batt_voltage_limit: Battery reaching its minimum voltage: constant current, then held at the limit [as stated]

Expressed as: the constant-current set-up with the pack's Min Voltage at V_min: the source-limit handshake cuts the load back to hold it. Solver step 10 ms, 40000 steps, 3.223 s wall (124x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| t_vmin | event | 1.676e-03 | - | 1.50e-05 | 1.22e-03 | 1.115012e+02 | 1.115029e+02 | no | 7.51e-06 |
| V | signal | 3.327e-04 at 112 s | 2.276e-04 | 9.23e-07 | 3.60e-02 | - | - | yes | 4.62e-07 |
| I | signal | 4.902e-03 at 112 s | 1.641e-03 | 4.08e-05 | 1.20e-02 | - | - | yes | 2.04e-05 |
| SOC | signal | 1.063e-06 at 400 s | 7.134e-07 | 2.13e-06 | 5.00e-05 | - | - | yes | 1.06e-06 |
| E_terminal | energy | 8.633e+01 | - | 6.22e-06 | 1.39e+03 | 1.343405e+07 | 1.343414e+07 | yes | 3.11e-06 |
| E_chem | energy | 8.634e+01 | - | 6.22e-06 | 1.39e+03 | 1.387771e+07 | 1.387780e+07 | yes | 3.11e-06 |
| E_R0 | energy | - | - | - | 1.39e+03 | 3.034557e+05 | - | not given | - |
| E_R1 | energy | - | - | - | 1.39e+03 | 1.376800e+05 | - | not given | - |
| E_C1 | energy | - | - | - | 1.39e+03 | 2.523366e+03 | - | not given | - |
| E_internal | energy | 1.479e+00 | - | 1.07e-07 | 1.39e+03 | 4.436591e+05 | 4.436606e+05 | yes | 5.33e-08 |
| energy balance | closure | | | 2.15e-15 | 1e-06 | | | yes | |

Engine messages: warning: Power consumer 'load' cut back at t = 112 s — its source cannot supply it.

### mech_clutch_lockup: Dry clutch engaging two inertias: slip, then lock-up at an exact time [as stated]

Expressed as: E-Motor (J1) spun up for 0.5 s with the Clutch open, then T_drive with the Clutch closed onto a Propeller of zero torque (J2). Solver step 10 ms, 200 steps, 0.023 s wall (88x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| omega1 | signal | 3.040e+00 at 0.61 s | 2.859e-01 | 1.23e-02 | 2.47e-02 | - | - | no | 2.70e-04 |
| omega2 | signal | 7.600e-01 at 0.61 s | 7.147e-02 | 1.03e-02 | 7.40e-03 | - | - | no | 2.25e-04 |
| T_clutch | signal | 1.160e+02 at 0.61 s | 1.358e+01 | 1.16e+00 | 1.00e-02 | - | - | no | 6.67e-03 |
| t_lock | event | 8.882e-16 | - | 1.49e-15 | 1.06e-04 | 5.950000e-01 | 5.950000e-01 | yes | 3.30e-13 |
| E_drive | energy | 1.672e+01 | - | 2.25e-03 | 7.44e-01 | 3.050476e+03 | 3.067200e+03 | no | 1.76e-03 |
| E_clutch | energy | 7.238e-01 | - | 9.73e-05 | 7.44e-01 | 7.440476e+03 | 7.441200e+03 | yes | 9.73e-05 |
| E_kin | energy | 6.400e-04 | - | 8.60e-08 | 7.44e-01 | -4.390000e+03 | -4.389999e+03 | yes | 8.60e-08 |
| energy balance | closure | | | 2.15e-03 | 1e-06 | | | no | |

### mech_gear_change: Gear change: an instantaneous ratio change between a motor and its load [rotational load]

Expressed as: E-Motor (J1) -> Gearbox (12, 7; lossless) -> Propeller of zero torque (J2); the gear signal steps from 1 to 2 at t_shift. Solver step 10 ms, 800 steps, 0.074 s wall (108x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| omega2 | signal | 4.736e+01 at 8 s | 3.351e+01 | 4.34e-01 | 1.09e-02 | - | - | no | 4.34e-01 |
| omega1 | signal | 3.315e+02 at 8 s | 2.346e+02 | 4.10e-01 | 8.08e-02 | - | - | no | 4.10e-01 |
| E_drive | energy | 2.641e+05 | - | 3.22e-01 | 8.21e+01 | 8.210010e+05 | 1.085134e+06 | no | 3.22e-01 |
| E_kin | energy | 8.645e+05 | - | 1.05e+00 | 8.21e+01 | 8.182032e+05 | 1.682680e+06 | no | 1.05e+00 |
| E_shift | energy | - | - | - | 8.21e+01 | 2.797772e+03 | - | not given | - |
| energy balance | closure | | | 1.33e-03 | 1e-06 | | | no | |

Engine messages: Energy books: the stored kinetic energy changed by 1.08623e+06 J; the engine's own speeds say 1.68268e+06 J (E_kin is taken from the speeds)

### mech_gear_change: Gear change: an instantaneous ratio change between a motor and its load [vehicle load]

Expressed as: E-Motor (J1) -> Gearbox -> one wheel (0.3 m, no inertia, stiff tyre) -> Vehicle of mass J2 / r^2. Solver step 1.43 ms, 5600 steps, 0.637 s wall (13x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| omega2 | signal | 8.759e-01 at 8 s | 6.190e-01 | 8.03e-03 | 1.09e-02 | - | - | no | 8.03e-03 |
| omega1 | signal | 4.744e+00 at 4.01 s | 3.382e+00 | 5.87e-03 | 8.08e-02 | - | - | no | 5.86e-03 |
| E_drive | energy | 2.239e+03 | - | 2.73e-03 | 8.21e+01 | 8.210010e+05 | 8.187624e+05 | no | 2.66e-03 |
| E_kin | energy | 1.300e+04 | - | 1.58e-02 | 8.21e+01 | 8.182032e+05 | 8.052050e+05 | no | 1.58e-02 |
| E_shift | energy | - | - | - | 8.21e+01 | 2.797772e+03 | - | not given | - |
| energy balance | closure | | | 3.74e-04 | 1e-06 | | | no | |

Engine messages: Data Check error, run anyway: No Driver follows the Driving Task 'gear' — add a Driver, wire the task to its Target Speed and its Traction Command to the powertrain.; Energy books: the stored kinetic energy changed by 816127 J; the engine's own speeds say 805205 J (E_kin is taken from the speeds); warning: No Driver element — nothing commands the powertrain unless you wire demands yourself.

### mech_inertia_coastdown: Rotating inertia coasting down against viscous and Coulomb friction [as stated]

Expressed as: E-Motor (inertia J) spun up for 1 s, then off: its drag table c w is the viscous friction and a Brake held at T_c the Coulomb friction. Solver step 10 ms, 6100 steps, 0.582 s wall (105x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| omega | signal | 1.833e-02 at 45.8 s | 1.204e-02 | 6.12e-05 | 3.00e-02 | - | - | yes | 3.06e-05 |
| theta | signal | - | - | - | - | - | - | not given | - |
| t_stop | event | 4.537e-03 | - | 9.90e-05 | 5.58e-04 | 4.581454e+01 | 4.581000e+01 | no | 1.01e-05 |
| E_viscous | energy | 2.985e+00 | - | 1.33e-04 | 2.25e+00 | 1.082581e+04 | 1.082880e+04 | no | 1.33e-04 |
| E_coulomb | energy | 6.146e-01 | - | 2.73e-05 | 2.25e+00 | 1.167419e+04 | 1.167480e+04 | yes | 2.73e-05 |
| E_kin | energy | 0 | - | 0 | 2.25e+00 | -2.250000e+04 | -2.250000e+04 | yes | 0 |
| energy balance | closure | | | 1.60e-04 | 1e-06 | | | no | |

### motor_dc_spinup_l0: DC motor spinning up from a voltage step (winding inductance neglected) [as stated]

Expressed as: Voltage Source -> E-Motor whose full-load curve is the DC motor's torque-speed line (k(V - k w)/R - b w) and whose loss map is R i^2 + b w^2, at full demand, on its own rotor inertia J. Solver step 10 ms, 150 steps, 0.013 s wall (116x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| omega | signal | 4.509e+00 at 0.2 s | 2.223e+00 | 9.41e-03 | 4.79e-02 | - | - | no | 4.65e-03 |
| i | signal | 2.341e+01 at 0.01 s | 4.920e+00 | 5.13e-02 | 4.57e-02 | - | - | no | 2.50e-02 |
| t_event | event | 1.158e-02 | - | 2.52e-02 | 1.05e-04 | 4.600570e-01 | 4.484790e-01 | no | 1.25e-02 |
| E_in | energy | 4.939e-01 | - | 1.07e-04 | 4.63e-01 | 4.630799e+03 | 4.631293e+03 | no | 6.06e-05 |
| E_R | energy | - | - | - | 4.63e-01 | 2.306319e+03 | - | not given | - |
| E_kin | energy | 4.458e-01 | - | 9.63e-05 | 4.63e-01 | 2.296875e+03 | 2.297321e+03 | yes | 4.96e-05 |
| E_friction | energy | - | - | - | 4.63e-01 | 2.760472e+01 | - | not given | - |
| energy balance | closure | | | 1.27e-02 | 1e-06 | | | no | |

### veh_coastdown: Car coasting down against air drag and rolling resistance, to a stop [as stated]

Expressed as: a Vehicle body alone, road load A + C v^2 as coefficients A/B/C. Solver step 10 ms, 22000 steps, 0.697 s wall (316x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| v | signal | 1.093e-01 at 193.2 s | 1.090e-02 | 3.65e-03 | 3.00e-03 | - | - | no | 3.66e-03 |
| x | signal | 2.537e-01 at 190.4 s | 1.580e-01 | 1.11e-04 | 2.29e-01 | - | - | no | 1.40e-04 |
| t_event | event | 3.769e-03 | - | 5.75e-05 | 7.56e-04 | 6.555702e+01 | 6.555325e+01 | no | 2.87e-05 |
| t_stop | event | - | - | - | 2.03e-03 | 1.931831e+02 | - | never reached | - |
| E_aero | energy | 1.604e+01 | - | 2.38e-05 | 6.75e+01 | 3.308132e+05 | 3.308292e+05 | yes | 1.31e-05 |
| E_roll | energy | 1.604e+01 | - | 2.38e-05 | 6.75e+01 | 3.441868e+05 | 3.441708e+05 | yes | 1.31e-05 |
| E_kin | energy | 1.513e-07 | - | 2.24e-13 | 6.75e+01 | -6.750000e+05 | -6.750000e+05 | yes | 2.29e-13 |
| energy balance | closure | | | 2.24e-13 | 1e-06 | | | yes | |

Engine messages: Data Check error, run anyway: Vehicle present but no connected wheels — it will not move.; warning: Vehicle present but no connected wheels — it will not move.

### veh_constant_power: Car accelerating at constant power [as stated]

Expressed as: E-Motor with a P / w full-load curve -> 9:1 final drive -> one wheel (stiff tyre) -> Vehicle of mass m without drag. Solver step 1.43 ms, 10500 steps, 1.003 s wall (15x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| v | signal | 2.992e-02 at 15 s | 2.544e-02 | 8.55e-04 | 3.50e-03 | - | - | no | 8.72e-04 |
| x | signal | 3.510e-01 at 15 s | 1.861e-01 | 9.85e-04 | 3.56e-02 | - | - | no | 1.02e-03 |
| t_event | event | 1.935e-02 | - | 2.07e-03 | 1.93e-04 | 9.332562e+00 | 9.351916e+00 | no | 2.12e-03 |
| E_supplied | energy | 1.080e+01 | - | 1.20e-05 | 9.00e+01 | 9.000000e+05 | 9.000108e+05 | yes | 1.20e-05 |
| E_kin | energy | 1.570e+03 | - | 1.74e-03 | 9.00e+01 | 9.000000e+05 | 8.984298e+05 | no | 1.78e-03 |
| energy balance | closure | | | 1.76e-03 | 1e-06 | | | no | |

Engine messages: warning: No Driver element — nothing commands the powertrain unless you wire demands yourself.

## Speed

Median of 5 warm runs (one warm-up run first, untimed), wall clock, one process. *CPU share* is the lowest CPU time / wall time of the runs (well under 1: the run waited for a CPU; above 1: a worker process, such as a Script block's sandbox, ran alongside and its CPU time is counted). Steps are the solver's (master) steps of one run; *Load* is the machine's one-minute load average around the case (other jobs included).

| Case | Kind | Simulated, s | Median wall, s | x real time | Steps | Steps/s | CPU share | Load before -> after | >= 1000x |
|---|---|---|---|---|---|---|---|---|---|
| BEV City | cycle | 600 | 10.645 | 56.4 | 60000 | 5636 | 0.98 | 1.48 -> 1.16 | no |
| BEV WLTC | cycle | 1800 | 32.592 | 55.2 | 180000 | 5523 | 0.98 | 1.16 -> 1.43 | no |
| Hybrid EPA city (UDDS) | cycle | 1369 | 41.057 | 33.3 | 136900 | 3334 | 1.88 | 1.43 -> 3.51 | no |
| Hybrid Mixed | cycle | 600 | 18.691 | 32.1 | 60000 | 3210 | 1.85 | 3.51 -> 3.96 | no |
| FS acceleration 75 m | acceleration | 3.985 | 0.274 | 14.5 | 797 | 2906 | 0.97 | 3.96 -> 3.96 | no |
| FS autocross | lap | 116.856 | 0.768 | 152.2 | 1958 | 2549 | 0.98 | 3.96 -> 3.88 | no |
| FS endurance | lap | 1428.53 | 8.029 | 177.9 | 22517 | 2805 | 0.98 | 3.88 -> 2.67 | no |

Fast mode target (about 1e+06x real time on the WLTC): today's engine has no fast mode for drive cycles; its full run of BEV WLTC is 55.2x, 18,107 times short of it.
