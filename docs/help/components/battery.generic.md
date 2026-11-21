## How it computes

The battery is an equivalent circuit: a voltage that follows the state of
charge (SOC, how full the battery is), a series resistance R0 and, if you
set one, an RC pair (a resistance with a capacitor across it, for the slow
part of the voltage sag). With *Defined By*: *Pack values* you give the
pack's own values and it has no temperature; with *Cells* LightSim builds
it from a cell and a layout (see *Defined By: Cells* below).

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

A DC-DC converter that holds its output voltage and draws from the bus
counts in step 1 for the consumers behind it and in step 2 for the driving
motors behind it. It feeds nothing back.

### Losses and stored energy

The energy view books as loss R0·I² + U₁² ÷ R1, plus (1 − η_c)·OCV·|I|
while charging (the charge not stored). It books as stored the chemical
energy, ∫OCV·dQ over the charge stored, plus ½·C1·U₁². The *Losses* output
is the cells' power less the terminal power (OCV·I discharging,
η_c·OCV·I charging, minus U·I); it differs from the booked loss only by the
power that charges or empties the RC pair.

### Defined By: Cells

With *Cells* the pack is Ns cells in series × Np in parallel (*Cells in
Series (s)* and *Cells in Parallel (p)*, each rounded to a whole number and
at least 1). Everything above holds, with these quantities taken from the
cells. The Pack values' *Usable Capacity*, *Charge Capacity*,
*Open-Circuit Voltage*, *Series Resistance R0* and pack current and voltage
limits are hidden and not used.

- **Charge**: Q = Np × *Cell Capacity* (Ah).
- **Open-circuit voltage**: OCV = Ns × v_cell, with v_cell the *Cell
  Open-Circuit Voltage* table at the SOC.
- **R0 changes during the run**:

  `R0 = Ns·R_cell ÷ Np + Ns·R_ic + R_ct`

  R_ic is *Interconnect Resistance (per series group)* and R_ct *Contactor
  and Cable Resistance*. A cell's resistance is

  `R_cell = R_dc × f(pulse, SOC) × f_T(T)`

  with R_dc the *Cell DC Resistance* (at least 1 nΩ), f the *Cell
  Resistance Factor* table and f_T the *Cell Temperature Factor* table.
- **The pulse** is the time since the current last changed direction. A
  new pulse starts when the current turns from charging to discharging or
  back. A pause at zero current does not start one: when the current flows
  the same way again, the old pulse goes on counting. Before the first
  pulse, f is read at a pulse of 0 s; with the axis set to *Clamp* (the
  default) that is the table's first pulse length, 2 s in the default
  table.
- **Temperature**: the cells are at the first Ambient's temperature, 20 °C
  without an Ambient. They do not warm up under load.
- Each table axis has its own outside-the-data setting: *Clamp* holds the
  edge value, *Linear* extends the edge slope, *Error* stops the run.

**The cells' limits** take the place of the pack current and voltage
limits (I_dis and I_ch above). Each is a cell's value × Np for the pack:

- **Discharge current**: *Cell Continuous Discharge Current*, or *Cell
  Peak Discharge Current* while a discharge pulse is no longer than *Peak
  Duration*. While the pack charges, or before its first pulse, the peak
  value applies: it is the limit for a discharge that starts now. A peak of
  0 means the continuous current throughout; a continuous current of 0
  means no limit once the Peak Duration has passed; both 0, no current
  limit.
- **Charge current**: the same with *Cell Continuous Charge Current* and
  *Cell Peak Charge Current*.
- With only a peak current set that way (no continuous current and no cell
  voltage limit), once the Peak Duration has passed the pack is as if it had
  no limit that way: the *SOC Derating Band* lowers the maximum-power point
  (discharging) or the current at *Max Charge Power* (charging), and without
  a band the limit output reads 0.
- **Cell voltage**: a cell's terminal voltage is

  `v = v_cell − U₁ ÷ Ns − (I ÷ Np)·R_cell`

  (the RC pair's voltage shared evenly by the series cells).
  *Cell Minimum Voltage* holds the discharge current to at most
  (v_cell − U₁ ÷ Ns − V_min)·Np ÷ R_cell, and *Cell Maximum Voltage* the
  charge current to at most (V_max − v_cell + U₁ ÷ Ns)·Np ÷ R_cell, never
  below 0 (0 = no limit). R_cell is that of the pulse going on when it goes
  that way, else that of a pulse starting now. The interconnects and
  contactors are outside the cells and do not count here.

The *SOC Derating Band*, the *Minimum SOC* and full, the maximum-power
point, *Max Charge Power* and the *Output Power Limit* then apply as for
Pack values, with this R0.

**The weakest group.** With *Weakest Group Capacity* or *Weakest Group
Resistance* not at 100 %, one series group differs from the others:

- It has its own SOC, starting at *Initial SOC*, with a charge of Q × its
  capacity share (at least 0.1 %). The same current flows through it, so
  its SOC moves faster.
- Its open-circuit voltage stands in for one cell's: OCV = (Ns − 1)·v_cell
  + v_weak, with v_weak the cell table at its own SOC.
- Its resistance is R_cell × its resistance share (at least 50 %), so
  R0 = (Ns − 1 + k)·R_cell ÷ Np + Ns·R_ic + R_ct, k that share.
- Its cell voltage limits hold the current too, worked out with v_weak and
  k·R_cell.
- The pack stops discharging when the pack's SOC or the group's reaches
  *Minimum SOC*, and stops charging when either reaches 100 % (the same
  10 ms rule as above).
- The derating band's discharge factor uses the lower of the two SOCs; its
  charge factor uses the pack's SOC.
- The *State of Charge* output is the pack's (the normal groups') SOC.

**The outputs only Cells has.**

- *Lowest Cell Voltage* and *Highest Cell Voltage*: v for the normal cells
  and for the weakest group; without a weakest group both are the same.
- *Discharge Power Limit* and *Charge Power Limit*, each for 2 s, 10 s and
  30 s (the state of power): the most power the pack can give or take in a
  pulse of that length d starting now. The cell resistance is read at a
  pulse of d; the RC pair's voltage now fades to U₁·e^(−d/τ) and the RC
  pair adds R1·(1 − e^(−d/τ)) to R0; the peak currents count when d is no
  longer than *Peak Duration*, else the continuous ones; the cell voltage
  limits and the derating band count, and the discharge value is at most
  the maximum-power point's. With A_d = OCV − U₁·e^(−d/τ), R_d that
  resistance and I the current allowed: P_dis = I·(A_d − R_d·I) and P_ch =
  I·(A_d + R_d·I). They leave out the *Minimum SOC*, *Max Charge Power* and
  the *Output Power Limit*. A charge value is 0 when no charge current or
  voltage limit applies to that pulse length.
- A Battery with *Pack values* does not have these eight outputs. If you
  link one, the run is refused: "Port '…' of '…' is linked, but its block
  has no such signal port."

The losses (R0·I² with this R0) are one *internal losses* figure: the
cells', the weakest group's, the interconnects' and the contactors'
together. The run summary gives the layout (cells), the charge capacity
(Ah), an estimate of the pack's mass (Ns × Np × *Cell Mass* × *Packaging
Factor*, at least 1) and the lowest and highest cell voltage of the run.

### Quick mode

In quick mode (*Run mode*: *Quick*) the window does not hold the motors;
power beyond it is flagged as a warning, and so is an SOC below *Minimum
SOC* or above 100 %.

### Not used by the run

*Charger Efficiency*, *Power Check Window (0 = instant)*, *Voltage Class
(0 = none)*, *Cell Mass* and *Packaging Factor* do not change the run; the
run summary uses them.
