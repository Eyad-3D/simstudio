## How it computes

The stack is a voltage that follows its *Polarization Curve*: at the
current I it gives (A, out of its positive terminal), its voltage is
U = U(I) from the curve, interpolated linearly. The circuit sets I: the
stack runs where U(I)·I is the power its bus takes. Its electrical power
is P = U·I (*Electrical Power*).

### Hydrogen and losses

It uses hydrogen in proportion to the energy it gives:

ṁ_H2 = k·P (and 0 while P ≤ 0)

with k the *Specific H₂ Consumption* (g/kWh). The hydrogen comes from the
Hydrogen Tank; without a tank, from an inexhaustible supply. Its heating
value is fixed at 119.96 MJ/kg, so the stack's efficiency is
1 ÷ (k × 119.96 MJ/kg): 54.6 % at the default 55 g/kWh. The energy view
books as loss the *Losses* output, ṁ_H2 × 119.96 MJ/kg − P; the stack
stores nothing.

### Its limits

The stack does not limit its own current. It hands its bus a window, as a
battery does (see the Battery's page, [HV Battery Pack](battery.generic.md)):

- it gives at most P_max = U(I_max)·I_max, with I_max the *Maximum
  Current* (A): 250 V × 400 A = 100 kW with the defaults;
- it gives nothing once its Hydrogen Tank is empty;
- it takes nothing back: the motors on its bus may recuperate only what
  the other loads there use at that moment.

The consumers, the motors and the DC-DC converters running at a power
setpoint that draw from its bus are held to that window. In the usual
layout, a DC-DC Converter at a power setpoint feeds a battery's bus from
the stack, and it is cut back with the stack's window.

A DC-DC Converter that holds its output voltage is held to the window too:
the consumers and the motors behind it are served together with the stack's
own, and it passes nothing back into the stack.

### Edges

- The curve's current axis stops the run by default (*Error*) where it is
  read past its data: a current above the curve's last point, a current
  below 0 (power fed back into the stack), or a *Maximum Current* beyond
  the curve, which is read there for the window.
- The stack has no ramp rate: its power can change at once.
