## How it computes

### Tyre force

The tyre's longitudinal slip κ says how much faster the tread turns than
the road passes under it:

`κ = (ω·r − v) / max(|v|, 0.5 m/s)`

ω is the wheel speed (rad/s), r the *Wheel Radius* (m, at least 1 mm) and
v the Vehicle's speed (m/s). Below 0.5 m/s the divisor stays at 0.5 m/s,
so the slip stays finite at standstill.

The traction force is

`F = N·clamp(k·κ, −μ_eff, +μ_eff)`

with N the normal load, k the *Slip Stiffness* (at least 0.1) and μ_eff
the friction coefficient at this load (below). A positive F drives the car,
a negative F brakes it. At the grip limit the force stays at `μ_eff·N`
however large the slip: the wheel spins up (driving) or locks (braking),
and the torque beyond the limit goes into the wheel's inertia.

### Wheel speed

The wheel's own inertia J (*Inertia*, kg·m²) turns with the shaft:

`J·dω/dt = τ_shaft − F·r`

τ_shaft is the torque the *Mechanical Shaft* gives it. *Drive Torque* is
`F·r`, the torque the road takes, not the shaft's torque.

### Normal load and grip

`N = max(s·m·g·cos θ + p·ΔF_axle, 0)`

- s is this wheel's *Vehicle Load Share*, after the shares of all the
  Wheels on the Vehicle are scaled to add up to 100 %; m the Vehicle Mass,
  g = 9.81 m/s², θ the slope angle.
- ΔF_axle is the load the Vehicle moves onto this wheel's axle (load
  transfer and downforce: see [Vehicle](vehicle.body.md)), and p this
  wheel's part of it: its share over the sum of the shares on its axle.

`μ_eff = max(0, μ + (dμ/dFz)·(N − Fz0))`

μ is the *Friction Coefficient μ*, dμ/dFz the *Load Sensitivity dμ/dFz*
(1/kN) and Fz0 the *Nominal Load Fz0* (N). With Fz0 at 0, Fz0 is
`s·m·9.81 m/s²`, this wheel's share of the weight on level ground.

The rolling resistance `c_rr·N` (c_rr the *Rolling Resistance Coeff.*) is
handed to the Vehicle, which applies it to the car as friction. It is not a
torque on the wheel.

### Energy

The wheel books its slip loss `F·(ω·r − v)` (the *Slip Losses* output) and
stores `½·J·ω²`. F and the slip always have the same sign, so the loss is
never negative.

At a gear shift the gearbox's speeds jump, and the wheel's speed with
them. The tyre's force is limited, so it passes no jump of momentum to the
car: its slip then relaxes through the law above, inside its grip or
sliding at it, and passes the momentum on over the time that takes. That
energy is booked as its slip loss.

### Edges

- A Wheel whose *Mechanical Shaft* is connected to nothing is not on the
  Vehicle: it carries no load, passes no force and takes no share of the
  weight. The shares are scaled over the connected Wheels only. A model
  with no Vehicle gives the same.
- *Lateral Friction μ_y* and the *Friction Ellipse Exponent* are used by
  lap cases only.

### Quick mode

In quick mode (**Run mode** **Quick**) the wheel rolls with the
prescribed speed, without slip: `ω·r = v`. Its slip is then
`κ = F / (k·N)` (N at least 1 N), held to ±μ_eff/k, and the shaft gives
`F·r·(1 + κ)`, so the slip loss `F·κ·v` is still paid. A force beyond
`μ_eff·N` is flagged.
