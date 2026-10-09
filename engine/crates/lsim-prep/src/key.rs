//! The structure key: a digest of everything that shapes the generated
//! code, and nothing that does not (runtime parameter values, table data,
//! labels, origins). Two models with the same key share compiled code.
//!
//! The model is fed to SHA-256 in a compact binary encoding (a tag byte
//! per expression node, numbers as their bits), so a 10⁵-equation model
//! hashes in milliseconds.

use lsim_ir::expr::Expr;
use lsim_ir::prepared::{AliasTarget, PreparedModel, Slot};
use sha2::{Digest, Sha256};

struct Feed {
    sha: Sha256,
    buf: Vec<u8>,
}

impl Feed {
    fn new() -> Self {
        Feed { sha: Sha256::new(), buf: Vec::with_capacity(1 << 16) }
    }

    fn flush(&mut self) {
        self.sha.update(&self.buf);
        self.buf.clear();
    }

    fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
        if self.buf.len() >= 1 << 16 {
            self.flush();
        }
    }

    fn tag(&mut self, t: u8) {
        self.bytes(&[t]);
    }

    fn u64(&mut self, x: u64) {
        self.bytes(&x.to_le_bytes());
    }

    fn u32(&mut self, x: u32) {
        self.bytes(&x.to_le_bytes());
    }

    fn f64(&mut self, x: f64) {
        self.u64(x.to_bits());
    }

    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.bytes(s.as_bytes());
    }

    fn slot(&mut self, s: Slot) {
        match s {
            Slot::Var(v) => {
                self.tag(0);
                self.u32(v.0);
            }
            Slot::Der(v) => {
                self.tag(1);
                self.u32(v.0);
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Const(v) => {
                self.tag(0);
                self.f64(*v);
            }
            Expr::Time => self.tag(1),
            Expr::Name(n) => {
                self.tag(2);
                self.str(n);
            }
            Expr::Var(v) => {
                self.tag(3);
                self.u32(v.0);
            }
            Expr::Param(p) => {
                self.tag(4);
                self.u32(p.0);
            }
            Expr::Der(v) => {
                self.tag(5);
                self.u32(v.0);
            }
            Expr::Pre(v) => {
                self.tag(6);
                self.u32(v.0);
            }
            Expr::Neg(a) => {
                self.tag(7);
                self.expr(a);
            }
            Expr::Binary(op, a, b) => {
                self.tag(8);
                self.tag(*op as u8);
                self.expr(a);
                self.expr(b);
            }
            Expr::Call(f, args) => {
                self.tag(9);
                self.tag(*f as u8);
                self.u32(args.len() as u32);
                for a in args {
                    self.expr(a);
                }
            }
            Expr::Compare(op, a, b) => {
                self.tag(10);
                self.tag(*op as u8);
                self.expr(a);
                self.expr(b);
            }
            Expr::And(a, b) => {
                self.tag(11);
                self.expr(a);
                self.expr(b);
            }
            Expr::Or(a, b) => {
                self.tag(12);
                self.expr(a);
                self.expr(b);
            }
            Expr::Not(a) => {
                self.tag(13);
                self.expr(a);
            }
            Expr::If(c, a, b) => {
                self.tag(14);
                self.expr(c);
                self.expr(a);
                self.expr(b);
            }
            Expr::NoEvent(a) => {
                self.tag(15);
                self.expr(a);
            }
            Expr::Table { table, args } => {
                self.tag(16);
                self.u32(*table);
                self.u32(args.len() as u32);
                for a in args {
                    self.expr(a);
                }
            }
        }
    }

    fn section(&mut self, name: &str, len: usize) {
        self.str(name);
        self.u64(len as u64);
    }
}

/// SHA-256 (hex) of the model's structure.
pub fn structure_key(m: &PreparedModel) -> String {
    let mut f = Feed::new();
    f.str("lsim-prep structure key 2");
    f.section("sizes", m.flat.vars.len());
    f.u64(m.flat.params.len() as u64);
    f.u64(m.flat.tables.len() as u64);
    f.section("states", m.states.len());
    for v in &m.states {
        f.u32(v.0);
    }
    f.section("algebraics", m.algebraics.len());
    for s in &m.algebraics {
        f.slot(*s);
    }
    f.section("discretes", m.discretes.len());
    for v in &m.discretes {
        f.u32(v.0);
    }
    f.section("inputs", m.inputs.len());
    for v in &m.inputs {
        f.u32(v.0);
    }
    f.section("assignments", m.assignments.len());
    for a in &m.assignments {
        f.slot(a.target);
        f.expr(&a.expr);
    }
    f.section("residuals", m.residuals.len());
    for r in &m.residuals {
        f.expr(&r.expr);
    }
    f.section("aliases", m.aliases.len());
    for a in &m.aliases {
        f.u32(a.var.0);
        match a.target {
            AliasTarget::Var { var, negated } => {
                f.tag(u8::from(negated));
                f.u32(var.0);
            }
            AliasTarget::Const(c) => {
                f.tag(2);
                f.f64(c);
            }
        }
    }
    f.section("crossings", m.zero_crossings.len());
    for z in &m.zero_crossings {
        f.expr(&z.expr);
    }
    f.section("whens", m.whens.len());
    for w in &m.whens {
        f.u64(w.crossing as u64);
        f.tag(w.direction as u8);
        f.u64(w.assign.len() as u64);
        for (v, x) in &w.assign {
            f.u32(v.0);
            f.expr(x);
        }
    }
    f.section("structural parameters", 0);
    for p in m.flat.params.iter().filter(|p| p.structural) {
        f.str(&p.name);
        f.f64(p.value);
    }
    f.section("tables", m.flat.tables.len());
    for t in &m.flat.tables {
        // the rules shape the code, the data do not
        f.tag(t.data.interpolation as u8);
        f.tag(t.data.outside[0] as u8);
        f.tag(t.data.outside[1] as u8);
        f.tag(u8::from(!t.data.y.is_empty()));
    }
    f.section("modes", m.modes.len());
    for x in &m.modes {
        f.u32(x.var.0);
        f.expr(&x.relation);
        f.u64(x.crossing as u64);
    }
    let i = &m.init;
    f.section("init", i.unknowns.len());
    for (s, g) in i.unknowns.iter().zip(&i.guesses) {
        f.slot(*s);
        f.expr(g);
    }
    f.u64(i.assignments.len() as u64);
    for a in &i.assignments {
        f.slot(a.target);
        f.expr(&a.expr);
    }
    f.u64(i.residuals.len() as u64);
    for r in &i.residuals {
        f.expr(&r.expr);
    }
    f.u64(i.discrete_starts.len() as u64);
    for d in &i.discrete_starts {
        f.expr(d);
    }
    f.section("limits", m.limits.len());
    for l in &m.limits {
        f.expr(&l.value);
        f.expr(&l.lo);
        f.expr(&l.hi);
    }
    f.flush();
    f.sha.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
