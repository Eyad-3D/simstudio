## How it computes

The PID Controller is a digital controller: it runs at fixed instants (it
*samples*) and holds its output in between, as a controller in a car's
electronics does.

### When it runs

- Once at the start of the run, then every 10 ms while *Sample Time* is
  10 ms or less (0 included). Its time step is then Δt = 10 ms.
- With a longer *Sample Time* T, at the first 10 ms instant at or after
  each multiple of T, with Δt = T.

### What it computes at each sample

With e = Setpoint − Feedback, the error:

- derivative: d = (e − e_prev) / Δt, with e_prev the error at the sample
  before (0 at the first sample, so an error at the start gives a one-time
  kick of *Derivative Gain* × e / Δt);
- integral: I grows by e·Δt, but only when that keeps the output inside
  its limits, or when the error pulls the output back towards them
  (anti-windup: the sum does not keep growing while the output is held at a
  limit);
- output: u = Kp·e + Ki·I + Kd·d, with Kp, Ki and Kd the *Proportional*,
  *Integral* and *Derivative Gain*, then held between *Output Minimum* and
  *Output Maximum*. If you set the minimum above the maximum, the output is
  the minimum.

### Edges

- An unwired input reads 0.
- Its ports have no unit: the setpoint and the feedback arrive as numbers
  in the unit the app shows for the port they come from (a Vehicle's speed
  in km/h), and the output leaves as a number in the unit of the input it
  feeds. *Setpoint & Feedback Unit* converts nothing; Data Checks use it to
  warn when the wired signal's unit differs.
- A change of a gain or a limit while the run goes on (a live edit) takes
  effect at the next sample.
