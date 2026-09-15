"""Wholesale-wrapped domain slaves (Phase 1.4).

The former monolithic step loop is decomposed into five slaves stepped by
the co-simulation master in today's exact order:

    control (÷n_sub) → gear (÷n_sub) → driver → mechanical+vehicle → electrical

The pass bodies are moved verbatim; shared physics state lives in a single
:class:`RunContext` that every slave reads and writes — the honest wrap
stage. True decoupling (declared variables exchanged through the master's
pool, per-component model extraction, tanks as their own slaves) is the
next phase; until then slaves deliberately use ``ctx.t_rec`` / ``ctx.dt`` /
``ctx.dt_rec`` instead of their ``do_step(t, h)`` arguments so results stay
bit-identical to the pre-refactor solver (golden fixtures).
"""
from __future__ import annotations

import math
from collections import defaultdict

from .maps import TableError, interp1, interp2, parse_table1d, parse_table2d
from .network import Driveline, Model, ModelError, Segment, build_model
from .profiles import interp_profile, parse_profile
from .runtime import (
    AIR_DENSITY,
    CLUTCH_BAND,
    GRAVITY,
    RPM,
    V_EPS,
    W_EPS,
    BatteryState,
    DrivelineState,
    DrivePlan,
    EngineCache,
    FuelCellCache,
    MotorCache,
    Runtime,
    SingularMatrixError,
    TankState,
    _sign,
    make_plan,
    solve_linear,
)
from .scripting import ScriptError, compile_script, run_script
from .slave import ParamResult, Slave, StepResult, VarDef, split_var


class ModelInitError(Exception):
    """Element caches could not be built (bad tables / scripts)."""

    def __init__(self, messages: list[str]):
        super().__init__("; ".join(messages))
        self.messages = messages


class RunContext:
    """All shared solver state for one run — the former closure variables of
    simulate(), promoted to attributes so the domain slaves can share them."""

    def __init__(
        self,
        project,
        model: Model,
        rt: Runtime,
        gear_of: dict[str, float],
        case_overrides: dict,
        dt_rec: float,
        n_sub: int,
        dt: float,
    ):
        self.project = project
        self.model = model
        self.rt = rt
        self.gear_of = gear_of
        self.case_overrides = case_overrides
        self.dt_rec = dt_rec
        self.n_sub = n_sub
        self.dt = dt
        self.t_rec = 0.0  # current recorded-step time (set by simulate per step)

        # ---- element caches --------------------------------------------------
        self.batteries: dict[str, BatteryState] = {}
        self.motors: dict[str, MotorCache] = {}
        self.engines: dict[str, EngineCache] = {}
        self.fuelcells: dict[str, FuelCellCache] = {}
        self.tanks: dict[str, TankState] = {}
        self.lookup_cache: dict[str, tuple[list, list]] = {}
        self.pid_state: dict[str, dict[str, float]] = {}
        table_errors: list[str] = []
        for el_id, cdef in model.cdef_of.items():
            p = self.params(el_id)
            label = model.elements[el_id].label
            try:
                if cdef.id == "battery.generic":
                    b = BatteryState(
                        el_id=el_id,
                        soc=float(p.get("initial_soc_pct", 90)) / 100.0,
                        capacity_wh=max(1e-3, float(p.get("capacity_kWh", 60))) * 1000.0,
                        min_soc=float(p.get("min_soc_pct", 10)) / 100.0,
                        r0=max(1e-6, float(p.get("internal_resistance_ohm", 0.08))),
                        r1=max(0.0, float(p.get("rc_resistance_ohm", 0))),
                        tau=max(0.0, float(p.get("rc_time_constant_s", 0))),
                        max_charge_w=max(0.0, float(p.get("max_charge_power_kW", 120))) * 1000.0,
                        ocv_pts=parse_table1d(p.get("ocv_table", {"0": 300, "100": 400})),
                    )
                    b.v_term = b.ocv()
                    self.batteries[el_id] = b
                elif cdef.id == "motor.emotor":
                    self.motors[el_id] = MotorCache(
                        el_id=el_id,
                        full_load=parse_table2d(p.get("full_load_torque", {"330": {"0": 100}})),
                        loss=parse_table2d(p.get("power_loss", {"0": {"0": 0}})),
                        drag=parse_table1d(p.get("drag_torque", {"0": 0})),
                        q4_scale=max(0.0, float(p.get("q4_torque_scale_pct", 100)) / 100.0),
                    )
                elif cdef.id == "engine.combustion":
                    self.engines[el_id] = EngineCache(
                        el_id=el_id,
                        full_load=parse_table1d(p.get("full_load_torque", {"1000": 100})),
                        drag=parse_table1d(p.get("drag_torque", {"0": 20})),
                        fuel_map=parse_table2d(p.get("fuel_map", {"1000": {"0": 1}})),
                        idle_rpm=max(1.0, float(p.get("idle_speed_rpm", 800))),
                    )
                elif cdef.id == "fuelcell.stack":
                    self.fuelcells[el_id] = FuelCellCache(
                        el_id=el_id,
                        pol=parse_table1d(p.get("polarization", {"0": 400, "400": 260})),
                        i_max=max(1.0, float(p.get("max_current_A", 400))),
                        h2_g_per_kwh=max(0.0, float(p.get("h2_per_kwh_g", 55))),
                    )
                elif cdef.id in ("fuel.tank", "fuel.h2_tank"):
                    cap = max(1e-3, float(p.get("capacity_kg", 45)))
                    self.tanks[el_id] = TankState(
                        el_id=el_id, capacity_kg=cap,
                        mass_kg=cap * max(0.0, min(1.0, float(p.get("initial_fill_pct", 90)) / 100.0)),
                    )
                elif cdef.id == "signal.lookup":
                    self.lookup_cache[el_id] = (
                        parse_table1d(p.get("table_1d", {"0": 0, "1": 1})),
                        parse_table2d(p.get("table_2d", {"0": {"0": 0}})),
                    )
                elif cdef.id == "control.pid":
                    self.pid_state[el_id] = {"integral": 0.0, "prev_err": 0.0}
            except TableError as e:
                table_errors.append(f"'{label}': {e}")
        if table_errors:
            raise ModelInitError(table_errors)

        self.script_fns: dict[str, object] = {}
        self.script_states: dict[str, dict] = {}
        for el_id in model.signal_blocks:
            if model.cdef_of[el_id].id != "signal.script":
                continue
            label = model.elements[el_id].label
            try:
                self.script_fns[el_id] = compile_script(
                    str(self.params(el_id).get("code", "")), label)
            except ScriptError as e:
                raise ModelInitError([str(e)])
            self.script_states[el_id] = {}

        # ---- driveline & vehicle states --------------------------------------
        self.veh_id = model.vehicle
        veh_p = self.params(self.veh_id) if self.veh_id else {}
        self.veh_mass = max(1.0, float(veh_p.get("mass_kg", 1800))) if self.veh_id else 0.0
        self.v = max(0.0, float(veh_p.get("initial_speed_kmh", 0)) / 3.6) if self.veh_id else 0.0
        self.distance = 0.0
        self.driver_integral = 0.0

        self.dls = [DrivelineState(dl=dl) for dl in model.drivelines]
        self.el_axis_speed: dict[str, float] = {}  # anchor speeds for plan rebuilds
        for st in self.dls:
            self.rebuild_plan(st, initial=True)

        # bus lookups
        self.motor_bus: dict[str, object] = {}
        for bus in model.buses:
            for m_id in bus.motors:
                self.motor_bus[m_id] = bus
        self.bus_voltage: dict[int, float] = {}
        for bus in model.buses:
            if bus.battery:
                self.bus_voltage[bus.id] = self.batteries[bus.battery].v_term
            elif bus.vsource:
                self.bus_voltage[bus.id] = float(self.params(bus.vsource).get("voltage_V", 400))
            elif bus.fuelcell:
                self.bus_voltage[bus.id] = interp1(self.fuelcells[bus.fuelcell].pol, 0.0)
            elif bus.dcdc_out:
                self.bus_voltage[bus.id] = float(
                    self.params(bus.dcdc_out[0]).get("output_voltage_V", 400))
            else:
                self.bus_voltage[bus.id] = 0.0

        self.vsource_energy_wh: dict[str, float] = defaultdict(float)
        self.dcdc_flows: dict[str, tuple[float, float]] = {}
        self.bus_loads: dict[int, float] = {}
        self.last_forces: dict[str, float] = {}

    # ---- shared helpers ---------------------------------------------------------

    def params(self, el_id: str) -> dict:
        return self.model.params_of[el_id]

    def anchor_of_group(self, dl: Driveline, plan: DrivePlan, coord: int) -> tuple[str, float] | None:
        root = plan.coord_root[coord]
        for s, seg in enumerate(dl.segments):
            if plan.root_of_seg[s] != root:
                continue
            for w in seg.wheels:
                return w.el_id, w.m * plan.scale_of_seg[s]
            for src in seg.sources:
                return src.el_id, src.m * plan.scale_of_seg[s]
            for pr in seg.props:
                return pr.el_id, pr.m * plan.scale_of_seg[s]
        return None

    def rebuild_plan(self, st: DrivelineState, initial: bool = False) -> None:
        st.plan = make_plan(st.dl, self.model.params_of, self.gear_of)
        if st.plan.over_constrained:
            self.rt.warn_once("overconstrained",
                              "Driveline became kinematically over-constrained — "
                              "its motion is frozen.")
            return
        for k in range(st.plan.n):
            anchor = self.anchor_of_group(st.dl, st.plan, k)
            if anchor is None:
                st.plan.x[k] = 0.0
                continue
            el_id, factor = anchor
            if initial:
                # initialize wheel-bearing groups from the vehicle speed
                omega = 0.0
                for seg in st.dl.segments:
                    for w in seg.wheels:
                        if w.el_id == el_id and self.v > 0:
                            omega = self.v / w.radius
                st.plan.x[k] = omega / factor if factor else 0.0
            else:
                st.plan.x[k] = self.el_axis_speed.get(el_id, 0.0) / factor if factor else 0.0

    def seg_speed(self, st: DrivelineState, s: int) -> float:
        g = st.plan.gvec[s]
        return sum(g[i] * st.plan.x[i] for i in range(st.plan.n))

    # ---- live parameter updates ---------------------------------------------------

    def apply_set_param(self, el_id: str, key: str, value) -> ParamResult:
        model, rt = self.model, self.rt
        if el_id not in model.params_of:
            return "invalid"
        model.params_of[el_id][key] = value
        label = model.elements[el_id].label
        if key == "locked":
            for st in self.dls:
                if any(j.el_id == el_id for j in st.dl.joints):
                    self.rebuild_plan(st)
            return "applied"
        pdef = next(
            (pp for pp in model.cdef_of[el_id].parameters if pp.key == key), None)
        if pdef is not None and pdef.variability == "fixed":
            rt.warn_once(
                f"live-structural:{el_id}:{key}",
                f"'{label}.{key}' changed — structural parameters take effect on the next run.",
                level="info",
            )
            return "deferred"
        try:
            if el_id in self.motors:
                mc = self.motors[el_id]
                p = self.params(el_id)
                mc.q4_scale = max(0.0, float(p.get("q4_torque_scale_pct", 100)) / 100.0)
                mc.full_load = parse_table2d(p.get("full_load_torque", {}))
                mc.loss = parse_table2d(p.get("power_loss", {}))
                mc.drag = parse_table1d(p.get("drag_torque", {}))
            if el_id in self.engines:
                ec = self.engines[el_id]
                p = self.params(el_id)
                ec.idle_rpm = max(1.0, float(p.get("idle_speed_rpm", ec.idle_rpm)))
                ec.full_load = parse_table1d(p.get("full_load_torque", {}))
                ec.drag = parse_table1d(p.get("drag_torque", {}))
                ec.fuel_map = parse_table2d(p.get("fuel_map", {}))
            if el_id in self.fuelcells:
                fc = self.fuelcells[el_id]
                p = self.params(el_id)
                fc.i_max = max(1.0, float(p.get("max_current_A", fc.i_max)))
                fc.h2_g_per_kwh = max(0.0, float(p.get("h2_per_kwh_g", fc.h2_g_per_kwh)))
                fc.pol = parse_table1d(p.get("polarization", {}))
            if el_id in self.batteries:
                b = self.batteries[el_id]
                p = self.params(el_id)
                b.min_soc = float(p.get("min_soc_pct", 10)) / 100.0
                b.r0 = max(1e-6, float(p.get("internal_resistance_ohm", b.r0)))
                b.max_charge_w = max(0.0, float(p.get("max_charge_power_kW", 120))) * 1000.0
                b.ocv_pts = parse_table1d(p.get("ocv_table", {}))
        except TableError:
            rt.warn_once(f"live-table:{el_id}", f"Live table edit on '{label}' is invalid — ignored.")
        p = self.params(el_id)
        for st in self.dls:
            for seg in st.dl.segments:
                for w in seg.wheels:
                    if w.el_id == el_id:
                        w.mu = max(0.0, float(p.get("mu", w.mu)))
                        w.c_slip = max(0.1, float(p.get("slip_stiffness", w.c_slip)))
                        w.c_rr = max(0.0, float(p.get("rolling_resistance", w.c_rr)))
                        w.load_share = max(0.0, float(p.get("vehicle_load_share_pct", 25)) / 100.0)
                for br in seg.brakes:
                    if br.el_id == el_id:
                        br.max_torque = max(0.0, float(p.get("max_torque_Nm", br.max_torque)))
        return "applied"

    # ---- behaviors -------------------------------------------------------------

    def motor_torque(self, mc: MotorCache, demand: float, omega_m: float) -> float:
        rt, model = self.rt, self.model
        rpm = abs(omega_m) * RPM
        volts = (self.bus_voltage.get(self.motor_bus[mc.el_id].id, 0.0)
                 if mc.el_id in self.motor_bus else 0.0)
        if volts <= 1.0:
            rt.warn_once(f"deadbus:{mc.el_id}",
                         f"E-Motor '{model.elements[mc.el_id].label}' has no live electrical "
                         f"supply — it produces no torque.")
            t_raw = 0.0
        else:
            if volts < mc.full_load[0][0] - 1e-9 or volts > mc.full_load[-1][0] + 1e-9:
                rt.warn_once(
                    f"mapclamp:{mc.el_id}:volt",
                    f"E-Motor '{model.elements[mc.el_id].label}' is operating outside its "
                    f"full-load map's voltage range ({volts:.0f} V) — torque clamped to "
                    f"the nearest map edge.",
                )
            t_full = interp2(mc.full_load, volts, rpm)
            demand = max(-1.0, min(1.0, demand))
            t_raw = demand * t_full * (mc.q4_scale if demand < 0 else 1.0)
        t_drag = interp1(mc.drag, rpm)
        t_net = t_raw - _sign(omega_m) * t_drag
        p_loss = interp2(mc.loss, rpm, abs(t_raw)) * 1000.0
        mc.rpm = rpm
        mc.torque = t_raw
        mc.p_mech_w = t_raw * omega_m
        mc.p_loss_w = p_loss
        mc.p_elec_w = mc.p_mech_w + p_loss
        return t_net

    def engine_torque(self, ec: EngineCache, omega_e: float) -> float:
        rt, model = self.rt, self.model
        rpm = abs(omega_e) * RPM
        throttle = rt.read_signal(ec.el_id, "sig_throttle_in")
        throttle = max(0.0, min(1.0, throttle if throttle is not None else 0.0))
        on_sig = rt.read_signal(ec.el_id, "sig_on_in")
        on = (on_sig is None) or (on_sig >= 0.5)  # unwired → always on
        tank = self.tanks.get(model.fuel_tank) if model.fuel_tank else None
        if on and tank is not None and tank.mass_kg <= 0:
            on = False
            if not ec.stalled_flagged:
                ec.stalled_flagged = True
                rt.message("warning",
                           f"Fuel tank empty — engine '{model.elements[ec.el_id].label}' shut off.")
        t_prod = 0.0
        if on:
            if rpm > ec.full_load[-1][0] + 1e-9:
                rt.warn_once(
                    f"mapclamp:{ec.el_id}:rpm",
                    f"Engine '{model.elements[ec.el_id].label}' exceeds its full-load "
                    f"curve's speed range ({rpm:.0f} 1/min) — torque clamped to the "
                    f"curve edge.",
                )
            # idle governor: throttle floor rises as speed falls below idle
            governor = max(0.0, min(1.0, (ec.idle_rpm - rpm) / (0.25 * ec.idle_rpm)))
            t_prod = max(throttle, governor) * interp1(ec.full_load, rpm)
        t_net = t_prod - _sign(omega_e) * interp1(ec.drag, rpm)
        fuel = interp2(ec.fuel_map, rpm, max(0.0, t_prod)) if on else 0.0
        if tank is not None and fuel > 0:
            burn = fuel / 3600.0 * self.dt
            tank.mass_kg = max(0.0, tank.mass_kg - burn)
            ec.fuel_used_kg += burn
        elif tank is None and fuel > 0:
            ec.fuel_used_kg += fuel / 3600.0 * self.dt
        ec.rpm = rpm
        ec.torque = t_prod
        ec.fuel_kgh = fuel
        ec.p_mech_w = t_prod * omega_e
        return t_net

    def wheel_force(self, w, omega_ref: float) -> tuple[float, float, float]:
        n_load = w.load_share * self.veh_mass * GRAVITY if self.veh_id else 0.0
        if n_load <= 0:
            return 0.0, 0.0, 0.0
        omega_w = w.m * omega_ref
        v_den = max(abs(self.v), V_EPS)
        slip = (omega_w * w.radius - self.v) / v_den
        fx_over_n = max(-w.mu, min(w.mu, w.c_slip * slip))
        force = n_load * fx_over_n
        saturated = abs(w.c_slip * slip) >= w.mu
        damping = 0.0 if saturated else n_load * w.c_slip * w.radius ** 2 * w.m ** 2 / v_den
        return force, -force * w.radius * w.m, damping

    def brake_capacity(self, seg: Segment) -> float:
        cap = 0.0
        for br in seg.brakes:
            cmd = self.rt.read_signal(br.el_id, "sig_demand_in") or 0.0
            cmd = max(0.0, min(1.0, cmd))
            self.rt.publish(br.el_id, "sig_torque", cmd * br.max_torque)
            cap += cmd * br.max_torque * br.m
        return cap

    def prop_torque(self, seg: Segment, omega_ref: float) -> float:
        total = 0.0
        for pr in seg.props:
            omega_p = pr.m * omega_ref
            rpm_p = abs(omega_p) * RPM
            total += -_sign(omega_p) * pr.t_ref * (rpm_p / pr.n_ref) ** 2 * pr.m
        return total

    @staticmethod
    def apply_brake(tau_other: float, omega: float, cap: float, j_over_dt: float) -> float:
        if cap <= 0:
            return 0.0
        if abs(omega) > W_EPS:
            return -_sign(omega) * cap
        return -max(-cap, min(cap, tau_other + j_over_dt * omega))

    # ---- gear selection (rebuild plans on shift) ------------------------------

    def check_gear_shifts(self) -> bool:
        """Re-extract the drivelines when a gearbox's selected gear changed.
        Returns True when a rebuild happened (a 'reconfigured' event)."""
        rt = self.rt
        for st in self.dls:
            changed = False
            for seg in st.dl.segments:
                for gb in seg.gearboxes:
                    sig = rt.read_signal(gb.el_id, "sig_gear_in")
                    gear = round(sig) if sig is not None else float(
                        self.params(gb.el_id).get("default_gear", 1) or 1)
                    if self.gear_of.get(gb.el_id) != gear:
                        self.gear_of[gb.el_id] = gear
                        changed = True
            if changed:
                # ratios are baked into segment reflections — re-extract them
                try:
                    new_model = build_model(self.project, self.gear_of, self.case_overrides)
                except ModelError:
                    new_model = None
                if new_model is not None:
                    for st2, new_dl in zip(self.dls, new_model.drivelines):
                        st2.dl = new_dl
                        self.rebuild_plan(st2)
                return True
        return False


class _CtxSlave(Slave):
    """Base for the wholesale-wrapped slaves: no declared variables yet —
    the shared RunContext carries all coupling until per-component models
    are extracted (then VarDefs and master-pool routing take over)."""

    def __init__(self, ctx: RunContext):
        self.ctx = ctx

    def variables(self) -> list[VarDef]:
        return []

    def setup(self, t0: float) -> None:
        pass

    def set_inputs(self, values) -> None:
        pass

    def get_outputs(self) -> dict[str, float]:
        return {}


class ControlSlave(_CtxSlave):
    """Signal sources + signal blocks (Script, PID, Lookup, Road Profile),
    evaluated once per recorded step in the model's topological order.
    Also claims live parameter writes for the whole context (broadcast)."""

    slave_id = "control"

    def __init__(self, ctx: RunContext):
        super().__init__(ctx)
        self.rate_divisor = ctx.n_sub

    def set_parameter(self, name: str, value: object) -> ParamResult:
        el_id, key = split_var(name)
        return self.ctx.apply_set_param(el_id, key, value)

    def do_step(self, t: float, h: float) -> StepResult:
        ctx = self.ctx
        model, rt = ctx.model, ctx.rt
        t = ctx.t_rec

        # -- signal sources ----------------------------------------------------
        for el_id, cdef in model.cdef_of.items():
            p = ctx.params(el_id)
            if cdef.id == "signal.constant":
                rt.publish(el_id, "sig_out", float(p.get("value", 0)))
            elif cdef.id == "signal.driving_task":
                pts = parse_profile(str(p.get("profile", "")))
                scale = float(p.get("scale_pct", 100)) / 100.0
                rt.publish(el_id, "sig_demand",
                           interp_profile(pts, t, bool(p.get("repeat", False))) * scale)

        # -- signal blocks (topological order, once per recorded step) ----------
        for el_id in model.signal_blocks:
            el = model.elements[el_id]
            kind = model.cdef_of[el_id].id
            p = ctx.params(el_id)
            if kind == "signal.script":
                inputs = {}
                for port in (el.dynamicPorts or []):
                    if port.direction == "input":
                        inputs[port.id] = rt.read_signal(el_id, port.id) or 0.0
                try:
                    outs = run_script(ctx.script_fns[el_id], el.label, t, ctx.dt_rec, inputs,
                                      ctx.script_states[el_id], dict(p))
                except ScriptError as e:
                    rt.message("error", str(e))
                    return StepResult(status="error", detail=str(e))
                out_ids = {po.id for po in (el.dynamicPorts or []) if po.direction == "output"}
                for key, value in outs.items():
                    if key in out_ids:
                        rt.publish(el_id, key, value)
                    else:
                        rt.warn_once(f"script-out:{el_id}:{key}",
                                     f"Script '{el.label}' returned '{key}' which is not one "
                                     f"of its output ports — value dropped.")
            elif kind == "control.pid":
                sp = rt.read_signal(el_id, "sig_setpoint_in") or 0.0
                fb = rt.read_signal(el_id, "sig_feedback_in") or 0.0
                st_pid = ctx.pid_state[el_id]
                err = sp - fb
                kp = float(p.get("kp", 1.0))
                ki = float(p.get("ki", 0.0))
                kd = float(p.get("kd", 0.0))
                lo = float(p.get("out_min", -1.0))
                hi = float(p.get("out_max", 1.0))
                deriv = (err - st_pid["prev_err"]) / ctx.dt_rec
                out_unsat = kp * err + ki * (st_pid["integral"] + err * ctx.dt_rec) + kd * deriv
                if lo <= out_unsat <= hi or err * out_unsat < 0:  # anti-windup
                    st_pid["integral"] += err * ctx.dt_rec
                out = max(lo, min(hi, kp * err + ki * st_pid["integral"] + kd * deriv))
                st_pid["prev_err"] = err
                rt.publish(el_id, "sig_out", out)
            elif kind == "signal.lookup":
                t1, t2 = ctx.lookup_cache[el_id]
                x_in = rt.read_signal(el_id, "sig_x_in") or 0.0
                if str(p.get("mode", "1D")) == "2D":
                    y_in = rt.read_signal(el_id, "sig_y_in") or 0.0
                    rt.publish(el_id, "sig_out", interp2(t2, x_in, y_in))
                else:
                    rt.publish(el_id, "sig_out", interp1(t1, x_in))
            elif kind == "signal.road_profile":
                pts = parse_profile(str(p.get("profile", "")))
                mode = str(p.get("mode", "distance"))
                if mode == "distance":
                    x_in = rt.read_signal(el_id, "sig_distance_in")
                    if x_in is None and ctx.veh_id:
                        x_in = rt.signal_values.get((ctx.veh_id, "sig_distance"), 0.0)
                    x_in = x_in or 0.0
                else:
                    x_in = t
                rt.publish(el_id, "sig_grade",
                           interp_profile(pts, x_in, bool(p.get("repeat", False))))
        return StepResult()


class GearSlave(_CtxSlave):
    """Gear-selection check at recorded-step cadence (after signals, before
    the driver) — a driveline rebuild surfaces as a 'reconfigured' event."""

    slave_id = "gear"

    def __init__(self, ctx: RunContext):
        super().__init__(ctx)
        self.rate_divisor = ctx.n_sub

    def do_step(self, t: float, h: float) -> StepResult:
        changed = self.ctx.check_gear_shifts()
        return StepResult(events=["reconfigured"] if changed else [])


class DriverSlave(_CtxSlave):
    """Speed-following PI with capability-aware recuperation blending."""

    slave_id = "driver"

    def do_step(self, t: float, h: float) -> StepResult:
        ctx = self.ctx
        model, rt, dt = ctx.model, ctx.rt, ctx.dt
        drv_id = model.driver
        if not drv_id:
            return StepResult()
        dp = ctx.params(drv_id)
        target_kmh = rt.read_signal(drv_id, "sig_target_in")
        if target_kmh is None:
            rt.warn_once("driver-no-target",
                         "Driver has no Target Speed signal — it holds 0 km/h.",
                         level="info")
            target_kmh = 0.0
        # actual-speed feedback: wired signal, else the vehicle state
        fb_kmh = rt.read_signal(drv_id, "sig_speed_in")
        if fb_kmh is None:
            fb_kmh = ctx.v * 3.6
        kp = float(dp.get("driver_kp", 0.35))
        ki = float(dp.get("driver_ki", 0.08))
        regen_w = max(0.0, min(1.0, float(dp.get("regen_weight_pct", 80)) / 100.0))
        err = target_kmh - fb_kmh
        cmd_unsat = kp * err + ki * ctx.driver_integral
        cmd = max(-1.0, min(1.0, cmd_unsat))
        if cmd == cmd_unsat or err * cmd_unsat < 0:
            ctx.driver_integral += err * dt

        t_motor_cap = 0.0
        for st in ctx.dls:
            if st.plan.over_constrained or not st.plan.n:
                continue
            has_wheels = any(seg.wheels for seg in st.dl.segments)
            if not has_wheels:
                continue
            ones = [1.0] * st.plan.n
            for s_idx, seg in enumerate(st.dl.segments):
                for src in seg.sources:
                    if src.kind != "motor" or src.el_id not in ctx.motors:
                        continue
                    mc = ctx.motors[src.el_id]
                    g = st.plan.gvec[s_idx]
                    r_eff = abs(src.m * sum(g[i] * ones[i] for i in range(st.plan.n)))
                    omega_m = src.m * ctx.seg_speed(st, s_idx)
                    volts = (ctx.bus_voltage.get(ctx.motor_bus[mc.el_id].id, 0.0)
                             if mc.el_id in ctx.motor_bus else 0.0)
                    t_q4 = interp2(mc.full_load, volts, abs(omega_m) * RPM) * mc.q4_scale
                    t_motor_cap += t_q4 * r_eff * src.eff * st.plan.eff_chain[s_idx]
        fr_cap = sum(br.max_torque * br.m
                     for st in ctx.dls for seg in st.dl.segments for br in seg.brakes)
        taper = max(0.0, min(1.0, ctx.v / 3.0))
        batt_cap_w = sum(b.max_charge_w for b in ctx.batteries.values())
        radii = [w.radius for st in ctx.dls for seg in st.dl.segments for w in seg.wheels]
        r_avg = sum(radii) / len(radii) if radii else 0.33
        t_batt_cap = batt_cap_w * r_avg / max(ctx.v, V_EPS)
        regen_avail = min(t_motor_cap, t_batt_cap) * taper

        if cmd >= 0:
            traction_cmd, brake_cmd = cmd, 0.0
        else:
            d = -cmd
            t_req = d * (fr_cap + regen_w * regen_avail)
            t_rg = min(regen_w * regen_avail, t_req)
            t_fr = min(fr_cap, t_req - t_rg)
            traction_cmd = -t_rg / max(t_motor_cap, 1e-6) if t_motor_cap > 0 else 0.0
            traction_cmd = max(-1.0, traction_cmd)
            brake_cmd = t_fr / max(fr_cap, 1e-6) if fr_cap > 0 else 0.0
        rt.publish(drv_id, "sig_traction_cmd", traction_cmd)
        rt.publish(drv_id, "sig_brake_cmd", brake_cmd)
        rt.publish(drv_id, "sig_accel_pedal", max(0.0, cmd))
        rt.publish(drv_id, "sig_brake_pedal", max(0.0, -cmd))
        return StepResult()


class MechanicalSlave(_CtxSlave):
    """Per-driveline Lagrangian solve (M ẋ = Q) plus the longitudinal
    vehicle integration — one slave because tire slip couples them stiffly."""

    slave_id = "mechanical"

    def do_step(self, t: float, h: float) -> StepResult:
        ctx = self.ctx
        rt, dt = ctx.rt, ctx.dt

        # mechanics ------------------------------------------------------------
        for st in ctx.dls:
            plan = st.plan
            if plan.over_constrained or plan.n == 0:
                continue
            n = plan.n
            m_mat = [[0.0] * n for _ in range(n)]
            q_vec = [0.0] * n
            omega_seg = [ctx.seg_speed(st, s) for s in range(len(st.dl.segments))]
            # torque-source bookkeeping for joint channels
            torque_above: dict[int, float] = defaultdict(float)  # root seg → torque at axis
            for s_idx, seg in enumerate(st.dl.segments):
                g = plan.gvec[s_idx]
                j_seg = max(1e-4, seg.inertia)
                for i in range(n):
                    gi = g[i]
                    if gi == 0.0:
                        continue
                    for k in range(i, n):
                        m_mat[i][k] += j_seg * gi * g[k]
                tau = 0.0
                for src in seg.sources:
                    omega_src = src.m * omega_seg[s_idx]
                    if src.kind == "motor" and src.el_id in ctx.motors:
                        demand = rt.read_signal(src.el_id, "sig_demand_in") or 0.0
                        t_net = ctx.motor_torque(ctx.motors[src.el_id], demand, omega_src)
                    elif src.kind == "engine" and src.el_id in ctx.engines:
                        t_net = ctx.engine_torque(ctx.engines[src.el_id], omega_src)
                    else:
                        continue
                    driving = t_net * omega_src >= 0
                    eff = src.eff * plan.eff_chain[s_idx]
                    t_at_ref = t_net * src.m * (eff if driving else 1.0 / max(1e-3, eff))
                    tau += t_at_ref
                    torque_above[plan.root_of_seg[s_idx]] += (
                        t_at_ref / max(1e-9, plan.scale_of_seg[s_idx]))
                for w in seg.wheels:
                    f, tq, dmp = ctx.wheel_force(w, omega_seg[s_idx])
                    tau += tq
                    ctx.last_forces[w.el_id] = f
                    if dmp > 0:
                        for i in range(n):
                            gi = g[i]
                            if gi == 0.0:
                                continue
                            for k in range(i, n):
                                m_mat[i][k] += dt * dmp * gi * g[k]
                tau += ctx.prop_torque(seg, omega_seg[s_idx])
                cap = ctx.brake_capacity(seg)
                if cap > 0:
                    # static hold only when this segment carries a coordinate
                    coord = None
                    for kk in range(n):
                        if plan.coord_root[kk] == plan.root_of_seg[s_idx]:
                            coord = kk
                            break
                    if coord is not None and abs(omega_seg[s_idx]) <= W_EPS:
                        j_over_dt = max(m_mat[coord][coord], 1e-4) / dt
                        tau += ctx.apply_brake(tau, omega_seg[s_idx], cap, j_over_dt)
                    else:
                        tau += -_sign(omega_seg[s_idx]) * cap
                for i in range(n):
                    q_vec[i] += g[i] * tau

            # clutches: smooth Coulomb coupling, implicit in Δω
            for j in st.dl.joints:
                if j.kind != "clutch":
                    continue
                engage = rt.read_signal(j.el_id, "sig_engage_in")
                engage = max(0.0, min(1.0, engage if engage is not None else 1.0))
                cap_c = engage * max(0.0, float(ctx.params(j.el_id).get("max_torque_Nm", 0)))
                ga = [j.child_a_m * x for x in plan.gvec[j.child_a]]
                gb = [j.child_b_m * x for x in plan.gvec[j.child_b]]
                d_omega = (j.child_a_m * omega_seg[j.child_a]
                           - j.child_b_m * omega_seg[j.child_b])
                st.clutch_slip[j.el_id] = d_omega
                if cap_c <= 0:
                    st.clutch_torque[j.el_id] = 0.0
                    continue
                k_c = cap_c / CLUTCH_BAND
                t_c = max(-cap_c, min(cap_c, k_c * d_omega))
                st.clutch_torque[j.el_id] = t_c
                rel = [ga[i] - gb[i] for i in range(n)]
                for i in range(n):
                    q_vec[i] += -t_c * ga[i] + t_c * gb[i]
                    if abs(k_c * d_omega) < cap_c:  # unclamped → implicit
                        ri = rel[i]
                        if ri == 0.0:
                            continue
                        for k in range(i, n):
                            m_mat[i][k] += dt * k_c * ri * rel[k]

            for i in range(n):  # symmetrize
                for k in range(i + 1, n):
                    m_mat[k][i] = m_mat[i][k]
            try:
                alpha = solve_linear(m_mat, q_vec)
            except SingularMatrixError:
                detail = (
                    f"Driveline equations became numerically singular at t = {ctx.t_rec:g} s "
                    "(check gear ratios, inertias and joint configuration) — "
                    "solve aborted."
                )
                rt.message("error", detail)
                return StepResult(status="error", detail=detail)
            for i in range(n):
                x_new = plan.x[i] + alpha[i] * dt
                # brake zero-crossing clamp on braked coordinates
                root = plan.coord_root[i]
                braked = any(
                    seg.brakes and plan.root_of_seg[s2] == root
                    for s2, seg in enumerate(st.dl.segments)
                )
                if braked and plan.x[i] * x_new < 0:
                    x_new = 0.0
                plan.x[i] = x_new

            # update anchors + joint channels
            omega_seg = [ctx.seg_speed(st, s) for s in range(len(st.dl.segments))]
            st.chain_power_w = 0.0
            for s_idx, seg in enumerate(st.dl.segments):
                for w in seg.wheels:
                    ctx.el_axis_speed[w.el_id] = w.m * omega_seg[s_idx]
                for src in seg.sources:
                    ctx.el_axis_speed[src.el_id] = src.m * omega_seg[s_idx]
                    cache = ctx.motors.get(src.el_id) or ctx.engines.get(src.el_id)
                    if cache is not None:
                        st.chain_power_w += getattr(cache, "p_mech_w", 0.0)
                for pr in seg.props:
                    ctx.el_axis_speed[pr.el_id] = pr.m * omega_seg[s_idx]
            for j in st.dl.joints:
                if j.kind != "split":
                    continue
                omega_in = (j.parent_m * omega_seg[j.parent_seg]
                            if j.parent_seg >= 0 else 0.0)
                st.joint_speed_in[j.el_id] = omega_in
                t_cross = (torque_above.get(plan.root_of_seg[j.parent_seg], 0.0)
                           * plan.scale_of_seg[j.parent_seg] / max(1e-9, j.parent_m)
                           if j.parent_seg >= 0 else 0.0)
                locked_now = bool(ctx.params(j.el_id).get("locked", j.locked))
                t_out = t_cross * j.ratio * j.eff
                if not locked_now:
                    st.joint_torque_a[j.el_id] = (1.0 - j.f_b) * t_out
                    st.joint_torque_b[j.el_id] = j.f_b * t_out
                else:
                    # emergent split: each side consumes inertia + local loads
                    for side, child, child_m in (("a", j.child_a, j.child_a_m),
                                                 ("b", j.child_b, j.child_b_m)):
                        seg = st.dl.segments[child]
                        g = plan.gvec[child]
                        a_axis = sum(g[i] * alpha[i] for i in range(n))
                        local = sum(ctx.wheel_force(w, omega_seg[child])[1] for w in seg.wheels)
                        val = max(1e-4, seg.inertia) * a_axis - local
                        if side == "a":
                            st.joint_torque_a[j.el_id] = val
                        else:
                            st.joint_torque_b[j.el_id] = val

        # vehicle --------------------------------------------------------------
        if ctx.veh_id:
            vp = ctx.params(ctx.veh_id)
            cda = max(0.0, float(vp.get("cd", 0.28))) * max(0.0, float(vp.get("frontal_area_m2", 2.2)))
            grade_pct = rt.read_signal(ctx.veh_id, "sig_grade_in") or 0.0
            f_tire = 0.0
            f_roll = 0.0
            for st in ctx.dls:
                if st.plan.over_constrained or not st.plan.n:
                    continue
                for s_idx, seg in enumerate(st.dl.segments):
                    omega_ref = ctx.seg_speed(st, s_idx)
                    for w in seg.wheels:
                        f, _, _ = ctx.wheel_force(w, omega_ref)
                        f_tire += f
                        n_load = w.load_share * ctx.veh_mass * GRAVITY
                        f_roll += w.c_rr * n_load
            f_aero = 0.5 * AIR_DENSITY * cda * ctx.v * ctx.v
            f_grade = ctx.veh_mass * GRAVITY * grade_pct / 100.0
            roll_taper = max(0.0, min(1.0, ctx.v / 0.3))
            accel = (f_tire - f_aero - f_roll * roll_taper - f_grade) / ctx.veh_mass
            ctx.v = max(0.0, ctx.v + accel * dt)
            ctx.distance += ctx.v * dt
            rt.publish(ctx.veh_id, "sig_speed", ctx.v * 3.6)
            rt.publish(ctx.veh_id, "sig_distance", ctx.distance)
        return StepResult()


class ElectricalSlave(_CtxSlave):
    """Bus power balance in dependency order: consumers, motors, DC-DC
    bridges, then the bus source (battery ECM / voltage source / fuel cell)."""

    slave_id = "electrical"

    def do_step(self, t: float, h: float) -> StepResult:
        ctx = self.ctx
        model, rt, dt = ctx.model, ctx.rt, ctx.dt
        dcdc_draw: dict[str, float] = {}
        for bus in model.buses:
            load_w = 0.0
            for c_id in bus.consumers:
                p_kw = rt.read_signal(c_id, "sig_demand_in")
                if p_kw is None:
                    p_kw = float(ctx.params(c_id).get("power_kW", 0))
                p_w = max(0.0, p_kw) * 1000.0
                rt.publish(c_id, "sig_power", p_w / 1000.0)
                load_w += p_w
            for m_id in bus.motors:
                load_w += ctx.motors[m_id].p_elec_w if m_id in ctx.motors else 0.0
            for d_id in bus.dcdc_in:
                load_w += dcdc_draw.get(d_id, 0.0)
            # DC-DC feeding a battery bus works at a power setpoint
            if bus.battery:
                for d_id in bus.dcdc_out:
                    sp_kw = rt.read_signal(d_id, "sig_setpoint_in")
                    if sp_kw is None:
                        sp_kw = float(ctx.params(d_id).get("power_setpoint_kW", 0))
                    sp_w = max(0.0, sp_kw) * 1000.0
                    eta = max(1e-3, float(ctx.params(d_id).get("efficiency_pct", 97)) / 100.0)
                    dcdc_draw[d_id] = sp_w / eta
                    ctx.dcdc_flows[d_id] = (sp_w / eta, sp_w)
                    load_w -= sp_w
            ctx.bus_loads[bus.id] = load_w
            for n_id in bus.nodes:
                rt.publish(n_id, "sig_power", load_w / 1000.0)

            if bus.battery:
                b = ctx.batteries[bus.battery]
                p_w = load_w
                if b.soc <= b.min_soc and p_w > 0:
                    if not b.depleted_flagged:
                        b.depleted_flagged = True
                        rt.message("warning",
                                   f"Battery '{model.elements[b.el_id].label}' reached minimum "
                                   f"SOC ({b.min_soc * 100:.0f} %) at t = {ctx.t_rec:.0f} s — no further discharge.")
                    p_w = 0.0
                if p_w < -b.max_charge_w:
                    rt.warn_once(f"chg:{b.el_id}",
                                 f"Charging exceeds max charge power of "
                                 f"'{model.elements[b.el_id].label}' — clamped.")
                    p_w = -b.max_charge_w
                a_volt = b.ocv() - b.v_rc
                disc = a_volt * a_volt - 4.0 * b.r0 * p_w
                if disc < 0:
                    rt.warn_once(f"maxp:{b.el_id}",
                                 f"Battery '{model.elements[b.el_id].label}' demand exceeds its "
                                 f"deliverable power — clamped at the maximum power point.")
                    current = a_volt / (2.0 * b.r0)
                    p_w = a_volt * a_volt / (4.0 * b.r0)
                else:
                    current = (a_volt - math.sqrt(disc)) / (2.0 * b.r0)
                v_term = a_volt - current * b.r0
                if b.r1 > 0 and b.tau > 0:
                    b.v_rc = (b.v_rc + dt * current * b.r1 / b.tau) / (1.0 + dt / b.tau)
                e_wh = b.ocv() * current * dt / 3600.0
                b.soc = max(0.0, min(1.0, b.soc - e_wh / b.capacity_wh))
                if current >= 0:
                    b.energy_out_wh += p_w * dt / 3600.0
                else:
                    b.energy_in_wh += -p_w * dt / 3600.0
                b.loss_wh += current * current * b.r0 * dt / 3600.0
                b.current, b.power_w, b.v_term = current, p_w, v_term
                ctx.bus_voltage[bus.id] = v_term
            elif bus.vsource:
                vs_p = ctx.params(bus.vsource)
                ctx.bus_voltage[bus.id] = float(vs_p.get("voltage_V", 400))
                ctx.vsource_energy_wh[bus.vsource] += load_w * dt / 3600.0
                rt.publish(bus.vsource, "sig_power", load_w / 1000.0)
                rt.publish(bus.vsource, "sig_voltage", ctx.bus_voltage[bus.id])
            elif bus.fuelcell:
                fc = ctx.fuelcells[bus.fuelcell]
                tank = ctx.tanks.get(model.h2_tank) if model.h2_tank else None
                p_req = max(0.0, load_w)
                if tank is not None and tank.mass_kg <= 0:
                    if not tank.empty_flagged:
                        tank.empty_flagged = True
                        rt.message("warning", "Hydrogen tank empty — fuel cell shut down.")
                    p_req = 0.0
                p_max = interp1(fc.pol, fc.i_max) * fc.i_max
                if p_req > p_max:
                    rt.warn_once(f"fcmax:{fc.el_id}",
                                 f"Fuel cell '{model.elements[fc.el_id].label}' demand exceeds "
                                 f"its maximum power — clamped.")
                    p_req = p_max
                lo_i, hi_i = 0.0, fc.i_max
                for _ in range(40):
                    mid = 0.5 * (lo_i + hi_i)
                    if interp1(fc.pol, mid) * mid < p_req:
                        lo_i = mid
                    else:
                        hi_i = mid
                current = 0.5 * (lo_i + hi_i)
                volt = interp1(fc.pol, current)
                fc.current, fc.voltage, fc.power_w = current, volt, p_req
                fc.h2_kgh = p_req / 1000.0 * fc.h2_g_per_kwh / 1000.0
                fc.energy_wh += p_req * dt / 3600.0
                if tank is not None:
                    tank.mass_kg = max(0.0, tank.mass_kg - fc.h2_kgh / 3600.0 * dt)
                ctx.bus_voltage[bus.id] = volt
            elif bus.dcdc_out:
                d_id = bus.dcdc_out[0]
                eta = max(1e-3, float(ctx.params(d_id).get("efficiency_pct", 97)) / 100.0)
                draw = max(0.0, load_w) / eta
                dcdc_draw[d_id] = draw
                ctx.dcdc_flows[d_id] = (draw, max(0.0, load_w))
                ctx.bus_voltage[bus.id] = float(ctx.params(d_id).get("output_voltage_V", 400))
            else:
                if load_w > 1.0:
                    rt.warn_once(f"nosrc:{bus.id}",
                                 "An electrical bus has load but no source — demand is unmet.")
                ctx.bus_voltage[bus.id] = 0.0
        return StepResult()


def build_slaves(ctx: RunContext) -> list[Slave]:
    """The wholesale-wrapped slave set in the canonical solve order."""
    return [
        ControlSlave(ctx),
        GearSlave(ctx),
        DriverSlave(ctx),
        MechanicalSlave(ctx),
        ElectricalSlave(ctx),
    ]
