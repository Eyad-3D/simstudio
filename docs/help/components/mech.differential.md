## How it computes

The differential's case (its carrier) turns with *Flange In* at ω_in. Its
inertia J (*Inertia*) takes its share of the input torque first; the gears
carry the rest:

`τ_x = τ_in − J·dω_in/dt`

With i the *Ratio* (0 counts as 1) and η_d the efficiency factor below:

**Open** (*Locked* unticked):

`ω_in = i·(ω_A + ω_B)/2`

`τ_A = τ_B = ½·i·η_d·τ_x`

Both outputs always get the same torque, and their speeds are free. If one
wheel loses grip and spins up, the other gets no more torque than the
spinning one takes.

**Locked** (*Locked* ticked):

`ω_A = ω_B = ω_in/i`

`τ_A + τ_B = i·η_d·τ_x`

The two outputs turn together, and the rest of the driveline (each wheel's
grip and inertia) sets how the torque splits between them.

### Efficiency and energy

η_d follows the direction of the power `P = τ_x·ω_in`, with η the
*Efficiency* (at least 0.1 %): `η_d = η` while power flows from the input
to the wheels, `η_d = 1/η` while it flows back from the wheels. Within 1 W
of no power it moves smoothly between the two.

It books the loss `P·(1 − η_d)`: `(1 − η)` times the power entering the
gears, from whichever side. It stores `½·J·ω_in²`.

*Torque Out A* and *Torque Out B* are the torques passed to each side,
*Input Speed* is `|ω_in|`, *Input Power* is P (after the case's inertia)
and *Losses* the loss.

With the Vehicle's *Road Load From* set to Coefficients A/B/C and
*Coefficients Include Driveline Losses* ticked, the efficiency is set to
100 %: the differential runs lossless.
