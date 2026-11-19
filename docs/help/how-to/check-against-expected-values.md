# Check a result against a number you trust

You often know roughly what a result should be: a maker's 0-100 km/h
time, last year's measured 75 m time, or your own hand calculation. Type
that number into the case as an *expected value*, and every run of the
case shows how far its result lands from it.

## Add an expected value

1. Run the case once, so LightSim knows which results it gives.
2. On the **Home** tab, open the *Cases* tab on the right and pick the case.
3. Under *Expected values*, click **Add**. A row appears with the case's
   first result and its value.
4. In the first field, pick the result to check from the list, for example
   *Consumption*. The list holds the summary values of the case's last run.
5. Type the number you expect in **Expected**, for example `11`.
6. Type the tolerance after **±**, and choose whether it is in **%** of the
   expected value or **in its unit**.
7. In the last field, say where the number comes from, for example
   *maker's figure* or *FSG 2025 best time*.
8. Press **Run**.

The expected values are saved with the case, in the project file.

## Read the gap

Above the chart on the *Results* page, a line shows each expected value:
the run's value, the expected value and the gap, with a coloured word in
front:

- **within** (green): the gap is at most the tolerance,
- **near** (amber): it is at most twice the tolerance,
- **outside** (red): it is more than that,
- **no value**: the run has no result of that name (check the spelling),
- **not valid**: the run itself rules the value out, for example because
  the car did not follow its cycle.

On the Battery Electric Car's *City Cycle*, an expected *Consumption* of
11 kWh/100 km ± 5 % reads **within**, +0.12 kWh/100km (+1.09 %).
*Run info* lists the same lines with the run, and a parameter sweep's
table in *Saved studies* gets an *Expected* column with the gap of each
point when you pick a result that has an expected value.

## Hand calculations

Every run also gets two checks LightSim works out by itself, marked
*hand calculation*. Under the line of expected values, *Hand
calculations: 2 passed* folds them away while they pass:

- **Top speed allowed by the E-Motor**: the car's highest speed must not be
  above the motor's *Maximum Speed* ÷ the overall gear ratio × the wheel
  radius. On the Battery Electric Car that is 16,000 1/min ÷ 12.8 ×
  0.3488 m = 164.4 km/h. A motor that is driven past its maximum speed,
  for example downhill with no brakes, fails it.
- **Energy from the sources**: the energy the batteries, voltage sources
  and fuel cells gave must be at least what the car needed to speed up,
  climb and push through the road load, summed over the run's recorded
  speed. Every other loss only adds to it, so a run below it got energy
  from nowhere. Runs with a combustion engine skip this check.

The checks allow 2 % (top speed) and 3 % (energy) for the 1 s recording
step. They catch gross mistakes; they do not show that a result is right.
See [Known issues](../../KNOWN-LIMITS.md) and
[Validation](../../VALIDATION-STATUS.md) for that.
