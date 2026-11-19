//! The battery problems of `benchmarks/reference/problems/` on the vehicle
//! library's battery block (`battery.generic`, OCV table + R0 + RC pair,
//! with its voltage limit): every compared signal and energy term at the
//! checkpoint times, the limit's event time, and the block's books.

mod common;

use common::*;
use lsim_ir::component::build::{connect, eq};
use lsim_ir::{ComponentDef, Library, Modifier, ParamValue, SubDecl, TableData};
use lsim_lib::blocks::battery::{BatteryConfig, PackLimits, battery};
use lsim_lib::table::Interpolation;
use lsim_lib::x::*;
use lsim_project::reference::{Problem, load};

const RTOL: f64 = 1e-10;
const TOL: f64 = 1e-6;

fn bat_part(
    name: &str,
    def: &str,
    pr: &Problem,
    ocv: (f64, f64),
    extra: &[(&str, f64)],
) -> SubDecl {
    let (a, b) = ocv;
    let real = |k: &str, v: f64| Modifier {
        param: k.into(),
        value: ParamValue::Real(lsim_ir::expr::c(v)),
    };
    let mut m = vec![
        real("capacity_Ah", pr.p("Q_Ah") * 3600.0),
        real("internal_resistance_ohm", pr.p("R0")),
        real("rc_resistance_ohm", pr.p("R1")),
        real("rc_time_constant_s", pr.p("tau1")),
        real("initial_soc_pct", pr.init("SOC")),
        real("coulombic_efficiency_pct", 1.0),
        real("min_soc_pct", 0.0),
        real("output_power_limit_kW", 0.0),
        Modifier {
            param: "ocv_table".into(),
            value: ParamValue::Table(TableData {
                interpolation: Interpolation::Linear,
                axis_units: ["1".into(), String::new()],
                ..TableData::new_1d(vec![0.0, 1.0], vec![a, a + b])
            }),
        },
        real("ocv_mean", a + 0.5 * b),
    ];
    for (k, v) in extra {
        m.push(real(k, *v));
    }
    SubDecl { name: name.into(), def: def.into(), modifiers: m, label: None, ui_id: None }
}

fn lib_with(cfg: &BatteryConfig) -> (Library, String) {
    let mut lib = lib();
    let def = battery(cfg);
    let name = def.name.clone();
    lib.add(def);
    // a load drawing a constant current, held to a limit it is given
    lib.add(ComponentDef {
        name: "Test.LimitedCurrent".into(),
        doc: "Draws I_cc, or less when the battery's discharge limit is lower.".into(),
        ports: vec![
            lsim_ir::component::build::port("p", "Pin", ""),
            lsim_ir::component::build::port("n", "Pin", ""),
            input("i_lim", "A", "the discharge current limit"),
        ],
        params: vec![p("I_cc", "A", 0.0, "the current asked for")],
        vars: vec![var("i", "A", "current p to n")],
        equations: vec![
            eq(n("i"), n("p.i"), "current in at p"),
            eq(lsim_ir::expr::c(0.0), n("p.i") + n("n.i"), "and out at n"),
            eq(n("i"), min(n("I_cc"), n("i_lim")), "the current, held to the limit"),
        ],
        ..Default::default()
    });
    (lib, name)
}

fn rc_cfg(limits: Option<PackLimits>) -> BatteryConfig {
    BatteryConfig { rc: true, limits, ..BatteryConfig::default() }
}

/// Checks the energy terms from the battery's channels and books.
fn energies(res: &lsim_solve::SimResult, pr: &Problem, c1: f64) {
    let (t, cp) = (&pr.times, &pr.checkpoints);
    let (ports, lost, stored, _) = books(res, "bat");
    let s = cp["E_chem"][3];
    for (q, tt) in t.iter().enumerate() {
        let i = res.times.iter().position(|x| (x - tt).abs() < 1e-9).unwrap();
        let e_chem = -res.channel("bat.e_chem").unwrap()[i];
        let v1 = res.channel("bat.v1").unwrap()[i];
        let e_c1 = 0.5 * c1 * v1 * v1;
        let e_term = -ports[i];
        let checks = [
            ("E_chem", e_chem, cp["E_chem"][q]),
            ("E_terminal", e_term, cp["E_terminal"][q]),
            ("E_R0 + E_R1", lost[i], cp["E_R0"][q] + cp["E_R1"][q]),
            ("E_C1", e_c1, cp["E_C1"][q]),
            ("E_internal", e_chem - e_term, cp["E_internal"][q]),
            ("stored (−E_chem + E_C1)", stored[i], -cp["E_chem"][q] + cp["E_C1"][q]),
        ];
        for (what, got, want) in checks {
            assert!((got - want).abs() < TOL * s, "{what} at {tt}: {got} vs exact {want}");
        }
    }
}

#[test]
fn batt_cc_rc() {
    let pr = load("batt_cc_rc").unwrap();
    let (lib, def) = lib_with(&rc_cfg(None));
    let top = ComponentDef {
        name: "CC".into(),
        components: vec![
            bat_part("bat", &def, &pr, (pr.p("ocv_a"), pr.p("ocv_b")), &[]),
            part("load", "Electrical.ConstantCurrent", &[("I", pr.p("I"))]),
            part("gnd", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("bat.pos", "load.p"),
            connect("load.n", "bat.neg"),
            connect("bat.neg", "gnd.p"),
        ],
        ..Default::default()
    };
    let (_m, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    check(&res, "bat.v", t, &cp["V"], 400.0, TOL);
    check(&res, "bat.soc", t, &cp["SOC"], 1.0, TOL);
    check(&res, "bat.i", t, &cp["I"], 120.0, TOL);
    check(&res, "bat.v1", t, &cp["v1"], 4.8, TOL);
    energies(&res, &pr, pr.p("tau1") / pr.p("R1"));
    books_close(&res, 1e-8);
}

#[test]
fn batt_cp_rc() {
    let pr = load("batt_cp_rc").unwrap();
    let (lib, def) = lib_with(&rc_cfg(None));
    let top = ComponentDef {
        name: "CP".into(),
        components: vec![
            bat_part("bat", &def, &pr, (pr.p("E"), 0.0), &[]),
            part("load", "Electrical.PowerLoad", &[]),
            part("p", "Signal.Constant_W", &[("k", pr.p("P"))]),
            part("gnd", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("bat.pos", "load.p"),
            connect("load.n", "bat.neg"),
            connect("bat.neg", "gnd.p"),
            connect("p.y", "load.P_in"),
        ],
        ..Default::default()
    };
    let (_m, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    check(&res, "bat.i", t, &cp["I"], 174.0, TOL);
    check(&res, "bat.v", t, &cp["V"], 380.0, TOL);
    check(&res, "bat.soc", t, &cp["SOC"], 1.0, TOL);
    energies(&res, &pr, pr.p("tau1") / pr.p("R1"));
    books_close(&res, 1e-8);
}

#[test]
fn batt_voltage_limit() {
    let pr = load("batt_voltage_limit").unwrap();
    let limits = PackLimits { v_min: true, ..Default::default() };
    let (lib, def) = lib_with(&rc_cfg(Some(limits)));
    let top = ComponentDef {
        name: "VLimit".into(),
        components: vec![
            bat_part(
                "bat",
                &def,
                &pr,
                (pr.p("ocv_a"), pr.p("ocv_b")),
                &[("min_voltage_V", pr.p("V_min"))],
            ),
            part("load", "Test.LimitedCurrent", &[("I_cc", pr.p("I"))]),
            part("x", "Test.Crossing_A", &[("level", pr.p("I"))]),
            part("gnd", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("bat.pos", "load.p"),
            connect("load.n", "bat.neg"),
            connect("bat.neg", "gnd.p"),
            connect("bat.sig_i_dis_limit", "load.i_lim"),
            connect("bat.sig_i_dis_limit", "x.u"),
        ],
        ..Default::default()
    };
    let (_m, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    check(&res, "bat.v", t, &cp["V"], 400.0, TOL);
    check(&res, "bat.i", t, &cp["I"], 120.0, TOL);
    check(&res, "bat.soc", t, &cp["SOC"], 1.0, TOL);
    energies(&res, &pr, pr.p("tau1") / pr.p("R1"));
    // the limit takes over when the current limit falls below the current
    let t_star = last(&res, "x.t_down");
    let want = pr.events["t_vmin"];
    assert!((t_star - want).abs() < 1e-7 * want.max(1.0), "t_vmin: {t_star} vs exact {want}");
    books_close(&res, 1e-8);
}
