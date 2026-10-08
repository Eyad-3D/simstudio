---
name: lightsim-wiring-rules
description: The rules for wiring LightSim parts (electrical, mechanical) and linking signals on the data bus, and the mistakes Data Checks report. Use before adding or connecting parts, or when a check says something is unconnected, wired twice or has no Target Speed.
license: See README.md of the LightSim skill pack
metadata:
  version: "0.3.0"
  app: LightSim 0.3.0
---

# Wiring rules

LightSim has two kinds of connection:

- **Wires** join two ports of the same physical domain: electrical to
  electrical, mechanical to mechanical. They carry power both ways. A wire
  joins two parts of the same system.
- **Signals** go from a signal output to a signal input on the *data bus*.
  They carry one number one way (a speed, a command, a charge). Signals can
  cross system boundaries.

With `model_edit`, both are `connect` operations, with `from` and `to` as
`"element:port"`; two signal ports make a signal, anything else a wire.
`element_read` and `lightsim://components/<type>` list a part's port ids.

## The power path of an electric drive

1. Battery `pos` to the motor's `pos`, battery `neg` to the motor's `neg`.
   With more than one consumer, put an Electric Node (`electric.node`)
   between them: battery `pos` to node `t1`, node `t2` to the motor's
   `pos`, node `t3` to a Power Consumer's `pos`; every `neg` goes to a
   Ground (`boundary.ground`).
2. Motor `shaft` to Final Drive `flange_in`; Final Drive `flange_out` to a
   Differential `flange_in`.
3. Differential `flange_out_a` and `flange_out_b` each to a Mechanical
   Node (`mech.node`); each node to its Wheel's `shaft` and its Brake's
   `flange`.

One battery or voltage source per electrical bus; one E-Motor and one
differential per driveline. Two independent drivelines (front and rear
axle, each with its own motor) work.

## The signals every drive-cycle model needs

| Input | Source | Without it |
|---|---|---|
| Driver `sig_target_in` (Target Speed) | Driving Task `sig_demand` | The Driver holds 0 km/h |
| Driver `sig_speed_in` (Actual Speed) | Vehicle `sig_speed` | The Driver cannot follow the target |
| E-Motor `sig_demand_in` (Traction Command) | Driver `sig_traction_cmd` | The motor never pushes |
| Brake `sig_demand_in` (Brake Command) | Driver `sig_brake_cmd` | Only the motor brakes (recuperation) |

The Vehicle needs no wire: the wheels push it. An *Acceleration* case
needs no Target Speed (the Driver holds full throttle).

## What Data Checks say when wiring is wrong

- *Driver '…' has no Target Speed signal — it will hold 0 km/h*: link
  the Driving Task's `sig_demand` to the Driver's `sig_target_in`.
- *E-Motor '…' has no Traction Command signal — it will never produce
  torque* (an Engine: *no Throttle signal — it will only idle*): link the
  Driver's `sig_traction_cmd`, or a controller's output, to it.
- *E-Motor '…' has no live electrical connection* or *has no power
  source*: wire its `pos` to a bus with a battery, fuel cell or voltage
  source.
- *… is not connected to any wheel — it cannot move the vehicle*: finish
  the driveline to the wheels.
- *'…' is not connected to anything, so the simulation leaves it out*:
  wire it in or remove it.
- *… has 2 sources: … — an input takes one*: remove one signal; an input
  takes one source.
- *Wheel load shares add up to … %, not 100 %*: each Wheel's
  `vehicle_load_share_pct` is the part of the weight it carries; together
  they make 100 %.
- *A bus with more than one primary source*: one battery, fuel cell or
  voltage source per bus; split buses with a DC-DC Converter.
- An input with no source reads 0. That is fine for inputs you do not use.

Run `run_checks` after every batch of edits; fix every error before a run.
