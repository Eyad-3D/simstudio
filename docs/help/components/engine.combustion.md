## How it computes

In this section ω is the engine speed (rad/s), θ the *Throttle* input held
to 0–1, and T the torque the engine gives its shaft. With the *Throttle*
input unwired, θ = 0, so the engine only idles.

The maps are read at the speed n held between the *Full-Load Torque*
curve's first and last speed. T_full is the full-load curve at n, T_drag
the *Drag Torque (unfired)* at |ω|, and ṁ(n, T) the *Fuel Consumption Map*.
The idle governor's demand is

g = (ω_idle − |ω|) ÷ (0.25·ω_idle)

with ω_idle the *Idle Speed* (1/min): 0 at idle, 1 at 75 % of idle, below 0
above idle.

### When it fires, and what it gives

The engine is switched on while the *Enable* input is 0.5 or more (always,
unwired) and its Fuel Tank holds more than 0 kg.

- **Fired with throttle** (switched on, θ > 0, |ω| at most the last
  speed): T = max(θ, min(1, g))·T_full. Below idle the governor adds torque.
  It burns ṁ(n, T).
- **Held at idle** (switched on, θ = 0, |ω| at most the *Fuel Cut-Off
  Re-Entry Speed*): T = g·T_full, held between −T_drag and T_full. Above
  idle that torque is negative: the governor brakes the engine back to
  idle, by at most its drag. It burns ṁ(n, T) for T ≥ 0; below 0 the fuel
  falls along a straight line (a Willans line) from ṁ(n, 0) to nothing at
  −T_drag: ṁ(n, 0)·(1 + T ÷ T_drag).
- **Not fired** (switched off, out of fuel, θ = 0 above the re-entry speed,
  which is overrun fuel cut-off, or above the last speed): T = −T_drag
  against the motion, fading in proportion to the speed below 0.1 rad/s;
  no fuel.
- **Rev limiter** (θ > 0 above the last speed n_last): over a band of 0.1 %
  of n_last above it (6 1/min at 6,000 1/min), it fires a share
  λ = (1.001·n_last − |ω|) ÷ (0.001·n_last) of the time. Its torque is
  λ of the way from the drag to the fired torque, and it burns λ·ṁ. The
  engine settles inside the band where that torque holds the load.

Below the full-load curve's first speed, n stays at that speed, so a fired
engine gives that point's torque and burns that point's fuel.

The engine turns as J·dω/dt = T + τ_shaft, with τ_shaft the torque from
the shaft and J the *Inertia* (kg·m²) × *Engine Scale*. Behind a clutch it
keeps its own speed at a gear change and the clutch slips. A run that
starts at speed starts the engine at the speed its gears give it, or at
rest behind a clutch that starts open.

### Engine Scale

*Engine Scale* (%) multiplies the full-load and drag torques, the fuel
map's torque axis and its fuel flows, and the inertia. The fuel per kWh at
each scaled point stays as typed.

### Fuel, losses and stored energy

The fuel comes out of the Fuel Tank. Without a tank the engine runs on an
inexhaustible supply at 42.9 MJ/kg. *Fuel Power* is ṁ × the heating value
(the tank's *Fuel Heating Value*, or 42.9 MJ/kg without a tank). The energy
view books as loss the *Losses* output, Fuel Power − T·ω; while the engine
is not fired that is the power its drag takes. It books ½·J·ω² as stored.

### Edges of the maps

- The full-load curve is never read past its own speeds.
- The fuel map stops the run by default (*Error*) where it is read past its
  data: it must cover the full-load curve's speeds and the torques the
  engine fires at. It is read only while the engine fires.
- *Drag Torque (unfired)* holds its end value by default (*Clamp*). Set to
  *Error*, it stops a run wherever the engine turns faster than the table's
  last speed, fired or not: the engine reads the table at every moment, as
  the lower bound of its fired torque too. Below the first speed it holds
  that speed's value down to 0.
- The engine has no speed governor above idle: declutched with any
  throttle, it runs up to its rev limiter.
