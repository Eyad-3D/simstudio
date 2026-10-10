//! What the lowering emits into: Cranelift IR (machine code) or a tape
//! (interpreted). Both take the same operations in the same order, so a
//! function and its tape compute bitwise the same values.

use crate::CodegenError;
use crate::jit::Import;
use cranelift_codegen::ir::condcodes::FloatCC;
use cranelift_codegen::ir::types::{F64, I64};
use cranelift_codegen::ir::{
    FuncRef, InstBuilder, MemFlagsData, StackSlot, StackSlotData, StackSlotKind, Value,
};
use cranelift_frontend::FunctionBuilder;
use std::collections::HashMap;

/// What a generated function reads and writes, found through its one
/// argument, a pointer to a [`crate::CallCtx`]: one value live across the
/// whole function (and every call it makes), the arrays' addresses loaded
/// from it where a segment needs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Base {
    Y,
    P,
    D,
    U,
    V,
    Work,
    Out,
}

impl Base {
    /// The field's offset in [`crate::CallCtx`].
    fn offset(self) -> i32 {
        8 * match self {
            Base::Y => 1,
            Base::P => 2,
            Base::D => 3,
            Base::U => 4,
            Base::V => 5,
            Base::Work => 6,
            Base::Out => 7,
        }
    }

    pub(crate) fn index(self) -> usize {
        self.offset() as usize / 8 - 1
    }
}

/// [`crate::CallCtx`]'s table pointers' offset.
const TABS_OFFSET: i32 = 64;

/// A comparison (Cranelift's meaning: `Ne` is true when unordered).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cc {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    /// either is NaN
    Uno,
}

impl Cc {
    fn clif(self) -> FloatCC {
        match self {
            Cc::Lt => FloatCC::LessThan,
            Cc::Le => FloatCC::LessThanOrEqual,
            Cc::Gt => FloatCC::GreaterThan,
            Cc::Ge => FloatCC::GreaterThanOrEqual,
            Cc::Eq => FloatCC::Equal,
            Cc::Ne => FloatCC::NotEqual,
            Cc::Uno => FloatCC::Unordered,
        }
    }

    /// The comparison as the tape evaluates it.
    #[inline]
    pub(crate) fn eval(self, a: f64, b: f64) -> bool {
        match self {
            Cc::Lt => a < b,
            Cc::Le => a <= b,
            Cc::Gt => a > b,
            Cc::Ge => a >= b,
            Cc::Eq => a == b,
            Cc::Ne => a != b,
            Cc::Uno => a.is_nan() || b.is_nan(),
        }
    }
}

/// A function of the mathematical library (the interpreter's own: Rust's
/// standard library).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Lib {
    Exp,
    Log,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
    Pow,
    Atan2,
}

impl Lib {
    pub(crate) fn symbol(self) -> &'static str {
        match self {
            Lib::Exp => "lsim_exp",
            Lib::Log => "lsim_log",
            Lib::Sin => "lsim_sin",
            Lib::Cos => "lsim_cos",
            Lib::Tan => "lsim_tan",
            Lib::Asin => "lsim_asin",
            Lib::Acos => "lsim_acos",
            Lib::Atan => "lsim_atan",
            Lib::Sinh => "lsim_sinh",
            Lib::Cosh => "lsim_cosh",
            Lib::Tanh => "lsim_tanh",
            Lib::Pow => "lsim_pow",
            Lib::Atan2 => "lsim_atan2",
        }
    }

    /// The function itself (exactly what the generated code calls).
    #[inline]
    pub(crate) fn eval(self, a: f64, b: f64) -> f64 {
        match self {
            Lib::Exp => a.exp(),
            Lib::Log => a.ln(),
            Lib::Sin => a.sin(),
            Lib::Cos => a.cos(),
            Lib::Tan => a.tan(),
            Lib::Asin => a.asin(),
            Lib::Acos => a.acos(),
            Lib::Atan => a.atan(),
            Lib::Sinh => a.sinh(),
            Lib::Cosh => a.cosh(),
            Lib::Tanh => a.tanh(),
            Lib::Pow => a.powf(b),
            Lib::Atan2 => a.atan2(b),
        }
    }

    pub(crate) fn arity(self) -> usize {
        if matches!(self, Lib::Pow | Lib::Atan2) { 2 } else { 1 }
    }
}

/// The operations the lowering emits. Values are numbers (f64) or truths
/// (the result of a comparison, `and`, `or`).
pub(crate) trait Emit {
    /// A value of the emitted code.
    type V: Copy + std::fmt::Debug;

    /// Starts a new segment (nothing loaded before is reused).
    fn new_segment(&mut self);
    fn konst(&mut self, x: f64) -> Self::V;
    fn time(&mut self) -> Self::V;
    /// `arr[i]`
    fn load(&mut self, arr: Base, i: usize) -> Self::V;
    /// `arr[i] = v`
    fn store(&mut self, arr: Base, i: usize, v: Self::V);
    fn add(&mut self, a: Self::V, b: Self::V) -> Self::V;
    fn sub(&mut self, a: Self::V, b: Self::V) -> Self::V;
    fn mul(&mut self, a: Self::V, b: Self::V) -> Self::V;
    fn div(&mut self, a: Self::V, b: Self::V) -> Self::V;
    fn neg(&mut self, a: Self::V) -> Self::V;
    fn abs(&mut self, a: Self::V) -> Self::V;
    fn sqrt(&mut self, a: Self::V) -> Self::V;
    /// `a b + c`, rounded once
    fn fma(&mut self, a: Self::V, b: Self::V, c: Self::V) -> Self::V;
    /// a truth
    fn cmp(&mut self, cc: Cc, a: Self::V, b: Self::V) -> Self::V;
    fn and(&mut self, a: Self::V, b: Self::V) -> Self::V;
    fn or(&mut self, a: Self::V, b: Self::V) -> Self::V;
    /// `if c then a else b` (`c` a truth)
    fn select(&mut self, c: Self::V, a: Self::V, b: Self::V) -> Self::V;
    /// the bitwise `and` (`or`) of two numbers' representations
    fn bits_and(&mut self, a: Self::V, b: Self::V) -> Self::V;
    fn bits_or(&mut self, a: Self::V, b: Self::V) -> Self::V;
    fn call(&mut self, f: Lib, args: &[Self::V]) -> Self::V;
    /// Table `k` at `args` (1 or 2), with its partial derivatives when
    /// asked.
    fn table(&mut self, k: u32, args: &[Self::V], derivs: bool) -> (Self::V, [Option<Self::V>; 2]);
    /// Table `k`'s guard of axis `axis` at `x`.
    fn table_guard(&mut self, k: u32, axis: u8, x: Self::V) -> Self::V;
}

fn mem() -> MemFlagsData {
    MemFlagsData::trusted()
}

/// Emission into a Cranelift function.
pub(crate) struct Clif<'a, 'f> {
    pub b: &'a mut FunctionBuilder<'f>,
    ctx: Value,
    decls: &'a HashMap<&'static str, Import>,
    refs: HashMap<&'static str, FuncRef>,
    /// time, the arrays' and the tables' addresses loaded in this segment
    bases: [Option<Value>; 9],
    scratch: Option<StackSlot>,
}

impl<'a, 'f> Clif<'a, 'f> {
    pub(crate) fn new(
        b: &'a mut FunctionBuilder<'f>,
        ctx: Value,
        decls: &'a HashMap<&'static str, Import>,
    ) -> Self {
        Clif { b, ctx, decls, refs: HashMap::new(), bases: [None; 9], scratch: None }
    }

    fn base_at(&mut self, slot: usize, offset: i32) -> Value {
        if let Some(v) = self.bases[slot] {
            return v;
        }
        let v = self.b.ins().load(I64, mem(), self.ctx, offset);
        self.bases[slot] = Some(v);
        v
    }

    fn base(&mut self, arr: Base) -> Value {
        self.base_at(arr.index() + 1, arr.offset())
    }

    fn import(&mut self, name: &'static str) -> Result<FuncRef, CodegenError> {
        if let Some(r) = self.refs.get(name) {
            return Ok(*r);
        }
        let imp = self
            .decls
            .get(name)
            .ok_or_else(|| CodegenError::Backend(format!("internal: no symbol {name}")))?;
        let sig = self.b.func.import_signature(imp.sig.clone());
        let user =
            self.b.func.declare_imported_user_function(cranelift_codegen::ir::UserExternalName {
                namespace: 0,
                index: imp.id.as_u32(),
            });
        let r = self.b.func.import_function(cranelift_codegen::ir::ExtFuncData {
            name: cranelift_codegen::ir::ExternalName::user(user),
            signature: sig,
            colocated: false,
            patchable: false,
        });
        self.refs.insert(name, r);
        Ok(r)
    }

    fn call_symbol(&mut self, name: &'static str, args: &[Value]) -> Value {
        let f = self.import(name).expect("every runtime symbol is declared");
        let inst = self.b.ins().call(f, args);
        self.b.inst_results(inst)[0]
    }

    fn table_ptr(&mut self, k: u32) -> Value {
        let tabs = self.base_at(8, TABS_OFFSET);
        self.b.ins().load(I64, mem(), tabs, (8 * k) as i32)
    }

    fn scratch(&mut self) -> Value {
        let ss = match self.scratch {
            Some(s) => s,
            None => {
                let s = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    16,
                    3,
                ));
                self.scratch = Some(s);
                s
            }
        };
        self.b.ins().stack_addr(I64, ss, 0)
    }
}

impl Emit for Clif<'_, '_> {
    type V = Value;

    fn new_segment(&mut self) {
        self.bases = [None; 9];
    }

    fn konst(&mut self, x: f64) -> Value {
        self.b.ins().f64const(x)
    }

    fn time(&mut self) -> Value {
        if let Some(v) = self.bases[0] {
            return v;
        }
        let v = self.b.ins().load(F64, mem(), self.ctx, 0);
        self.bases[0] = Some(v);
        v
    }

    fn load(&mut self, arr: Base, i: usize) -> Value {
        let base = self.base(arr);
        self.b.ins().load(F64, mem(), base, (8 * i) as i32)
    }

    fn store(&mut self, arr: Base, i: usize, v: Value) {
        let base = self.base(arr);
        self.b.ins().store(mem(), v, base, (8 * i) as i32);
    }

    fn add(&mut self, a: Value, b: Value) -> Value {
        self.b.ins().fadd(a, b)
    }

    fn sub(&mut self, a: Value, b: Value) -> Value {
        self.b.ins().fsub(a, b)
    }

    fn mul(&mut self, a: Value, b: Value) -> Value {
        self.b.ins().fmul(a, b)
    }

    fn div(&mut self, a: Value, b: Value) -> Value {
        self.b.ins().fdiv(a, b)
    }

    fn neg(&mut self, a: Value) -> Value {
        self.b.ins().fneg(a)
    }

    fn abs(&mut self, a: Value) -> Value {
        self.b.ins().fabs(a)
    }

    fn sqrt(&mut self, a: Value) -> Value {
        self.b.ins().sqrt(a)
    }

    fn fma(&mut self, a: Value, b: Value, c: Value) -> Value {
        self.b.ins().fma(a, b, c)
    }

    fn cmp(&mut self, cc: Cc, a: Value, b: Value) -> Value {
        self.b.ins().fcmp(cc.clif(), a, b)
    }

    fn and(&mut self, a: Value, b: Value) -> Value {
        self.b.ins().band(a, b)
    }

    fn or(&mut self, a: Value, b: Value) -> Value {
        self.b.ins().bor(a, b)
    }

    fn select(&mut self, c: Value, a: Value, b: Value) -> Value {
        self.b.ins().select(c, a, b)
    }

    fn bits_and(&mut self, a: Value, b: Value) -> Value {
        let flags = MemFlagsData::new();
        let (x, y) = (self.b.ins().bitcast(I64, flags, a), self.b.ins().bitcast(I64, flags, b));
        let r = self.b.ins().band(x, y);
        self.b.ins().bitcast(F64, flags, r)
    }

    fn bits_or(&mut self, a: Value, b: Value) -> Value {
        let flags = MemFlagsData::new();
        let (x, y) = (self.b.ins().bitcast(I64, flags, a), self.b.ins().bitcast(I64, flags, b));
        let r = self.b.ins().bor(x, y);
        self.b.ins().bitcast(F64, flags, r)
    }

    fn call(&mut self, f: Lib, args: &[Value]) -> Value {
        self.call_symbol(f.symbol(), args)
    }

    fn table(&mut self, k: u32, args: &[Value], derivs: bool) -> (Value, [Option<Value>; 2]) {
        let ptr = self.table_ptr(k);
        let mut call_args = vec![ptr];
        call_args.extend_from_slice(args);
        let name = match (args.len(), derivs) {
            (1, false) => "lsim_tab1",
            (1, true) => "lsim_tab1d",
            (_, false) => "lsim_tab2",
            (_, true) => "lsim_tab2d",
        };
        if !derivs {
            return (self.call_symbol(name, &call_args), [None, None]);
        }
        let addr = self.scratch();
        call_args.push(addr);
        let v = self.call_symbol(name, &call_args);
        let mut g = [None, None];
        for (k, gk) in g.iter_mut().enumerate().take(args.len()) {
            *gk = Some(self.b.ins().load(F64, mem(), addr, (8 * k) as i32));
        }
        (v, g)
    }

    fn table_guard(&mut self, k: u32, axis: u8, x: Value) -> Value {
        let ptr = self.table_ptr(k);
        let ax = self.b.ins().iconst(I64, axis as i64);
        self.call_symbol("lsim_tab_guard", &[ptr, ax, x])
    }
}
