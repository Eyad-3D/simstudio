# LightSim against FASTSim, on EPA's reference cars

Measured on 2026-10-08 (LightSim 0.2.0, commit `da599d25c2a7`, reference suite v1.1; FASTSim 3.1.0). Produced by `benchmarks/fastsim/compare.py`; how to repeat it and what the comparison does not show are in [README.md](README.md). The raw numbers are in `results.lightsim.json` and `results.fastsim.json`.

## 1. Energy at the wall against EPA

Same cycle files (LightSim's `udds.csv` and `hwfet.csv`, 1 Hz, flat road), same EPA figure (the *unadjusted* mpge of the car's own EPA test, as Wh/km at the wall socket), same post-processing (net battery chemical energy per km driven, divided by 0.86). **Each tool drives its own vehicle definition**: LightSim the suite's model (EPA test weight and road load, FASTSim's powertrain values), FASTSim the vehicle file the suite's powertrain values came from. Section 3 lists what differs.

| Car | Cycle | EPA, Wh/km | LightSim | Gap | FASTSim | Gap |
|---|---|---|---|---|---|---|
| 2022 Tesla Model 3 RWD | UDDS | 113.0 | 118.6 | +5.0 % | 107.9 | −4.5 % |
|  | HWFET | 123.1 | 132.2 | +7.4 % | 114.7 | −6.8 % |
| 2022 Chevrolet Bolt EUV | UDDS | 117.5 | 118.2 | +0.6 % | 120.7 | +2.7 % |
|  | HWFET | 140.6 | 151.1 | +7.5 % | 147.7 | +5.1 % |
| 2022 Nissan Leaf (40 kWh) | UDDS | 119.3 | 117.6 | −1.4 % | 118.0 | −1.1 % |
|  | HWFET | 148.3 | 154.4 | +4.1 % | 149.2 | +0.6 % |
| 2022 MINI Cooper SE Hardtop 2 door | UDDS | 123.5 | 115.0 | −6.8 % | 118.7 | −3.8 % |
|  | HWFET | 145.9 | 145.9 | 0.0 % | 146.2 | +0.2 % |
| **Mean absolute gap** | | | | **4.1 %** | | **3.1 %** |
| Largest absolute gap | | | | 7.5 % | | 6.8 % |
| Cases within 5 % of EPA / within 8 % | | | | 5 of 8 / 8 of 8 | | 6 of 8 / 8 of 8 |

Battery-terminal energy before the internal losses and the charger, Wh/km (for reference):

| Car | Cycle | LightSim | FASTSim |
|---|---|---|---|
| 2022 Tesla Model 3 RWD | UDDS | 101.1 | 89.8 |
|  | HWFET | 113.1 | 96.8 |
| 2022 Chevrolet Bolt EUV | UDDS | 100.9 | 100.7 |
|  | HWFET | 129.3 | 124.7 |
| 2022 Nissan Leaf (40 kWh) | UDDS | 100.1 | 98.5 |
|  | HWFET | 131.9 | 126.1 |
| 2022 MINI Cooper SE Hardtop 2 door | UDDS | 97.8 | 99.2 |
|  | HWFET | 124.5 | 123.5 |

The gap is (tool − EPA) / EPA. EPA's own repeat tests of one car differ by up to 6 %, and the charger efficiency (0.86, FASTSim's value, used for both) moves every figure of both tools by about ±5 % if it is really 0.82 or 0.90. Differences between the two tools smaller than that are not significant.

### 1b. The same, with FASTSim given LightSim's inputs

Section 1 compares two different vehicle definitions. To see how much of the difference comes from the inputs and how much from the models, FASTSim is run again with LightSim's suite values wherever FASTSim has a place for them (EPA test weight; EPA road load refitted to FASTSim's two terms; no driveline loss; the suite's motor power, battery size and limit, auxiliaries; the suite's battery loss as an efficiency against C-rate; FASTSim's motor efficiency curve is already the suite's). Nothing is tuned to EPA's results. What cannot be aligned is in section 3.

| Car | Cycle | EPA, Wh/km | LightSim | Gap | FASTSim, LightSim's inputs | Gap | LightSim − FASTSim |
|---|---|---|---|---|---|---|---|
| 2022 Tesla Model 3 RWD | UDDS | 113.0 | 118.6 | +5.0 % | 119.7 | +5.9 % | −0.9 % |
|  | HWFET | 123.1 | 132.2 | +7.4 % | 131.3 | +6.6 % | +0.7 % |
| 2022 Chevrolet Bolt EUV | UDDS | 117.5 | 118.2 | +0.6 % | 119.3 | +1.5 % | −0.9 % |
|  | HWFET | 140.6 | 151.1 | +7.5 % | 149.2 | +6.1 % | +1.3 % |
| 2022 Nissan Leaf (40 kWh) | UDDS | 119.3 | 117.6 | −1.4 % | 119.2 | −0.1 % | −1.3 % |
|  | HWFET | 148.3 | 154.4 | +4.1 % | 152.4 | +2.8 % | +1.3 % |
| 2022 MINI Cooper SE Hardtop 2 door | UDDS | 123.5 | 115.0 | −6.8 % | 115.6 | −6.4 % | −0.5 % |
|  | HWFET | 145.9 | 145.9 | 0.0 % | 144.1 | −1.3 % | +1.3 % |
| **Mean absolute gap / difference** | | | | **4.1 %** | | **3.8 %** | **1.0 %** |
| Largest | | | | 7.5 % | | 6.6 % | 1.3 % |

## 2. Run time

Median of 5 warm runs per car and cycle (one untimed warm-up run first), wall-clock, one process at a time, "× real time" = the cycle's length divided by the run time. LightSim: its Python engine, `simulate()` on the prepared project (10 ms solver step, a point recorded every second; building the project, about 3 ms, is outside the timer). FASTSim: its compiled core, constructing the `SimDrive` and `run()` (1 s steps; the vehicle files save history at every step, left on).

| Car | Cycle | Cycle length | LightSim | × real time | FASTSim | × real time | FASTSim is faster by |
|---|---|---|---|---|---|---|---|
| 2022 Tesla Model 3 RWD | UDDS | 1369 s | 24.0 s | 57 | 36.6 ms | 37,422 | 657× |
|  | HWFET | 765 s | 13.2 s | 58 | 31.2 ms | 24,557 | 425× |
| 2022 Chevrolet Bolt EUV | UDDS | 1369 s | 24.9 s | 55 | 33.0 ms | 41,494 | 754× |
|  | HWFET | 765 s | 13.0 s | 59 | 32.4 ms | 23,617 | 400× |
| 2022 Nissan Leaf (40 kWh) | UDDS | 1369 s | 25.0 s | 55 | 34.3 ms | 39,873 | 728× |
|  | HWFET | 765 s | 13.4 s | 57 | 18.7 ms | 41,017 | 720× |
| 2022 MINI Cooper SE Hardtop 2 door | UDDS | 1369 s | 26.0 s | 53 | 32.1 ms | 42,602 | 810× |
|  | HWFET | 765 s | 15.2 s | 50 | 31.3 ms | 24,477 | 486× |
| **Geometric mean** | | | | 55 | | 33,365 | **603×** |

Spread inside a car and cycle (slowest of the 5 runs over the fastest): LightSim up to 1.25×, FASTSim up to 1.81×. The same ratio taken other ways, as a geometric mean over the cases: CPU time instead of wall-clock 599×; the fastest run of the 5 instead of the median 673×.

Machine and load while measuring (the 1-minute load average includes the benchmark itself, which is one busy process):

| | LightSim run | FASTSim run |
|---|---|---|
| Started (UTC) | 2026-10-08 15:39:56 | 2026-10-08 15:55:42 |
| CPUs | 4 | 4 |
| Load average (1, 5, 15 min) at start | 0.52, 1.01, 1.28 | 1.86, 1.55, 1.35 |
| Load average (1, 5, 15 min) at end | 2.02, 1.56, 1.35 | 1.88, 1.57, 1.36 |
| Highest 1-minute load seen between runs | 2.02 | 1.88 |
| Python | 3.11.15 | 3.11.15 |
| Platform | Linux-6.18.44-fc-v80-x86_64-with-glibc2.39 | Linux-6.18.44-fc-v80-x86_64-with-glibc2.39 |

Other agents share these 4 CPUs. The times are not corrected for that: the highest 1-minute load seen between runs was 2.02 (this benchmark is one of the busy processes in it), and a busy neighbour on a shared core makes a time longer, never shorter. The spread above shows how much the repeats moved.

## 3. What differs between the two vehicle definitions

Per car: LightSim's value (the suite's rules applied to EPA's data and FASTSim's powertrain values) against FASTSim's own vehicle file. The road load is the force at constant speed on a flat road; for FASTSim it is rolling coefficient × mass × 9.81 + ½ × 1.2 kg/m³ × Cd × A × v².

Every cell reads LightSim / FASTSim.

| | 2022 Tesla Model 3 RWD | 2022 Chevrolet Bolt EUV | 2022 Nissan Leaf (40 kWh) | 2022 MINI Cooper SE Hardtop 2 door |
|---|---|---|---|---|
| FASTSim's vehicle file | `2022 Tesla Model 3 RWD` | `2017 CHEVROLET Bolt` | `2016 Nissan Leaf 30 kWh` | `2022 MINI Cooper SE Hardtop 2 door` |
| Mass, kg | 1928 / 1752 | 1814 / 1758 | 1758 / 1636 | 1588 / 1588 |
| Road load at 48 km/h, N | 229 / 175 | 231 / 215 | 239 / 222 | 239 / 225 |
| Road load at 97 km/h, N | 408 / 341 | 504 / 482 | 519 / 503 | 476 / 478 |
| Rotating inertia, as extra mass, kg | 29 / 29 | 27 / 29 | 26 / 29 | 24 / 34 |
| Driveline efficiency | 1.00 / 0.98 | 1.00 / 0.98 | 1.00 / 0.98 | 1.00 / 0.98 |
| Motor power, kW | 191.6 / 239.0 | 150.0 / 150.0 | 110.0 / 80.0 | 135.0 / 135.0 |
| Battery, kWh | 54.0 / 54.0 | 60.0 / 60.0 | 40.0 / 30.0 | 32.6 / 32.6 |
| Battery power limit, kW | 201 / 201 | 160 / 160 | 110 / 86 | 135 / 1000 |
| Auxiliaries, kW | 0.25 / 0.25 | 0.25 / 0.25 | 0.25 / 0.25 | 0.20 / 0.20 |

The suite says it took these values from the FASTSim file. Checked against the file on this run:

| Value | Identical in the file | Differs (the suite's own choice, see the table) |
|---|---|---|
| wheel radius | 4 of 4 | none |
| auxiliaries | 4 of 4 | none |
| motor efficiency curve | 4 of 4 | none |
| motor power | 2 of 4 | 2022 Nissan Leaf (40 kWh); 2022 Tesla Model 3 RWD (EPA's rated horsepower) |
| battery size | 3 of 4 | 2022 Nissan Leaf (40 kWh) |
| battery power limit | 2 of 4 | 2022 Nissan Leaf (40 kWh); 2022 MINI Cooper SE Hardtop 2 door |

Differences that remain even in section 1b, because the models differ, not the data:

- **Road load.** LightSim takes EPA's A + B·v + C·v² as it is. FASTSim has only a rolling term and a v² term, so in 1b the pair is fitted to EPA's curve over each cycle's speeds, weighted by speed; the fitted curve's road-load energy over the cycle matches EPA's to 0.000 % but not the shape.
- **Battery.** LightSim: a flat 350 V with a resistance (loss grows with the square of the power). FASTSim's own files: a constant 98.49 % each way (97 % round trip) at any power; in 1b, the suite's loss written as an efficiency against C-rate.
- **Motor.** LightSim has a torque-speed envelope and a loss map built from the suite's efficiency curve; FASTSim has that efficiency curve against the fraction of its rated power, with no speed dependence.
- **Regeneration.** LightSim's driver uses 98 % of the motor's generator torque before the friction brakes and fades recuperation out below about 11 km/h. FASTSim 3.1.0 has no such setting for a battery electric car (the 98 % in the FASTSim 2 files is not read, and its source lists the low-speed fade and the friction/regeneration split as not yet done): it recovers whatever the motor and battery limits allow, down to a stop.
- **Following the cycle.** LightSim's driver is a controller chasing the trace at 10 ms steps; FASTSim solves each 1 s step for the trace speed directly (all runs meet the trace).
- **Charger.** FASTSim 3.1.0 vehicle files carry no charger efficiency; 0.86 (FASTSim's own default, the suite's value) is applied to both tools outside the tools.

## 4. Files and versions

- FASTSim vehicle files, from `cal_and_val/f2-vehicles` of a FASTSim checkout at git `330af9368b3e`:
  - `2022 Tesla Model 3 RWD.yaml`, SHA-256 `9e12a72925adada239cd65ec49c5ffdbe1b04b82dcba0173b31e7c18a62a8c55`
  - `2017 CHEVROLET Bolt.yaml`, SHA-256 `67716d373e60b1c15f068849d01f0e4c3f06d55579d215d517cd97a40642cc98`
  - `2016 Nissan Leaf 30 kWh.yaml`, SHA-256 `a2d505aefe259d50cc8e4b2798c54f76b5d94cf0fdba610ebec620b1bae322ed`
  - `2022 MINI Cooper SE Hardtop 2 door.yaml`, SHA-256 `b85616e679fe3f8f2952c13d8c5b7bbfab7fed6e18a5bc699bc9ca6c85ec9167`
- LightSim timing note: simulate(project, 'case') only; the project is built outside the timer; solver step 10 ms, recorded step 1 s.
- FASTSim timing note: SimDrive(vehicle, cycle) and .run(); the vehicle's own history saving (every step) stays on.
