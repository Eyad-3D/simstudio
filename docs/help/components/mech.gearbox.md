## How it computes

### Which gear

The gearbox reads a gear number: the *Gear Select* input when it is wired,
else the *Default Gear*. It uses the listed gear in *Gear Ratios* nearest
that number. It switches halfway between two listed gears; exactly halfway
it keeps the lower one. Below the lowest listed gear it uses the lowest,
above the highest the highest. *Active Gear* is the gear it uses.

The gear's ratio is i (input speed ÷ output speed; a ratio of 0 counts
as 1).

### Torque and speed

`ω_in = i·ω_out`

The input side's inertia J_in (*Moment of Inertia In*) takes its share of
the torque first; the gear mesh carries the rest, τ_x:

`τ_x = τ_in − J_in·dω_in/dt`

The output side gets i times that torque, less the mesh's loss, and its
inertia J_out (*Moment of Inertia Out*) takes its share:

`τ_out = i·η_d·τ_x − J_out·dω_out/dt`

η_d follows the direction of the power `P = τ_x·ω_in` through the mesh,
with η the *Efficiency* (the same in every gear, at least 0.1 %):

- driving (power from input to output, P > 0): `η_d = η`;
- recuperating or coasting (power from output to input, P < 0):
  `η_d = 1/η`, so the input gets the share η of what the output puts in.

Within 1 W of no power, η_d moves smoothly from η to 1/η, so the torque
has no jump when the power reverses.

### A gear shift

The shift happens at the instant the gear number crosses halfway between
two listed gears. It is rigid and takes no time:

- the speeds of everything the gears tie rigidly together (shafts, gears,
  motors, wheels) jump to the new ratio so that their angular momentum is
  kept;
- the kinetic energy that loses is booked as the gearbox's loss, its *gear
  shifts* term;
- the car itself is not part of the jump. The tyres' forces are limited, so
  the wheels' slip changes at the shift and then passes the momentum on to
  the car over the next moments, as the tyres' slip loss;
- a Clutch is not rigid: when the shift pushes its two sides apart by more
  than 0.001 rad/s it slips, so an engine behind it keeps its speed and the
  clutch takes up the difference.

### Energy

The gearbox books the loss `P·(1 − η_d)`: `(1 − η)` times the power
entering the mesh, from whichever side, plus the gear shifts loss at each
shift. It stores `½·J_in·ω_in² + ½·J_out·ω_out²`.

*Transmitted Power* is P (after the input inertia), *Output Speed* is
`|ω_out|` and *Losses* the mesh's loss.

Unlike the Final Drive, the Differential and the Transfer Case, the
gearbox keeps its efficiency when the Vehicle's *Coefficients Include
Driveline Losses* is ticked.

### What this version refuses

A *Gear Ratios* table with no gear, or with a gear number or a ratio that
is not a number, is refused before the run.
