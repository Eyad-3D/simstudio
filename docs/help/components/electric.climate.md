## How it computes

The outside temperature T_out is the first Ambient's *Temperature*, or
20 °C without an Ambient. The *Enable* input switches the part on at 0.5 or
more (always on when unwired).

### Heat, COP and electrical power

The heat the cabin needs, Q (positive heating, negative cooling), is the
*Heating/Cooling Demand* table (kW) at T_out, interpolated linearly, and 0
while switched off.

The COP (coefficient of performance, the heat moved per unit of electrical
power) comes from the temperatures in kelvin, with T_set the *Cabin Set
Point* and *s* the *Heat Pump Quality (share of Carnot)*. Each exchanger
runs 15 K past the air it serves, and the lift T_h − T_c is at least 1 K:

- Heating with the PTC heater, or with the heat pump below *Heat Pump
  Minimum Outside Temperature*: COP = 1.
- Heating with the heat pump: T_h = T_set + 15 K, T_c = T_out − 15 K,
  COP = max(1, s·T_h ÷ (T_h − T_c)).
- Cooling (always the air-conditioning): T_h = T_out + 15 K,
  T_c = T_set − 15 K, COP = max(0.5, s·T_c ÷ (T_h − T_c)).
- No heat asked for: COP = 0.

The electrical power it asks for is

P = |Q| ÷ COP + P_blower

while Q is not 0, and 0 otherwise, with P_blower the *Blower and Controls*
(kW). With the defaults at −7 °C, Q = 4.05 kW: the PTC heater draws
4.25 kW, the heat pump (COP 1.87) 2.37 kW. At 35 °C, Q = −2.65 kW and the
air-conditioning (COP 2.22) draws 1.39 kW.

The *Cabin Set Point* changes only the COP. The demand table is for one set
point: change both together.

### When its source falls short

On a bus whose source is an HV Battery Pack or a Fuel Cell Stack, the part
is one of the consumers that the bus serves first, all cut back by one
share s when the source cannot carry them (see the Battery's page,
[HV Battery Pack](battery.generic.md)). Then *Drawn Power* = s·P and *Heat
to Cabin* = s·Q: it heats or cools that much less. On other buses s = 1.

### Losses and edges

- The energy view books all its electrical power as loss; it stores
  nothing. The heat a heat pump moves from outside is not in the books.
- Past the table's temperatures the default (*Clamp*) holds the end value:
  6 kW below −20 °C and −3.4 kW above 40 °C with the default table. An axis
  set to *Error* stops the run there, but only while the part is on.
- Between 20 and 24 °C the default table asks for nothing, so the part,
  blower included, draws nothing.
- A Climate Control wired to nothing fails the build (`STRUCT-OVER`).
