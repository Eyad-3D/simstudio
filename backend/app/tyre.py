"""A tyre from its size code and load index (MOD-48).

Reads ISO metric codes such as '205/55 R16 91V' (also 'P205/55R16',
'225/45ZR17 94W XL', 'LT245/75R16 120/116S', '195/75 R16C 107/105R') and
flotation codes such as '20.5x7.0-13' (Formula Student tyres, inches). From
the size: the unloaded radius, rim ÷ 2 + width × aspect ratio (metric) or
the overall diameter ÷ 2 (flotation). From the load index: the most a tyre
may carry, from the standard load-index table (0-279). From both: tyre-model
estimates after Rill's "engineer's guess" for a passenger-car tyre, as
Project Chrono's TMeasy tyre implements it (GuessPassCar70Par; Chrono is
BSD-3-Clause, see THIRD-PARTY-NOTICES.txt): its nominal load is half the
load-index capacity, and at that load its initial slip stiffness is 18.37
times the load, its peak longitudinal friction 1.129 and its peak lateral
friction 1.001; at twice that load 1.090 longitudinal, which gives the load
sensitivity.

frontend/src/tyre.ts fills a Wheel's fields from a code the same way;
Data Checks (validation.py) read it here to warn about overloaded wheels.
"""
from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path

GRAVITY = 9.81
INCH = 0.0254

_DATA = json.loads((Path(__file__).parent / "library" / "tyres.json").read_text(encoding="utf-8"))
# Load index → maximum load per tyre, kg (0-279): the ISO 4000-1 / ETRTO
# table, as in Chrono's ChTMeasyTire::GetTireMaxLoad
LOAD_INDEX_KG: tuple[float, ...] = tuple(_DATA["load_index_kg"])
# TMeasy passenger-car guess (Chrono GuessPassCar70Par): at the nominal load
# pn, initial slip stiffness ÷ pn, peak longitudinal and lateral force ÷ pn;
# at 2 pn, peak longitudinal force ÷ 2 pn
_TM = _DATA["tmeasy_passenger_car"]
DFX0_PN, FXM_PN, FYM_PN, FXM_P2N = _TM["dfx0_pn"], _TM["fxm_pn"], _TM["fym_pn"], _TM["fxm_p2n"]
# EU tyre label (Regulation (EU) 2020/740, Annex I Part A, C1 tyres): the
# rolling resistance LightSim takes for each class, N/kN
LABEL_RRC: dict[str, float] = dict(_DATA["eu_label_rrc_n_per_kn"])

_METRIC = re.compile(
    r"^(P|LT|ST|T)?\s*(\d{3})\s*/\s*(\d{2,3})\s*(?:Z?R|-|D|B|ZR)\s*(\d{2}(?:\.\d)?)\s*(C)?"
    r"(?:\s*(\d{2,3})(?:\s*/\s*\d{2,3})?\s*\(?([A-Z]\d?)?\)?)?(?:\s*(?:XL|RF|EXTRA\s*LOAD|REINF))?\s*$",
    re.IGNORECASE)
_FLOTATION = re.compile(r"^(\d{2}(?:\.\d)?)\s*[xX×]\s*(\d{1,2}(?:\.\d{1,2})?)\s*-\s*(\d{2})\b.*$")


@dataclass
class TyreSpec:
    code: str
    width_m: float
    unloaded_radius_m: float
    rim_m: float
    load_index: int | None = None
    speed_symbol: str | None = None

    @property
    def max_load_n(self) -> float | None:
        """The most one tyre may carry by its load index, N."""
        if self.load_index is None:
            return None
        return LOAD_INDEX_KG[min(self.load_index, len(LOAD_INDEX_KG) - 1)] * GRAVITY


def parse_tyre_code(code: str) -> TyreSpec | None:
    """The tyre a size code describes, or None when it is not one."""
    text = " ".join(str(code or "").split())
    m = _METRIC.match(text)
    if m:
        width = int(m.group(2)) / 1000.0
        aspect = int(m.group(3)) / 100.0
        rim = float(m.group(4)) * INCH
        li = int(m.group(6)) if m.group(6) else None
        if li is not None and li >= len(LOAD_INDEX_KG):
            li = None
        return TyreSpec(code=text, width_m=width, unloaded_radius_m=rim / 2 + width * aspect,
                        rim_m=rim, load_index=li,
                        speed_symbol=m.group(7).upper() if m.group(7) else None)
    m = _FLOTATION.match(text)
    if m:
        return TyreSpec(code=text, width_m=float(m.group(2)) * INCH,
                        unloaded_radius_m=float(m.group(1)) * INCH / 2,
                        rim_m=float(m.group(3)) * INCH)
    return None


def tyre_values(spec: TyreSpec, rolling_radius_factor: float = 0.97) -> dict[str, float]:
    """A Wheel's parameter values from a tyre: its rolling radius, and with a
    load index the TMeasy estimates (slip stiffness, friction, load
    sensitivity about its nominal load)."""
    out = {"radius_m": round(spec.unloaded_radius_m * rolling_radius_factor, 4)}
    if spec.max_load_n is not None:
        pn = 0.5 * spec.max_load_n
        out.update({
            "slip_stiffness": DFX0_PN,
            "mu": FXM_PN,
            "mu_lateral": FYM_PN,
            "mu_nominal_load_N": round(pn, 1),
            # per kN: from 1.129 at pn to 1.090 at 2 pn
            "mu_load_sensitivity_per_kN": round((FXM_P2N - FXM_PN) / (pn / 1000.0), 5),
        })
    return out
