## How it computes

The final drive is a gear with an inertia on each side. With i the
*Transmission Ratio* (a ratio of 0 counts as 1):

`ω_in = i·ω_out`

The input side's inertia J_in (*Moment of Inertia In*) takes its share of
the torque first; the gear mesh carries the rest, τ_x:

`τ_x = τ_in − J_in·dω_in/dt`

The output side gets i times that torque, less the mesh's loss, and its
inertia J_out (*Moment of Inertia Out*) takes its share:

`τ_out = i·η_d·τ_x − J_out·dω_out/dt`

τ_in is the torque into *Flange In* and τ_out the torque *Flange Out*
passes on. η_d follows the direction of the power `P = τ_x·ω_in` through
the mesh, with η the *Efficiency* (at least 0.1 %):

- driving (power from input to output, P > 0): `η_d = η`;
- recuperating or coasting (power from output to input, P < 0):
  `η_d = 1/η`, so the input gets the share η of what the output puts in.

Within 1 W of no power, η_d moves smoothly from η to 1/η, so the torque
has no jump when the power reverses.

### Energy

It books the loss `P·(1 − η_d)`: `(1 − η)` times the power entering the
mesh, from whichever side. It stores `½·J_in·ω_in² + ½·J_out·ω_out²`.

*Transmitted Power* is P (after the input inertia), *Output Speed* is
`|ω_out|` and *Losses* the loss.

With the Vehicle's *Road Load From* set to Coefficients A/B/C and
*Coefficients Include Driveline Losses* ticked, the efficiency is set to
100 %: the final drive runs lossless.
