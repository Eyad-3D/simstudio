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
                ..TableData::new_1d(vec![0.0, 1.0], vec![a, a + b]).with_units(&["1", ""])
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

/// A pack built from cells (the library's 96s30p of 5 Ah cells) at a
/// constant 300 A from 50 %, the cells at 25 °C: its terminal voltage is Ns
/// cells' open-circuit voltage less R0 × I, R0 from the cell's DC
/// resistance × its factor at the pulse length and SOC (today's `interp2`
/// of the table, [`Table2::eval`]) per series element ÷ Np plus the
/// interconnects and the contactors; the pulse starts at the first look
/// (1 ms); its discharge limit is the peak's 900 A for the pulse's first
/// 10 s and the continuous 450 A after, the switch a time event at
/// exactly 10.001 s; the books close.
#[test]
fn a_pack_built_from_cells_against_its_hand_values() {
    use lsim_lib::blocks::battery::CellsConfig;
    let cells = CellsConfig::default();
    let cfg = BatteryConfig {
        ocv: cells.pack_ocv(),
        cells: Some(cells.clone()),
        ..BatteryConfig::default()
    };
    let (lib, def) = lib_with(&cfg);
    let temp = lsim_lib::signal::constant("K").name;
    let i_load = 300.0;
    let top = ComponentDef {
        name: "Cells".into(),
        components: vec![
            part("bat", &def, &[("initial_soc_pct", 0.5)]),
            part("load", "Electrical.ConstantCurrent", &[("I", i_load)]),
            part("gnd", "Electrical.Ground", &[]),
            part("temp", &temp, &[("k", 298.15)]),
        ],
        connections: vec![
            connect("bat.pos", "load.p"),
            connect("load.n", "bat.neg"),
            connect("bat.neg", "gnd.p"),
            connect("temp.y", "bat.T_cell"),
        ],
        ..Default::default()
    };
    let (_m, res) = run(&lib, &top, 30.0, 0.5, RTOL);
    let q = 30.0 * 5.0 * 3600.0;
    for t in [1.0, 5.0, 9.5, 15.0, 30.0] {
        let soc = 0.5 - i_load * t / q;
        let pulse = t - 1e-3;
        let r_cell = 0.02 * cells.r_factor.eval(pulse, soc) * cells.t_factor.eval(298.15);
        let r0 = 96.0 * r_cell / 30.0 + 96.0 * 0.0002 + 0.0005;
        let v = 96.0 * cells.ocv.eval(soc) - r0 * i_load;
        let got = at(&res, "bat.v", t);
        assert!((got - v).abs() < 1e-6 * v, "v at {t}: {got} against {v} by hand");
        assert!((at(&res, "bat.soc", t) - soc).abs() < 1e-9, "SOC at {t}");
        let lim = if t < 10.0 { 900.0 } else { 450.0 };
        assert!((at(&res, "bat.i_dis_lim", t) - lim).abs() < 1e-9, "the limit at {t}");
    }
    assert!((cells.t_factor.eval(298.15) - 1.0).abs() < 1e-12, "25 °C is the factor's 1");
    let switch = ev(&res, "the peak's up to its duration");
    assert!((switch - 10.001).abs() < 1e-9, "the peak ends at {switch}");
    books_close(&res, 1e-8);
}

/// A pack built from cells with only peak currents set (no continuous
/// current, no cell voltage limit), inside its derating band: for the
/// pulse's first 10 s its limit is the peak's × the band's factor; after,
/// there is no current limit, and the band lowers the current the pack
/// would give without one (its maximum-power point), as today's engine
/// does. No limit is never shown as a number: the state of power for a
/// charge pulse longer than the peak's duration is 0, for a shorter one a
/// power a pack can take.
#[test]
fn a_cells_pack_with_only_peak_currents_in_its_derating_band() {
    use lsim_lib::blocks::battery::CellsConfig;
    let cells = CellsConfig {
        i_dis: false,
        i_ch: false,
        v_min: false,
        v_max: false,
        ..CellsConfig::default()
    };
    assert!(cells.dis_lapses() && cells.ch_lapses());
    let cfg = BatteryConfig {
        ocv: cells.pack_ocv(),
        cells: Some(cells),
        cells_band: true,
        ..BatteryConfig::default()
    };
    let (lib, def) = lib_with(&cfg);
    let temp = lsim_lib::signal::constant("K").name;
    let bat = [
        ("initial_soc_pct", 0.95),
        ("min_soc_pct", 0.9),
        ("soc_derate_band_pct", 0.2),
        ("max_charge_power_kW", 50e3),
    ];
    let top = |i_load: f64| ComponentDef {
        name: "PeakOnly".into(),
        components: vec![
            part("bat", &def, &bat),
            part("load", "Electrical.ConstantCurrent", &[("I", i_load)]),
            part("gnd", "Electrical.Ground", &[]),
            part("temp", &temp, &[("k", 298.15)]),
        ],
        connections: vec![
            connect("bat.pos", "load.p"),
            connect("load.n", "bat.neg"),
            connect("bat.neg", "gnd.p"),
            connect("temp.y", "bat.T_cell"),
        ],
        ..Default::default()
    };
    // discharging at 100 A
    let (_m, res) = run(&lib, &top(100.0), 20.0, 0.5, RTOL);
    let q = 30.0 * 5.0 * 3600.0;
    for t in [5.0, 15.0] {
        let k = at(&res, "bat.k_dis", t);
        let by_hand = (0.95 - 100.0 * t / q - 0.9) / 0.2;
        assert!((k - by_hand).abs() < 1e-8, "the band's factor at {t}: {k} against {by_hand}");
        let want = if t < 10.0 { 900.0 * k } else { at(&res, "bat.i_mpp", t) * k };
        for name in ["bat.i_dis_lim", "bat.sig_i_dis_limit"] {
            let got = at(&res, name, t);
            assert!((got - want).abs() < 1e-9 * want, "{name} at {t}: {got} against {want}");
        }
        assert_eq!(at(&res, "bat.sig_p_ch_30s", t), 0.0, "no charge limit for a 30 s pulse");
        let p2 = at(&res, "bat.sig_p_ch_2s", t);
        assert!(p2 > 0.0 && p2 < 1e6, "the 2 s charge power at {t}: {p2} W");
        let p30 = at(&res, "bat.sig_p_dis_30s", t);
        assert!(p30 > 0.0 && p30 < 1e7, "the 30 s discharge power at {t}: {p30} W");
    }
    books_close(&res, 1e-8);
    // charging at 100 A: for the first 10 s the peak's × the band's factor;
    // after, the band lowers the current its Max Charge Power would take
    let (_m, res) = run(&lib, &top(-100.0), 20.0, 0.5, RTOL);
    let k = at(&res, "bat.k_ch", 5.0);
    let got = at(&res, "bat.i_ch_lim", 5.0);
    assert!((got - 300.0 * k).abs() < 1e-9 * 300.0, "the peak's × the band: {got}");
    let (k, a, r0) =
        (at(&res, "bat.k_ch", 15.0), at(&res, "bat.a_volt", 15.0), at(&res, "bat.r0", 15.0));
    let want = 2.0 * 50e3 / (a + (a * a + 4.0 * r0 * 50e3).sqrt()) * k;
    let got = at(&res, "bat.i_ch_lim", 15.0);
    assert!((got - want).abs() < 1e-9 * want, "past the peak's duration: {got} against {want}");
    books_close(&res, 1e-8);
}
