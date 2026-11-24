## How it computes

The differential's case (its carrier) turns with *Flange In* at ω_in. Its
inertia J (*Inertia*) takes its share of the input torque first; the gears
carry the rest:

`τ_x = τ_in − J·dω_in/dt`

With i the *Ratio* (0 counts as 1) and η_d the efficiency factor below, the
gears pass `T = i·η_d·τ_x` to the two outputs together. *Type* says how
they share it.

**Open**:

`ω_in = i·(ω_A + ω_B)/2`

`τ_A = τ_B = ½·T`

Both outputs always get the same torque, and their speeds are free. If one
wheel loses grip and spins up, the other gets no more torque than the
spinning one takes: the wheel with less grip sets what the axle can push.

**Locked**:

`ω_A = ω_B = ω_in/i`

`τ_A + τ_B = T`

The two outputs turn together, and the rest of the driveline (each wheel's
grip and inertia) sets how the torque splits between them. The axle can
push with both wheels' grip together.

**Limited-slip**: an open differential with a friction clutch between its
two outputs (the clutch-pack type: springs press the plates together, and
ramps press them harder the more torque goes through). The clutch's locking
torque is

`T_L = P + ½·L·|T|`

with P the *Preload* and L the *Locking Under Power* while the torque
through it drives forwards, the *Locking Off Power* while it drives
backwards (lifting off, recuperating). With τ_c the torque the clutch
passes from output A to output B:

`ω_in = i·(ω_A + ω_B)/2`

`τ_A = ½·T − τ_c`, `τ_B = ½·T + τ_c`

The clutch works like the Clutch part:

- while both outputs turn at the same speed it holds (stiction: it holds
  until the torque it must pass exceeds T_L), passing whatever torque keeps
  them together, up to T_L;
- when that takes more than T_L it slips and passes exactly T_L from the
  faster output to the slower one. A spinning wheel is the faster one, so
  the wheel with grip gets more torque: the low-grip wheel's torque plus
  2·T_L, at most what locked would give;
- the changes between the two are located exactly in time;
- with both outputs at one speed at the start (a car at rest, or driving
  straight), it starts holding, and lets go at once if it cannot.

So the two outputs' torques differ by at most 2·T_L: twice the Preload
with no torque through it, and L × the torque through it without preload.
A data sheet's torque bias ratio TBR (the most torque one side gets over
the other's) is `(1 + L)/(1 − L)`: L = 33 % is TBR 2, L = 50 % is TBR 3.

Example: a car with one driven axle carrying 8,829 N on each wheel (1,800 kg
on two wheels of 0.33 m), its left wheel on ice (μ 0.3) and its right one
on dry road (μ 1.0). Open, the axle pushes with twice the icy wheel's grip,
2 × 0.3 × 8,829 = 5,297 N. Locked, with both wheels' grip, 1.3 × 8,829 =
11,478 N. Limited-slip with 40 N·m of preload and 40 % under power, with
the icy wheel spinning: its 874 N·m plus the clutch's 2·T_L, which grows
with the torque, `2·(874 + 40)/(1 − 0.4)` = 3,047 N·m at the axle, 9,233 N.

### Efficiency and energy

η_d follows the direction of the power `P = τ_x·ω_in`, with η the
*Efficiency* (at least 0.1 %): `η_d = η` while power flows from the input
to the wheels, `η_d = 1/η` while it flows back from the wheels. Within 1 W
of no power it moves smoothly between the two.

It books the loss `P·(1 − η_d)`: `(1 − η)` times the power entering the
gears, from whichever side. A limited-slip one adds its clutch's heat,
`τ_c·(ω_A − ω_B)`: 0 while it holds, never negative. It stores `½·J·ω_in²`.

*Torque Out A* and *Torque Out B* are the torques passed to each side,
*Input Speed* is `|ω_in|`, *Input Power* is P (after the case's inertia)
and *Losses* the loss.

With the Vehicle's *Road Load From* set to Coefficients A/B/C and
*Coefficients Include Driveline Losses* ticked, the efficiency is set to
100 %: the differential runs lossless.

### In quick mode and in a lap

Quick mode and lap mode hold every wheel at the car's speed: the two
wheels of an axle turn at one speed, in a corner too. A locked or
limited-slip differential's outputs therefore never slip there, and their
speeds cannot say how the torque splits. It splits by the two wheels'
grip instead (each wheel's μ × its load; in a corner the share of it the
corner leaves for driving and braking), as it does at a locked axle's
limit:

- **Locked**: `τ_A : τ_B = G_A : G_B`, so both wheels reach their grip
  together and the axle passes the sum of the two;
- **Limited-slip**: the open split moved toward that one by at most T_L.
  At the limit of traction that is the low-grip wheel's torque plus 2·T_L,
  as in a full run.

With the same grip on both wheels (driving straight, both on the same
road) all three Types split the torque equally. In a corner the inner
wheel carries less load, so the outer wheel gets more torque: under power
a real clutch-type differential sends torque to the slower inner wheel
until the inner one spins, and these modes see only the limit. The
clutch's heat in a corner is not counted there (the outputs do not slip),
and a locked axle's resistance to turning (it scrubs a tyre in a corner)
is not modelled.

Each output of a locked or limited-slip differential needs one Wheel on
the Vehicle on its own shaft, joined by Shafts, Mechanical Nodes or Brakes
only; quick mode and a lap name one that has none.

### Good to know

- The older engine (Python) models Open and Locked; it does not run a
  Limited-slip one, and says so.
- A project saved before the Differential had a Type, with its Locked box
  ticked, opens with Type Locked.
