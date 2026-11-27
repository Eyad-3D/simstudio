## How it computes

### The pedal command

The speed error is

`e = v_target − v_actual`

v_target is the *Target Speed* input (0 km/h with nothing wired). v_actual
is the *Actual Speed* input, or the Vehicle's speed when nothing is wired
to it.

A PI controller (proportional-integral: it acts on the error and on its
sum over time) turns the error into a command:

`u = Kp·e + Ki·I`, with `I = ∫ e dt` (0 at the start)

Kp is the *P Gain* (1/(km/h)) and Ki the *I Gain* (1/(km/h·s)). The pedal
command is u held to the pedals' range:

`c = clamp(u, −1, 1)`

+1 is full throttle, −1 full brake.

While c is held at a limit, anti-windup keeps the integral from growing
without end (back-calculation, as Modelica's `LimPID` does it):

`dI/dt = e + (c − u) / (0.9·Kp)`

Inside the limits c = u, and this is the plain integral. At a limit, the
part of u beyond it pulls the integral back. In this formula Kp is never
less than `Ki·0.001 s`, so it also works with a P Gain of 0.

The raw pedals: *Accel Pedal (raw)* = `max(c, 0)`, *Brake Pedal (raw)* =
`max(−c, 0)`.

### Driving

With c ≥ 0, *Traction Command* = c and *Brake Command* = 0.

### Braking: recuperation first

With c < 0 the Driver asks for a braking torque at the wheels and splits it
between the motors and the friction brakes. Recuperation is braking with
the motors running as generators.

- `T_fr,max`: the friction brakes' torque at the wheels at a full Brake
  Command. It is the sum, over every Brake, of its *Maximum Brake Torque*
  times its speed per wheel speed (the gear ratio between it and the
  wheels).
- `T_rg,max`: the motors' generator torque at the wheels. For each E-Motor
  it is its generator torque limit (its full-load torque at its speed
  times its *Generator Torque Limit Scale*, and 0 above its maximum speed
  or on a supply of 1 V or less), times the ratio from the wheels to the motor with each
  Gearbox in its current gear, divided by the efficiency of the gears
  between.
- `T_rg,avail = T_rg,max·clamp(v / 3 m/s, 0, 1)`: recuperation fades out
  in a straight line below 3 m/s (10.8 km/h), and is 0 when the car rolls
  backwards.
- w is the *Recuperation Weight*, held to 0–100 %.

Then:

`T_req = |c|·(T_fr,max + w·T_rg,avail)` (the braking torque asked for)

`T_rg = min(w·T_rg,avail, T_req)` (the motors take it first)

`T_fr = min(T_fr,max, T_req − T_rg)` (the friction brakes do the rest)

*Traction Command* = `−T_rg / T_rg,max` (negative: the share of the
motors' full generator torque) and *Brake Command* = `T_fr / T_fr,max`. At
full brake the friction brakes give their whole torque and the motors w
times what they have.

Where the motors' supply takes less than they would feed back (a full or
charge-limited battery, a fuel cell, a DC-DC that passes power one way),
the motors recuperate what it takes and the friction brakes take the rest,
so the car brakes as asked. A full battery is said in the messages. With
no E-Motor, all braking is friction; with no Brake, all braking is
recuperation, and where the supply takes less the motors brake less (the
run says the charging was held back).

### Other case kinds

- *Acceleration* case: *Traction Command* is 1 and *Brake Command* 0 from
  start to end. The speed inputs are not read.
- *Performance* case: full throttle while the speed is below the target,
  until it first reaches the target; then the PI controller holds the
  target. While held at full throttle the integral does not wind up.
- Quick mode (**Run mode** **Quick**): the pedal command is worked out
  back from the cycle's speed, so the P Gain and I Gain are not used. A
  command beyond full throttle or full brake is flagged: the car cannot
  follow the cycle there.

The Driver stores and loses no energy.

### What this version refuses

One Driver per model. A model with two is refused: "Only one driver.driver
element per model is supported."
