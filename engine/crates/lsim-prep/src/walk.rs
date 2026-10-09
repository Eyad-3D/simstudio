//! Traversals of expressions that allocate nothing but what they build:
//! the hot paths of preparation (10⁵-equation models) use these instead of
//! `Expr::walk`/`any`/`rewrite`, which collect each node's children into a
//! vector, or clone a tree only to rebuild it.

use lsim_ir::expr::Expr;

/// Calls `f` on every node of `e`, parents first.
pub fn visit(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Neg(a) | Expr::Not(a) | Expr::NoEvent(a) => visit(a, f),
        Expr::Binary(_, a, b) | Expr::Compare(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
            visit(a, f);
            visit(b, f);
        }
        Expr::Call(_, args) | Expr::Table { args, .. } => {
            for a in args {
                visit(a, f);
            }
        }
        Expr::If(c, a, b) => {
            visit(c, f);
            visit(a, f);
            visit(b, f);
        }
        _ => {}
    }
}

/// Whether `pred` holds for any node of `e` (stops at the first).
pub fn any(e: &Expr, pred: &mut impl FnMut(&Expr) -> bool) -> bool {
    if pred(e) {
        return true;
    }
    match e {
        Expr::Neg(a) | Expr::Not(a) | Expr::NoEvent(a) => any(a, pred),
        Expr::Binary(_, a, b) | Expr::Compare(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
            any(a, pred) || any(b, pred)
        }
        Expr::Call(_, args) | Expr::Table { args, .. } => args.iter().any(|a| any(a, pred)),
        Expr::If(c, a, b) => any(c, pred) || any(a, pred) || any(b, pred),
        _ => false,
    }
}

/// A copy of `e` with nodes replaced: where `f` gives a replacement it is
/// used as it is (not entered), elsewhere the node is rebuilt from its
/// mapped children.
pub fn map(e: &Expr, f: &mut impl FnMut(&Expr) -> Option<Expr>) -> Expr {
    if let Some(r) = f(e) {
        return r;
    }
    let b = Box::new;
    match e {
        Expr::Neg(a) => Expr::Neg(b(map(a, f))),
        Expr::Not(a) => Expr::Not(b(map(a, f))),
        Expr::NoEvent(a) => Expr::NoEvent(b(map(a, f))),
        Expr::Binary(op, x, y) => Expr::Binary(*op, b(map(x, f)), b(map(y, f))),
        Expr::Compare(op, x, y) => Expr::Compare(*op, b(map(x, f)), b(map(y, f))),
        Expr::And(x, y) => Expr::And(b(map(x, f)), b(map(y, f))),
        Expr::Or(x, y) => Expr::Or(b(map(x, f)), b(map(y, f))),
        Expr::Call(g, args) => Expr::Call(*g, args.iter().map(|a| map(a, f)).collect()),
        Expr::Table { table, args } => {
            Expr::Table { table: *table, args: args.iter().map(|a| map(a, f)).collect() }
        }
        Expr::If(c, x, y) => Expr::If(b(map(c, f)), b(map(x, f)), b(map(y, f))),
        leaf => leaf.clone(),
    }
}

/// `e.clone().rewrite(f)` without the clone: the tree is rebuilt bottom-up
/// from references, letting `f` replace any node (its children already
/// rebuilt).
pub fn map_up(e: &Expr, f: &mut impl FnMut(Expr) -> Expr) -> Expr {
    let b = Box::new;
    let rebuilt = match e {
        Expr::Neg(a) => Expr::Neg(b(map_up(a, f))),
        Expr::Not(a) => Expr::Not(b(map_up(a, f))),
        Expr::NoEvent(a) => Expr::NoEvent(b(map_up(a, f))),
        Expr::Binary(op, x, y) => Expr::Binary(*op, b(map_up(x, f)), b(map_up(y, f))),
        Expr::Compare(op, x, y) => Expr::Compare(*op, b(map_up(x, f)), b(map_up(y, f))),
        Expr::And(x, y) => Expr::And(b(map_up(x, f)), b(map_up(y, f))),
        Expr::Or(x, y) => Expr::Or(b(map_up(x, f)), b(map_up(y, f))),
        Expr::Call(g, args) => Expr::Call(*g, args.iter().map(|a| map_up(a, f)).collect()),
        Expr::Table { table, args } => {
            Expr::Table { table: *table, args: args.iter().map(|a| map_up(a, f)).collect() }
        }
        Expr::If(c, x, y) => Expr::If(b(map_up(c, f)), b(map_up(x, f)), b(map_up(y, f))),
        leaf => leaf.clone(),
    };
    f(rebuilt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::VarId;
    use lsim_ir::expr::{c, name};

    #[test]
    fn traversals_agree_with_the_ir_ones() {
        let e = (name("a") + c(2.0)) * Expr::Var(VarId(3)) - name("a");
        let mut n = 0;
        visit(&e, &mut |_| n += 1);
        assert_eq!(n, e.size());
        assert!(any(&e, &mut |x| matches!(x, Expr::Var(_))));
        let m = map(&e, &mut |x| match x {
            Expr::Name(s) if s == "a" => Some(c(1.0)),
            _ => None,
        });
        assert_eq!(m.to_string(), "(1 + 2) * v[3] - 1");
    }
}

/// Calls `f` on every node of `e`, children first, letting it change the
/// node in place: mapping leaves (variables to slots) allocates nothing.
pub fn mutate(e: &mut Expr, f: &mut impl FnMut(&mut Expr)) {
    match e {
        Expr::Neg(a) | Expr::Not(a) | Expr::NoEvent(a) => mutate(a, f),
        Expr::Binary(_, a, b) | Expr::Compare(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
            mutate(a, f);
            mutate(b, f);
        }
        Expr::Call(_, args) | Expr::Table { args, .. } => {
            for a in args {
                mutate(a, f);
            }
        }
        Expr::If(c, a, b) => {
            mutate(c, f);
            mutate(a, f);
            mutate(b, f);
        }
        _ => {}
    }
    f(e);
}

/// A fast, non-cryptographic hasher for the maps of preparation (the
/// multiply-rotate hash of rustc's `FxHasher`): names and indices, never
/// input an attacker chooses to collide.
#[derive(Clone, Copy, Default)]
pub struct FxHasher {
    hash: u64,
}

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, i: u64) {
        self.hash = (self.hash.rotate_left(5) ^ i).wrapping_mul(SEED);
    }
}

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes(c.try_into().expect("eight bytes")));
        }
        for &b in chunks.remainder() {
            self.add(b as u64);
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

/// A hash map with [`FxHasher`].
pub type FxMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<FxHasher>>;
