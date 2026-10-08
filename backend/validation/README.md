# LightSim reference suite

Real vehicles with official test results, run through LightSim and
compared automatically (roadmap item VAL-05). The results and what they
mean are in [What is validated](../../docs/VALIDATION-STATUS.md).

## What is in it

- `suite.json`: the suite's version, its cases, the fixed rules that turn a
  case's published facts into a LightSim model, and the sources.
- `cases/*.json`: one small file per vehicle. Each holds the facts in the
  units they were published in, with their source: EPA's test weight,
  road-load coefficients, gearing and test results (repeat tests too), and
  FASTSim's powertrain values. It also holds the drive cycles' SHA-256, the
  tolerance and whether the case is *blind* or *calibrated*.
- `exact.json`: the exact-answer tier, coast-downs that have a closed-form
  answer, held to 0.5 %.
- `suite.py`: builds the models, runs them and reports the gaps.
  `python -m validation.suite`, run from `backend/`, prints the tables.
- `backend/tests/test_reference_suite.py` runs the suite in CI.

## The comparison

EPA publishes the energy a battery electric car takes from the wall socket
to drive the city (UDDS) and highway (HWFET) cycles, as *unadjusted* miles
per gallon equivalent (mpge, 33.705 kWh a gallon). The suite turns each
into Wh per km and compares it with LightSim's energy at the wall: the net
energy at the battery's terminals plus the battery's internal losses,
divided by a charger efficiency of 0.86 (FASTSim's value; real chargers
are about 0.82-0.90, so this alone moves the result by about ±5 %).

Each case also runs:

- a **virtual coast-down**: the car is let go at 130 km/h with no drive and
  no brakes, a road-load curve is fitted to how it slows down, and the
  force must match EPA's target coefficients within 2 % from 20 to
  120 km/h;
- a **step-halving check**: the city cycle at half the solver step (5 ms)
  must give the same energy within 0.5 %.

## The rules (no tuning)

A case is **blind** when no input was tuned to the result it is compared
with. All four cases today are blind:

- **Test mass**: EPA's equivalent test weight, with 1.5 % more for the
  spinning parts (EPA ALPHA's convention, simulated inertia = test weight ×
  1.015), all of it in the four wheels.
- **Road load**: EPA's *target* coefficients A, B and C, which hold the
  driveline's own drag as the car coasts, with *Coefficients Include
  Driveline Losses* ticked so the final drive and differential run
  lossless. EPA also publishes *set* coefficients (target minus what the
  dynamometer saw of the car's own losses); they are stored in each case
  for a later, calibrated tier and not used yet.
- **Gearing**: EPA's N/V ratio (motor speed per mph) and FASTSim's wheel
  radius.
- **Motor**: its power is EPA's Rated Horsepower where the case gives it
  (the Model 3), else FASTSim's figure. FASTSim's efficiency against
  output power, the same curve in
  all four files (84 % at no load, 95 % at 40-60 % power); no loss at zero
  power and no drag torque, so the spin losses the coast-down holds are
  not counted twice. Constant torque up to a third of the maximum speed,
  then constant power; the maximum speed is that of 110 mph.
- **Battery**: a flat 350 V, and an internal resistance that loses 1.5 %
  at a one-hour discharge (FASTSim's 97 % round trip, split each way).
- **Auxiliaries, recuperation, charger**: FASTSim's values.

A calibrated case would say what was tuned, on which other test (for
example the motor losses on the HWFET, compared on the UDDS).

## Adding a vehicle

1. Find it in EPA's Test Car List for its model year and in fueleconomy.gov's
   vehicle data; copy the facts into a new file in `cases/`, with every
   repeat test.
2. Take the powertrain values from an Apache-2.0 FASTSim vehicle file, or
   another source whose licence allows it, and name the file. Read the
   file's comments first: a value it cites from a source the data register
   bans (DATA-REGISTER.md rule 3) is not taken; take EPA's figure instead.
3. Add it to `suite.json`, add a row to `docs/data-register.csv`, run
   `python -m validation.suite` and update `docs/VALIDATION-STATUS.md`.

Never change a case's facts to make it pass. A case whose inputs change gets
a new `version`, and the status page says why.

## Licences

The suite copies individual facts, not files. EPA's pages may be "freely
distributed and used for non-commercial, scientific and educational
purposes" (EPA disclaimers); fueleconomy.gov is U.S. Government data whose
own terms were not re-checked; FASTSim's files are Apache-2.0. None of this
folder ships in the installer. See `docs/DATA-REGISTER.md`.
