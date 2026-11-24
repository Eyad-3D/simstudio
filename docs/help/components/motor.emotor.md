## How it computes

In this section ω is the shaft speed (rad/s), U the supply voltage between
the terminals (V), and c the *Traction Command*, held to −1…1.

### Torque from the command

The full-load torque T_fl is the *Full-Load Torque* map at U and |ω|, with
|ω| read at most at the maximum speed ω_max. ω_max is *Maximum Speed* (1/min)
× *Speed Scale*; at 0 it is the full-load map's last speed (the lowest one,
if the voltage rows end at different speeds).

- Driving (c > 0): T = c·T_fl. Over the last 2 % below ω_max, T_fl falls
  linearly to 0: T_fl·(ω_max − |ω|) ÷ (0.02·ω_max).
- Braking as a generator (c < 0): T = c·T_fl·k_q4, with k_q4 the *Generator
  Torque Limit Scale* (%).

A positive command gives a positive torque, whichever way the shaft turns.
The rotor follows J·dω/dt = T + τ_shaft, with τ_shaft the torque from the
shaft and J the *Rotor Inertia* (kg·m²) × *Torque Scale*.

### Electrical power

The inverter runs when the command is not 0 (near standstill, below about
1 1/min, more than 10⁻⁹ either way: less is rounding), U is above 1 V and
|ω| is at most ω_max. It then draws

P_el = T·ω + P_loss(|ω|, |T|)

where P_loss is the *Power Loss (Motor + Inverter)* map. P_el is negative
while the motor recuperates: power goes back to the bus. The current is
I = P_el ÷ U.

A motor standing with no command has its inverter off and draws nothing,
at the start of a cycle as at every stop; commanded while it stands (held
by brakes, say), it draws the loss map's value at 0 1/min and that torque.

With the inverter off, it draws nothing and the *Drag Torque (unpowered)*
at |ω| acts against the motion. Below 0.1 rad/s (about 1 1/min) the drag
fades in proportion to the speed, so a rotor coming to rest settles at 0.

### Held to its supply

On a bus whose source is a Battery or a Fuel Cell Stack, the bus manager
gives the motor a window, the most it may draw (P_hi) and the most it may
feed back (P_lo), after the other loads (see the Battery's page,
[HV Battery Pack](battery.generic.md)). When its command asks for more,
the torque is cut to the largest share k of the command whose power fits:

k·T·ω + P_loss(|ω|, |k·T|) = P_hi

and recuperation is cut the same way to P_lo. The run summary gives the
time the motor was held back and the recuperation not recovered.

When P_hi is below even the spin losses (P_loss at 0 N·m, as with an empty
battery), the inverter stays on. The motor takes all of P_hi, and its shaft
pays the rest: a torque against the motion of (P_loss(|ω|, 0) − P_hi) ÷ |ω|,
at most its generator torque limit, fading below 0.1 rad/s. Below that
speed the losses the supply cannot pay are left out.

On a Voltage Source's bus nothing holds the motor back. Behind a DC-DC
Converter that holds its output voltage, the motor is held to what the
converter's input side can give, and it recuperates only what the loads on
its own bus use.

### What it tells the Driver

The motor gives the Driver its generator torque limit, T_fl·k_q4 (0 with
the supply below 1 V or above ω_max), so the Driver can blend recuperation
and friction brakes. This is the map's limit, not the supply window.

### Scaling

With k_T the *Torque Scale*, k_n the *Speed Scale* and k_U the *Voltage
Scale*: every speed in the maps × k_n, every torque × k_T ÷ k_n, every
loss × k_T, the full-load map's voltages × k_U, and the rotor inertia ×
k_T.

### Losses and stored energy

The energy view books as loss U·I − T·ω, the *Losses* output: the loss map
while powered, and the drag's power while coasting. It books ½·J·ω² as
stored.

### Edges of the maps

- *Full-Load Torque*: the voltage axis holds its end value by default
  (*Clamp*); the speed axis stops the run by default (*Error*). The map is
  read at most at ω_max, so the speed axis stops a run only if the map ends
  below ω_max, or starts above 0 1/min and the motor runs slower.
- *Power Loss (Motor + Inverter)*: both axes stop the run by default. It is
  read at the torque the command asks for and at the torque after a cut;
  if its torque axis ends below the full-load torque, a run that asks for
  that much stops. While the inverter is off the map is not used, so it
  does not stop the run.
- *Drag Torque (unpowered)*: holds its end value by default.

### Quick mode

In quick mode (*Run mode*: *Quick*) the command is not held to −1…1, the
inverter runs at a command of 0 (drawing nothing while the shaft stands),
and the supply window does not cut the torque: power beyond what the supply
gives is flagged as a warning, and recuperation beyond what it takes is
lost.
