## How it computes

The transfer case turns with *Flange In* at ω_in. Its inertia J
(*Inertia*) takes its share of the input torque first; the gears carry the
rest:

`τ_x = τ_in − J·dω_in/dt`

With i the *Ratio* (0 counts as 1), s_A the *Torque Split to Output A*
(held to 0–100 %) and η_d the efficiency factor below:

**Unlocked** (*Locked* unticked):

`τ_A = s_A·i·η_d·τ_x`

`τ_B = (1 − s_A)·i·η_d·τ_x`

`ω_in = i·(s_A·ω_A + (1 − s_A)·ω_B)`

The torque splits by s_A whatever the outputs' speeds, and the speeds are
free. The input's speed is the outputs' mean weighted by their torque
shares, so the power out of both outputs adds up to the power through the
gears. The two torques keep the ratio s_A : (1 − s_A), so if one axle
loses grip and spins up, the other's torque is held to that ratio of what
the spinning one takes.

**Locked** (*Locked* ticked):

`ω_A = ω_B = ω_in/i`

`τ_A + τ_B = i·η_d·τ_x`

Both outputs turn together, and the rest of the driveline (the axles'
grip and inertia) sets how the torque splits; *Torque Split to Output A* is
not used.

### Efficiency and energy

η_d follows the direction of the power `P = τ_x·ω_in`, with η the
*Efficiency* (at least 0.1 %): `η_d = η` while power flows from the input
to the outputs, `η_d = 1/η` while it flows back. Within 1 W of no power it
moves smoothly between the two.

It books the loss `P·(1 − η_d)`: `(1 − η)` times the power entering the
gears, from whichever side. It stores `½·J·ω_in²`.

*Torque Out A* and *Torque Out B* are the torques passed to each output,
*Input Speed* is `|ω_in|`, *Input Power* is P (after the inertia) and
*Losses* the loss.

With the Vehicle's *Road Load From* set to Coefficients A/B/C and
*Coefficients Include Driveline Losses* ticked, the efficiency is set to
100 %: the transfer case runs lossless.
