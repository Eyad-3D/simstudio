# LightSim against FASTSim

A benchmark, not part of the app. It runs LightSim's reference suite
(`backend/validation/`: four EPA-tested electric cars on the UDDS city and
HWFET highway cycles) next to
[FASTSim](https://github.com/NatLabRockies/fastsim), the open-source vehicle
simulator of the U.S. National Laboratory of the Rockies (formerly NREL,
Apache-2.0), and compares:

1. **Accuracy**: each tool's energy at the wall against EPA's published
   figure, and its gap in %.
2. **Speed**: the median of 5 warm runs per car and cycle, as a multiple of
   real time.

The latest numbers, with their date, are in [results.md](results.md), the raw
numbers in `results.lightsim.json` and `results.fastsim.json`.

FASTSim is **never** a LightSim dependency: it is not in any requirements
file, `compare.py` is not imported by LightSim's tests (it lives outside
`backend/`, and its name does not start with `test_`), and nothing here ships
in the installer. FASTSim runs only in its own virtualenv.

## Set-up

You need three things. Replace `$WORK` with any folder outside the repository.

1. **LightSim's Python environment**, as for the tests
   (`pip install -r backend/requirements.txt`). It must not contain `fastsim`.

2. **A FASTSim 3.1.0 virtualenv.** The only package to install is `fastsim`
   (its own dependencies come with it):

   ```bash
   python3 -m venv $WORK/fastsim-venv
   $WORK/fastsim-venv/bin/pip install fastsim==3.1.0
   ```

3. **FASTSim's vehicle files for the four cars.** The 3.1.0 wheel holds only
   nine vehicles (`fastsim.Vehicle.list_resources()`), none of them in the form
   the suite used: its Model 3, 2020 Bolt EV and 2016 Leaf 30 kWh are
   `... thrml.yaml` files that add a cabin, air-conditioning and battery heat
   model, and it has no MINI and no 2017 Bolt. The suite took its powertrain
   values from the files in FASTSim's `cal_and_val/f2-vehicles` folder, so
   get them from the source repository:

   ```bash
   git clone https://github.com/NatLabRockies/fastsim $WORK/fastsim
   ```

   The files used are `2022 Tesla Model 3 RWD.yaml`, `2017 CHEVROLET
   Bolt.yaml`, `2016 Nissan Leaf 30 kWh.yaml` and `2022 MINI Cooper SE
   Hardtop 2 door.yaml`. `backend/validation/suite.json` names commit
   `6a07d4d` of 28 September 2026. `results.md` lists the commit that was
   actually used and the SHA-256 of each file, and `compare.py` checks the
   values the suite says it took from the files (wheel radius, auxiliaries,
   motor efficiency curve, battery size and limit, motor power) and reports
   which are identical. `--vehicles` takes the `f2-vehicles` folder or the
   `f3-vehicles` folder next to it (FASTSim 3.1.0 reads both; for these four
   cars they give identical results, checked to the last digit on the UDDS).

## Run

From the repository root. The three steps only exchange JSON files, so the two
Python environments never meet:

```bash
cd backend
<LightSim python> ../benchmarks/fastsim/compare.py lightsim --out ../benchmarks/fastsim/results.lightsim.json
cd ..
$WORK/fastsim-venv/bin/python benchmarks/fastsim/compare.py fastsim \
    --vehicles $WORK/fastsim/cal_and_val/f2-vehicles \
    --out benchmarks/fastsim/results.fastsim.json
python3 benchmarks/fastsim/compare.py report \
    benchmarks/fastsim/results.lightsim.json benchmarks/fastsim/results.fastsim.json \
    --out benchmarks/fastsim/results.md
```

or in one call:

```bash
python3 benchmarks/fastsim/compare.py all \
    --lightsim-python <LightSim python> \
    --fastsim-python $WORK/fastsim-venv/bin/python \
    --vehicles $WORK/fastsim/cal_and_val/f2-vehicles
```

The LightSim step takes about 15 minutes (it runs every case six times: one
warm-up that also gives the result, then five timed runs, at about 25 s for
the UDDS and 13 s for the HWFET); the FASTSim step takes seconds. Options
for both steps: `--runs N` (timed runs, default 5) and `--cars tesla mini`
(only cases whose id contains one of the words). Use `--runs 1 --cars mini`
to try it out. The LightSim step is single-threaded.

## What is compared, and how

**Same inputs.** The cycle: LightSim's own `udds.csv` and `hwfet.csv` (SHA-256
checked against the suite's cases), expanded to 1 Hz by the same straight-line
interpolation LightSim's Driving Task uses, and given to FASTSim as a flat-road
cycle. (FASTSim's own bundled `udds.csv`/`hwfet.csv` differ from these by at
most about 0.002 km/h at any second, which is rounding.) The target: the EPA
*unadjusted* mpge of each car's own EPA test, turned into Wh/km at the wall
socket by the suite's conversion (33,705 Wh per gallon equivalent). There is no
EPA 0.7 label factor because the figures are unadjusted.

**Same post-processing.** The suite's, applied to both: energy at the wall =
(net energy out of the battery's chemistry, regeneration subtracted, which is
the terminal energy plus the battery's own internal losses and includes the
auxiliaries) per km actually driven, divided by a charger efficiency of 0.86.
For LightSim that is `validation.suite.run_cycle`, unchanged. For FASTSim it is
`energy_out_chemical_joules` of its battery state divided by its achieved
distance. FASTSim 3.1.0 vehicle files carry no charger efficiency, so its raw
result is battery energy only; the 0.86 (FASTSim's own default, and the
suite's value) is applied outside the tool, the same as for LightSim.

**Different vehicle definitions (the main comparison).** Each tool runs *its
own* model of the car. LightSim: the suite's model, which takes mass, road load
and gearing from EPA's test data and the powertrain values from the FASTSim
file. FASTSim: that file as it is. They differ in mass, road load, motor power
(Model 3: FASTSim 239 kW, the suite EPA's 257 hp = 192 kW), battery size and
limit (Leaf: FASTSim's file is the 2016 30 kWh car, the suite the 2022 40 kWh
car), driveline efficiency and the battery loss model; `results.md` section 3
lists them per car. FASTSim has no file for the 2022 Bolt EUV or the 40 kWh
Leaf, so for those two cars its definition is a related earlier model (2017
Bolt EV, 2016 Leaf 30 kWh) compared with the EPA figure of the 2022 car.

**Same inputs, as far as the models allow (section 1b of `results.md`).** To
separate "different data" from "different models", FASTSim is also run with
LightSim's values wherever it has a place for them: EPA test weight, the 1.5 %
rotating inertia put in the wheels, EPA's road load refitted to FASTSim's two
terms (rolling and v², least squares over the cycle's speeds weighted by speed,
so the cycle's road-load energy matches EPA's), no driveline loss, the suite's
motor power, battery size, battery limit and auxiliaries, and the suite's
battery loss (1.5 % of the power at 1C) as FASTSim's efficiency against C-rate.
Nothing is tuned to EPA's results. What stays different is in `results.md`
section 3 (road-load shape, battery and motor model, regeneration policy, how
the cycle is followed, step size).

**Speed.** Per car and cycle: one warm-up run, then 5 timed runs, median wall
clock (`time.perf_counter`; CPU time is also recorded). "x real time" is the
cycle's length (1,369 s UDDS, 765 s HWFET) divided by that time. LightSim: its
Python engine, `simulate(project, "case")`, with the project built outside the
timer (about 3 ms) and the engine's normal behaviour otherwise (10 ms solver
step, a point recorded every second, all channels and summaries produced).
FASTSim: its compiled core, constructing the `SimDrive` and calling `run()`
(1 s steps, with the history the vehicle files ask to be saved at every step).
The two do not do the same amount of work per simulated second: LightSim
integrates 100 solver steps per second with controllers and every
channel; FASTSim solves one quasi-static step per second. The ratio says what
a user waits for, not what is "better engineering".

## Caveats

- The two sides are **not** the same vehicle. A difference in section 1 may come
  from the data (mass, road load, motor) as much as from the models; section 1b
  removes most of the data difference, not all of it.
- The numbers are small samples: four cars of one class, two gentle cycles.
  EPA's own repeat tests of one car differ by up to 6 %, and the charger
  efficiency alone (0.82-0.90 instead of 0.86) moves every figure of both tools
  by about 5 %. A difference of 1 percentage point in the mean gap between the
  tools is inside that noise.
- LightSim's suite is *blind* by construction (no input tuned to the results),
  and its road load is EPA's own test data. FASTSim's vehicle files do not say
  how their mass and road-load values were obtained; the MINI file's mass is
  exactly EPA's 3,500 lb test weight and its drag coefficient is not a round
  number (0.32618...), which suggests parts of it were derived from EPA data. I
  could not check this, so FASTSim's gaps here may not be blind either.
- The charger efficiency is not tool-specific and not measured: both tools
  use 0.86.
- Run times depend on the load. The machine is shared (4 CPUs), `results.md`
  records the load average before and after every group of runs, and the times
  are not corrected for it. The ratio between the tools is hundreds of times, far
  larger than any load effect.
- FASTSim 3.1.0's Python package does not give a charger-inclusive "kWh/mile"
  figure of the kind FASTSim 2 printed; the post-processing above replaces it.
- Both tools are run at their defaults otherwise: FASTSim with
  `SimParams.default()` and its constant air density of 1.2 kg/m3 (LightSim's
  suite uses EPA's coefficients A, B and C, which include the air).
- FASTSim's bundled `2022 Tesla Model 3 RWD thrml.yaml`, with its cabin and
  air-conditioning model, was not used. For the record it gives 109.5 Wh/km on
  the UDDS against 107.9 for the non-thermal file used here.
