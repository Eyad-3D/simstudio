"""Build the library engine's default fuel map (CON-14) from the P2 Hybrid
Car's synthetic engine (docs/data-register.csv DR-29).

DR-29 is a Willans-line model of a hybrid petrol engine: fuel flow =
(brake torque + friction torque) x speed / (indicated efficiency x LHV), with
the friction 12 + 0.0012 n + 3e-7 n^2 N m and petrol at 42.9 MJ/kg. This script
reads the indicated efficiency back out of that map at each speed and share of
full load, and writes it for the library's larger engine (full-load curve
DR-11, 175 N m peak), whose friction is scaled with its peak torque. So the
default engine gets the same kind of map: a best-efficiency island near
2,000 1/min and 60-80 % load, rising consumption towards full load.

    python scripts/defaults/engine_fuel_map.py   # prints the map as JSON
"""
from __future__ import annotations

import json
from bisect import bisect_right
from math import pi
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LHV = 42.9e6  # J/kg, petrol, as DR-29
TORQUES = [0, 10, 20, 30, 45, 60, 80, 100, 120, 140, 160, 175]


def friction(n: float) -> float:
    return 12 + 0.0012 * n + 3e-7 * n * n


def interp(xs: list[float], ys: list[float], x: float) -> float:
    x = min(max(x, xs[0]), xs[-1])
    i = min(max(bisect_right(xs, x) - 1, 0), len(xs) - 2)
    return ys[i] + (ys[i + 1] - ys[i]) * (x - xs[i]) / (xs[i + 1] - xs[i])


def main() -> dict:
    hybrid = json.loads((ROOT / "backend/projects/hybrid-car.json").read_text(encoding="utf-8"))
    engine = next(e for s in hybrid["systems"] for e in s["elements"] if e["id"] == "el-engine")
    hmap = engine["parameterOverrides"]["fuel_map"]
    hfull = engine["parameterOverrides"]["full_load_torque"]
    lib = json.loads((ROOT / "backend/app/library/components.json").read_text(encoding="utf-8"))
    eng = next(c for c in lib["components"] if c["id"] == "engine.combustion")
    lfull = next(p for p in eng["parameters"] if p["key"] == "full_load_torque")["default"]

    hn = sorted(hfull, key=float)
    lfn = sorted(lfull, key=float)
    scale = max(lfull.values()) / max(hfull.values())  # friction grows with the engine
    out: dict[str, dict[str, float]] = {}
    for n_key in sorted(hmap, key=float):
        n = float(n_key)
        w = n * 2 * pi / 60
        th = sorted(hmap[n_key], key=float)
        tq = [float(t) for t in th]
        # indicated efficiency of the hybrid engine at each torque of its map
        eta = [(float(t) + friction(n)) * w * 3600 / (hmap[n_key][t] * LHV) for t in th]
        tmax_h = interp([float(x) for x in hn], [hfull[x] for x in hn], n)
        tmax_l = interp([float(x) for x in lfn], [lfull[x] for x in lfn], n)
        row = {}
        for t in TORQUES:
            share = t / tmax_l  # the same share of full load on the hybrid engine
            e = interp(tq, eta, share * tmax_h)
            row[str(t)] = round((t + friction(n) * scale) * w * 3600 / (e * LHV), 3)
        out[str(int(n))] = row
    return out


if __name__ == "__main__":
    fmap = main()
    print(json.dumps(fmap))
    best = min(((f / ((float(t) * float(n) * 2 * pi / 60) / 1000)) * 1000, n, t)
               for n, row in fmap.items() for t, f in row.items() if float(t) > 0)
    print(f"best: {best[0]:.0f} g/kWh at {best[1]} 1/min and {best[2]} N m")
