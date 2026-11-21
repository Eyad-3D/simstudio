## How it computes

The Road Profile has no state and no physics: at every moment its output,
the road's grade in %, is read from its profile.

- **Where it reads.** With *Profile Axis* set to *Distance*, at x = the
  *Distance* input when it is wired, else the distance the Vehicle has
  driven, in m (a model with no Vehicle reads at 0 m, its first point).
  With *Time*, at x = the time since the run started, in s.
- **The profile.** The points of *Profile* (grade in %, rise ÷ run × 100,
  uphill positive), sorted by x, or, with *Grade From Cycle* chosen, that
  cycle's grade column in their place. A pair that is not two numbers is
  skipped; a profile with no points stops the run (*its profile has no
  points*).
- **Between points** the grade follows a straight line (linear
  interpolation). Two points at the same x are a step: the grade jumps just
  after that x.
- **Before the first point and after the last** the grade holds that
  point's value.
- **Repeat Profile.** Ticked, when x reaches the last point it starts again
  from the first point's x, so one pass lasts last x − first x.

Wired to the Vehicle's *Road Grade* input, the grade g sets the slope angle
θ = atan(g / 100): the Vehicle's page says what the slope does to the car.
