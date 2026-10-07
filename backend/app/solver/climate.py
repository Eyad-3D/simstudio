"""Climate control as an electrical load (MOD-41, fidelity L0).

A Climate Control part turns the outside temperature into the electrical
power the cabin's heating or air-conditioning draws, before LightSim has a
cabin model (MOD-46). Its demand table gives the heat the cabin needs at
each outside temperature (heating positive, cooling negative, kW); a heat
source turns that into electrical power:

- a PTC heater (an electric resistance heater) turns every watt into heat:
  COP 1;
- a heat pump moves heat with a coefficient of performance (COP, heat moved
  per watt of electricity) of a share of the ideal (Carnot) COP between its
  two heat exchangers, as FASTSim's HVAC model does; below its minimum
  outside temperature the PTC heater takes over.

Cooling always runs the air-conditioning compressor (a heat pump run the
other way), so it uses the same Carnot share. The exchangers run warmer and
colder than the air on each side: the condenser APPROACH_K above the cabin
set point (heating) or the outside air (cooling), the evaporator APPROACH_K
below the outside air (heating) or the set point (cooling). With a
heat-pump quality of 0.35, heating at −7 °C has a COP of about 2.0 and
cooling at 35 °C about 2.2 (estimates).
"""
from __future__ import annotations

from dataclasses import dataclass

KELVIN = 273.15
APPROACH_K = 15.0  # exchanger temperature difference to the air it serves
PTC, HEAT_PUMP = "PTC heater", "Heat pump"


def carnot_share_cop(t_hot_c: float, t_cold_c: float, share: float, heating: bool) -> float:
    """A heat pump's COP: ``share`` × the Carnot COP between a hot and a
    cold exchanger (°C) — heat given off per watt for heating, heat taken
    in per watt for cooling. Never below 1 for heating (a heat pump is no
    worse than a resistance heater: below that a real one switches over)
    and never below 0.5 for cooling."""
    t_hot, t_cold = t_hot_c + KELVIN, t_cold_c + KELVIN
    lift = max(1.0, t_hot - t_cold)
    ideal = (t_hot if heating else t_cold) / lift
    return max(1.0 if heating else 0.5, share * ideal)


@dataclass
class ClimateState:
    """What the part did in the last solver step (for its channels and the
    energy book)."""
    heat_w: float = 0.0  # heat to the cabin (+) or taken from it (−), W
    cop: float = 0.0  # heat moved per watt of electricity (0 when off)
    asked_w: float = 0.0  # the electrical power that heat asks for, W
    heat_j: float = 0.0  # heating delivered over the run, J
    cool_j: float = 0.0  # cooling delivered over the run, J
    energy_j: float = 0.0  # electricity used over the run, J


def climate_power(demand_kw: float, t_out_c: float, p: dict) -> tuple[float, float]:
    """(electrical power in kW, COP) of a Climate Control asked for
    ``demand_kw`` of heat (+) or cooling (−) at ``t_out_c`` outside, with
    its parameters ``p``. Off (0 kW, COP 0) when nothing is asked for."""
    if demand_kw == 0.0:
        return 0.0, 0.0
    set_c = float(p.get("cabin_setpoint_C", 21))
    share = min(1.0, max(0.01, float(p.get("cop_carnot_share", 0.35))))
    fan = max(0.0, float(p.get("fan_power_kW", 0.2)))
    if demand_kw > 0:
        hp = (str(p.get("heat_source", PTC)) == HEAT_PUMP
              and t_out_c >= float(p.get("heat_pump_min_C", -10)))
        cop = (carnot_share_cop(set_c + APPROACH_K, t_out_c - APPROACH_K, share, True)
               if hp else 1.0)
    else:
        cop = carnot_share_cop(t_out_c + APPROACH_K, set_c - APPROACH_K, share, False)
    return abs(demand_kw) / cop + fan, cop
