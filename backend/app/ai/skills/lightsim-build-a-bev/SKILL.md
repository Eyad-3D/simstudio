---
name: lightsim-build-a-bev
description: Build a battery electric car (BEV) in LightSim from scratch, or adapt the Battery Electric Car example. Use when a user asks to model an electric car, change its battery, motor or gearing, or start a new EV model.
license: See README.md of the LightSim skill pack
metadata:
  version: "0.3.0"
  app: LightSim 0.3.0
---

# Build a battery electric car

## Fastest start: copy the example

`example:bev-car` (Battery Electric Car, after a 2021 Cupra Born) is a
complete, checked BEV with four wheels and brakes, a differential and an
auxiliary load. Read it with `lightsim_overview`, then change its values
with `model_edit` (an example is saved as a new project of the user's). Do
this unless the user wants to learn by building.

## From scratch: the smallest car that drives

Seven parts, four wires, three signals. With the LightSim MCP tools, one
`model_edit` call on `project: "new:My EV"` builds it:

```json
[
  {"op": "add", "type": "vehicle.body", "label": "Vehicle"},
  {"op": "add", "type": "signal.driving_task", "label": "Driving Task"},
  {"op": "add", "type": "driver.driver", "label": "Driver"},
  {"op": "add", "type": "battery.generic", "label": "Battery"},
  {"op": "add", "type": "motor.emotor", "label": "E-Motor"},
  {"op": "add", "type": "mech.final_drive", "label": "Final Drive"},
  {"op": "add", "type": "propulsion.wheel", "label": "Wheel"},
  {"op": "connect", "from": "Battery:pos", "to": "E-Motor:pos"},
  {"op": "connect", "from": "Battery:neg", "to": "E-Motor:neg"},
  {"op": "connect", "from": "E-Motor:shaft", "to": "Final Drive:flange_in"},
  {"op": "connect", "from": "Final Drive:flange_out", "to": "Wheel:shaft"},
  {"op": "connect", "from": "Driving Task:sig_demand", "to": "Driver:sig_target_in"},
  {"op": "connect", "from": "Vehicle:sig_speed", "to": "Driver:sig_speed_in"},
  {"op": "connect", "from": "Driver:sig_traction_cmd", "to": "E-Motor:sig_demand_in"},
  {"op": "set", "element": "Wheel", "param": "vehicle_load_share_pct", "value": 100}
]
```

Run it as a dry run first (the default), show the user the changes and the
Data Checks, then save with `dry_run: false` (the user confirms). Its
*Case 1* (600 s of the default town profile) ends *success*: 7.292 km at
about 11.0 kWh/100 km, with the library's default values.

One Wheel stands for the whole axle, so it carries 100 % of the weight.
With four wheels, each carries its share (the four add up to 100 %), and
the front and rear axles each need a Differential and Mechanical Nodes as
in the example.

## Make it a real car

Set these from the car's data sheet (units as listed; see
`lightsim-units-and-parameters`):

| Part | Parameter (`key`) | Where the number comes from |
|---|---|---|
| Vehicle | `mass_kg`, `cd`, `frontal_area_m2` | Test mass, drag coefficient, frontal area |
| Battery | `capacity_kWh` (usable), `min_soc_pct` | Usable energy, the lowest charge the car allows |
| E-Motor | `full_load_torque` table, `max_speed_rpm` | Torque curve over speed (per voltage) |
| Final Drive | `ratio` | Gear ratio motor to wheel |
| Wheel | `radius_m`, `rolling_resistance` | Tyre size; rolling resistance coefficient |
| Driving Task | `cycle` | A standard cycle such as `wltc-3b` |

Read a part with `element_read` before you change it: it lists every
parameter with its unit, default, limits and a note on where to find the
real value.

## Then

Follow `lightsim-verify-a-model` before you report any number.
