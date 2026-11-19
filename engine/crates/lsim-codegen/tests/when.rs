//! The semantics of `when` clauses in the generated code (DESIGN.md §16,
//! WP3's acceptance), pinned:
//!
//! * a discrete variable read directly gives its new value (the clauses'
//!   assignments are applied in an order that puts each after every
//!   assignment of a variable it reads, whatever order they were written
//!   in); `pre(v)` gives the value before the event; an assignment reading
//!   its own target reads the value so far;
//! * a continuous variable (an assignment of the equations) reads as just
//!   before the event, whatever discrete values it depends on;
//! * a clause that does not fire changes nothing, also not what a firing
//!   clause assigned;
//! * `reinit(x, v)` is an assignment of the state's jump part
//!   (`x.jump := v - x.continuous`, lsim-prep): right after the event
//!   `x = v` to a rounding or two (of `v - x.continuous`, then of the
//!   sum), the continuous part integrates on, and the value `v` reads the
//!   clause's discrete values as any assignment does.

#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, compile};
use lsim_ir::component::build::{discrete, state};
use lsim_ir::component::{Equation, EquationDecl, WhenAction};
use lsim_ir::expr::{Builtin, CmpOp, Expr, c, call, cmp, der, name as n};
use lsim_ir::prepared::{Direction, PreparedWhen};
use lsim_ir::runtime::{EvalInput, ModelFunctions};
use lsim_ir::{ComponentDef, PreparedModel, VarId};
use synth::{Builder, origin, v};

/// The discrete variables of [`model`], in their order in `d`.
const NAMES: [&str; 6] = ["a", "b", "c", "g", "h", "k"];

/// A state x (rising at 1/s), discrete a … k, an assignment `w = 2a + x`,
/// and two clauses on two crossings: the first assigns its variables in
/// the order `order` (a permutation of its five assignments).
fn model(order: &[usize]) -> PreparedModel {
    let mut b = Builder::new();
    let x = b.state("x", 0.25);
    let starts = [1.0, 10.0, 100.0, 0.0, 0.0, 3.0];
    let d: Vec<VarId> = NAMES.iter().zip(starts).map(|(name, s)| b.discrete(name, s)).collect();
    let (a, bb, cc, g, h, k) = (d[0], d[1], d[2], d[3], d[4], d[5]);
    let w = b.let_("w", c(2.0) * v(a) + v(x));
    b.der(x, c(1.0));
    let z0 = b.crossing(v(x) - c(0.5));
    let z1 = b.crossing(v(x) - c(2.0));
    let first = [
        // reads a directly: a's new value
        (bb, v(a) + c(1.0)),
        // pre(a): before the event
        (a, Expr::Pre(a) + c(5.0)),
        // an assignment of the equations: as before the event
        (cc, w),
        (g, Expr::Pre(bb)),
        // reads two variables assigned in this clause
        (h, v(bb) * c(100.0) + v(a)),
        // reads its own target: the value so far
        (k, v(k) + c(1.0)),
    ];
    let assign = order.iter().map(|&i| first[i].clone()).collect();
    b.m.whens.push(PreparedWhen::new(z0, Direction::Rising, assign, origin()));
    b.m.whens.push(PreparedWhen::new(
        z1,
        Direction::Rising,
        vec![(a, c(1000.0)), (bb, v(a))],
        origin(),
    ));
    b.finish()
}

/// The discrete values after the clauses `fired` fire at x.
fn after(m: &JitModel, x: f64, fired: [f64; 2]) -> Vec<f64> {
    let l = *m.layout();
    let d0 = [1.0, 10.0, 100.0, 0.0, 0.0, 3.0];
    let inp = EvalInput { t: 1.0, y: &[x], p: &[], d: &d0, u: &[] };
    let mut work = vec![f64::NAN; l.n_work];
    let mut d = d0.to_vec();
    m.when(&inp, &fired, &mut work, &mut d);
    d
}

#[test]
fn discrete_values_are_new_pre_values_old_and_equations_before_the_event() {
    // every order of writing the first clause's assignments
    let mut orders = vec![];
    let mut perm: Vec<usize> = (0..6).collect();
    // (Heap's algorithm)
    fn heap(k: usize, p: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if k == 1 {
            out.push(p.clone());
            return;
        }
        for i in 0..k {
            heap(k - 1, p, out);
            let j = if k.is_multiple_of(2) { i } else { 0 };
            p.swap(j, k - 1);
        }
    }
    heap(6, &mut perm, &mut orders);
    assert_eq!(orders.len(), 720);
    for (n, order) in orders.iter().enumerate() {
        let m = model(order);
        for opts in [
            CodegenOptions::default(),
            CodegenOptions { opt_level: "none", regalloc: "single_pass", ..Default::default() },
        ] {
            if n % 60 != 0 && opts.opt_level == "none" {
                continue;
            }
            let jit = compile(&m, &opts).expect("compiles");
            let x = 0.7;
            // the first clause: a = pre(a) + 5 = 6, b = a + 1 = 7 (new a),
            // c = w = 2 pre(a) + x, g = pre(b), h = 100 b + a with both
            // new, k = k + 1 = 4
            let want = [6.0, 7.0, 2.0 * 1.0 + x, 10.0, 706.0, 4.0];
            assert_eq!(after(&jit, x, [1.0, 0.0]), want, "order {order:?}");
            // the second alone: a = 1000, b = a = 1000 (new); the first's
            // variables keep their values
            assert_eq!(
                after(&jit, x, [0.0, 1.0]),
                [1000.0, 1000.0, 100.0, 0.0, 0.0, 3.0],
                "order {order:?}"
            );
            // none: nothing changes
            assert_eq!(after(&jit, x, [0.0, 0.0]), [1.0, 10.0, 100.0, 0.0, 0.0, 3.0]);
        }
    }
}

/// x rises at 1/s; when x > 0.5, x restarts from `-2 x + n` and the
/// count n goes up by one (in the same clause, so `n` in the new value is
/// the new count).
fn bouncing() -> ComponentDef {
    ComponentDef {
        name: "Test.Restart".into(),
        vars: vec![state("x", "1", 0.25, ""), discrete("n", "1", 0.0, "")],
        equations: vec![
            lsim_ir::component::build::eq(der("x"), c(1.0), ""),
            EquationDecl {
                eq: Equation::When {
                    condition: cmp(CmpOp::Gt, n("x"), c(0.5)),
                    actions: vec![
                        WhenAction::Reinit { var: "x".into(), value: c(-2.0) * n("x") + n("n") },
                        WhenAction::Assign {
                            var: "n".into(),
                            value: call(Builtin::Pre, vec![n("n")]) + c(1.0),
                        },
                    ],
                },
                label: None,
            },
        ],
        ..Default::default()
    }
}

#[test]
fn reinit_assigns_the_jump_part_of_a_state() {
    let lib = lsim_lib::library();
    let m = lsim_prep::prepare(&lib, &bouncing(), &Default::default()).expect("prepares");
    let jit = compile(&m, &CodegenOptions::default()).expect("compiles");
    let l = *jit.layout();
    let var = |name: &str| {
        let i = m.flat.vars.iter().position(|v| v.name == name).unwrap_or_else(|| {
            panic!("{name} in {:?}", m.flat.vars.iter().map(|v| &v.name).collect::<Vec<_>>())
        });
        VarId(i as u32)
    };
    let (x, xc, jump, count) = (var("x"), var("x.continuous"), var("x.jump"), var("n"));
    // the continuous part is the state; x and the jump are not
    assert_eq!(m.states, vec![xc]);
    let d_of = |v: VarId| m.discretes.iter().position(|d| *d == v).expect("discrete");
    let (dj, dn) = (d_of(jump), d_of(count));
    assert_eq!((l.n_x, l.n_whens), (1, 1));
    let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
    let u = vec![0.0; l.n_u];
    let mut work = vec![0.0; l.n_work];
    let channels = |y: &[f64], d: &[f64], work: &mut Vec<f64>| {
        let mut out = vec![0.0; l.n_vars];
        jit.vars(&EvalInput { t: 0.0, y, p: &p, d, u: &u }, work, &mut out);
        out
    };
    // before: continuous 0.7 with an earlier jump of 0.3 (x = 1.0), one
    // restart so far
    let y = [0.7];
    let mut d = vec![0.0; l.n_d];
    d[dj] = 0.3;
    d[dn] = 1.0;
    let before = channels(&y, &d, &mut work);
    assert_eq!(before[x.0 as usize], 0.7 + 0.3);
    let mut d_new = d.clone();
    jit.when(&EvalInput { t: 0.0, y: &y, p: &p, d: &d, u: &u }, &[1.0], &mut work, &mut d_new);
    // n = pre(n) + 1 = 2; x restarts from -2 x + n with x before the
    // event and n new: -2 + 2 = 0
    assert_eq!(d_new[dn], 2.0);
    let want = -2.0 * (0.7 + 0.3) + 2.0;
    assert_eq!(d_new[dj], want - 0.7, "the jump part");
    let now = channels(&y, &d_new, &mut work);
    let got = now[x.0 as usize];
    assert!(
        (got - want).abs() <= 2.0 * f64::EPSILON * 0.7f64.max(want.abs()),
        "x = {got}, not {want}"
    );
    // the continuous part integrates on: der(x) is read as its rate
    let mut rate = vec![0.0; l.n_y()];
    jit.residual(&EvalInput { t: 0.0, y: &y, p: &p, d: &d_new, u: &u }, &mut work, &mut rate);
    assert_eq!(rate, [1.0]);
    // a restart from a value of several magnitudes: x = v to one rounding
    for (xc0, j0) in [(1e6, -3.5), (-2.5e-3, 7.0), (123.456, 0.0)] {
        let y = [xc0];
        let mut d = vec![0.0; l.n_d];
        d[dj] = j0;
        let mut d_new = d.clone();
        jit.when(&EvalInput { t: 0.0, y: &y, p: &p, d: &d, u: &u }, &[1.0], &mut work, &mut d_new);
        let want = -2.0 * (xc0 + j0) + 1.0;
        let got = channels(&y, &d_new, &mut work)[x.0 as usize];
        let tol = 2.0 * f64::EPSILON * xc0.abs().max(want.abs());
        assert!((got - want).abs() <= tol, "from {xc0} + {j0}: x = {got}, not {want}");
    }
}
