"""Resizing a motor or engine with scale factors (MOD-47).

Sizing sweeps resize machines; multiplying the torque alone makes small
motors too efficient and large ones too lossy. These rules rescale a
machine's maps together, so a sweep stays physically sensible within about
0.5-2 times the original (Data Checks warn outside it). After Stipetic et
al., "Scaling laws for synchronous permanent magnet machines" (IET Electric
Power Applications, 2016; not re-checked at its source), and EPA ALPHA's
engine_scale keys:

E-Motor
- Torque Scale k_T: a longer (or shorter) machine of the same cross-section.
  Torque × k_T at every speed; the loss at k_T × a torque is k_T × the loss
  at that torque (copper and iron both grow with the active length); drag
  torque × k_T; rotor inertia × k_T.
- Speed Scale k_n: the machine rewound (fewer turns of thicker wire) to run
  k_n × as fast at the same voltage and power: every speed × k_n, torque
  ÷ k_n, and the loss at a point equal to the original's at the matching
  point (as if through an ideal gear: the machine's own frequencies are its
  own speed's, so iron loss rises with speed, not more); maximum speed × k_n;
  rotor inertia unchanged (the same rotor turns faster).
- Voltage Scale k_V: rewound for k_V × the voltage at the same torque and
  speed: the full-load map's voltage axis × k_V; losses unchanged.

Combustion Engine
- Engine Scale k: displacement × k at the same brake mean effective pressure
  and speeds: torque, drag and fuel flow × k at k × the torque, so the brake
  specific fuel consumption map is unchanged; inertia × k. (ALPHA's BSFC
  adjustment for small engines is not applied.)

A machine at 100 % on every scale reads its tables as typed.
"""
from __future__ import annotations

MOTOR_KEYS = ("full_load_torque", "power_loss", "drag_torque")
ENGINE_KEYS = ("full_load_torque", "drag_torque", "fuel_map")
VALID_RANGE = (0.5, 2.0)  # the scale factors' sensible range


def _pct(p: dict, key: str) -> float:
    try:
        v = float(p.get(key, 100) or 0)
    except (TypeError, ValueError):
        v = 100.0
    return max(0.01, v / 100.0) if v > 0 else 1.0


def motor_scales(p: dict) -> tuple[float, float, float]:
    """An E-Motor's (k_T, k_n, k_V)."""
    return _pct(p, "torque_scale_pct"), _pct(p, "speed_scale_pct"), _pct(p, "voltage_scale_pct")


def engine_scale(p: dict) -> float:
    return _pct(p, "engine_scale_pct")


def scaled(part: str, key: str, pts: list, p: dict) -> list:
    """A motor's or engine's parsed table ``key`` as its scale factors make
    it; any other table as it is."""
    if part == "motor.emotor" and key in MOTOR_KEYS:
        k_t, k_n, k_v = motor_scales(p)
        if k_t == k_n == k_v == 1.0:
            return pts
        if key == "full_load_torque":  # voltage → (speed → torque)
            return [(v * k_v, [(n * k_n, t * k_t / k_n) for n, t in row]) for v, row in pts]
        if key == "power_loss":  # speed → (torque → loss)
            return [(n * k_n, [(t * k_t / k_n, w * k_t) for t, w in row]) for n, row in pts]
        return [(n * k_n, t * k_t / k_n) for n, t in pts]  # drag torque
    if part == "engine.combustion" and key in ENGINE_KEYS:
        k = engine_scale(p)
        if k == 1.0:
            return pts
        if key == "fuel_map":  # speed → (torque → fuel flow)
            return [(n, [(t * k, f * k) for t, f in row]) for n, row in pts]
        return [(n, t * k) for n, t in pts]
    return pts


def max_speed_rpm(p: dict) -> object:
    """An E-Motor's Maximum Speed as its Speed Scale makes it (0 stays 0:
    the scaled full-load curve's last speed)."""
    try:
        n = float(p.get("max_speed_rpm", 0) or 0)
    except (TypeError, ValueError):
        return p.get("max_speed_rpm", 0)
    return n * motor_scales(p)[1] if n > 0 else 0


def inertia_scale(part: str, p: dict) -> float:
    """What a motor's or engine's rotor inertia is multiplied by."""
    if part == "motor.emotor":
        return motor_scales(p)[0]
    if part == "engine.combustion":
        return engine_scale(p)
    return 1.0


def scale_keys(part: str) -> tuple[str, ...]:
    return {"motor.emotor": ("torque_scale_pct", "speed_scale_pct", "voltage_scale_pct"),
            "engine.combustion": ("engine_scale_pct",)}.get(part, ())
