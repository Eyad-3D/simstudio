"""Where the energy goes: every part's power in, power out, losses and
change in stored energy, every solver step (MOD-10).

Each part books, per solver step, the power that enters it (at any of its
ports), the power that leaves it and the change of the energy it stores;
its loss is what is left, so in − out − loss − Δstored = 0 holds for each
part by construction. The flows between parts are worked out independently
(a motor's shaft power is what its gears take in, the gears pass on what
their efficiency leaves, and so on), so the sum of in − out over all parts
is where the books do not close: the energy the solver lost or made at the
interfaces (its step, the clamps of the source-limit handshake). It is
reported as the closing residual.

Directions: "in" and "out" are both ≥ 0. A motor that regenerates takes
mechanical power in and gives electrical power out; that part of its input,
which came back from the road side, is also counted as ``in_rev``. Each
part's duty values (peak, mean and RMS) are of its throughput power: the
terminal power of a battery, the electrical power of a motor, the input
power of a gear, the braking power of a brake.

Lap cases book the electrical side and the vehicle's road load; their
gears, wheels and brakes are booked by the lap's own energy pass (lapsim).
"""
from __future__ import annotations

import math

H2_LHV_J_PER_KG = 119.96e6  # hydrogen's lower heating value
FUEL_LHV_MJ = 42.9  # petrol, when no Fuel Tank gives a value
# a shaft power's running totals, as kept per motor and engine: ∫p⁺ dt,
# ∫p⁻ dt (magnitude), ∫p⁺² dt, ∫p⁻² dt, the largest p⁺ and |p⁻|
LINEAR_KEYS = ("pos_j", "neg_j", "pos2", "neg2", "pos_peak", "neg_peak")


def through_stage(acc: list, eff: float) -> list:
    """The running totals (LINEAR_KEYS) of the power a gear of efficiency
    ``eff`` passes on, from those of the power it takes in: driving, it
    passes on eff × its input; flowing back, it asks 1/eff of the road side."""
    pos, neg, pos2, neg2, pk_pos, pk_neg = acc
    return [pos * eff, neg / eff, pos2 * eff * eff, neg2 / (eff * eff), pk_pos * eff,
            pk_neg / eff]


def book_linear(f: "Flow", acc: list, eff: float) -> list:
    """Book a gear of efficiency ``eff`` whose input power had the running
    totals ``acc`` (LINEAR_KEYS): it takes in p⁺ and, flowing back, p⁻/eff
    from the road side, and gives out eff·p⁺ and p⁻. Returns its output's
    totals."""
    pos, neg, pos2, neg2, pk_pos, pk_neg = acc
    f.in_j += pos + neg / eff
    f.in_rev_j += neg / eff
    f.out_j += pos * eff + neg
    f.peak_w = max(f.peak_w, pk_pos, pk_neg)
    f.p_int += pos - neg
    f.p2_int += pos2 + neg2
    return through_stage(acc, eff)


# the Vehicle's named terms (PartEnergyFlow.terms)
ROAD_TERMS = ("air drag", "rolling resistance", "climbing", "acceleration")


class Flow:
    """One part's energy book over the run (J) and its last step's powers (W).
    Booked every solver step, so kept lean: its loss is worked out from the
    totals (in − out − stored), and its mean and RMS power are over the run's
    time (EnergyBook.time_s)."""

    __slots__ = ("el_id", "label", "part", "in_j", "out_j", "stored_j", "in_rev_j", "peak_w",
                 "p_int", "p2_int", "terms", "p_in_w", "p_out_w", "p_stored_w", "p_w", "n",
                 "fuel_j")

    def __init__(self, el_id: str | None, label: str, part: str) -> None:
        self.el_id = el_id  # None for a driveline's rotating parts
        self.label = label
        self.part = part  # its component type, e.g. "motor.emotor"
        self.in_j = self.out_j = 0.0
        self.stored_j = 0.0  # change in the energy it stores (+ when it fills)
        self.in_rev_j = 0.0  # of in_j: what came back from the road side
        self.peak_w = 0.0  # largest |throughput power|
        self.p_int = 0.0  # ∫ throughput power dt, J
        self.p2_int = 0.0  # ∫ throughput power² dt, W²·s
        self.terms: dict[str, float] = {}  # named parts of its loss or store, J
        # the last solver step's powers, W (for the channels)
        self.p_in_w = self.p_out_w = self.p_stored_w = self.p_w = 0.0
        # the solver step it was last booked in (EnergyBook.n): a part not
        # booked in a step had no power through it then (its channels read 0)
        self.n = -1
        self.fuel_j = 0.0  # an engine's fuel energy already added to in_j, J

    @property
    def loss_j(self) -> float:
        return self.in_j - self.out_j - self.stored_j

    @property
    def p_loss_w(self) -> float:
        return self.p_in_w - self.p_out_w - self.p_stored_w

    def step(self, n: int, dt: float, p_in: float, p_out: float, stored: float = 0.0,
             rev: float = 0.0, duty: float | None = None) -> None:
        """Book solver step ``n``, of length dt: ``p_in`` and ``p_out`` (W, ≥ 0),
        the stored energy's rate of change ``stored`` (W), the part of p_in
        that came back from the road side ``rev``, and the throughput power
        ``duty`` (p_in when not given)."""
        p = p_in if duty is None else duty
        self.p_in_w, self.p_out_w, self.p_stored_w, self.p_w = p_in, p_out, stored, p
        self.n = n
        self.in_j += p_in * dt
        self.out_j += p_out * dt
        if stored:
            self.stored_j += stored * dt
        if rev:
            self.in_rev_j += rev * dt
        if p:
            if p > self.peak_w or -p > self.peak_w:
                self.peak_w = p if p > 0 else -p
            self.p_int += p * dt
            self.p2_int += p * p * dt

    def port2(self, n: int, dt: float, p_a: float, p_b: float) -> None:
        """Book solver step ``n`` of a two-port part (no store): port a takes in
        ``p_a`` (W, + into the part), port b, the road side, gives out
        ``p_b`` (W, + out of it); what enters at either port is in, what
        leaves at either is out. Its throughput is p_a."""
        if p_a >= 0.0:
            p_in, p_out = p_a, 0.0
        else:
            p_in, p_out = 0.0, -p_a
        if p_b >= 0.0:
            p_out += p_b
        else:
            p_in -= p_b
            self.in_rev_j -= p_b * dt
        self.p_in_w, self.p_out_w, self.p_stored_w, self.p_w = p_in, p_out, 0.0, p_a
        self.n = n
        self.in_j += p_in * dt
        self.out_j += p_out * dt
        if p_a:
            if p_a > self.peak_w or -p_a > self.peak_w:
                self.peak_w = p_a if p_a > 0 else -p_a
            self.p_int += p_a * dt
            self.p2_int += p_a * p_a * dt

    def term(self, name: str, joules: float) -> None:
        self.terms[name] = self.terms.get(name, 0.0) + joules

    def mean_w(self, time_s: float) -> float:
        return self.p_int / time_s if time_s > 0 else 0.0

    def rms_w(self, time_s: float) -> float:
        return math.sqrt(self.p2_int / time_s) if time_s > 0 else 0.0


class EnergyBook:
    """The flows of one run, in the order parts were first booked."""

    def __init__(self) -> None:
        self.flows: dict[str, Flow] = {}
        self.time_s = 0.0  # the time booked, s (for the mean and RMS powers)
        self.n = 0  # the solver step being booked

    def power(self, key: str, attr: str) -> float | None:
        """A part's power ``attr`` (W) in the last solver step: 0 when it was
        not booked in it, None before it was ever booked."""
        f = self.flows.get(key)
        if f is None:
            return None
        return getattr(f, attr) if f.n == self.n - 1 else 0.0

    def flow(self, key: str, el_id: str | None, label: str, part: str) -> Flow:
        f = self.flows.get(key)
        if f is None:
            f = self.flows[key] = Flow(el_id=el_id, label=label, part=part)
        return f

    def residual_j(self) -> float:
        """Σ (in − out) over every part: 0 when every flow out of one part is
        the flow into the next."""
        return sum(f.in_j - f.out_j for f in self.flows.values())

    def released_j(self) -> float:
        """The energy the sources gave up: batteries, tanks and voltage
        sources (their stored energy fell), J."""
        return sum(-f.stored_j for f in self.flows.values()
                   if f.part in SOURCE_PARTS and f.stored_j < 0)


# parts with no duty values: a wheel's power is its tyre's, a rotating
# part's is no one part's
NO_DUTY = ("propulsion.wheel", "driveline.inertia")
# parts whose stored energy feeds the model (the books' sources)
SOURCE_PARTS = ("battery.generic", "fuel.tank", "fuel.h2_tank", "electric.voltage_source")


# the order parts are listed in: where the energy comes from, what converts
# it, the electrical loads, the driveline, the road
ORDER = ("battery.generic", "electric.voltage_source", "fuel.tank", "fuel.h2_tank",
         "fuelcell.stack", "engine.combustion", "controller.dcdc", "motor.emotor",
         "electric.constant_drive", "electric.climate", "mech.clutch", "mech.shaft",
         "mech.gearbox", "mech.final_drive", "mech.transfer_case", "mech.differential",
         "driveline.inertia", "mech.brake", "propulsion.propeller", "propulsion.wheel",
         "vehicle.body")


def add_lap(book: EnergyBook, lap_book, veh_id: str, label: str) -> None:
    """A lap case's mechanical side, from the lap's own energy pass (J): the
    Vehicle takes in what the lap needed of the motors (kinetic, road load,
    slope, friction brakes, gear losses), loses the road load, brakes and
    gears, and stores the kinetic and potential energy."""
    f = book.flow(veh_id, veh_id, label, "vehicle.body")
    need = (lap_book.kinetic + lap_book.road + lap_book.grade + lap_book.friction
            + lap_book.gears)
    f.in_j, f.out_j = max(need, 0.0), max(-need, 0.0)
    f.stored_j = lap_book.kinetic + lap_book.grade  # (its loss: road, brakes, gears)
    f.terms = {"road load": lap_book.road, "friction brakes": lap_book.friction,
               "gears": lap_book.gears, "climbing": lap_book.grade,
               "acceleration": lap_book.kinetic}


def energy_flows(book: EnergyBook) -> list:
    """The book as the run result's energy list (kWh, kW), in ORDER."""
    from ..schemas import PartEnergyFlow

    def rank(f: Flow) -> int:
        return ORDER.index(f.part) if f.part in ORDER else len(ORDER)

    kwh = 1.0 / 3.6e6
    return [PartEnergyFlow(
        elementId=f.el_id, label=f.label, part=f.part,
        energyIn=round(f.in_j * kwh, 6), energyOut=round(f.out_j * kwh, 6),
        losses=round(f.loss_j * kwh, 6), stored=round(f.stored_j * kwh, 6),
        energyInReverse=round(f.in_rev_j * kwh, 6),
        **({"peakPower": round(f.peak_w / 1000.0, 4),
            "meanPower": round(f.mean_w(book.time_s) / 1000.0, 4),
            "rmsPower": round(f.rms_w(book.time_s) / 1000.0, 4)}
           if f.part not in NO_DUTY else {}),
        terms={k: round(v * kwh, 6) for k, v in f.terms.items()},
    ) for f in sorted(book.flows.values(), key=rank)]
