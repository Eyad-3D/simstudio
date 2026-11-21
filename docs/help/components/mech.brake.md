## How it computes

The brake's capacity is

`T_c = c·max(T_max, 0)`

with c the *Brake Command* held to 0–1 and T_max the *Maximum Brake
Torque* (N·m). With nothing wired to Brake Command, c is 0 and the brake
never brakes.

The torque it takes from the shaft acts against the rotation:

`τ_b = T_c·clamp(ω / 0.001 rad/s, −1, 1)`

- Above 0.001 rad/s, either way, it is the full capacity against the
  motion.
- Within 0.001 rad/s of rest it grows with the speed, like a stiff damper.
  That holds a shaft at rest: a torque T smaller than `T_c` on it lets it
  creep at `0.001 rad/s × T/T_c` at most (under 0.01 1/min). A torque
  larger than `T_c` turns it.

The brake never drives the shaft: its torque never changes sign against
the motion.

Its disc's inertia J (*Inertia*, kg·m²) turns with the shaft:

`J·dω/dt = τ_flange − τ_b`

### Energy

It books `τ_b·ω` as its loss (*Braking Power*), never negative, and stores
`½·J·ω²`.

*Brake Torque* is the capacity `T_c`, not the torque it takes: a brake
holding a shaft at rest shows its whole capacity.

### Quick mode

In quick mode (**Run mode** **Quick**) the brake takes its whole capacity
against forward rotation at every speed, and follows a Brake Command
beyond 1 (quick mode flags it).
