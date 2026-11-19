"""A battery pack built from cells, with the limits a battery management
system keeps (MOD-08, fidelity L1: maps and tables with limits).

*Defined By: Cells* builds the pack from a cell datasheet and a layout of
Ns cells in series and Np in parallel (96s30p):

- charge capacity Np × the cell's; open-circuit voltage Ns × the cell's;
- resistance: the cell's DC resistance × a factor for the pulse length (how
  long the current has flowed one way, read from 2 to 120 s and held at the
  table's edges, as VECTO does) and the state of charge × a factor for the
  cell temperature, per series element ÷ Np, plus each series element's
  interconnect and the contactors;
- an optional weak series element (a share of the capacity and a multiple of
  the resistance): it empties first and sags most, so the pack's limits
  follow it, as the weakest module limits a string in VECTO and Modelica;
- mass Ns × Np × the cell's × a packaging factor (an estimate).

Limits, in both modes (*Pack values* sets them for the pack, 0 = none): the
discharge current never exceeds the cell's continuous or, for pulses up to
its peak duration, peak current, and never pulls a cell below its minimum
voltage (behind its resistance and the RC pair's voltage, which is spread
evenly over the series cells); the charge current likewise against the
maximum. A weak element stops the string when it reaches the minimum SOC
or 100 %, as the same current flows through every series element. A
derating band above the minimum SOC and below 100 % lowers both linearly
to 0 there, as FASTSim's buffers do: the current limits, or without one
the maximum-power-point current (discharge) and the current at the Max
Charge Power (charge). The source-limit handshake holds the motors to them.

Until LightSim has a thermal model (MOD-09), the cells are at the first
Ambient's temperature (20 °C without one).
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field

from .maps import Map

PACK, CELLS = "Pack values", "Cells"
SOP_PULSES = (2.0, 10.0, 30.0)  # the state-of-power durations published, s
SIGN_A = 1e-6  # a current below this (A) does not start a new pulse


@dataclass
class CellPack:
    """A pack's cells and layout (CELLS) or its own limits (PACK)."""

    ns: int = 1
    np: int = 1
    cell_ah: float = 0.0
    cell_ocv: Map | None = None  # V vs SOC % (CELLS)
    v_min: float = 0.0  # per cell (CELLS) or pack (PACK); 0 = none
    v_max: float = 0.0
    dcr: float = 0.0  # cell DC resistance, Ω (CELLS)
    r_factor: Map | None = None  # vs pulse length s × SOC % (CELLS)
    t_factor: Map | None = None  # vs cell temperature °C (CELLS)
    i_dis: float = 0.0  # continuous discharge current per cell (pack), A; 0 = none
    i_dis_peak: float = 0.0  # for pulses up to peak_s
    i_ch: float = 0.0
    i_ch_peak: float = 0.0
    peak_s: float = 10.0
    r_ic: float = 0.0  # per series element, Ω
    r_contactor: float = 0.0
    weak_cap: float = 1.0  # the weak element's share of capacity
    weak_res: float = 1.0  # and its resistance multiple
    mass_kg: float = 0.0  # cells × cell mass × packaging factor
    band: float = 0.0  # derating band, share of SOC
    cells: bool = False  # CELLS (else PACK)
    # the run's state: the weak element's SOC, the current's sign and how
    # long it has flowed that way, and what held the pack this step
    soc_weak: float = 1.0
    pulse_sign: int = 0
    pulse_s: float = 0.0
    r_cell_now: float = 0.0  # this step's cell resistance (CELLS)
    bound_dis: str | None = None
    bound_ch: str | None = None
    v_cell: tuple[float, float] = (0.0, 0.0)  # lowest and highest cell voltage now
    v_cell_low: float = float("inf")  # over the run
    v_cell_high: float = float("-inf")
    limit_s: dict[str, float] = field(default_factory=dict)  # time held per limit
    sop_at: tuple = (None, [])  # (when, sop()) for the recorded point's channels

    @property
    def weak(self) -> bool:
        return self.cells and (self.weak_cap != 1.0 or self.weak_res != 1.0)

    def r_cell(self, soc_pct: float, t_c: float, pulse_s: float) -> float:
        """A normal cell's resistance, Ω (CELLS)."""
        return (self.dcr * self.r_factor.at(pulse_s, soc_pct) * self.t_factor.at(t_c))

    def r_pack(self, r_cell: float) -> float:
        """The pack's resistance with this cell resistance, Ω (CELLS)."""
        return ((self.ns - 1 + self.weak_res) * r_cell / self.np + self.ns * self.r_ic
                + self.r_contactor)

    def ocv_cells(self, soc: float) -> tuple[float, float]:
        """(a normal cell's, the weak cell's) open-circuit voltage (CELLS)."""
        v = self.cell_ocv.at(max(0.0, min(1.0, soc)) * 100.0)
        if not self.weak:
            return v, v
        return v, self.cell_ocv.at(max(0.0, min(1.0, self.soc_weak)) * 100.0)

    def weak_shift(self, soc: float) -> float:
        """What the weak element changes the pack's open-circuit voltage by, V."""
        if not self.weak:
            return 0.0
        v, w = self.ocv_cells(soc)
        return w - v

    def derate(self, soc: float, min_soc: float) -> tuple[float, float]:
        """(discharge, charge) factors of the derating band, 0-1."""
        if self.band <= 0:
            return 1.0, 1.0
        soc_d = min(soc, self.soc_weak) if self.weak else soc
        return (max(0.0, min(1.0, (soc_d - min_soc) / self.band)),
                max(0.0, min(1.0, (1.0 - soc) / self.band)))

    def currents(self, soc: float, min_soc: float, t_c: float, pulse_dis: float,
                 pulse_ch: float, ocv_pack: float, r0: float, v_rc: float = 0.0,
                 r_rc: float = 0.0, i_free: tuple[float, float] = (math.inf, math.inf)
                 ) -> tuple[float, str | None, float, str | None]:
        """(most discharge current, what sets it, most charge current, what
        sets it), in A at the pack's terminals, for a discharge pulse that
        has lasted ``pulse_dis`` and a charge pulse that has lasted
        ``pulse_ch`` (s); inf and None where nothing limits it. ``ocv_pack``
        and ``r0`` are the pack's open-circuit voltage and resistance (PACK).
        With an RC pair the voltage limits are met behind it too: ``v_rc``
        is the part of its voltage still there at the end of the time looked
        at and ``r_rc`` the resistance the current meets in it over that
        time (rc_horizon()), both for the pack; the series cells share them
        evenly. ``i_free`` is the (discharge, charge) current with no limit,
        which the derating band lowers when no current or voltage limit is
        set."""
        inf = math.inf
        dis = ch = inf
        why_dis = why_ch = None
        i_map_dis = (self.i_dis_peak if pulse_dis <= self.peak_s and self.i_dis_peak > 0
                     else self.i_dis) * self.np
        i_map_ch = (self.i_ch_peak if pulse_ch <= self.peak_s and self.i_ch_peak > 0
                    else self.i_ch) * self.np
        if i_map_dis > 0:
            dis, why_dis = i_map_dis, "current"
        if i_map_ch > 0:
            ch, why_ch = i_map_ch, "current"
        if self.cells:
            v_n, v_w = self.ocv_cells(soc)
            # the RC pair's share per cell: its voltage, and its resistance
            # as the pack current meets it (× Np for the cell's own current)
            v_rc_cell, r_rc_np = v_rc / self.ns, r_rc * self.np / self.ns
            for v_ocv, k in ((v_n, 1.0), (v_w, self.weak_res)):
                v_src = v_ocv - v_rc_cell  # the cell's voltage behind its resistance
                if self.v_min > 0:
                    r_e = self.r_cell(soc * 100.0, t_c, pulse_dis) * k + r_rc_np
                    i_v = max(0.0, (v_src - self.v_min) * self.np / r_e)
                    if i_v < dis:
                        dis, why_dis = i_v, "cell voltage"
                if self.v_max > 0:
                    r_e = self.r_cell(soc * 100.0, t_c, pulse_ch) * k + r_rc_np
                    i_v = max(0.0, (self.v_max - v_src) * self.np / r_e)
                    if i_v < ch:
                        ch, why_ch = i_v, "cell voltage"
                if not self.weak:
                    break
        elif r0 > 0:
            v_src, r_e = ocv_pack - v_rc, r0 + r_rc
            if self.v_min > 0:
                i_v = max(0.0, (v_src - self.v_min) / r_e)
                if i_v < dis:
                    dis, why_dis = i_v, "pack voltage"
            if self.v_max > 0:
                i_v = max(0.0, (self.v_max - v_src) / r_e)
                if i_v < ch:
                    ch, why_ch = i_v, "pack voltage"
        # the derating band lowers whatever limit there is; with none, the
        # current the pack would otherwise give or take (``i_free``)
        k_dis, k_ch = self.derate(soc, min_soc)
        if k_dis < 1.0:
            base = dis if dis < inf else i_free[0]
            if base < inf or k_dis == 0.0:
                dis, why_dis = (base * k_dis if base < inf else 0.0), "SOC derating"
        if k_ch < 1.0:
            base = ch if ch < inf else i_free[1]
            if base < inf or k_ch == 0.0:
                ch, why_ch = (base * k_ch if base < inf else 0.0), "SOC derating"
        return dis, why_dis, ch, why_ch

    def weak_currents(self, min_soc: float, soc_per_amp: float, eta: float
                      ) -> tuple[float, float]:
        """(discharge, charge) current, A, that takes the weak element to the
        minimum SOC or to 100 % within the step, when ``soc_per_amp`` is the
        pack's SOC one ampere moves over it and ``eta`` the share of the
        charge stored; inf without a weak element. The same current flows
        through every series element, so the string stops there (CELLS)."""
        if not self.weak:
            return math.inf, math.inf
        per = soc_per_amp / self.weak_cap  # its smaller capacity moves further
        return (max(0.0, (self.soc_weak - min_soc) / per),
                max(0.0, (1.0 - self.soc_weak) / (per * eta)))

    def track(self, current: float, dt: float, eta: float, q_ah: float) -> None:
        """After a step: the pulse timer (time since the current last changed
        direction) and the weak element's SOC."""
        sign = 1 if current > SIGN_A else -1 if current < -SIGN_A else 0
        if sign and sign != self.pulse_sign:
            self.pulse_sign, self.pulse_s = sign, 0.0
        self.pulse_s += dt
        if self.weak:
            e = eta if current < 0 else 1.0
            self.soc_weak = max(0.0, min(1.0, self.soc_weak - e * current * dt
                                         / (3600.0 * q_ah * self.weak_cap)))

    def cell_voltages(self, soc: float, current: float, v_rc: float = 0.0
                      ) -> tuple[float, float]:
        """(lowest, highest) cell terminal voltage at this current, with the
        pack's RC pair at ``v_rc`` (shared evenly by the series cells) (CELLS)."""
        v, w = self.ocv_cells(soc)
        i_cell = current / self.np
        v_rc_cell = v_rc / self.ns
        vn = v - v_rc_cell - i_cell * self.r_cell_now
        vw = w - v_rc_cell - i_cell * self.r_cell_now * self.weak_res
        return min(vn, vw), max(vn, vw)


def rc_horizon(v_rc: float, r1: float, tau: float, decay: float) -> tuple[float, float]:
    """(v_keep, r_rc): an RC pair at ``v_rc`` with a current I held over a
    time ahead is at ``v_keep`` + I · ``r_rc`` at its end. ``decay`` is the
    share of its voltage now still there then: exp(−t/τ), or 1/(1 + dt/τ)
    over a solver step, as the step integrates it. Without an RC pair (R1
    or τ 0) its voltage stays as it is."""
    if r1 <= 0 or tau <= 0:
        return v_rc, 0.0
    return v_rc * decay, r1 * (1.0 - decay)


def sop(cp: CellPack, soc: float, min_soc: float, t_c: float, ocv_pack: float,
        v_rc: float, r1: float = 0.0, tau: float = 0.0) -> list[tuple[float, float]]:
    """The state of power for each of SOP_PULSES: (most discharge power, most
    charge power) at the terminals, W, at the end of a pulse of that length
    from now: the current limits for it, at the resistance of that pulse
    length and with the RC pair (``r1``, ``tau``) charged over it, and no
    more than the pack's maximum-power point (CELLS)."""
    out = []
    for d in SOP_PULSES:
        v_keep, r_rc = rc_horizon(v_rc, r1, tau, math.exp(-d / tau) if tau > 0 else 1.0)
        a_volt = ocv_pack - v_keep
        r = cp.r_pack(cp.r_cell(soc * 100.0, t_c, d))
        i_mpp = max(0.0, a_volt) / (2.0 * (r + r_rc))
        i_dis, _, i_ch, _ = cp.currents(soc, min_soc, t_c, d, d, ocv_pack, r, v_keep, r_rc,
                                        (i_mpp, math.inf))
        i_dis = min(i_dis, i_mpp)
        i_ch = 0.0 if i_ch == math.inf else i_ch
        r += r_rc
        out.append((i_dis * (a_volt - i_dis * r), i_ch * (a_volt + i_ch * r)))
    return out
