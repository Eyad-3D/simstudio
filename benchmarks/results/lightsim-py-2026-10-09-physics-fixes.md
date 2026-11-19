# LightSim yardstick: lightsim-py (2026-10-09)

Engine **lightsim-py** 0.2.0 (its code as of commit `2bad2e4`; benchmarks run from `2bad2e4`), default solver step 10 ms. Machine: Intel(R) Xeon(R) Processor @ 2.80GHz, 4 CPUs, load average at the start 2.48, Python 3.11.15.

Targets (benchmarks/targets.toml): a signal within 0.0001 of its scale, an event within 0.1 ms + 1e-05 of its time, an energy term within 0.0001 and the energy balance within 1e-06 of the energy scale; at least 1000x real time in full dynamic simulation and about 1e+06x on the WLTC in a fast mode.

## Reference problems

Errors are shares of each quantity's scale (see benchmarks/README.md). *Order* is the observed order of convergence of the worst signal error between the default step and half of it: about 1 for a first-order integrator, near 0 when the error comes from how the model is built rather than from the step.

| Problem | Expressed as | Worst signal | Worst event | Worst energy | Energy balance | Target | Wall, s | x real time | Steps | Order |
|---|---|---|---|---|---|---|---|---|---|---|
| batt_cc_rc | as stated | 5.83e-05 | - | 1.02e-06 | 4.18e-15 | pass | 4.14 | 145 | 60000 | 1.00 |
| batt_cp_rc | as stated | 3.08e-05 | - | 6.57e-06 | 1.61e-13 | pass | 0.69 | 173 | 12000 | 1.00 |
| batt_voltage_limit | as stated | 4.08e-05 | 1.50e-05 | 6.33e-06 | 2.15e-15 | miss | 3.79 | 105 | 40000 | 1.00 |
| elec_rc_step | cannot be expressed | | | | | | | | | |
| elec_rl_step | cannot be expressed | | | | | | | | | |
| mech_clutch_lockup | as stated | 4.40e-01 | 7.64e-03 | 1.13e-05 | 1.10e-15 | miss | 0.03 | 79 | 200 | 3.46 |
| mech_gear_change | rotational load | 6.64e-15 | - | 1.33e-14 | 1.19e-14 | pass | 0.10 | 77 | 800 | -1.11 |
| mech_gear_change | vehicle load | 4.56e-03 | - | 3.37e-03 | 8.64e-16 | miss | 0.61 | 13 | 5600 | -0.07 |
| mech_inertia_coastdown | as stated | 6.12e-05 | 9.90e-05 | 5.19e-05 | 3.88e-15 | miss | 0.58 | 105 | 6100 | 1.00 |
| motor_dc_spinup | cannot be expressed | | | | | | | | | |
| motor_dc_spinup_l0 | as stated | 5.13e-02 | 2.52e-02 | 1.07e-04 | 2.95e-16 | miss | 0.02 | 96 | 150 | 1.04 |
| therm_lumped_mass | cannot be expressed | | | | | | | | | |
| therm_two_masses | cannot be expressed | | | | | | | | | |
| veh_coastdown | as stated | 4.66e-05 | 5.75e-05 | 2.37e-05 | 3.45e-15 | miss | 0.68 | 322 | 22000 | 1.00 |
| veh_constant_power | as stated | 1.05e-03 | 2.07e-03 | 1.74e-03 | 1.85e-03 | miss | 1.14 | 13 | 10500 | -0.20 |

Today's engine cannot express:

- **elec_rc_step** (RC circuit: DC-link precharge from a voltage step): no resistor or capacitor part; the battery's RC pair cannot be charged from a voltage source (an electrical bus has exactly one source).
- **elec_rl_step** (RL circuit: contactor coil energised from a voltage step): no inductor or resistor part.
- **motor_dc_spinup** (DC motor spinning up from a voltage step (with winding inductance)): no winding inductance: the E-Motor is a torque-demand map model with no electrical state (the L = 0 case, motor_dc_spinup_l0, is expressed).
- **therm_lumped_mass** (Lumped thermal mass heated, then cooling): no thermal model (no heat capacity or conductance parts; MOD-09).
- **therm_two_masses** (Two coupled thermal masses: battery cells on a cooled plate): no thermal model (no heat capacity or conductance parts; MOD-09).

### batt_cc_rc: Battery (OCV + R0 + one RC pair) at constant current [as stated]

Expressed as: Battery (pack values, linear OCV table, R0, one RC pair) feeding a Power Consumer whose demand a Lookup sets to I x the terminal voltage of the step before. Solver step 10 ms, 60000 steps, 4.144 s wall (145x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| V | signal | 2.716e-04 at 31.5 s | 8.170e-05 | 7.07e-07 | 3.84e-02 | - | - | yes | 3.53e-07 |
| SOC | signal | 1.388e-07 at 600 s | 1.388e-07 | 1.54e-07 | 9.00e-05 | - | - | yes | 7.72e-08 |
| I | signal | 2.842e-13 at 599.5 s | 1.243e-13 | 2.37e-15 | 1.20e-02 | - | - | yes | 3.55e-15 |
| v1 | signal | 2.799e-04 at 31.5 s | 8.508e-05 | 5.83e-05 | 4.80e-04 | - | - | yes | 2.92e-05 |
| E_chem | energy | 2.322e+01 | - | 8.40e-07 | 2.76e+03 | 2.764800e+07 | 2.764802e+07 | yes | 4.20e-07 |
| E_terminal | energy | 2.825e+01 | - | 1.02e-06 | 2.76e+03 | 2.662848e+07 | 2.662851e+07 | yes | 5.11e-07 |
| E_R0 | energy | - | - | - | 2.76e+03 | 6.912000e+05 | - | not given | - |
| E_R1 | energy | - | - | - | 2.76e+03 | 3.196800e+05 | - | not given | - |
| E_C1 | energy | - | - | - | 2.76e+03 | 8.640000e+03 | - | not given | - |
| E_internal | energy | 5.033e+00 | - | 1.82e-07 | 2.76e+03 | 1.019520e+06 | 1.019515e+06 | yes | 9.10e-08 |
| energy balance | closure | | | 4.18e-15 | 1e-06 | | | yes | |

### batt_cp_rc: Battery (OCV + R0 + one RC pair) at constant power [as stated]

Expressed as: Battery (flat OCV) feeding a Power Consumer at a constant 60 kW. Solver step 10 ms, 12000 steps, 0.692 s wall (173x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| I | signal | 5.357e-03 at 0.1 s | 2.267e-03 | 3.08e-05 | 1.74e-02 | - | - | yes | 1.54e-05 |
| V | signal | 2.734e-03 at 20.8 s | 1.578e-03 | 7.35e-06 | 3.72e-02 | - | - | yes | 3.68e-06 |
| SOC | signal | 6.255e-07 at 120 s | 4.900e-07 | 6.95e-07 | 9.00e-05 | - | - | yes | 3.48e-07 |
| E_chem | energy | 5.134e+01 | - | 6.57e-06 | 7.82e+02 | 7.818932e+06 | 7.818880e+06 | yes | 3.28e-06 |
| E_terminal | energy | 1.263e-06 | - | 1.62e-13 | 7.82e+02 | 7.200000e+06 | 7.200000e+06 | yes | 3.23e-13 |
| E_R0 | energy | - | - | - | 7.82e+02 | 1.764632e+05 | - | not given | - |
| E_R1 | energy | - | - | - | 7.82e+02 | 3.975178e+05 | - | not given | - |
| E_C1 | energy | - | - | - | 7.82e+02 | 4.495059e+04 | - | not given | - |
| E_internal | energy | 5.134e+01 | - | 6.57e-06 | 7.82e+02 | 6.189316e+05 | 6.188802e+05 | yes | 3.28e-06 |
| energy balance | closure | | | 1.61e-13 | 1e-06 | | | yes | |

### batt_voltage_limit: Battery reaching its minimum voltage: constant current, then held at the limit [as stated]

Expressed as: the constant-current set-up with the pack's Min Voltage at V_min: the source-limit handshake cuts the load back to hold it. Solver step 10 ms, 40000 steps, 3.794 s wall (105x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| t_vmin | event | 1.676e-03 | - | 1.50e-05 | 1.22e-03 | 1.115012e+02 | 1.115029e+02 | no | 7.51e-06 |
| V | signal | 3.327e-04 at 112 s | 2.276e-04 | 9.23e-07 | 3.60e-02 | - | - | yes | 4.62e-07 |
| I | signal | 4.902e-03 at 112 s | 1.641e-03 | 4.08e-05 | 1.20e-02 | - | - | yes | 2.04e-05 |
| SOC | signal | 1.063e-06 at 400 s | 7.134e-07 | 2.13e-06 | 5.00e-05 | - | - | yes | 1.06e-06 |
| E_terminal | energy | 8.633e+01 | - | 6.22e-06 | 1.39e+03 | 1.343405e+07 | 1.343414e+07 | yes | 3.11e-06 |
| E_chem | energy | 8.781e+01 | - | 6.33e-06 | 1.39e+03 | 1.387771e+07 | 1.387780e+07 | yes | 3.16e-06 |
| E_R0 | energy | - | - | - | 1.39e+03 | 3.034557e+05 | - | not given | - |
| E_R1 | energy | - | - | - | 1.39e+03 | 1.376800e+05 | - | not given | - |
| E_C1 | energy | - | - | - | 1.39e+03 | 2.523366e+03 | - | not given | - |
| E_internal | energy | 1.479e+00 | - | 1.07e-07 | 1.39e+03 | 4.436591e+05 | 4.436606e+05 | yes | 5.33e-08 |
| energy balance | closure | | | 2.15e-15 | 1e-06 | | | yes | |

Engine messages: warning: Power consumer 'load' cut back at t = 112 s — its source cannot supply it.

### mech_clutch_lockup: Dry clutch engaging two inertias: slip, then lock-up at an exact time [as stated]

Expressed as: E-Motor (J1) spun up for 0.5 s with the Clutch open, then T_drive with the Clutch closed onto a Propeller of zero torque (J2). Solver step 10 ms, 200 steps, 0.025 s wall (79x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| omega1 | signal | 4.405e-13 at 0.42 s | 2.171e-13 | 1.78e-15 | 2.47e-02 | - | - | yes | 2.65e-15 |
| omega2 | signal | 2.274e-13 at 1.5 s | 8.757e-14 | 3.07e-15 | 7.40e-03 | - | - | yes | 2.30e-15 |
| T_clutch | signal | 4.400e+01 at 0.6 s | 3.593e+00 | 4.40e-01 | 1.00e-02 | - | - | no | 4.00e-02 |
| t_lock | event | 4.545e-03 | - | 7.64e-03 | 1.06e-04 | 5.950000e-01 | 5.995455e-01 | no | 5.50e-14 |
| E_drive | energy | 8.381e-02 | - | 1.13e-05 | 7.44e-01 | 3.050476e+03 | 3.050560e+03 | yes | 5.12e-07 |
| E_clutch | energy | 8.381e-02 | - | 1.13e-05 | 7.44e-01 | 7.440476e+03 | 7.440560e+03 | yes | 5.12e-07 |
| E_kin | energy | 2.183e-11 | - | 2.93e-15 | 7.44e-01 | -4.390000e+03 | -4.390000e+03 | yes | 2.20e-15 |
| energy balance | closure | | | 1.10e-15 | 1e-06 | | | yes | |

### mech_gear_change: Gear change: an instantaneous ratio change between a motor and its load [rotational load]

Expressed as: E-Motor (J1) -> Gearbox (12, 7; lossless) -> Propeller of zero torque (J2); the gear signal steps from 1 to 2 at t_shift. Solver step 10 ms, 800 steps, 0.104 s wall (77x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| omega2 | signal | 7.248e-13 at 8 s | 2.695e-13 | 6.64e-15 | 1.09e-02 | - | - | yes | 8.86e-15 |
| omega1 | signal | 4.889e-12 at 8 s | 2.109e-12 | 6.05e-15 | 8.08e-02 | - | - | yes | 1.43e-14 |
| E_drive | energy | 1.048e-09 | - | 1.28e-15 | 8.21e+01 | 8.210010e+05 | 8.210010e+05 | yes | 5.96e-15 |
| E_kin | energy | 1.094e-08 | - | 1.33e-14 | 8.21e+01 | 8.182032e+05 | 8.182032e+05 | yes | 2.98e-15 |
| E_shift | energy | 5.866e-11 | - | 7.15e-17 | 8.21e+01 | 2.797772e+03 | 2.797772e+03 | yes | 2.13e-16 |
| energy balance | closure | | | 1.19e-14 | 1e-06 | | | yes | |

### mech_gear_change: Gear change: an instantaneous ratio change between a motor and its load [vehicle load]

Expressed as: E-Motor (J1) -> Gearbox -> one wheel (0.3 m, no inertia, stiff tyre) -> Vehicle of mass J2 / r^2. Solver step 1.43 ms, 5600 steps, 0.613 s wall (13x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| omega2 | signal | 1.636e-02 at 3.99 s | 1.033e-02 | 1.50e-04 | 1.09e-02 | - | - | no | 1.51e-04 |
| omega1 | signal | 3.682e+00 at 3.99 s | 1.918e+00 | 4.56e-03 | 8.08e-02 | - | - | no | 4.60e-03 |
| E_drive | energy | 2.770e+03 | - | 3.37e-03 | 8.21e+01 | 8.210010e+05 | 8.237706e+05 | no | 3.42e-03 |
| E_kin | energy | 1.006e+02 | - | 1.22e-04 | 8.21e+01 | 8.182032e+05 | 8.181026e+05 | no | 1.24e-04 |
| E_shift | energy | - | - | - | 8.21e+01 | 2.797772e+03 | - | not given | - |
| energy balance | closure | | | 8.64e-16 | 1e-06 | | | yes | |

### mech_inertia_coastdown: Rotating inertia coasting down against viscous and Coulomb friction [as stated]

Expressed as: E-Motor (inertia J) spun up for 1 s, then off: its drag table c w is the viscous friction and a Brake held at T_c the Coulomb friction. Solver step 10 ms, 6100 steps, 0.583 s wall (105x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| omega | signal | 1.833e-02 at 45.8 s | 1.204e-02 | 6.12e-05 | 3.00e-02 | - | - | yes | 3.06e-05 |
| theta | signal | - | - | - | - | - | - | not given | - |
| t_stop | event | 4.537e-03 | - | 9.90e-05 | 5.58e-04 | 4.581454e+01 | 4.581000e+01 | no | 1.01e-05 |
| E_viscous | energy | 1.167e+00 | - | 5.19e-05 | 2.25e+00 | 1.082581e+04 | 1.082698e+04 | yes | 2.59e-05 |
| E_coulomb | energy | 1.167e+00 | - | 5.19e-05 | 2.25e+00 | 1.167419e+04 | 1.167302e+04 | yes | 2.59e-05 |
| E_kin | energy | 0 | - | 0 | 2.25e+00 | -2.250000e+04 | -2.250000e+04 | yes | 0 |
| energy balance | closure | | | 3.88e-15 | 1e-06 | | | yes | |

### motor_dc_spinup_l0: DC motor spinning up from a voltage step (winding inductance neglected) [as stated]

Expressed as: Voltage Source -> E-Motor whose full-load curve is the DC motor's torque-speed line (k(V - k w)/R - b w) and whose loss map is R i^2 + b w^2, at full demand, on its own rotor inertia J. Solver step 10 ms, 150 steps, 0.016 s wall (96x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| omega | signal | 4.509e+00 at 0.2 s | 2.223e+00 | 9.41e-03 | 4.79e-02 | - | - | no | 4.65e-03 |
| i | signal | 2.341e+01 at 0.01 s | 4.920e+00 | 5.13e-02 | 4.57e-02 | - | - | no | 2.50e-02 |
| t_event | event | 1.158e-02 | - | 2.52e-02 | 1.05e-04 | 4.600570e-01 | 4.484790e-01 | no | 1.25e-02 |
| E_in | energy | 4.939e-01 | - | 1.07e-04 | 4.63e-01 | 4.630799e+03 | 4.631293e+03 | no | 6.06e-05 |
| E_R | energy | - | - | - | 4.63e-01 | 2.306319e+03 | - | not given | - |
| E_kin | energy | 4.458e-01 | - | 9.63e-05 | 4.63e-01 | 2.296875e+03 | 2.297321e+03 | yes | 4.96e-05 |
| E_friction | energy | - | - | - | 4.63e-01 | 2.760472e+01 | - | not given | - |
| energy balance | closure | | | 2.95e-16 | 1e-06 | | | yes | |

### veh_coastdown: Car coasting down against air drag and rolling resistance, to a stop [as stated]

Expressed as: a Vehicle body alone, road load A + C v^2 as coefficients A/B/C. Solver step 10 ms, 22000 steps, 0.684 s wall (322x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| v | signal | 6.254e-04 at 103.3 s | 5.328e-04 | 2.09e-05 | 3.00e-03 | - | - | yes | 1.04e-05 |
| x | signal | 1.068e-01 at 220 s | 6.628e-02 | 4.66e-05 | 2.29e-01 | - | - | yes | 2.33e-05 |
| t_event | event | 3.769e-03 | - | 5.75e-05 | 7.56e-04 | 6.555702e+01 | 6.555325e+01 | no | 2.87e-05 |
| t_stop | event | 3.057e-03 | - | 1.58e-05 | 2.03e-03 | 1.931831e+02 | 1.931800e+02 | no | 1.58e-05 |
| E_aero | energy | 1.602e+01 | - | 2.37e-05 | 6.75e+01 | 3.308132e+05 | 3.308292e+05 | yes | 1.19e-05 |
| E_roll | energy | 1.602e+01 | - | 2.37e-05 | 6.75e+01 | 3.441868e+05 | 3.441708e+05 | yes | 1.19e-05 |
| E_kin | energy | 0 | - | 0 | 6.75e+01 | -6.750000e+05 | -6.750000e+05 | yes | 0 |
| energy balance | closure | | | 3.45e-15 | 1e-06 | | | yes | |

### veh_constant_power: Car accelerating at constant power [as stated]

Expressed as: E-Motor with a P / w full-load curve -> 9:1 final drive -> one wheel (stiff tyre) -> Vehicle of mass m without drag. Solver step 1.43 ms, 10500 steps, 1.139 s wall (13x real time).

| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance | Exact | Engine | Pass | Half step: share |
|---|---|---|---|---|---|---|---|---|---|
| v | signal | 2.992e-02 at 15 s | 2.544e-02 | 8.55e-04 | 3.50e-03 | - | - | no | 8.72e-04 |
| x | signal | 3.724e-01 at 15 s | 2.002e-01 | 1.05e-03 | 3.56e-02 | - | - | no | 1.07e-03 |
| t_event | event | 1.935e-02 | - | 2.07e-03 | 1.93e-04 | 9.332562e+00 | 9.351916e+00 | no | 2.12e-03 |
| E_supplied | energy | 9.277e+01 | - | 1.03e-04 | 9.00e+01 | 9.000000e+05 | 9.000928e+05 | no | 9.15e-05 |
| E_kin | energy | 1.570e+03 | - | 1.74e-03 | 9.00e+01 | 9.000000e+05 | 8.984298e+05 | no | 1.78e-03 |
| energy balance | closure | | | 1.85e-03 | 1e-06 | | | no | |
