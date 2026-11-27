## How it computes

The Ambient holds its *Temperature* T and *Pressure* p for the whole run
and gives the air density:

`ρ = p / (R·T)`

with p in Pa, T in K (°C + 273.15) and R = 287.05 J/(kg·K). At 20 °C and
101.325 kPa this is 1.204 kg/m³.

What reads it:

- the [Vehicle](vehicle.body.md): its air drag, its coefficient C and its
  downforce all scale with ρ;
- each Climate Control: its outside temperature is T (20 °C without an
  Ambient).

Its *Thermal Port* holds the temperature T. No other part in this
version's library has a heat port, so nothing exchanges heat with it.

The Ambient stores and loses no energy.

## Presets

**Presets**, at the top of the Ambient's values, sets *Temperature* and
*Pressure* to a test's weather: *Cold day (−7 °C)*, *Standard day (23 °C)*,
*Hot and sunny day (35 °C)* or *High altitude (1,500 m)*. Choosing one shows
the values it sets and where they come from; **Apply** sets them in one step
(one undo), **Cancel** leaves the values as they were.
