## How it computes

Traction Control is a digital controller: it runs at the start of the run
and then every *Sample Time* (every 10 ms when it is 0), and holds its
output in between. Any Sample Time above 0 is used as it is, one shorter
than 10 ms too. Its output is the Driver's demand, capped by a limit L
between 0 and 1: out = min(demand, L).

### At each run

- **Braking and regeneration pass through.** With a demand of 0 or less the
  output is the demand, and the controller starts afresh (its integral back
  to 1, any launch forgotten).
- **Slow.** The car counts as slow while the *Speed* input is wired and the
  speed, in km/h, is below *Minimum Speed*. Without the *Speed* input wired
  the car never counts as slow.
- **Launch ramp.** When the car is slow and asks for drive, a launch starts.
  From then on L = *Launch Torque* / 100 + (1 − *Launch Torque* / 100) ×
  (time since the launch started) / *Launch Ramp Time*, up to 1. With a
  *Launch Ramp Time* of 0 there is no ramp. The launch ends once L has
  reached 1 and the car is no longer slow.
- **Slip control.** Once the car is not slow, and at least one *Slip* input
  is wired, a PI loop (proportional and integral) acts on the larger of the
  two slips s: with the error e = *Target Slip* − s, the integral grows by
  Ki·e·Δt each run (Ki the *Integral Gain*, Δt the period), held between 0
  and 1 (anti-windup), and L becomes the smaller of the launch limit and
  integral + Kp·e (Kp the *Proportional Gain*), also held between 0 and 1.
  A wheel that spins (s above the target) makes e negative and lowers L.
- **While slow** the integral follows the launch limit, so slip control
  takes over from where the ramp is when the car passes *Minimum Speed*.

### Edges

- An unwired *Slip* input is left out; with neither wired there is no slip
  control, only the launch ramp.
- Its ports have no unit: the demand and the slips are plain numbers, and
  the speed arrives in km/h.
