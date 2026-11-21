## How it computes

The Driving Task has no state and no physics: at every moment its output,
the target speed, is read from its profile.

- **Where it reads.** With *Profile Axis* set to *Time*, at x = the time
  since the run started, in s. With *Distance*, at x = the distance the
  Vehicle has driven, in m (from the Vehicle's distance; a model with no
  Vehicle reads at 0 m, its first point).
- **The profile.** The points of *Profile* (km/h), sorted by x, or, with a
  *Drive Cycle* chosen, that cycle's speed trace in their place. A cycle of
  your own that is laid out against distance switches the axis to
  *Distance*. A pair that is not two numbers is skipped; a profile with no
  points stops the run (*its profile has no points*).
- **Between points** the speed follows a straight line (linear
  interpolation). Two points at the same x are a step: the speed jumps just
  after that x.
- **Before the first point and after the last** the speed holds that
  point's value.
- **Scale.** The output is v = *Scale* / 100 × the profile's speed at x.
- **Repeat Profile.** Ticked, when x reaches the last point it starts again
  from the first point's x, so one pass lasts last x − first x (a lap, with
  the axis on *Distance*). A profile of a single point does not repeat.

The output is a target, not a speed the car is held to: wire it to the
Driver's *Target Speed* input, and the Driver works the pedals to follow it.
