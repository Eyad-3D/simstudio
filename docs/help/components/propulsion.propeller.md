## How it computes

The load torque grows with the square of the speed, against the motion:

`T_load = T_ref·ω·|ω| / ω_ref²`

with ω the shaft speed (rad/s), T_ref the *Torque at Reference Speed*
(N·m; below 0 counts as 0) and ω_ref the *Reference Speed* (at least
1 1/min). Turning backwards, the load reverses with it.

The propeller's inertia J (*Inertia*, kg·m²) turns with the shaft:

`J·dω/dt = τ_shaft − T_load`

### Energy

All the power the load takes, `T_load·ω`, is booked as the propeller's
loss: no share of it is counted as useful thrust. It stores `½·J·ω²`.

*Shaft Speed* is `|ω|` and *Shaft Power* is `|T_load·ω|`.
