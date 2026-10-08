//! Lowering of flat expressions to Cranelift IR, with optional forward-mode
//! tangents (dual numbers) for the Jacobian-vector product.

use crate::{CodegenError, Kind, Places};
use cranelift_codegen::ir::condcodes::FloatCC;
use cranelift_codegen::ir::types::F64;
use cranelift_codegen::ir::{AbiParam, FuncRef, InstBuilder, MemFlagsData, Type, Value};
use cranelift_frontend::FunctionBuilder;
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Linkage, Module};
use lsim_ir::VarId;
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use lsim_ir::prepared::{AliasTarget, PreparedModel, Slot};
use std::collections::HashMap;

macro_rules! unary_fns {
    ($($name:ident => $f:ident),* $(,)?) => {
        $(extern "C" fn $name(x: f64) -> f64 { x.$f() })*
        const UNARY: &[&str] = &[$(stringify!($name)),*];
        fn unary_ptrs() -> Vec<(&'static str, *const u8)> {
            vec![$((stringify!($name), $name as *const u8)),*]
        }
    };
}

unary_fns!(
    lsim_exp => exp, lsim_log => ln, lsim_sin => sin, lsim_cos => cos, lsim_tan => tan,
    lsim_asin => asin, lsim_acos => acos, lsim_atan => atan, lsim_sinh => sinh,
    lsim_cosh => cosh, lsim_tanh => tanh,
);

extern "C" fn lsim_pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

extern "C" fn lsim_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

const BINARY: &[&str] = &["lsim_pow", "lsim_atan2"];

/// The math functions generated code calls: name and address, registered
/// with the JIT so the code links against them.
pub fn math_symbols() -> Vec<(&'static str, *const u8)> {
    let mut v = unary_ptrs();
    v.push(("lsim_pow", lsim_pow as *const u8));
    v.push(("lsim_atan2", lsim_atan2 as *const u8));
    v
}

/// The names of [`math_symbols`].
pub const MATH_SYMBOLS: &[&str] = &[
    "lsim_exp",
    "lsim_log",
    "lsim_sin",
    "lsim_cos",
    "lsim_tan",
    "lsim_asin",
    "lsim_acos",
    "lsim_atan",
    "lsim_sinh",
    "lsim_cosh",
    "lsim_tanh",
    "lsim_pow",
    "lsim_atan2",
];

pub(crate) struct Math(HashMap<&'static str, FuncId>);

pub(crate) fn declare_math(m: &mut JITModule) -> Result<Math, CodegenError> {
    let mut ids = HashMap::new();
    for (names, n_args) in [(UNARY, 1), (BINARY, 2)] {
        for name in names {
            let mut sig = m.make_signature();
            for _ in 0..n_args {
                sig.params.push(AbiParam::new(F64));
            }
            sig.returns.push(AbiParam::new(F64));
            let id = m
                .declare_function(name, Linkage::Import, &sig)
                .map_err(|e| CodegenError::Backend(e.to_string()))?;
            ids.insert(*name, id);
        }
    }
    Ok(Math(ids))
}

pub(crate) struct MathRefs(HashMap<&'static str, FuncRef>);

pub(crate) fn import_math(m: &mut JITModule, b: &mut FunctionBuilder<'_>, math: &Math) -> MathRefs {
    MathRefs(math.0.iter().map(|(k, id)| (*k, m.declare_func_in_func(*id, b.func))).collect())
}

struct Lower<'a, 'b> {
    b: &'a mut FunctionBuilder<'b>,
    places: &'a Places,
    math: &'a MathRefs,
    t: Value,
    y: Value,
    p: Value,
    d: Value,
    u: Value,
    v: Option<Value>,
    dual: bool,
    vals: HashMap<Slot, (Value, Value)>,
    loads: HashMap<(u8, usize), Value>,
    zero: Value,
}

type Dual = (Value, Value);

fn mem() -> MemFlagsData {
    MemFlagsData::trusted()
}

impl Lower<'_, '_> {
    fn c(&mut self, x: f64) -> Value {
        self.b.ins().f64const(x)
    }

    fn load(&mut self, which: u8, base: Value, i: usize) -> Value {
        if let Some(v) = self.loads.get(&(which, i)) {
            return *v;
        }
        let v = self.b.ins().load(F64, mem(), base, (8 * i) as i32);
        self.loads.insert((which, i), v);
        v
    }

    fn y_at(&mut self, i: usize) -> Dual {
        let val = self.load(0, self.y, i);
        let tan = match (self.dual, self.v) {
            (true, Some(vp)) => self.load(1, vp, i),
            _ => self.zero,
        };
        (val, tan)
    }

    fn slot(&mut self, s: Slot) -> Result<Dual, CodegenError> {
        if let Some(&i) = self.places.y_index.get(&s) {
            return Ok(self.y_at(i));
        }
        self.vals.get(&s).copied().ok_or_else(|| {
            CodegenError::Unsupported(format!(
                "{s:?} is used before it is computed (an ordering bug)"
            ))
        })
    }

    fn var(&mut self, v: VarId) -> Result<Dual, CodegenError> {
        if let Some(&k) = self.places.d_index.get(&v.0) {
            return Ok((self.load(2, self.d, k), self.zero));
        }
        if let Some(&k) = self.places.u_index.get(&v.0) {
            return Ok((self.load(3, self.u, k), self.zero));
        }
        self.slot(Slot::Var(v))
    }

    fn call1(&mut self, name: &str, a: Value) -> Value {
        let f = self.math.0[name];
        let inst = self.b.ins().call(f, &[a]);
        self.b.inst_results(inst)[0]
    }

    fn call2(&mut self, name: &str, a: Value, b: Value) -> Value {
        let f = self.math.0[name];
        let inst = self.b.ins().call(f, &[a, b]);
        self.b.inst_results(inst)[0]
    }

    fn mul(&mut self, a: Value, b: Value) -> Value {
        self.b.ins().fmul(a, b)
    }

    fn truth(&mut self, cond: Value) -> Value {
        let one = self.c(1.0);
        let zero = self.c(0.0);
        self.b.ins().select(cond, one, zero)
    }

    fn is_true(&mut self, x: Value) -> Value {
        let zero = self.c(0.0);
        self.b.ins().fcmp(FloatCC::NotEqual, x, zero)
    }

    /// x^n for a whole n with |n| <= 8, by multiplication.
    fn powi(&mut self, x: Value, n: i32) -> Value {
        let mut acc = self.c(1.0);
        for _ in 0..n.unsigned_abs() {
            acc = self.b.ins().fmul(acc, x);
        }
        if n < 0 {
            let one = self.c(1.0);
            acc = self.b.ins().fdiv(one, acc);
        }
        acc
    }

    fn lower(&mut self, e: &Expr) -> Result<Dual, CodegenError> {
        let z = self.zero;
        Ok(match e {
            Expr::Const(x) => (self.c(*x), z),
            Expr::Time => (self.t, z),
            Expr::Var(v) => self.var(*v)?,
            Expr::Der(v) => self.slot(Slot::Der(*v))?,
            Expr::Pre(v) => self.var(*v)?,
            Expr::Param(p) => (self.load(4, self.p, p.0 as usize), z),
            Expr::Name(n) => {
                return Err(CodegenError::Unsupported(format!("unresolved name '{n}'")));
            }
            Expr::Table { .. } => {
                return Err(CodegenError::Unsupported("tables (work package 3)".into()));
            }
            Expr::Neg(a) => {
                let (va, ta) = self.lower(a)?;
                let v = self.b.ins().fneg(va);
                let t = if self.dual { self.b.ins().fneg(ta) } else { z };
                (v, t)
            }
            Expr::NoEvent(a) => self.lower(a)?,
            Expr::Binary(op, a, b) => {
                if let (BinaryOp::Pow, Expr::Const(n)) = (op, &**b) {
                    let (va, ta) = self.lower(a)?;
                    let n = *n;
                    let (v, dv_da) = if n.fract() == 0.0 && n.abs() <= 8.0 {
                        let v = self.powi(va, n as i32);
                        let d = if self.dual {
                            let pm1 = self.powi(va, n as i32 - 1);
                            let cn = self.c(n);
                            self.mul(cn, pm1)
                        } else {
                            z
                        };
                        (v, d)
                    } else if n == 0.5 {
                        let v = self.b.ins().sqrt(va);
                        let d = if self.dual {
                            let two = self.c(2.0);
                            let den = self.mul(two, v);
                            let one = self.c(1.0);
                            self.b.ins().fdiv(one, den)
                        } else {
                            z
                        };
                        (v, d)
                    } else {
                        let cn = self.c(n);
                        let v = self.call2("lsim_pow", va, cn);
                        let d = if self.dual {
                            let cm = self.c(n - 1.0);
                            let pm1 = self.call2("lsim_pow", va, cm);
                            self.mul(cn, pm1)
                        } else {
                            z
                        };
                        (v, d)
                    };
                    let t = if self.dual { self.mul(dv_da, ta) } else { z };
                    return Ok((v, t));
                }
                let (va, ta) = self.lower(a)?;
                let (vb, tb) = self.lower(b)?;
                match op {
                    BinaryOp::Add => {
                        let v = self.b.ins().fadd(va, vb);
                        let t = if self.dual { self.b.ins().fadd(ta, tb) } else { z };
                        (v, t)
                    }
                    BinaryOp::Sub => {
                        let v = self.b.ins().fsub(va, vb);
                        let t = if self.dual { self.b.ins().fsub(ta, tb) } else { z };
                        (v, t)
                    }
                    BinaryOp::Mul => {
                        let v = self.b.ins().fmul(va, vb);
                        let t = if self.dual {
                            let x = self.b.ins().fmul(ta, vb);
                            let y = self.b.ins().fmul(va, tb);
                            self.b.ins().fadd(x, y)
                        } else {
                            z
                        };
                        (v, t)
                    }
                    BinaryOp::Div => {
                        let v = self.b.ins().fdiv(va, vb);
                        let t = if self.dual {
                            let x = self.b.ins().fmul(v, tb);
                            let y = self.b.ins().fsub(ta, x);
                            self.b.ins().fdiv(y, vb)
                        } else {
                            z
                        };
                        (v, t)
                    }
                    BinaryOp::Pow => {
                        let v = self.call2("lsim_pow", va, vb);
                        let t = if self.dual {
                            // v (tb ln a + b ta / a)
                            let ln = self.call1("lsim_log", va);
                            let x = self.b.ins().fmul(tb, ln);
                            let y = self.b.ins().fmul(vb, ta);
                            let y = self.b.ins().fdiv(y, va);
                            let s = self.b.ins().fadd(x, y);
                            self.b.ins().fmul(v, s)
                        } else {
                            z
                        };
                        (v, t)
                    }
                }
            }
            Expr::Call(f, args) => self.call(*f, args)?,
            Expr::Compare(op, a, b) => {
                let (va, _) = self.lower(a)?;
                let (vb, _) = self.lower(b)?;
                let cc = match op {
                    CmpOp::Lt => FloatCC::LessThan,
                    CmpOp::Le => FloatCC::LessThanOrEqual,
                    CmpOp::Gt => FloatCC::GreaterThan,
                    CmpOp::Ge => FloatCC::GreaterThanOrEqual,
                };
                let c = self.b.ins().fcmp(cc, va, vb);
                (self.truth(c), z)
            }
            Expr::And(a, b) | Expr::Or(a, b) => {
                let (va, _) = self.lower(a)?;
                let (vb, _) = self.lower(b)?;
                let (ca, cb) = (self.is_true(va), self.is_true(vb));
                let c = if matches!(e, Expr::And(..)) {
                    self.b.ins().band(ca, cb)
                } else {
                    self.b.ins().bor(ca, cb)
                };
                (self.truth(c), z)
            }
            Expr::Not(a) => {
                let (va, _) = self.lower(a)?;
                let zero = self.c(0.0);
                let c = self.b.ins().fcmp(FloatCC::Equal, va, zero);
                (self.truth(c), z)
            }
            Expr::If(c, a, b) => {
                let (vc, _) = self.lower(c)?;
                let cond = self.is_true(vc);
                let (va, ta) = self.lower(a)?;
                let (vb, tb) = self.lower(b)?;
                let v = self.b.ins().select(cond, va, vb);
                let t = if self.dual { self.b.ins().select(cond, ta, tb) } else { z };
                (v, t)
            }
        })
    }

    fn call(&mut self, f: Builtin, args: &[Expr]) -> Result<Dual, CodegenError> {
        let z = self.zero;
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.lower(a)?);
        }
        let (va, ta) = vals[0];
        let unary = |s: &mut Self, name: &str| s.call1(name, va);
        // value, and d(value)/d(arg0) (multiplied by the tangent below)
        let (v, dv): (Value, Option<Value>) = match f {
            Builtin::Der | Builtin::Pre => {
                return Err(CodegenError::Unsupported("der/pre in component scope".into()));
            }
            Builtin::Sqrt => {
                let v = self.b.ins().sqrt(va);
                let d = if self.dual {
                    let two = self.c(2.0);
                    let den = self.mul(two, v);
                    let one = self.c(1.0);
                    Some(self.b.ins().fdiv(one, den))
                } else {
                    None
                };
                (v, d)
            }
            Builtin::Abs => {
                let v = self.b.ins().fabs(va);
                let d = if self.dual { Some(self.sign(va)) } else { None };
                (v, d)
            }
            Builtin::Sign => (self.sign(va), None),
            Builtin::Exp => {
                let v = unary(self, "lsim_exp");
                (v, Some(v))
            }
            Builtin::Log => {
                let v = unary(self, "lsim_log");
                let d = if self.dual {
                    let one = self.c(1.0);
                    Some(self.b.ins().fdiv(one, va))
                } else {
                    None
                };
                (v, d)
            }
            Builtin::Sin => {
                let v = unary(self, "lsim_sin");
                let d = if self.dual { Some(unary(self, "lsim_cos")) } else { None };
                (v, d)
            }
            Builtin::Cos => {
                let v = unary(self, "lsim_cos");
                let d = if self.dual {
                    let s = unary(self, "lsim_sin");
                    Some(self.b.ins().fneg(s))
                } else {
                    None
                };
                (v, d)
            }
            Builtin::Tan => {
                let v = unary(self, "lsim_tan");
                let d = if self.dual {
                    let one = self.c(1.0);
                    let v2 = self.mul(v, v);
                    Some(self.b.ins().fadd(one, v2))
                } else {
                    None
                };
                (v, d)
            }
            Builtin::Asin | Builtin::Acos => {
                let name = if f == Builtin::Asin { "lsim_asin" } else { "lsim_acos" };
                let v = unary(self, name);
                let d = if self.dual {
                    let one = self.c(1.0);
                    let a2 = self.mul(va, va);
                    let s = self.b.ins().fsub(one, a2);
                    let r = self.b.ins().sqrt(s);
                    let q = self.b.ins().fdiv(one, r);
                    Some(if f == Builtin::Acos { self.b.ins().fneg(q) } else { q })
                } else {
                    None
                };
                (v, d)
            }
            Builtin::Atan => {
                let v = unary(self, "lsim_atan");
                let d = if self.dual {
                    let one = self.c(1.0);
                    let a2 = self.mul(va, va);
                    let s = self.b.ins().fadd(one, a2);
                    Some(self.b.ins().fdiv(one, s))
                } else {
                    None
                };
                (v, d)
            }
            Builtin::Sinh => {
                let v = unary(self, "lsim_sinh");
                let d = if self.dual { Some(unary(self, "lsim_cosh")) } else { None };
                (v, d)
            }
            Builtin::Cosh => {
                let v = unary(self, "lsim_cosh");
                let d = if self.dual { Some(unary(self, "lsim_sinh")) } else { None };
                (v, d)
            }
            Builtin::Tanh => {
                let v = unary(self, "lsim_tanh");
                let d = if self.dual {
                    let one = self.c(1.0);
                    let v2 = self.mul(v, v);
                    Some(self.b.ins().fsub(one, v2))
                } else {
                    None
                };
                (v, d)
            }
            Builtin::Atan2 => {
                let (vx, tx) = vals[1];
                let v = self.call2("lsim_atan2", va, vx);
                let t = if self.dual {
                    // (x dy - y dx) / (x² + y²)
                    let a = self.mul(vx, ta);
                    let b = self.mul(va, tx);
                    let num = self.b.ins().fsub(a, b);
                    let x2 = self.mul(vx, vx);
                    let y2 = self.mul(va, va);
                    let den = self.b.ins().fadd(x2, y2);
                    self.b.ins().fdiv(num, den)
                } else {
                    z
                };
                return Ok((v, t));
            }
            Builtin::Min | Builtin::Max | Builtin::Limit => {
                let pick = |s: &mut Self, cc: FloatCC, (a, ta): Dual, (b, tb): Dual| -> Dual {
                    let c = s.b.ins().fcmp(cc, a, b);
                    let v = s.b.ins().select(c, a, b);
                    let t = if s.dual { s.b.ins().select(c, ta, tb) } else { s.zero };
                    (v, t)
                };
                return Ok(match f {
                    Builtin::Min => pick(self, FloatCC::LessThan, vals[0], vals[1]),
                    Builtin::Max => pick(self, FloatCC::GreaterThan, vals[0], vals[1]),
                    _ => {
                        let lo = pick(self, FloatCC::GreaterThan, vals[0], vals[1]);
                        pick(self, FloatCC::LessThan, lo, vals[2])
                    }
                });
            }
        };
        let t = match (self.dual, dv) {
            (true, Some(d)) => self.mul(d, ta),
            _ => z,
        };
        Ok((v, t))
    }

    fn sign(&mut self, x: Value) -> Value {
        let zero = self.c(0.0);
        let one = self.c(1.0);
        let minus = self.c(-1.0);
        let pos = self.b.ins().fcmp(FloatCC::GreaterThan, x, zero);
        let neg = self.b.ins().fcmp(FloatCC::LessThan, x, zero);
        let m = self.b.ins().select(neg, minus, zero);
        self.b.ins().select(pos, one, m)
    }

    fn store(&mut self, base: Value, i: usize, v: Value) {
        self.b.ins().store(mem(), v, base, (8 * i) as i32);
    }
}

/// Generates the body of one function.
pub(crate) fn body(
    b: &mut FunctionBuilder<'_>,
    kind: Kind,
    model: &PreparedModel,
    places: &Places,
    math: &MathRefs,
    _ptr: Type,
) -> Result<(), CodegenError> {
    let block = b.create_block();
    b.append_block_params_for_function_params(block);
    b.switch_to_block(block);
    let params = b.block_params(block).to_vec();
    let extra = matches!(kind, Kind::Jvp | Kind::When);
    let (vptr, out) = if extra { (Some(params[5]), params[7]) } else { (None, params[6]) };
    let zero = b.ins().f64const(0.0);
    let mut lw = Lower {
        b,
        places,
        math,
        t: params[0],
        y: params[1],
        p: params[2],
        d: params[3],
        u: params[4],
        v: vptr,
        dual: kind == Kind::Jvp,
        vals: HashMap::new(),
        loads: HashMap::new(),
        zero,
    };
    for a in &model.assignments {
        let r = lw.lower(&a.expr)?;
        lw.vals.insert(a.target, r);
    }
    let n_x = places.n_x;
    match kind {
        Kind::Residual | Kind::Jvp => {
            let pick = |d: Dual| if kind == Kind::Jvp { d.1 } else { d.0 };
            for (i, x) in model.states.iter().enumerate() {
                let d = lw.slot(Slot::Der(*x))?;
                lw.store(out, i, pick(d));
            }
            for (k, r) in model.residuals.iter().enumerate() {
                let d = lw.lower(&r.expr)?;
                lw.store(out, n_x + k, pick(d));
            }
        }
        Kind::Roots => {
            for (k, zc) in model.zero_crossings.iter().enumerate() {
                let (v, _) = lw.lower(&zc.expr)?;
                lw.store(out, k, v);
            }
        }
        Kind::Vars => {
            let mut alias: HashMap<u32, AliasTarget> = HashMap::new();
            for a in &model.aliases {
                alias.insert(a.var.0, a.target);
            }
            for i in 0..model.flat.vars.len() {
                let v = match alias.get(&(i as u32)) {
                    Some(AliasTarget::Const(c)) => lw.c(*c),
                    Some(AliasTarget::Var { var, negated }) => {
                        let (x, _) = lw.var(*var)?;
                        if *negated { lw.b.ins().fneg(x) } else { x }
                    }
                    None => lw.var(VarId(i as u32))?.0,
                };
                lw.store(out, i, v);
            }
        }
        Kind::When => {
            let fired = vptr.expect("when has a fired pointer");
            for (k, w) in model.whens.iter().enumerate() {
                let f = lw.b.ins().load(F64, mem(), fired, (8 * k) as i32);
                let cond = lw.is_true(f);
                for (var, expr) in &w.assign {
                    let idx = places.d_index[&var.0];
                    let (new, _) = lw.lower(expr)?;
                    let old = lw.b.ins().load(F64, mem(), out, (8 * idx) as i32);
                    let v = lw.b.ins().select(cond, new, old);
                    lw.store(out, idx, v);
                }
            }
        }
    }
    lw.b.ins().return_(&[]);
    Ok(())
}
