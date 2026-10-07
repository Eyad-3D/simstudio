# Run the standard vehicle tests

The standard figures a vehicle engineer is asked for, from your model as it
is, without building special cycles. The tests add runs of their own; they
do not change the model or its cases, and their runs are not stored.

1. Open the **Simulations** tab of the ribbon.
2. Click **Vehicle tests** (group *Standard figures*).
3. Untick the tests you do not want. The coast-down and the steepest grade
   take the longest (several short runs each).
4. Click **Run the tests**. The table lists each figure, how it was worked
   out and, where it applies, what limits it.

## What each test does

| Test | How |
|---|---|
| 0-100 km/h | Full throttle from standstill (a *Performance* case); the time where the speed crosses 100 km/h. |
| 80-120 km/h | The same, from 80 km/h (the Vehicle's *Initial Speed*) to 120 km/h. |
| Top speed | Full throttle towards 300 km/h for 120 s; the highest speed, and what holds it there: a motor's maximum speed, an engine's rev limit, or the power (the road load takes all the drive gives). |
| Consumption and range at 50, 90 and 120 km/h | 600 s at a constant speed, starting at it. For an electric car also the range: the batteries' *Usable Capacity* above their *Minimum SOC* ÷ that consumption. |
| Steepest grade at 30 km/h | The steepest constant grade on which the car stays within 1 km/h of 30 km/h over the last 20 s of a 40 s run, found to 0.5 % by halving the interval. It uses the model's own Road Profile if one drives the Vehicle's grade. |
| Virtual coast-down | From 130 km/h with no pedal, the deceleration between 125 and 15 km/h, times the Vehicle's mass plus the wheels' rotating inertia, fitted to F = A + B·v + C·v² (v in km/h). Motor and gear drag count, as on a real coast-down. Compare the result with the coefficients on a car's certificate. |

## Good to know

- The figures are simulated, not certified.
- The hybrid example's coast-down returns the EPA coefficients it was built
  from within 2 %.
- Driving a battery down to empty over repeated cycles is not one of the
  tests yet; the run summary's *Range at this consumption* estimates it
  from one cycle ([Consumption figures](../reference/consumption-figures.md)).
