//! `parse(to_text(d)) == d`: every library component, a component using
//! every feature of the IR, and thousands of random expressions.

use lsim_ir::component::build::{eq, param, port, state, sub, var};
use lsim_ir::component::*;
use lsim_ir::expr::{self, BinaryOp, Builtin, CmpOp, Expr, c, call, cmp, der, if_, name as n};
use lsim_lang::{Report, parse, parse_library, parse_with, to_text};

/// The same definition with the parameter `old` renamed (declaration and
/// every use in values, modifiers and equations).
fn rename_param(def: &ComponentDef, old: &str, new: &str) -> ComponentDef {
    let ren = |e: &Expr| {
        e.clone().rewrite(&mut |x| match x {
            Expr::Name(m) if m == old => Expr::Name(new.into()),
            other => other,
        })
    };
    let mut d = def.clone();
    for p in &mut d.params {
        if p.name == old {
            p.name = new.into();
        }
        if let ParamValue::Real(e) = &p.default {
            p.default = ParamValue::Real(ren(e));
        }
    }
    for s in &mut d.components {
        for m in &mut s.modifiers {
            if let ParamValue::Real(e) = &m.value {
                m.value = ParamValue::Real(ren(e));
            }
        }
    }
    for e in d.equations.iter_mut().chain(d.initial_equations.iter_mut()) {
        if let Equation::Eq { lhs, rhs } = &e.eq {
            e.eq = Equation::Eq { lhs: ren(lhs), rhs: ren(rhs) };
        }
    }
    d
}

#[test]
fn every_library_component_round_trips() {
    let lib = lsim_lib::library();
    let mut checked = 0;
    for def in lib.components.values() {
        let text = to_text(def);
        let mut want = def.clone();
        let clashes: Vec<String> = def
            .params
            .iter()
            .filter(|p| def.components.iter().any(|s| s.name == p.name))
            .map(|p| p.name.clone())
            .collect();
        if !clashes.is_empty() {
            // Modelica forbids a parameter and a part of the same name
            // (DESIGN.md 5.4): the parser says so, at the parameter, and the
            // library renames such parameters (work package 5)
            let errs = parse(&text).expect_err("a name clash is rejected");
            assert_eq!(errs.len(), clashes.len(), "{}", Report(&errs));
            for (e, nm) in errs.iter().zip(&clashes) {
                assert_eq!(e.code, "NAME-CLASH");
                assert!(
                    e.message.contains(&format!("'{nm}' names both a parameter and a part")),
                    "{}",
                    e.message
                );
                assert!(e.span.line > 1 && e.span.col == 3, "{e:?}");
            }
            for nm in &clashes {
                want = rename_param(&want, nm, &format!("{nm}_value"));
            }
        }
        let text = to_text(&want);
        let back = parse(&text).unwrap_or_else(|e| panic!("{}\n{text}", Report(&e)));
        assert_eq!(back, vec![want.clone()], "{text}");
        // with the library: parts, ports and units of the parts checked too
        let back = parse_with(&text, &lib).unwrap_or_else(|e| panic!("{}\n{text}", Report(&e)));
        assert_eq!(back, vec![want], "{text}");
        checked += 1;
    }
    assert_eq!(checked, lib.components.len());
    println!("{checked} library components round-trip");
}

#[test]
fn the_connectors_round_trip_as_a_library() {
    let lib = lsim_lib::library();
    let mut only = Library::default();
    for c in lib.connectors.values() {
        only.add_connector(c.clone());
    }
    let text = lsim_lang::library_to_text(&only);
    let back = parse_library(&text, None).unwrap_or_else(|e| panic!("{}\n{text}", Report(&e)));
    assert_eq!(back, only, "{text}");
    assert!(text.contains("flow Real i(unit = \"A\");"), "{text}");
    assert!(text.contains("annotation(__LightSim_power = \"through\");"), "{text}");
}

fn equal(a: Expr, b: Expr) -> Expr {
    Expr::And(Box::new(cmp(CmpOp::Ge, a.clone(), b.clone())), Box::new(cmp(CmpOp::Le, a, b)))
}

/// A component that uses every feature the IR has.
fn everything() -> ComponentDef {
    let mode = EnumType {
        name: "Mode".into(),
        literals: vec![
            EnumLiteral { name: "Manual".into(), doc: "the driver shifts".into() },
            EnumLiteral { name: "Auto".into(), doc: String::new() },
        ],
        doc: "how gears are chosen".into(),
    };
    let mut gain = param("k", "N.m/A", 0.5, "gain with \"quotes\" and a \\ backslash");
    gain.display_unit = Some("N.m/kA".into());
    gain.min = Some(-1e-7);
    gain.max = Some(2.5e20);
    let mut n_cells = param("n_cells", "1", 96.0, "cells in series");
    n_cells.structural = true;
    let bound = ParamDecl {
        default: ParamValue::Real(n("k") * c(2.0) - (-n("k"))),
        ..param("k2", "N.m/A", 0.0, "bound to k")
    };
    let flag = ParamDecl {
        default: ParamValue::Bool(true),
        unit: String::new(),
        ..param("use_heat", "", 0.0, "")
    };
    let choice = ParamDecl {
        default: ParamValue::Enum("Mode.Auto".into()),
        unit: String::new(),
        ..param("mode", "", 0.0, "the mode")
    };
    let ocv = ParamDecl {
        default: ParamValue::Table1D {
            x: vec![0.0, 0.5, 1.0],
            y: vec![3.2, 3.6, 4.1],
            axis_unit: "1".into(),
        },
        ..param("ocv", "V", 0.0, "open-circuit voltage over SOC")
    };
    let eff = ParamDecl {
        default: ParamValue::Table2D {
            x1: vec![0.0, 100.0],
            x2: vec![-50.0, 0.0, 50.0],
            values: vec![0.9, 0.95, 0.9, 0.85, 0.9, -0.8],
            axis_units: ["rad/s".into(), "N.m".into()],
        },
        ..param("eff", "1", 0.0, "")
    };
    let loss = ParamDecl {
        default: ParamValue::Table(TableData {
            outside: [Outside::Linear, Outside::Clamp],
            interpolation: Interpolation::Linear,
            axis_units: ["A".into(), String::new()],
            ..TableData::new_1d(vec![1.0, 2.0], vec![10.0, 40.0])
        }),
        ..param("loss_map", "W", 0.0, "")
    };
    let mut gear = sub("box", "Mech.Gearbox", &[("ratio", n("k") / c(3.0))]);
    gear.modifiers
        .push(Modifier { param: "mode".into(), value: ParamValue::Enum("Mode.Manual".into()) });
    gear.modifiers.push(Modifier { param: "locked".into(), value: ParamValue::Bool(false) });
    gear.modifiers.push(Modifier {
        param: "map".into(),
        value: ParamValue::Table1D { x: vec![0.0, 1.0], y: vec![1.0, -1.0], axis_unit: "s".into() },
    });
    gear.label = Some("Main gearbox".into());
    gear.ui_id = Some("el-gear-1".into());
    let mut weird = var("R1.v", "V", "a Base Modelica name");
    weird.display_unit = Some("kV".into());
    weird.nominal = Some(400.0);
    let mut x = state("x", "1", 0.25, "a state");
    x.start = Some(n("n_cells") * c(0.0) + c(0.25));
    let mut gear_no = var("gear", "1", "");
    gear_no.kind = VarKind::Discrete;
    gear_no.start = Some(c(1.0));
    gear_no.fixed = true;
    ComponentDef {
        name: "Test.Everything".into(),
        doc: "Every feature, for the round trip.".into(),
        types: vec![mode],
        ports: vec![
            port("p", "Pin", "a pin"),
            PortDecl {
                name: "u".into(),
                kind: PortKind::Input { unit: "N.m".into() },
                doc: "in".into(),
            },
            PortDecl {
                name: "y".into(),
                kind: PortKind::Output { unit: String::new() },
                doc: String::new(),
            },
        ],
        params: vec![gain, n_cells, bound, flag, choice, ocv, eff, loss],
        vars: vec![
            weird,
            x,
            gear_no,
            var("soc", "1", ""),
            var("w", "rad/s", ""),
            var("i", "A", ""),
        ],
        components: vec![gear, sub("plain", "Lib.Part", &[])],
        connections: vec![Connect { a: "p".into(), b: "box.flange_a".into() }],
        equations: vec![
            eq(n("R1.v"), expr::table("ocv", vec![n("soc")]) * n("n_cells"), "a table read"),
            eq(
                der("x"),
                -(c(2.0)) * n("x") / Expr::Time
                    + c(-3.0) * call(Builtin::Sqrt, vec![n("x")]) / Expr::Time,
                "",
            ),
            EquationDecl {
                eq: Equation::Eq {
                    lhs: if_(n("use_heat"), n("y"), c(0.0)),
                    rhs: if_(
                        equal(n("mode"), n("Mode.Auto")),
                        expr::table("eff", vec![n("w"), n("u")]),
                        if_(
                            cmp(CmpOp::Lt, n("x"), c(0.0)),
                            c(1.0),
                            expr::table("loss_map", vec![n("i")]) / n("loss_map_ref"),
                        ),
                    ),
                },
                label: None,
            },
            eq(
                n("soc"),
                Expr::NoEvent(Box::new(call(Builtin::Limit, vec![n("x"), c(0.0), c(1.0)]))),
                "clamped",
            ),
            EquationDecl {
                eq: Equation::When {
                    condition: Expr::Or(
                        Box::new(cmp(CmpOp::Ge, n("x"), c(0.9))),
                        Box::new(Expr::Not(Box::new(cmp(CmpOp::Gt, Expr::Time, c(1.0))))),
                    ),
                    actions: vec![
                        WhenAction::Assign {
                            var: "gear".into(),
                            value: Expr::Call(Builtin::Pre, vec![n("gear")]) + c(1.0),
                        },
                        WhenAction::Reinit { var: "x".into(), value: c(0.0) },
                    ],
                },
                label: Some("shift up".into()),
            },
            EquationDecl {
                eq: Equation::Assert {
                    condition: cmp(CmpOp::Le, n("x"), c(1.0)),
                    message: "x stays below 1".into(),
                    error: false,
                },
                label: Some("range".into()),
            },
            EquationDecl {
                eq: Equation::Assert {
                    condition: cmp(CmpOp::Ge, n("x"), c(-1.0)),
                    message: "x \"low\"".into(),
                    error: true,
                },
                label: None,
            },
        ],
        initial_equations: vec![eq(n("soc"), c(0.5), "start half full")],
        energy: EnergyDecl { stored: None, loss: Some(expr::table("loss_map", vec![n("i")])) },
    }
}

#[test]
fn a_component_with_every_feature_round_trips() {
    let mut def = everything();
    def.params.push(param("loss_map_ref", "W", 1.0, "scale of the loss"));
    let text = to_text(&def);
    let back = parse(&text).unwrap_or_else(|e| panic!("{}\n{text}", Report(&e)));
    assert_eq!(back, vec![def], "{text}");
    for s in [
        "structural parameter Real n_cells(unit = \"1\") = 96",
        "parameter Boolean use_heat = true;",
        "parameter Mode mode = Mode.Auto \"the mode\";",
        "type Mode = enumeration(Manual \"the driver shifts\", Auto) \"how gears are chosen\";",
        "= table(x = {0, 0.5, 1}, y = {3.2, 3.6, 4.1}, xUnit = \"1\")",
        "values = [0.9, 0.95, 0.9; 0.85, 0.9, -0.8]",
        "interpolation = linear, outside = {linear}",
        "Real 'R1.v'(unit = \"V\", displayUnit = \"kV\", nominal = 400)",
        "mode == Mode.Auto",
        "R1.v = ocv(soc) * n_cells \"a table read\";",
        "der(x) = (-(2)) * x / time + (-3) * sqrt(x) / time \"\";",
        "min = -1e-7, max = 2.5e20",
        "annotation(__LightSim(id = \"el-gear-1\"))",
        "end when \"shift up\";",
        "assert(x <= 1, \"x stays below 1\", AssertionLevel.warning) \"range\";",
        "initial equation\n  soc = 0.5 \"start half full\";",
    ] {
        assert!(text.contains(s), "missing «{s}» in\n{text}");
    }
}

/// A small deterministic generator (no extra crates).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Random expressions over names whose units the parser cannot know (the
/// part `s` is of a library type it is not given), so only the syntax is
/// at stake.
fn random_expr(r: &mut Rng, depth: u32) -> Expr {
    if depth == 0 || r.below(4) == 0 {
        return match r.below(5) {
            0 => c((r.below(2000) as f64 - 1000.0) / 8.0),
            1 => c([1e-9, 3.5e21, 0.1, 1.0 / 3.0, -7.0, -0.0][r.below(6) as usize]),
            _ => n(["s.a", "s.b", "s.x", "s.'y z'"][r.below(4) as usize]),
        };
    }
    match r.below(15) {
        0 => Expr::Neg(Box::new(random_expr(r, depth - 1))),
        1..=5 => {
            let op = [BinaryOp::Add, BinaryOp::Sub, BinaryOp::Mul, BinaryOp::Div, BinaryOp::Pow]
                [r.below(5) as usize];
            Expr::bin(op, random_expr(r, depth - 1), random_expr(r, depth - 1))
        }
        6 => {
            let op = [CmpOp::Lt, CmpOp::Le, CmpOp::Gt, CmpOp::Ge][r.below(4) as usize];
            cmp(op, random_expr(r, depth - 1), random_expr(r, depth - 1))
        }
        7 => Expr::And(Box::new(random_expr(r, depth - 1)), Box::new(random_expr(r, depth - 1))),
        8 => Expr::Or(Box::new(random_expr(r, depth - 1)), Box::new(random_expr(r, depth - 1))),
        9 => Expr::Not(Box::new(random_expr(r, depth - 1))),
        10 => if_(random_expr(r, depth - 1), random_expr(r, depth - 1), random_expr(r, depth - 1)),
        11 => Expr::NoEvent(Box::new(random_expr(r, depth - 1))),
        12 => {
            let f = [Builtin::Sin, Builtin::Exp, Builtin::Atan2, Builtin::Max, Builtin::Limit]
                [r.below(5) as usize];
            let args = (0..f.arity()).map(|_| random_expr(r, depth - 1)).collect();
            call(f, args)
        }
        13 => Expr::Or(
            Box::new(cmp(CmpOp::Lt, n("s.a"), c(1.0))),
            Box::new(cmp(CmpOp::Gt, n("s.a"), c(1.0))),
        ),
        _ => equal(random_expr(r, depth - 1), random_expr(r, depth - 1)),
    }
}

#[test]
fn random_expressions_print_and_parse_back_exactly() {
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let mut chars = 0;
    let rounds = 4000;
    for round in 0..rounds {
        let lhs = random_expr(&mut r, 1 + (round % 3) as u32);
        let rhs = random_expr(&mut r, 2 + (round % 5) as u32);
        let def = ComponentDef {
            name: "R".into(),
            components: vec![sub("s", "Lib.Part", &[])],
            equations: vec![EquationDecl { eq: Equation::Eq { lhs, rhs }, label: None }],
            ..Default::default()
        };
        let text = to_text(&def);
        chars += text.len();
        let back = parse(&text).unwrap_or_else(|e| panic!("{}\n{text}", Report(&e)));
        assert_eq!(back, vec![def], "{text}");
    }
    println!("{rounds} random equations ({chars} characters) round-trip");
}
