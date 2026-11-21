## How it computes

Both flanges turn at one speed ω (rad/s): the shaft is rigid. Its inertia
J (*Inertia*, kg·m²) sits on the Flange A side, and its efficiency η
(*Mechanical Efficiency*, the share of the power that comes out; at least
0.1 %) acts on the torque that is left after the inertia:

`τ_x = τ_A − J·dω/dt`

`τ_B = η_d·τ_x`

τ_A is the torque into Flange A and τ_B the torque Flange B passes on.
The factor η_d follows the direction of the power `P = τ_x·ω`:

- power flowing from A to B (P > 0): `η_d = η`, so B gets the share η;
- power flowing from B to A (P < 0): `η_d = 1/η`, so A gets the share η
  of what B puts in.

Within 1 W of no power, η_d moves smoothly from η to 1/η, so the torque
has no jump when the power reverses.

### Energy

The shaft books the loss `P·(1 − η_d)`: `(1 − η)` times the power that
enters it, whichever side that is from. It is never negative. The shaft
stores `½·J·ω²`.

*Transmitted Power* is P (positive from A to B) and *Losses* the loss.
At 100 % the shaft is lossless.

The Vehicle's *Coefficients Include Driveline Losses* does not change the
shaft: it keeps its efficiency.
