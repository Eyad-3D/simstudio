//! The vehicle: body, wheels, driver and ambient air.
//!
//! **Vehicle body** (vehicle.body): mass, road load (drag and the wheels'
//! rolling resistance, or coefficients A/B/C), grade, load transfer and
//! downforce, as today, with three corrections today's 10 ms step could
//! not make:
//!
//! * rolling resistance (and coefficient A) is Coulomb friction with
//!   stiction: a coasting car stops, at the exact time, and stays stopped
//!   (today's fades out below 0.3 m/s, so a coasting car never stops);
//! * load transfer uses the acceleration now, not the last step's;
//! * the car may roll backwards (today's speed is held at 0 or above);
//!   drag and the linear coefficient act against the motion.
//!
//! Gravity is today's 9.81 m/s², the reference air density for C today's
//! 1.2041 kg/m³ (20 °C, 101.325 kPa).
//!
//! **Wheel** (propulsion.wheel): today's slip tyre: force = normal load ×
//! clamp(slip stiffness × slip, ±μ), slip = (ω·r − v)/max(|v|, 0.5 m/s),
//! μ with its load sensitivity; normal load = its share of the weight
//! normal to the road plus its part of its axle's load transfer and
//! downforce, never below 0. Its rolling resistance (c_rr × normal load)
//! is handed to the body's friction.
//!
//! **Driver** (driver.driver): today's PI speed controller with its
//! conditional-integration anti-windup and recuperation blending, in
//! continuous time.

use super::*;
use lsim_ir::EnergyDecl;

/// today's gravity
pub const GRAVITY: f64 = 9.81;
/// air density at 20 °C and 101.325 kPa (today's AIR_DENSITY)
pub const AIR_DENSITY: f64 = 101.325e3 / (287.05 * 293.15);
/// slip regularisation speed, m/s (today's V_EPS)
pub const V_EPS: f64 = 0.5;

/// The body's configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyConfig {
    /// Road Load From is Coefficients A/B/C
    pub abc: bool,
    /// its wheels are on both axles (else on one: `single_front`)
    pub two_axles: bool,
    /// with one axle, it is the front one
    pub single_front: bool,
    /// the Road Grade input is wired
    pub grade_wired: bool,
}

impl Default for BodyConfig {
    fn default() -> Self {
        BodyConfig { abc: false, two_axles: true, single_front: true, grade_wired: false }
    }
}

const BODY: &str = "vehicle.body";

/// The vehicle body.
pub fn body(cfg: BodyConfig) -> ComponentDef {
    let id = BODY;
    let mut params = cps(
        id,
        &[
            "mass_kg",
            "cd",
            "frontal_area_m2",
            "road_load_a_N",
            "road_load_b_N_per_kmh",
            "road_load_c_N_per_kmh2",
            "initial_speed_kmh",
            "cg_height_m",
            "wheelbase_m",
            "downforce_cza_m2",
            "aero_balance_front_pct",
            "track_front_m",
            "track_rear_m",
        ],
    );
    params.extend([
        p("share_front", "1", 0.5, "the front wheels' share of the weight (from the Wheels)"),
        p("share_rear", "1", 0.5, "the rear wheels' share of the weight (from the Wheels)"),
        p("g", "m/s2", GRAVITY, "gravity"),
        p("rho0", "kg/m3", AIR_DENSITY, "the air density coefficient C is taken at"),
        p(
            "eps",
            "N",
            1e-9,
            "how far the force must exceed the rolling resistance to start it moving",
        ),
        p("unit_kg", "kg", 1.0, "unit carrier"),
        p("unit_N", "N", 1.0, "unit carrier"),
        crate::rotational::s_small("m/s"),
        crate::rotational::s_jump("m/s"),
    ]);
    let mut ports =
        vec![lsim_ir::component::build::port("road", "TFlange", "where the wheels push it")];
    if cfg.grade_wired {
        ports.push(inp(id, "sig_grade_in"));
    }
    ports.push(input("rho", "kg/m3", "the air density (from the Ambient)"));
    if !cfg.abc {
        ports.push(input("f_rr", "N", "the wheels' rolling resistance: Σ c_rr × normal load"));
    }
    for p in [
        "sig_speed",
        "sig_distance",
        "sig_load_front",
        "sig_load_rear",
        "sig_p_aero",
        "sig_p_roll",
        "sig_p_grade",
        "sig_p_accel",
    ] {
        ports.push(out(id, p));
    }
    ports.push(output("w_n", "N", "its weight normal to the road (for the wheels)"));
    ports.push(output("df_front", "N", "the load moved onto the front axle (for the wheels)"));
    ports.push(output("df_rear", "N", "the load moved onto the rear axle (for the wheels)"));
    let mut v = state("v", "m/s", 0.0, "speed along the road");
    v.start = Some(n("initial_speed_kmh"));
    // the rolling resistance's friction modes (it starts sliding: see
    // rotational::friction_mode_eqs)
    let mut friction_vars = crate::rotational::friction_mode_vars("initial_speed_kmh");
    friction_vars[0].doc = "1 while it stands still, held by its rolling resistance".into();
    let mut vars = vec![
        v,
        var("acc", "m/s2", "acceleration"),
        state("z", "m", 0.0, "height climbed"),
        state("dist", "m", 0.0, "distance driven"),
        var("grade", "1", "the road's rise over run"),
        var("sin_t", "1", "sine of the slope angle"),
        var("cos_t", "1", "cosine of the slope angle"),
        var("f_aero", "N", "air drag"),
        var("f_visc", "N", "the road load that grows with speed (coefficient B)"),
        var("f_rr_c", "N", "rolling resistance, as a friction force's magnitude"),
        var("f_fric", "N", "the rolling resistance acting now"),
        var("f_grade", "N", "the weight's pull along the road"),
        var("down", "N", "downforce"),
        var("dz", "N", "load moved from the front axle to the rear"),
    ];
    vars.extend(friction_vars);
    // v·|v| is smooth at 0: no event needed for |v|'s kink
    let speed_abs = noev(abs(n("v")));
    let (f_aero, f_visc, f_rr) = if cfg.abc {
        (
            n("road_load_c_N_per_kmh2") * n("v") * speed_abs.clone() * n("rho") / n("rho0"),
            n("road_load_b_N_per_kmh") * n("v") * n("cos_t"),
            n("road_load_a_N") * n("cos_t"),
        )
    } else {
        (
            c(0.5)
                * n("rho")
                * max(n("cd"), c(0.0))
                * max(n("frontal_area_m2"), c(0.0))
                * n("v")
                * speed_abs,
            c(0.0) * n("unit_N"),
            n("f_rr"),
        )
    };
    let grade = if cfg.grade_wired { n("sig_grade_in") } else { c(0.0) };
    // rolling resistance as a Coulomb friction with stiction: it holds the
    // vehicle at rest until the drive overcomes it, and stops it when it
    // comes to rest (the shared friction logic)
    let friction = crate::rotational::friction_mode_eqs("v", "acc", "f_fric", "f_rr_c", "unit_kg");
    let mut eqs = vec![
        eq(n("road.v"), n("v"), "it moves with its wheels' contact"),
        eq(n("acc"), der("v"), "its acceleration"),
        eq(n("grade"), grade, "the road's grade"),
        eq(n("sin_t"), sin(atan(n("grade"))), "the slope angle's sine"),
        eq(n("cos_t"), cos(atan(n("grade"))), "the slope angle's cosine"),
        eq(n("f_aero"), f_aero, "air drag, against the motion"),
        eq(n("f_visc"), f_visc, "the road load growing with speed, against the motion"),
        eq(n("f_rr_c"), max(f_rr, c(0.0)), "rolling resistance's magnitude"),
        eq(n("f_grade"), n("mass_kg") * n("g") * n("sin_t"), "climbing"),
        eq(
            n("mass_kg") * n("acc"),
            n("road.f") - n("f_aero") - n("f_visc") - n("f_grade") - n("f_fric"),
            "m·a is the tyres' push less the road load",
        ),
        eq(der("z"), n("v") * n("sin_t"), "the height it climbs"),
        eq(der("dist"), n("v"), "the distance it drives"),
        eq(n("w_n"), n("mass_kg") * n("g") * n("cos_t"), "its weight normal to the road"),
        eq(
            n("down"),
            c(0.5) * n("rho") * n("downforce_cza_m2") * n("v") * n("v"),
            "downforce (negative: lift)",
        ),
        eq(
            n("dz"),
            ite(
                gt(n("wheelbase_m"), c(0.0)),
                n("mass_kg") * (n("acc") + n("g") * n("sin_t")) * n("cg_height_m")
                    / n("wheelbase_m"),
                c(0.0) * n("unit_N"),
            ),
            "load transfer m·(a + g·sin θ)·h/L, from the acceleration now",
        ),
        eq(n("sig_speed"), n("v"), "speed"),
        eq(n("sig_distance"), n("dist"), "distance"),
        eq(n("sig_p_aero"), n("f_aero") * n("v"), "air drag power"),
        eq(n("sig_p_roll"), (n("f_fric") + n("f_visc")) * n("v"), "rolling resistance power"),
        eq(n("sig_p_grade"), n("f_grade") * n("v"), "climbing power"),
        eq(n("sig_p_accel"), n("mass_kg") * n("acc") * n("v"), "acceleration power"),
    ];
    eqs.extend(friction);
    let bal = clamp(n("aero_balance_front_pct"), c(0.0), c(1.0));
    if cfg.two_axles {
        let wf = n("share_front") * n("w_n");
        let wr = n("share_rear") * n("w_n");
        let d_front = n("down") * bal.clone() - n("dz");
        let d_rear = n("down") * (c(1.0) - bal) + n("dz");
        // an axle that would carry less than nothing lifts and carries
        // nothing; the other takes the rest (today's axle_load_shift)
        let both = lt(wf.clone() + wr.clone() + n("down"), c(0.0));
        let front_lifts = lt(wf.clone() + d_front.clone(), c(0.0));
        let rear_lifts = lt(wr.clone() + d_rear.clone(), c(0.0));
        let df = ite(
            both.clone(),
            -wf.clone(),
            ite(
                front_lifts.clone(),
                -wf.clone(),
                ite(
                    rear_lifts.clone(),
                    d_front.clone() + wr.clone() + d_rear.clone(),
                    d_front.clone(),
                ),
            ),
        );
        let dr = ite(
            both,
            -wr.clone(),
            ite(
                front_lifts,
                d_rear.clone() + wf.clone() + d_front,
                ite(rear_lifts, -wr.clone(), d_rear),
            ),
        );
        eqs.push(eq(n("df_front"), df, "the load moved onto the front axle"));
        eqs.push(eq(n("df_rear"), dr, "the load moved onto the rear axle"));
        eqs.push(eq(n("sig_load_front"), wf + n("df_front"), "front axle load"));
        eqs.push(eq(n("sig_load_rear"), wr + n("df_rear"), "rear axle load"));
    } else {
        eqs.push(eq(n("df_front"), n("down"), "one axle: only the downforce"));
        eqs.push(eq(n("df_rear"), n("down"), "one axle: only the downforce"));
        let total = max(n("w_n") + n("down"), c(0.0));
        let (f, r) = if cfg.single_front {
            (total, c(0.0) * n("unit_N"))
        } else {
            (c(0.0) * n("unit_N"), total)
        };
        eqs.push(eq(n("sig_load_front"), f, "front axle load"));
        eqs.push(eq(n("sig_load_rear"), r, "rear axle load"));
    }
    ComponentDef {
        name: variant(
            "Blocks.VehicleBody",
            cfg == BodyConfig::default(),
            [
                cfg.abc as u8 as f64,
                cfg.two_axles as u8 as f64,
                cfg.single_front as u8 as f64,
                cfg.grade_wired as u8 as f64,
            ],
        ),
        doc: doc(id),
        ports,
        params,
        vars,
        equations: eqs,
        energy: EnergyDecl {
            stored: Some(c(0.5) * n("mass_kg") * n("v") * n("v") + n("mass_kg") * n("g") * n("z")),
            loss: Some((n("f_aero") + n("f_visc") + n("f_fric")) * n("v")),
        },
        ..Default::default()
    }
}

/// The wheel's configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelConfig {
    /// it rolls on a Vehicle (else it carries no load and pushes nothing)
    pub on_vehicle: bool,
}

impl Default for WheelConfig {
    fn default() -> Self {
        WheelConfig { on_vehicle: true }
    }
}

/// The wheel.
pub fn wheel(cfg: WheelConfig) -> ComponentDef {
    let id = "propulsion.wheel";
    let mut params = cps(
        id,
        &[
            "radius_m",
            "inertia_kgm2",
            "mu",
            "slip_stiffness",
            "rolling_resistance",
            "mu_load_sensitivity_per_kN",
            "mu_nominal_load_N",
        ],
    );
    params.extend([
        p(
            "share",
            "1",
            0.25,
            "its share of the vehicle's weight (the Wheels' shares scaled to 100 %)",
        ),
        p(
            "axle_part",
            "1",
            0.5,
            "its part of its axle's load transfer (its share over its axle's)",
        ),
        p("fz_static", "N", 0.0, "its share of the weight on level ground"),
        p("w0", "rad/s", 0.0, "its speed at the start"),
        pe("r", "m", max(n("radius_m"), c(1e-3)), "rolling radius, at least 1 mm"),
        pe("c_slip", "1", max(n("slip_stiffness"), c(0.1)), "slip stiffness, at least 0.1"),
        pe("mu0", "1", max(n("mu"), c(0.0)), "friction coefficient"),
        pe("c_rr", "1", max(n("rolling_resistance"), c(0.0)), "rolling resistance coefficient"),
        p("v_eps", "m/s", V_EPS, "the speed below which slip is taken over this speed"),
        p("unit_N", "N", 1.0, "unit carrier"),
    ]);
    let mut ports = vec![phys(id, "shaft", "Flange")];
    if cfg.on_vehicle {
        ports.push(lsim_ir::component::build::port(
            "road",
            "TFlange",
            "its contact with the road (the Vehicle)",
        ));
        ports.push(input("w_n", "N", "the vehicle's weight normal to the road"));
        ports.push(input("df", "N", "the load moved onto its axle"));
    }
    for q in
        ["sig_speed", "sig_slip", "sig_force", "sig_torque", "sig_normal_load", "sig_slip_losses"]
    {
        ports.push(out(id, q));
    }
    ports.push(output("f_rr", "N", "its rolling resistance: c_rr × normal load"));
    let mut w = state("w", "rad/s", 0.0, "wheel speed");
    w.start = Some(n("w0"));
    let mut vars =
        vec![w, var("N", "N", "normal load"), var("F", "N", "tyre force, driving positive")];
    let mut eqs = vec![
        eq(n("w"), n("shaft.w"), "it turns with its shaft"),
        eq(
            n("inertia_kgm2") * der("w"),
            n("shaft.tau") - n("F") * n("r"),
            "its inertia: the shaft's torque less the tyre's",
        ),
        eq(n("sig_speed"), noev(abs(n("w"))), "wheel speed"),
        eq(n("sig_force"), n("F"), "traction force"),
        eq(n("sig_torque"), n("F") * n("r"), "drive torque"),
        eq(n("sig_normal_load"), n("N"), "normal load"),
        eq(n("f_rr"), n("c_rr") * n("N"), "rolling resistance"),
    ];
    if cfg.on_vehicle {
        vars.extend([
            var("v", "m/s", "the vehicle's speed"),
            var("slip", "1", "longitudinal slip"),
            var("mu_eff", "1", "friction coefficient at this load"),
            var("at_grip", "1", "1 while its force is at the tyre's grip limit"),
        ]);
        let fz0 = ite(gt(n("mu_nominal_load_N"), c(0.0)), n("mu_nominal_load_N"), n("fz_static"));
        eqs.extend([
            eq(n("v"), n("road.v"), "the vehicle's speed"),
            eq(
                n("N"),
                max(n("share") * n("w_n") + n("axle_part") * n("df"), c(0.0) * n("unit_N")),
                "its share of the weight and of its axle's load transfer, never below 0",
            ),
            eq(
                n("mu_eff"),
                max(c(0.0), n("mu0") + n("mu_load_sensitivity_per_kN") * (n("N") - fz0)),
                "μ with its load sensitivity",
            ),
            eq(
                n("slip"),
                // |v|'s kink at 0 lies inside max(…, v_eps): no event for it
                (n("w") * n("r") - n("v")) / max(noev(abs(n("v"))), n("v_eps")),
                "slip: (ω·r − v) over the vehicle's speed (at least 0.5 m/s)",
            ),
            eq(
                n("F"),
                n("N") * clamp(n("c_slip") * n("slip"), -n("mu_eff"), n("mu_eff")),
                "slip stiffness × slip × load, up to μ × load",
            ),
            eq(
                n("at_grip"),
                ite(
                    noev(and(
                        ge(abs(n("c_slip") * n("slip")), n("mu_eff")),
                        gt(n("mu_eff") * n("N"), c(0.0) * n("unit_N")),
                    )),
                    c(1.0),
                    c(0.0),
                ),
                "at the grip limit (for the share of a run spent there; no event)",
            ),
            eq(n("road.f"), -n("F"), "it pushes the vehicle with F"),
            eq(n("sig_slip"), n("slip"), "slip"),
            eq(n("sig_slip_losses"), n("F") * (n("w") * n("r") - n("v")), "slip losses"),
        ]);
    } else {
        eqs.extend([
            eq(n("N"), c(0.0) * n("unit_N"), "no vehicle: no load"),
            eq(n("F"), c(0.0) * n("unit_N"), "no vehicle: no force"),
            eq(n("sig_slip"), c(0.0), "no vehicle: no slip"),
            eq(n("sig_slip_losses"), c(0.0) * n("unit_N") * n("v_eps"), "no vehicle: no loss"),
        ]);
    }
    let loss = if cfg.on_vehicle {
        n("F") * (n("w") * n("r") - n("v"))
    } else {
        c(0.0) * n("unit_N") * n("v_eps")
    };
    ComponentDef {
        name: variant("Blocks.Wheel", cfg == WheelConfig::default(), [cfg.on_vehicle as u8 as f64]),
        doc: doc(id),
        ports,
        params,
        vars,
        equations: eqs,
        energy: EnergyDecl {
            stored: Some(c(0.5) * n("inertia_kgm2") * n("w") * n("w")),
            loss: Some(loss),
        },
        ..Default::default()
    }
}

/// The driver's configuration.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct DriverConfig {
    /// an acceleration test: full throttle, no target read
    pub full_throttle: bool,
    /// the Actual Speed input is wired (else it reads the vehicle's speed)
    pub speed_wired: bool,
    /// the Target Speed input is wired (else it holds 0 km/h)
    pub target_wired: bool,
}

/// The driver.
pub fn driver(cfg: DriverConfig) -> ComponentDef {
    let id = "driver.driver";
    let name = variant(
        "Blocks.Driver",
        cfg == DriverConfig::default(),
        [
            cfg.full_throttle as u8 as f64,
            cfg.speed_wired as u8 as f64,
            cfg.target_wired as u8 as f64,
        ],
    );
    let outs = ["sig_traction_cmd", "sig_brake_cmd", "sig_accel_pedal", "sig_brake_pedal"];
    if cfg.full_throttle {
        return ComponentDef {
            name,
            doc: doc(id),
            ports: outs.iter().map(|q| out(id, q)).collect(),
            params: cps(id, &["driver_kp", "driver_ki", "regen_weight_pct"]),
            equations: vec![
                eq(n("sig_traction_cmd"), c(1.0), "an acceleration test: full throttle"),
                eq(n("sig_brake_cmd"), c(0.0), "no brakes"),
                eq(n("sig_accel_pedal"), c(1.0), "pedal down"),
                eq(n("sig_brake_pedal"), c(0.0), "no brake pedal"),
            ],
            ..Default::default()
        };
    }
    let mut ports = vec![];
    if cfg.target_wired {
        ports.push(inp(id, "sig_target_in"));
    }
    if cfg.speed_wired {
        ports.push(inp(id, "sig_speed_in"));
    }
    ports.push(input(
        "v_vehicle",
        "m/s",
        "the vehicle's speed (recuperation fades out below 3 m/s)",
    ));
    ports.push(input("regen_cap", "N.m", "the motors' generator torque at the wheels"));
    ports.extend(outs.iter().map(|q| out(id, q)));
    let mut params = cps(id, &["driver_kp", "driver_ki", "regen_weight_pct"]);
    params.push(p(
        "fr_cap",
        "N.m",
        0.0,
        "the friction brakes' torque at the wheels at a full command",
    ));
    params.push(p("unit_Nm", "N.m", 1.0, "unit carrier"));
    params.push(p("unit_ms", "m/s", 1.0, "unit carrier"));
    let target = if cfg.target_wired { n("sig_target_in") } else { c(0.0) * n("unit_ms") };
    let fb = if cfg.speed_wired { n("sig_speed_in") } else { n("v_vehicle") };
    let w = clamp(n("regen_weight_pct"), c(0.0), c(1.0));
    let taper = clamp(n("v_vehicle") / (c(3.0) * n("unit_ms")), c(0.0), c(1.0));
    let eqs = vec![
        eq(n("err"), target - fb, "the speed error"),
        eq(n("cmd_u"), n("driver_kp") * n("err") + n("driver_ki") * n("I"), "the PI command"),
        eq(n("cmd"), clamp(n("cmd_u"), c(-1.0), c(1.0)), "held to full brake … full throttle"),
        eq(
            der("I"),
            ite(
                // the error's sign needs no event: der(I) = err is 0 there
                or(
                    and(gt(n("cmd_u"), c(1.0)), noev(ge(n("err"), c(0.0)))),
                    and(lt(n("cmd_u"), c(-1.0)), noev(le(n("err"), c(0.0)))),
                ),
                c(0.0) * n("unit_ms"),
                n("err"),
            ),
            "the error's integral, frozen while the command is held at a limit it pushes against",
        ),
        eq(
            n("avail"),
            n("regen_cap") * taper,
            "the motors' generator torque, fading out below 3 m/s",
        ),
        eq(
            n("t_req"),
            max(-n("cmd"), c(0.0)) * (n("fr_cap") + w.clone() * n("avail")),
            "the braking torque asked for",
        ),
        eq(n("t_rg"), min(w * n("avail"), n("t_req")), "recuperation first, up to its weight"),
        eq(
            n("share"),
            ite(
                noev(gt(n("regen_cap"), c(0.0))),
                n("t_rg") / max(n("regen_cap"), c(1e-9) * n("unit_Nm")),
                c(0.0),
            ),
            "the share of full recuperation asked of the motors",
        ),
        eq(
            n("t_fr"),
            min(n("fr_cap"), max(n("t_req") - n("t_rg"), c(0.0) * n("unit_Nm"))),
            "the friction brakes do the rest",
        ),
        eq(
            n("sig_traction_cmd"),
            ite(noev(ge(n("cmd"), c(0.0))), n("cmd"), -n("share")),
            "throttle, or recuperation",
        ),
        eq(
            n("sig_brake_cmd"),
            ite(
                noev(and(lt(n("cmd"), c(0.0)), gt(n("fr_cap"), c(0.0)))),
                n("t_fr") / max(n("fr_cap"), c(1e-9) * n("unit_Nm")),
                c(0.0),
            ),
            "the friction brakes' command",
        ),
        eq(n("sig_accel_pedal"), max(n("cmd"), c(0.0)), "accelerator pedal"),
        eq(n("sig_brake_pedal"), max(-n("cmd"), c(0.0)), "brake pedal"),
    ];
    ComponentDef {
        name,
        doc: doc(id),
        ports,
        params,
        vars: vec![
            state("I", "m", 0.0, "the speed error's integral"),
            var("err", "m/s", "speed error"),
            var("cmd_u", "1", "the command before its limits"),
            var("cmd", "1", "the pedal command"),
            var("avail", "N.m", "recuperation available"),
            var("t_req", "N.m", "braking torque asked for"),
            var("t_rg", "N.m", "recuperation torque"),
            var("share", "1", "share of full recuperation"),
            var("t_fr", "N.m", "friction braking torque"),
        ],
        equations: eqs,
        ..Default::default()
    }
}

/// boundary.ambient: the outside air: its temperature on its heat port,
/// and the air density p/(R·T) for the vehicle's drag.
pub fn ambient() -> ComponentDef {
    let id = "boundary.ambient";
    ComponentDef {
        name: "Blocks.Ambient".into(),
        doc: doc(id),
        ports: vec![
            phys(id, "thermal", "HeatPort"),
            output("T_amb", "K", "the outside air temperature"),
            output("rho", "kg/m3", "the air density"),
        ],
        params: {
            let mut v = cps(id, &["temperature_C", "pressure_kPa"]);
            v.push(p("R_air", "J/(kg.K)", 287.05, "the gas constant of dry air"));
            v
        },
        equations: vec![
            eq(n("thermal.T"), n("temperature_C"), "the air holds its temperature"),
            eq(n("T_amb"), n("temperature_C"), "outside temperature"),
            eq(n("rho"), n("pressure_kPa") / (n("R_air") * n("temperature_C")), "rho = p / (R·T)"),
        ],
        ..Default::default()
    }
}

/// The vehicle blocks in their default configuration.
pub fn defaults() -> Vec<ComponentDef> {
    vec![
        body(BodyConfig::default()),
        wheel(WheelConfig::default()),
        driver(DriverConfig::default()),
        ambient(),
    ]
}
