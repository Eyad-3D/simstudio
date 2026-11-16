//! Evaluates a model's equations at given points, as the engine reads them
//! (parsed and flattened, every expression by the IR's reference
//! interpreter): the hard-problem harness's check that a transcription
//! gives the original's numbers (`benchmarks/hard`).
//!
//! ```text
//! cargo run --release -p lsim-project --example hard_eval -- MODEL.mo NAME POINTS.json OUT.json
//! ```
//!
//! `POINTS.json`: `[{"t": s, "vars": {name: value}, "ders": {name: value}}]`.
//! A variable not given is solved forward where an equation `v = e` has it
//! alone on its left and every name of `e` is known. `OUT.json`: per point,
//! each equation of the model's own equation section, in its order, as
//! `[lhs, rhs, largest term]` (null where a name is still unknown; the
//! largest term is the largest magnitude of a side or of an operand of a sum
//! or difference in it), and the names left unknown.

use lsim_ir::eval::{Env, eval};
use lsim_ir::expr::{BinaryOp, Expr};
use lsim_ir::flat::{FlatSystem, OriginKind, ParamId, VarId};
use lsim_lang::{Report, parse_library};
use serde_json::{Value, json};

struct Point<'a> {
    flat: &'a FlatSystem,
    t: f64,
    var: Vec<Option<f64>>,
    der: Vec<Option<f64>>,
}

impl Env for Point<'_> {
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> f64 {
        self.var[v.0 as usize].unwrap_or(f64::NAN)
    }
    fn der(&self, v: VarId) -> f64 {
        self.der[v.0 as usize].unwrap_or(f64::NAN)
    }
    fn param(&self, p: ParamId) -> f64 {
        let q = &self.flat.params[p.0 as usize];
        match &q.binding {
            Some(b) => eval(b, self),
            None => q.value,
        }
    }
}

impl Point<'_> {
    fn known(&self, e: &Expr) -> bool {
        !e.any(&mut |x| match x {
            Expr::Var(v) | Expr::Pre(v) => self.var[v.0 as usize].is_none(),
            Expr::Der(v) => self.der[v.0 as usize].is_none(),
            Expr::Name(_) | Expr::Table { .. } => true,
            _ => false,
        })
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 4 {
        eprintln!("usage: hard_eval MODEL.mo NAME POINTS.json OUT.json");
        std::process::exit(2);
    }
    let text = std::fs::read_to_string(&a[0]).expect("model text");
    let mut lib = lsim_lib::library();
    let parsed = match parse_library(&text, Some(&lib)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", Report(&e));
            std::process::exit(1);
        }
    };
    lib.types.extend(parsed.types);
    for def in parsed.components.into_values() {
        lib.add(def);
    }
    let model = lib.components.get(&a[1]).cloned().expect("no such model");
    let flat = match lsim_prep::flatten::flatten(&lib, &model) {
        Ok(f) => f,
        Err(d) => {
            for x in d {
                eprintln!("{x}");
            }
            std::process::exit(1);
        }
    };
    // the model's own equations, in the order written
    let mut own: Vec<(usize, &lsim_ir::flat::FlatEquation)> = flat
        .equations
        .iter()
        .filter_map(|e| match e.origin.kind {
            OriginKind::Component { index } if e.origin.instance.0 == 0 => Some((index, e)),
            _ => None,
        })
        .collect();
    own.sort_by_key(|(i, _)| *i);
    let points: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&a[2]).expect("points")).expect("JSON");
    let n = flat.vars.len();
    let mut out = vec![];
    for p in &points {
        let mut pt = Point {
            flat: &flat,
            t: p["t"].as_f64().expect("t"),
            var: vec![None; n],
            der: vec![None; n],
        };
        for (k, v) in flat.vars.iter().enumerate() {
            pt.var[k] = p["vars"][&v.name].as_f64();
            pt.der[k] = p["ders"][&v.name].as_f64();
        }
        // solve forward what stands alone on a left side
        loop {
            let mut moved = false;
            for e in &flat.equations {
                if let Expr::Var(v) = e.lhs
                    && pt.var[v.0 as usize].is_none()
                    && pt.known(&e.rhs)
                {
                    pt.var[v.0 as usize] = Some(eval(&e.rhs, &pt));
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        let rows: Vec<Value> = own
            .iter()
            .map(|(_, e)| {
                if pt.known(&e.lhs) && pt.known(&e.rhs) {
                    // the largest term the equation adds: each operand of a
                    // sum or difference, and each side
                    let (l, r) = (eval(&e.lhs, &pt), eval(&e.rhs, &pt));
                    let mut big = l.abs().max(r.abs());
                    for side in [&e.lhs, &e.rhs] {
                        side.walk(&mut |x| {
                            if let Expr::Binary(op, a, b) = x
                                && matches!(op, BinaryOp::Add | BinaryOp::Sub)
                            {
                                big = big.max(eval(a, &pt).abs()).max(eval(b, &pt).abs());
                            }
                        });
                    }
                    json!([l, r, big])
                } else {
                    Value::Null
                }
            })
            .collect();
        let unknown: Vec<&str> = flat
            .vars
            .iter()
            .enumerate()
            .filter(|(k, _)| pt.var[*k].is_none())
            .map(|(_, v)| v.name.as_str())
            .collect();
        out.push(json!({"rows": rows, "unknown": unknown}));
    }
    std::fs::write(&a[3], serde_json::to_string(&out).unwrap()).expect("output");
}
