## How it computes

The Vehicle is one mass m (*Vehicle Mass*, kg) that moves along the road
at the speed v. At every instant it solves Newton's second law along the
road:

`m·a = F_tyres − F_drag − F_B − F_grade − F_roll`

- `a` is the acceleration, dv/dt.
- `F_tyres` is the sum of the traction forces of the Wheels on the Vehicle
  (see [Wheel](propulsion.wheel.md)).
- `F_grade = m·g·sin θ` is the weight's pull along the road, with
  g = 9.81 m/s² and θ = atan(grade) the slope angle. The grade comes from
  the *Road Grade* input (in %); with nothing wired it is 0.
- `F_drag`, `F_B` and `F_roll` are the road load, below.

### Road load

With *Road Load From* set to Drag and rolling resistance:

- `F_drag = ½·ρ·Cd·A·v·|v|`, with ρ the air density, Cd the *Drag
  Coefficient (Cd)* and A the *Frontal Area* (m²). A Cd or an area below 0
  counts as 0.
- `F_B = 0`.
- `F_roll = Σ c_rr·N`, summed over the Wheels on the Vehicle: each Wheel's
  *Rolling Resistance Coeff.* c_rr times its normal load N. N already holds
  cos θ, the load transfer and the downforce.

With *Road Load From* set to Coefficients A/B/C (the app shows v in km/h;
the engine converts the coefficients to SI units):

- `F_drag = C·v·|v|·ρ/ρ0`, with C the *Road Load C (f2)* and
  ρ0 = 1.204 kg/m³, the density C is taken at.
- `F_B = B·v·cos θ`, with B the *Road Load B (f1)*.
- `F_roll = A·cos θ`, with A the *Road Load A (f0)*.

So on a slope A and B shrink with cos θ, as the normal load does; C does
not.

The car can roll backwards, for example down a slope. Drag and `F_B` act
against the motion in both directions, and so does `F_roll`.

### Rolling resistance holds the car at rest

`F_roll` is dry friction with stiction (stiction: friction that holds a
part at rest until a larger force pushes it). While the car moves,
`F_roll` acts against the motion at its full size. While the car stands,
it holds the car with whatever force that takes, up to its size. So:

- a coasting car slows down, stops at the exact moment v reaches 0, and
  stays stopped;
- a car at rest moves off only when the tyres' push, less the slope's
  pull, is larger than `F_roll`. On a slope where `m·g·sin θ` is smaller
  than `F_roll`, a car at rest stays put with no brake applied.

### Axle loads

The weight normal to the road is `W = m·g·cos θ`. Two terms move load
between the axles:

- load transfer `ΔF = m·(a + g·sin θ)·h/L`, from the front axle to the
  rear, with h the *Centre of Gravity Height* and L the *Wheelbase* (m).
  The acceleration a is the one at the same instant: the loads and the
  motion are solved together;
- downforce `D = ½·ρ·CzA·v²`, with CzA the *Downforce Area (CzA)*. A
  negative CzA is lift.

With b the *Front Aero Balance* (held to 0–100 %), and s_f and s_r the
front and rear Wheels' load shares added up:

`Front Axle Load = s_f·W + b·D − ΔF`

`Rear Axle Load = s_r·W + (1 − b)·D + ΔF`

If one of these would be below 0, that axle carries 0 and the other
carries `W + D`. If `W + D` itself is below 0, both carry 0.

With Wheels on one axle only, that axle carries `W + D` (at least 0). Then
there is no load transfer, and the Front Aero Balance is not used.

Each Wheel takes its share of its axle's load: see
[Wheel](propulsion.wheel.md).

The cornering load transfer and the *Front Track Width* and *Rear Track
Width* are used by lap cases only (see
[Known limits](../../../KNOWN-LIMITS.md)).

### Energy

The Vehicle stores kinetic energy `½·m·v²` and potential energy `m·g·z`,
with z the height climbed (`dz/dt = v·sin θ`). So the energy a climb takes
is stored, and comes back downhill.

It books `(F_drag + F_B + F_roll)·v` as its loss: the air drag and rolling
resistance in the energy view. A car at rest loses nothing.

The power outputs:

- *Air Drag Power* = `F_drag·v`;
- *Rolling Resistance Power* = `(F_roll + F_B)·v`, so it holds the B term;
- *Climbing Power* = `F_grade·v`;
- *Acceleration Power* = `m·a·v`.

*Vehicle Speed* is v with its sign (negative while rolling backwards).
*Distance* is the integral of v, so driving backwards takes distance off.

### Quick mode

In quick mode (**Run mode** **Quick**, see
[Run a cycle in quick mode](../../how-to/run-in-quick-mode.md)) the speed
follows the drive cycle exactly. `F_roll` then acts against the prescribed
motion, or against the acceleration that starts the car from rest, and is
0 at rest.

### What this version does not run

- One Vehicle per model. A model with two is refused: "Only one
  vehicle.body element per model is supported."
