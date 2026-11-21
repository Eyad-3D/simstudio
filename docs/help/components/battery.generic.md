## How it computes

The battery is an equivalent circuit: a voltage that follows the state of
charge (SOC, how full the battery is), a series resistance R0 and, if you
set one, an RC pair (a resistance with a capacitor across it, for the slow
part of the voltage sag). It has no temperature. This version runs only
*Defined By*: *Pack values* (see the end of this section).

### Charge and state of charge

The current I is positive while the pack discharges. The SOC counts the
charge that flows:

- discharging: dSOC/dt = −I ÷ Q
- charging: dSOC/dt = −η_c·I ÷ Q

Q is the charge capacity, in A·s. It is *Charge Capacity (0 = from Usable
Capacity)* (Ah) when that is above 0. At 0 it is *Usable Capacity* (kWh)
divided by the mean of the *Open-Circuit Voltage* table over 0–100 % SOC:
the default 60 kWh on the default table (mean 345 V) gives 173.9 Ah. η_c is
*Coulombic Efficiency (charging)* (%), the share of the charging current
that is stored. The SOC starts at *Initial SOC* (%).

The open-circuit voltage OCV (the voltage with no current) is the
*Open-Circuit Voltage* table at the SOC, interpolated linearly. The table
is read at the SOC held to 0–100 %. If your table does not reach 0 % or
100 %, its axis' outside-the-data setting decides: *Clamp* (the default)
holds the end value, *Linear* extends the end slope, *Error* stops the run.

### Terminal voltage

U = OCV − U₁ − R0·I

R0 is *Series Resistance R0* (Ω), at least 1 µΩ. U₁ is the RC pair's
voltage. With *RC Pair Resistance R1* (Ω) and *RC Pair Time Constant* (s)
both above 0:

τ·dU₁/dt = R1·I − U₁

where τ is the time constant and the capacitance is C1 = τ ÷ R1. With
either at 0, U₁ = 0. The *Discharge Power* output is P = U·I, negative
while the pack charges.

### What the pack can give and take

The battery does not stop its own current. At every moment it works out
the most power its terminals can give, P_deliver, and the most they can
take, P_absorb, and hands both to its bus (next section). With
A = OCV − U₁, the voltage behind R0:

- **Maximum-power point**: I_mpp = A ÷ (2·R0), where the terminal voltage
  is A ÷ 2 and the power A² ÷ (4·R0) (about 420 kW for the default pack at
  90 % SOC).
- **Discharge current limit** I_dis: *Max Discharge Current (0 = none)*
  (A); with *Minimum Pack Voltage (0 = none)* (V) set, at most
  (A − U_min) ÷ R0, so the terminals stay at U_min.
- **Charge current limit** I_ch: *Max Charge Current (0 = none)* (A); with
  *Maximum Pack Voltage (0 = none)* (V) set, at most (U_max − A) ÷ R0.
- **SOC Derating Band** (%), when above 0: the discharge limit × k_dis,
  with k_dis = (SOC − SOC_min) ÷ band held to 0–1, and the charge limit ×
  k_ch, with k_ch = (100 % − SOC) ÷ band held to 0–1. Without a discharge
  current or voltage limit, the band scales I_mpp instead; without a charge
  current or voltage limit, it scales the current at which the pack takes
  *Max Charge Power*.
- **Minimum SOC and full**: at most I_floor = (SOC − SOC_min)·Q ÷ 10 ms
  discharging and I_full = (100 % − SOC)·Q ÷ (η_c·10 ms) charging. These
  are the currents that would reach *Minimum SOC* (%) or 100 % within
  10 ms, so the window closes smoothly in the last moments before the
  limit, with no switch.

Then:

- P_deliver = I_d·(A − R0·I_d), with I_d the smallest of I_mpp, I_dis and
  I_floor, and at most *Output Power Limit (0 = none)* (kW) × (1 − *Power
  Limit Margin*) when the limit is above 0 and *Hold Power to Limit (off =
  check)* is ticked. Unticked, the limit is not held; the run only counts
  the time the power is above it.
- P_absorb = I_c·(A + R0·I_c), with I_c the smaller of I_ch and I_full,
  and at most *Max Charge Power* (kW).

The *Discharge Current Limit* and *Charge Current Limit* outputs show I_dis
and I_ch, and 0 when no current, voltage or derating limit is set.

### How the window reaches the loads

For each bus whose source is a battery (or a Fuel Cell Stack), LightSim adds
a bus manager that is not drawn on the diagram. A bus is everything wired
to the source's positive terminal, through Electric Nodes too.

1. The consumers are served first: Power Consumers, Climate Controls and
   DC-DC converters running at a power setpoint that draw from the bus. If
   they ask for more than P_deliver, each gets the same share,
   P_deliver ÷ their demand.
2. The motors that drive share what is left. Power that other motors feed
   back at that moment counts toward it. If they ask for more, each gets the
   same share of its request; if not, each may draw its request plus the
   room left over.
3. The motors that recuperate may feed back P_absorb plus what the
   consumers and the driving motors use at that moment. If they would feed
   back more, each gets the same share of its feedback.
4. A DC-DC converter at its power setpoint that feeds the bus adds its
   power to what the bus gets, and delivers at most its share of P_absorb
   plus what the bus's loads use. So it throttles back as the pack fills.

A DC-DC converter that holds its output voltage and draws from this bus is
not part of the window: what it draws comes on top, and can take the pack
past its limits, the *Minimum SOC* included.

### Losses and stored energy

The energy view books as loss R0·I² + U₁² ÷ R1, plus (1 − η_c)·OCV·|I|
while charging (the charge not stored). It books as stored the chemical
energy, ∫OCV·dQ over the charge stored, plus ½·C1·U₁². The *Losses* output
is the cells' power less the terminal power (OCV·I discharging,
η_c·OCV·I charging, minus U·I); it differs from the booked loss only by the
power that charges or empties the RC pair.

### Quick mode

In quick mode (*Run mode*: *Quick*) the window does not hold the motors;
power beyond it is flagged as a warning, and so is an SOC below *Minimum
SOC* or above 100 %.

### Not in this version

- *Defined By*: *Cells* is refused: the run fails with `Not run [NEW-CELLS]`.
- The *Discharge Power Limit* and *Charge Power Limit* outputs (2 s, 10 s,
  30 s) and *Lowest Cell Voltage* and *Highest Cell Voltage* belong to
  *Cells*. If you link one, the run is refused: "Port '…' of '…' is linked,
  but its block has no such signal port."
- *Charger Efficiency*, *Power Check Window (0 = instant)* and *Voltage
  Class (0 = none)* do not change the run; the run summary uses them.
