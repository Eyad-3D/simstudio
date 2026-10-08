---
name: lightsim-build-a-p2-hybrid
description: Build or adapt a parallel (P2) hybrid car in LightSim, with an engine, clutch, e-motor, gearbox and a control script. Use when a user asks about hybrids, fuel consumption with an engine, engine start/stop or a hybrid control strategy.
license: See README.md of the LightSim skill pack
metadata:
  version: "0.3.0"
  app: LightSim 0.3.0
---

# Build a P2 hybrid

A P2 hybrid has its e-motor between the clutch and the gearbox: the engine
can be declutched and stopped, and the motor drives, recuperates and helps.
LightSim has no built-in hybrid controller: a Script block decides when
the engine runs, the clutch closes, which gear is in and how the torque is
split.

## Start from the example

`example:hybrid-car` (P2 Hybrid Car, sized after the Hyundai Ioniq
Hybrid) is complete and checked: its EPA city (UDDS) case ends near
2.84 l/100 km. Copy it and change values, rather than building from
nothing: its *Hybrid Control Unit* script is about 100 lines and tuned
together with the parts around it. Read it with
`element_read` (element "Hybrid Control Unit").

## The structure (27 parts in the example)

Wires:

- Engine (`engine.combustion`) `shaft` → Clutch (`mech.clutch`)
  `flange_a`; Clutch `flange_b` → a Mechanical Node (`mech.node`) `f1`.
- E-Motor `shaft` → the same node's `f2`; node `f3` → Gearbox
  (`mech.gearbox`) `flange_in`; Gearbox `flange_out` → Final Drive →
  Differential → wheel nodes → Wheels and Brakes, as in a BEV.
- Battery → Electric Node → E-Motor and a 12 V load; every `neg` to Ground.
- A Fuel Tank (`fuel.tank`) needs no wire: the engine draws from it.

Signals: the Driver's `sig_traction_cmd` goes to the **script**, not to
the motor. The script's outputs drive the parts:

| Script port | Direction | Linked to |
|---|---|---|
| `soc` | input | Battery `sig_soc` (in %, 0-100) |
| `speed` | input | Vehicle `sig_speed` (km/h) |
| `traction` | input | Driver `sig_traction_cmd` (-1 to 1) |
| `motor_rpm`, `engine_rpm` | input | E-Motor and Engine `sig_speed` |
| `motor_cmd` | output | E-Motor `sig_demand_in` |
| `throttle` | output | Engine `sig_throttle_in` |
| `engine_on` | output | Engine `sig_on_in` |
| `clutch_cmd` | output | Clutch `sig_engage_in` |
| `gear` | output | Gearbox `sig_gear_in` |

Add a script's ports with `model_edit`'s `add_port` operation, then
`connect` them. See `lightsim-write-a-script-block` for the code.

## What the strategy must do

- Hold the battery's charge over the cycle (charge-sustaining): run the
  engine harder when the SOC is low, and drive electrically when it is
  high. The example targets 55 %.
- Avoid switching the engine on and off every step: use minimum on and off
  times, and ask for a switch for a short time in a row (the example uses
  8 s, 4 s and 0.2 s).
- Shift on vehicle speed with hysteresis (the example: up at 18, 30, 45,
  58 and 70 km/h, down 6 km/h lower).
- Open the clutch when the engine is off; with the engine off and the
  clutch open, keep the throttle at 0 (a declutched engine with throttle
  runs to its rev limiter: see `lightsim-what-it-cannot-do`).

## Compare fairly

A hybrid's fuel figure means little if the battery ends the cycle fuller
or emptier than it started. Report the start and end SOC with the fuel
consumption, and adjust the case's start SOC (Battery `initial_soc_pct`,
set per case) until they match within about 0.5 points, as the example's
cases do.
