//! Values, and the operations on them with CPython's exact semantics:
//! integer arithmetic (64 bits here: a result beyond them is outside the
//! subset, never wrapped), floats as IEEE doubles with CPython's own
//! floor division and modulo, exact comparisons between integers and
//! floats, `1 == 1.0 == True` as dict keys, and the errors Python raises
//! (division by zero, a missing key, an operation between the wrong types).
//!
//! What a call may use is metered here too: the work it does (every
//! operation whose work grows with the size of what it handles is charged
//! as it goes, so the time limit holds inside one statement), the memory
//! its values hold (every heap block a value makes counts, as the heap
//! keeps it, when it is made or grows, and is given back when it goes)
//! and the stack it nests into.

use crate::ast::FuncDef;
use crate::{Fault, Fault::*};
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::Instant;

/// A builtin function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    /// `abs`
    Abs,
    /// `min`
    Min,
    /// `max`
    Max,
    /// `float`
    Float,
    /// `int`
    Int,
    /// `bool`
    Bool,
    /// `len`
    Len,
    /// `range`
    Range,
    /// `round` (one argument)
    Round,
    /// `enumerate`
    Enumerate,
    /// `zip`
    Zip,
    /// `list`
    List,
    /// `tuple`
    Tuple,
    /// `dict`
    Dict,
    /// `any`
    Any,
    /// `all`
    All,
    /// `print` (output is not shown: it changes no result)
    Print,
    /// the scripts' `clamp(x, lo, hi)`
    Clamp,
    /// the scripts' `interp(table, x[, y])`
    Interp,
    /// a function of the math module
    Math(MathFn),
}

/// The math module's functions in the subset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathFn {
    /// `math.sqrt`
    Sqrt,
    /// `math.fabs`
    Fabs,
    /// `math.floor`
    Floor,
    /// `math.ceil`
    Ceil,
    /// `math.trunc`
    Trunc,
    /// `math.copysign`
    Copysign,
    /// `math.isnan`
    IsNan,
    /// `math.isinf`
    IsInf,
    /// `math.isfinite`
    IsFinite,
    /// `math.radians`
    Radians,
    /// `math.degrees`
    Degrees,
}

impl MathFn {
    /// Its name in the math module.
    pub fn name(self) -> &'static str {
        match self {
            MathFn::Sqrt => "sqrt",
            MathFn::Fabs => "fabs",
            MathFn::Floor => "floor",
            MathFn::Ceil => "ceil",
            MathFn::Trunc => "trunc",
            MathFn::Copysign => "copysign",
            MathFn::IsNan => "isnan",
            MathFn::IsInf => "isinf",
            MathFn::IsFinite => "isfinite",
            MathFn::Radians => "radians",
            MathFn::Degrees => "degrees",
        }
    }
}

impl Builtin {
    /// Python's name of its type: the types (`float`, `list`, …) are
    /// `type`, the scripts' `clamp` and `interp` Python functions.
    fn type_name(self) -> &'static str {
        match self {
            Builtin::Float
            | Builtin::Int
            | Builtin::Bool
            | Builtin::Range
            | Builtin::Enumerate
            | Builtin::Zip
            | Builtin::List
            | Builtin::Tuple
            | Builtin::Dict => "type",
            Builtin::Clamp | Builtin::Interp => "function",
            _ => "builtin_function_or_method",
        }
    }

    /// Its name in Python.
    pub fn name(self) -> &'static str {
        match self {
            Builtin::Abs => "abs",
            Builtin::Min => "min",
            Builtin::Max => "max",
            Builtin::Float => "float",
            Builtin::Int => "int",
            Builtin::Bool => "bool",
            Builtin::Len => "len",
            Builtin::Range => "range",
            Builtin::Round => "round",
            Builtin::Enumerate => "enumerate",
            Builtin::Zip => "zip",
            Builtin::List => "list",
            Builtin::Tuple => "tuple",
            Builtin::Dict => "dict",
            Builtin::Any => "any",
            Builtin::All => "all",
            Builtin::Print => "print",
            Builtin::Clamp => "clamp",
            Builtin::Interp => "interp",
            Builtin::Math(m) => m.name(),
        }
    }

    /// The builtin of a name, if a script may use it.
    pub fn of(name: &str) -> Option<Builtin> {
        Some(match name {
            "abs" => Builtin::Abs,
            "min" => Builtin::Min,
            "max" => Builtin::Max,
            "float" => Builtin::Float,
            "int" => Builtin::Int,
            "bool" => Builtin::Bool,
            "len" => Builtin::Len,
            "range" => Builtin::Range,
            "round" => Builtin::Round,
            "enumerate" => Builtin::Enumerate,
            "zip" => Builtin::Zip,
            "list" => Builtin::List,
            "tuple" => Builtin::Tuple,
            "dict" => Builtin::Dict,
            "any" => Builtin::Any,
            "all" => Builtin::All,
            "print" => Builtin::Print,
            "clamp" => Builtin::Clamp,
            "interp" => Builtin::Interp,
            _ => return None,
        })
    }

    /// A math module attribute: a function or a constant.
    pub fn math(name: &str) -> Option<Value> {
        Some(match name {
            "pi" => Value::Float(std::f64::consts::PI),
            "e" => Value::Float(std::f64::consts::E),
            "tau" => Value::Float(std::f64::consts::TAU),
            "inf" => Value::Float(f64::INFINITY),
            "nan" => Value::Float(f64::NAN),
            "sqrt" => Value::Builtin(Builtin::Math(MathFn::Sqrt)),
            "fabs" => Value::Builtin(Builtin::Math(MathFn::Fabs)),
            "floor" => Value::Builtin(Builtin::Math(MathFn::Floor)),
            "ceil" => Value::Builtin(Builtin::Math(MathFn::Ceil)),
            "trunc" => Value::Builtin(Builtin::Math(MathFn::Trunc)),
            "copysign" => Value::Builtin(Builtin::Math(MathFn::Copysign)),
            "isnan" => Value::Builtin(Builtin::Math(MathFn::IsNan)),
            "isinf" => Value::Builtin(Builtin::Math(MathFn::IsInf)),
            "isfinite" => Value::Builtin(Builtin::Math(MathFn::IsFinite)),
            "radians" => Value::Builtin(Builtin::Math(MathFn::Radians)),
            "degrees" => Value::Builtin(Builtin::Math(MathFn::Degrees)),
            _ => return None,
        })
    }
}

/// A function with its defaults.
pub struct Closure {
    /// the definition
    pub def: Rc<FuncDef>,
    /// the defaults of its last parameters, evaluated where it was defined
    pub defaults: Vec<Value>,
    charged: usize,
}

/// A method bound to its object (the name is the syntax tree's).
pub struct Bound(pub Value, pub Rc<str>);

/// An iterator object (what `enumerate` and `zip` give): its state, the
/// type Python gives it, and the memory it counts (its block, and what
/// `enumerate` boxes and `zip` holds of its sources).
pub struct IterCell {
    it: RefCell<crate::interp::Iter>,
    /// `enumerate` or `zip`
    pub kind: &'static str,
    charged: usize,
}

impl std::ops::Deref for IterCell {
    type Target = RefCell<crate::interp::Iter>;
    fn deref(&self) -> &Self::Target {
        &self.it
    }
}

impl Drop for IterCell {
    fn drop(&mut self) {
        memory_sub(self.charged);
        let mut out = vec![];
        iter_values(self.it.replace(crate::interp::Iter::Done), &mut out);
        drop_flat(out);
    }
}

impl Drop for Bound {
    fn drop(&mut self) {
        memory_sub(rc_block::<Bound>());
        drop_flat(vec![std::mem::replace(&mut self.0, Value::None)]);
    }
}

/// A dict: insertion-ordered entries, keys compared as Python compares
/// them (`1`, `1.0` and `True` are one key; the first key object stays).
#[derive(Default)]
pub struct Dict {
    /// (key, value) in insertion order
    pub entries: Vec<(Value, Value)>,
    /// bumped when a key is added or removed (not when a value changes):
    /// an iteration over the dict notices its keys changing
    pub keys: u64,
    /// its order is not the one Python would have (a Script's parameters,
    /// whose JSON object the importer keeps sorted): iterating it is left
    /// to Python
    pub unordered: bool,
    /// the positions of the entries by their keys' hash (`key_hash`), for
    /// the first `n` entries; rebuilt after a removal
    index: RefCell<Index>,
}

/// A dict's index: open addressing by the keys' hashes, probed linearly,
/// at most half full, its room counted towards the runner's memory.
#[derive(Default)]
struct Index {
    /// the hash of each of the first entries, kept across removals (as
    /// CPython keeps each entry's hash): a rebuild never hashes again
    hashes: Vec<u64>,
    /// how many of the entries are in `slots`
    n: usize,
    /// each an entry's position + 1, or 0 (free); a power of two long
    slots: Vec<u32>,
    charged: usize,
}

impl Index {
    /// Brings the memory the index counts up to date with its room.
    fn account(&mut self) {
        let now = heap(self.hashes.capacity() * 8) + heap(self.slots.capacity() * 4);
        let was = std::mem::replace(&mut self.charged, now);
        memory_add(now);
        memory_sub(was);
    }

    /// Puts entry `i`, whose key's hash is `h`, in its slot.
    fn place(&mut self, i: usize, h: u64) {
        let mask = self.slots.len() - 1;
        let mut at = h as usize & mask;
        while self.slots[at] != 0 {
            at = (at + 1) & mask;
        }
        self.slots[at] = i as u32 + 1;
    }
}

impl Clone for Index {
    fn clone(&self) -> Index {
        let mut ix =
            Index { hashes: self.hashes.clone(), n: self.n, slots: self.slots.clone(), charged: 0 };
        ix.account();
        ix
    }
}

impl Drop for Index {
    fn drop(&mut self) {
        memory_sub(self.charged);
    }
}

/// Below this many entries a dict is searched entry by entry.
const INDEX_FROM: usize = 16;

impl Dict {
    /// A shallow copy (`dict(d)`): the same keys and values in the same
    /// order, its index copied with it.
    fn copy(&self) -> Dict {
        Dict {
            entries: self.entries.clone(),
            keys: 0,
            unordered: self.unordered,
            index: RefCell::new(self.index.borrow().clone()),
        }
    }

    /// Removes entry `k`.
    pub fn remove(&mut self, k: usize) -> (Value, Value) {
        self.keys += 1;
        let ix = self.index.get_mut();
        if k < ix.hashes.len() {
            ix.hashes.remove(k);
        }
        ix.n = 0;
        ix.slots.clear();
        self.entries.remove(k)
    }

    /// Removes every entry (and lets go of their room, as Python does).
    pub fn clear(&mut self) {
        self.keys += 1;
        *self.index.get_mut() = Index::default();
        drop_flat(
            std::mem::take(&mut self.entries).into_iter().flat_map(|(k, v)| [k, v]).collect(),
        );
    }
}

/// A hash of a dict key that agrees with `eq`: equal keys hash alike
/// (`1`, `1.0`, `True`; `0`, `-0.0`, `False`; tuples item by item). Only
/// for finding entries: which entry matches is still decided by `eq`.
fn key_hash(k: &Value, h: &mut std::hash::DefaultHasher) -> Result<(), Fault> {
    // a tuple key shared many ways is a large walk: charged as work
    charge(1)?;
    match k {
        Value::None => 0u8.hash(h),
        Value::Bool(b) => int_hash(*b as i64, h),
        Value::Int(i) => int_hash(*i, h),
        Value::Float(x) => {
            // an integral float within 64 bits equals exactly that integer
            if x.trunc() == *x && *x >= i64::MIN as f64 && *x < -(i64::MIN as f64) {
                int_hash(*x as i64, h)
            } else {
                2u8.hash(h);
                x.to_bits().hash(h);
            }
        }
        Value::Str(s) => {
            charge(s.len() as u64)?;
            3u8.hash(h);
            s.as_str().hash(h);
        }
        Value::Tuple(t) => {
            stack_check()?;
            4u8.hash(h);
            t.len().hash(h);
            for x in t.iter() {
                key_hash(x, h)?;
            }
        }
        // no other value is a key (check_key)
        _ => 5u8.hash(h),
    }
    Ok(())
}

fn int_hash(i: i64, h: &mut std::hash::DefaultHasher) {
    1u8.hash(h);
    i.hash(h);
}

fn hash_of(k: &Value) -> Result<u64, Fault> {
    let mut h = std::hash::DefaultHasher::new();
    key_hash(k, &mut h)?;
    Ok(h.finish())
}

/// A string's text. Its block and its bytes count towards the runner's
/// memory while it lives.
pub struct Text(Box<str>);

impl Text {
    fn new(s: Box<str>) -> Text {
        memory_add(Text::bytes(s.len()));
        Text(s)
    }

    /// What a string of `n` bytes holds: its block and its text's.
    fn bytes(n: usize) -> usize {
        rc_block::<Text>() + heap(n)
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for Text {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl Drop for Text {
    fn drop(&mut self) {
        memory_sub(Text::bytes(self.0.len()));
    }
}

/// A list's items. Dropped without recursion (a list nested a million
/// deep must not overflow the stack when it goes). Its room counts
/// towards the runner's memory: [`ListCell::account`] after it grows.
pub struct ListCell {
    items: RefCell<Vec<Value>>,
    charged: Cell<usize>,
}

/// A tuple's items (dropped without recursion).
pub struct TupleCell {
    items: Vec<Value>,
    charged: usize,
}

/// A dict (dropped without recursion); [`DictCell::account`] after it
/// grows.
pub struct DictCell {
    dict: RefCell<Dict>,
    charged: Cell<usize>,
}

/// Charges or gives back the difference between what a container has
/// charged and its size `now`.
fn recharge(charged: &Cell<usize>, now: usize) {
    let was = charged.replace(now);
    if now >= was {
        memory_add(now - was);
    } else {
        memory_sub(was - now);
    }
}

impl ListCell {
    fn new(items: Vec<Value>) -> ListCell {
        let cell = ListCell { items: RefCell::new(items), charged: Cell::new(0) };
        cell.account();
        cell
    }

    /// Brings the memory the list counts up to date with its room (after
    /// it grew or shrank; not while it is borrowed mutably).
    pub fn account(&self) {
        recharge(
            &self.charged,
            rc_block::<ListCell>() + heap(self.items.borrow().capacity() * VALUE),
        );
    }
}

impl DictCell {
    fn new(dict: Dict) -> DictCell {
        let cell = DictCell { dict: RefCell::new(dict), charged: Cell::new(0) };
        cell.account();
        cell
    }

    /// Brings the memory the dict counts up to date with its room (its
    /// index counts its own).
    pub fn account(&self) {
        recharge(
            &self.charged,
            rc_block::<DictCell>() + heap(self.dict.borrow().entries.capacity() * ENTRY),
        );
    }
}

impl std::ops::Deref for ListCell {
    type Target = RefCell<Vec<Value>>;
    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl std::ops::Deref for TupleCell {
    type Target = Vec<Value>;
    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl std::ops::Deref for DictCell {
    type Target = RefCell<Dict>;
    fn deref(&self) -> &Self::Target {
        &self.dict
    }
}

/// Drops `stack`'s values, and every container only they hold, one at a
/// time instead of recursively.
fn drop_flat(mut stack: Vec<Value>) {
    while let Some(v) = stack.pop() {
        match v {
            Value::List(rc) => {
                if let Ok(cell) = Rc::try_unwrap(rc) {
                    stack.append(&mut cell.items.take());
                }
            }
            Value::Tuple(rc) => {
                if let Ok(mut cell) = Rc::try_unwrap(rc) {
                    stack.append(&mut cell.items);
                }
            }
            Value::Dict(rc) | Value::View(rc, _) => {
                if let Ok(cell) = Rc::try_unwrap(rc) {
                    for (k, v) in std::mem::take(&mut cell.dict.borrow_mut().entries) {
                        stack.push(k);
                        stack.push(v);
                    }
                }
            }
            Value::Method(rc) => {
                if let Ok(mut m) = Rc::try_unwrap(rc) {
                    stack.push(std::mem::replace(&mut m.0, Value::None));
                }
            }
            // iterators and functions hold values too: a chain through
            // them (`x = [enumerate(x)]`, `def g(a=x)`) must not recurse
            Value::Iter(rc) => {
                if let Ok(cell) = Rc::try_unwrap(rc) {
                    iter_values(cell.it.replace(crate::interp::Iter::Done), &mut stack);
                }
            }
            Value::Func(rc) => {
                if let Ok(mut c) = Rc::try_unwrap(rc) {
                    stack.append(&mut c.defaults);
                }
            }
            _ => {}
        }
    }
}

/// Moves what `it` reads from onto `out` as values, without recursion.
fn iter_values(it: crate::interp::Iter, out: &mut Vec<Value>) {
    use crate::interp::Iter;
    let mut its = vec![it];
    while let Some(it) = its.pop() {
        match it {
            Iter::List(l, _) => out.push(Value::List(l)),
            Iter::Tuple(t, _) => out.push(Value::Tuple(t)),
            Iter::Dict(d, ..) => out.push(Value::Dict(d)),
            Iter::Shared(rc) => out.push(Value::Iter(rc)),
            Iter::Enumerate(inner, _) => its.push(*inner),
            Iter::Zip(inner) => its.extend(inner),
            Iter::Range(..) | Iter::Chars(..) | Iter::Done => {}
        }
    }
}

impl Drop for ListCell {
    fn drop(&mut self) {
        memory_sub(self.charged.get());
        drop_flat(self.items.take());
    }
}

/// A function's defaults are dropped flat too: a chain of functions each
/// the default of the next (`def g(a=g)` in a loop) must not recurse.
impl Drop for Closure {
    fn drop(&mut self) {
        memory_sub(self.charged);
        drop_flat(std::mem::take(&mut self.defaults));
    }
}

impl Drop for TupleCell {
    fn drop(&mut self) {
        memory_sub(self.charged);
        drop_flat(std::mem::take(&mut self.items));
    }
}

impl Drop for DictCell {
    fn drop(&mut self) {
        memory_sub(self.charged.get());
        let entries = std::mem::take(&mut self.dict.get_mut().entries);
        drop_flat(entries.into_iter().flat_map(|(k, v)| [k, v]).collect());
    }
}

/// A value.
#[derive(Clone)]
pub enum Value {
    /// `None`
    None,
    /// `True`, `False`
    Bool(bool),
    /// an integer (within 64 bits)
    Int(i64),
    /// a float
    Float(f64),
    /// a string
    Str(Rc<Text>),
    /// a tuple
    Tuple(Rc<TupleCell>),
    /// a list
    List(Rc<ListCell>),
    /// a dict
    Dict(Rc<DictCell>),
    /// `range(start, stop, step)`
    Range(i64, i64, i64),
    /// a script's function
    Func(Rc<Closure>),
    /// a builtin function
    Builtin(Builtin),
    /// a method bound to its object
    Method(Rc<Bound>),
    /// the math module
    Math,
    /// a dict's keys (0), values (1) or items (2), read live
    View(Rc<DictCell>, u8),
    /// an iterator object (what `enumerate` and `zip` give), shared by
    /// everything that iterates it
    Iter(Rc<IterCell>),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.repr() {
            Ok(r) => f.write_str(&r),
            Err(_) => write!(f, "<{}>", self.type_name()),
        }
    }
}

/// 2^63 as a float: the first magnitude beyond every 64-bit integer.
const TWO_63: f64 = 9_223_372_036_854_775_808u64 as f64;

/// 2^53: integers up to this magnitude convert to floats exactly.
const EXACT: u64 = 1 << 53;

/// How deeply the recursive operations (comparisons, searches, keys) go
/// into nested containers before leaving the work to Python.
const MAX_NESTING: usize = 100;

/// How many items one recursive operation may visit before leaving the
/// work to Python.
const MAX_VISITS: usize = 1_000_000;

/// How much work passes between two readings of the clock (and of the
/// memory the values hold).
const CLOCK_WORK: u64 = 4096;

/// The most memory a runner's values may hold, bytes: what the app's
/// Script worker allows its whole process (sandbox_worker.py's
/// `DEFAULT_MEM_BYTES`). Beyond it the script is left to Python, whose
/// own limit then decides.
pub const MAX_LIVE_BYTES: usize = 512 * 1024 * 1024;

/// How much of the stack one call into the interpreter may use below the
/// point where it started, bytes: about half of the smallest caller
/// thread it is made for (1 MiB), the rest left to the caller's frames
/// and to the leaf calls below the last check. Measured where the
/// interpreter nests (expressions, blocks, calls, the parser, the walks
/// through nested data), so a script nested too deeply for it is left to
/// Python whatever the build's frames are, instead of overflowing.
pub const STACK_BUDGET: usize = 512 * 1024;

/// The bytes the heap takes for a block of `n`: what is asked with a
/// header, rounded up to 16 bytes (as the Windows heap and glibc's
/// malloc keep a block, or a little more), nothing for nothing.
pub const fn heap(n: usize) -> usize {
    if n == 0 { 0 } else { (n + 16 + 15) & !15 }
}

/// The block of an `Rc<T>`: its two counts and the value.
pub const fn rc_block<T>() -> usize {
    heap(2 * std::mem::size_of::<usize>() + std::mem::size_of::<T>())
}

/// Bytes of an item of a list or tuple.
pub const VALUE: usize = std::mem::size_of::<Value>();

/// Bytes of a dict's entry: its key and its value.
const ENTRY: usize = std::mem::size_of::<(Value, Value)>();

thread_local! {
    /// the work done in the current call, and the call's deadline
    static WORK: Cell<u64> = const { Cell::new(0) };
    static DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
    /// the items of ranges the current call has had CPython compare with
    /// a value that is no integer (see [`contains`])
    static SCANNED: Cell<u64> = const { Cell::new(0) };
    /// the bytes held by the values of the runner being called
    static LIVE: Cell<usize> = const { Cell::new(0) };
    /// where on the stack the current call into the interpreter started
    static STACK_BASE: Cell<usize> = const { Cell::new(0) };
}

/// Starts the clock of a call into the script: work charged after
/// `deadline` ends the call with [`Fault::Limit`] (None: no limit). The
/// stack the call may use is measured from here.
pub fn start_meter(deadline: Option<Instant>) {
    WORK.with(|w| w.set(0));
    SCANNED.with(|s| s.set(0));
    DEADLINE.with(|d| d.set(deadline));
    STACK_BASE.with(|b| b.set(stack_here()));
}

/// Charges `n` units of work (an item visited, a key compared or copied, a
/// byte of text), reading the clock and the memory every [`CLOCK_WORK`]
/// units: one operation over a large container stops at the time limit
/// too, not only between statements.
pub fn charge(n: u64) -> Result<(), Fault> {
    let before = WORK.with(|w| {
        let b = w.get();
        w.set(b.saturating_add(n));
        b
    });
    if before.saturating_add(n) / CLOCK_WORK != before / CLOCK_WORK {
        if let Some(d) = DEADLINE.with(|d| d.get())
            && Instant::now() > d
        {
            return Err(Limit);
        }
        memory_check()?;
    }
    Ok(())
}

/// Counts `n` more bytes held by the runner's values.
pub fn memory_add(n: usize) {
    LIVE.with(|l| l.set(l.get().saturating_add(n)));
}

/// Gives back `n` bytes the runner's values held.
pub fn memory_sub(n: usize) {
    LIVE.with(|l| l.set(l.get().saturating_sub(n)));
}

/// Makes `live` the bytes counted while a runner is called, until
/// [`memory_leave`]; returns what was counted before (another runner's).
pub fn memory_enter(live: usize) -> usize {
    LIVE.with(|l| l.replace(live))
}

/// Ends [`memory_enter`]: puts `outer` back and returns the bytes the
/// runner's values hold now.
pub fn memory_leave(outer: usize) -> usize {
    LIVE.with(|l| l.replace(outer))
}

/// Bytes counted towards the runner's memory while it lives (a call's
/// frame while the call runs).
pub struct Held(usize);

impl Held {
    /// Counts `n` bytes until it is dropped.
    pub fn new(n: usize) -> Held {
        memory_add(n);
        Held(n)
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        memory_sub(self.0);
    }
}

/// Whether the runner's values hold no more than [`MAX_LIVE_BYTES`].
pub fn memory_check() -> Result<(), Fault> {
    fits(0)
}

/// Whether `more` bytes would still fit within [`MAX_LIVE_BYTES`] with what
/// the runner's values hold now (asked before a large block is made, so
/// that the cap holds as it is reached, not one container later).
pub fn fits(more: usize) -> Result<(), Fault> {
    if LIVE.with(Cell::get).saturating_add(more) > MAX_LIVE_BYTES {
        return Err(over_the_cap());
    }
    Ok(())
}

/// Whether `v` may take `more` items: the room it would grow to (as a
/// vector grows, doubling or to what it needs) [`fits`].
pub fn room<T>(v: &Vec<T>, more: usize) -> Result<(), Fault> {
    let need = v.len().saturating_add(more);
    if need <= v.capacity() {
        return Ok(());
    }
    let cap = need.max(2 * v.capacity()).max(4);
    fits(heap(cap.saturating_mul(std::mem::size_of::<T>())))
}

fn over_the_cap() -> Fault {
    Outside(format!(
        "values holding more than {} MB (what the app's Script worker allows)",
        MAX_LIVE_BYTES >> 20
    ))
}

/// Where on the stack this is (the address of a local).
#[inline(always)]
pub fn stack_here() -> usize {
    let probe = 0u8;
    std::hint::black_box(&probe) as *const u8 as usize
}

/// Whether the current call into the interpreter has used no more than
/// [`STACK_BUDGET`] of the stack (outside a call nothing is checked).
pub fn stack_check() -> Result<(), Fault> {
    let base = STACK_BASE.with(Cell::get);
    if base != 0 && base.abs_diff(stack_here()) > STACK_BUDGET {
        return Err(Outside("code or data nested this deeply on the native runner's stack".into()));
    }
    Ok(())
}

/// The budget of one recursive operation.
pub struct Walk {
    left: usize,
}

impl Default for Walk {
    fn default() -> Self {
        Walk { left: MAX_VISITS }
    }
}

impl Walk {
    fn step(&mut self, depth: usize) -> Result<(), Fault> {
        if depth > MAX_NESTING {
            return Err(Outside("containers nested this deeply".into()));
        }
        if depth > 0 {
            stack_check()?;
        }
        if self.left == 0 {
            return Err(Outside("comparing or searching containers this large".into()));
        }
        self.left -= 1;
        charge(1)
    }
}

impl Value {
    /// A new list.
    pub fn list(v: Vec<Value>) -> Value {
        Value::List(Rc::new(ListCell::new(v)))
    }

    /// A new tuple.
    pub fn tuple(v: Vec<Value>) -> Value {
        let charged = rc_block::<TupleCell>() + heap(v.capacity() * VALUE);
        memory_add(charged);
        Value::Tuple(Rc::new(TupleCell { items: v, charged }))
    }

    /// A new, empty dict.
    pub fn new_dict() -> Rc<DictCell> {
        Rc::new(DictCell::new(Dict::default()))
    }

    /// `dict(d)`: a shallow copy, in `d`'s order.
    pub fn dict_copy(d: &DictCell) -> Result<Value, Fault> {
        let dm = d.borrow();
        charge(dm.entries.len() as u64)?;
        fits(heap(dm.entries.len() * ENTRY) + dm.index.borrow().charged)?;
        Ok(Value::Dict(Rc::new(DictCell::new(dm.copy()))))
    }

    /// A new dict (keys checked; values may be anything).
    pub fn dict(entries: Vec<(Value, Value)>) -> Result<Value, Fault> {
        let d = Value::new_dict();
        for (k, v) in entries {
            dict_insert(&d, k, v)?;
        }
        Ok(Value::Dict(d))
    }

    /// A function with its defaults: its block and its defaults count
    /// towards the runner's memory.
    pub fn func(def: Rc<FuncDef>, defaults: Vec<Value>) -> Value {
        let charged = rc_block::<Closure>() + heap(defaults.capacity() * VALUE);
        memory_add(charged);
        Value::Func(Rc::new(Closure { def, defaults, charged }))
    }

    /// `obj.name`, a method bound to its object.
    pub fn method(obj: &Value, name: &Rc<str>) -> Value {
        memory_add(rc_block::<Bound>());
        Value::Method(Rc::new(Bound(obj.clone(), name.clone())))
    }

    /// An iterator object of `kind` (`enumerate`, `zip`) over `it`.
    pub fn iterator(it: crate::interp::Iter, kind: &'static str) -> Value {
        let charged = rc_block::<IterCell>() + crate::interp::iter_heap(&it);
        memory_add(charged);
        Value::Iter(Rc::new(IterCell { it: RefCell::new(it), kind, charged }))
    }

    /// A string.
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::new(Text::new(s.into())))
    }

    /// A string made from `s`.
    pub fn string(s: String) -> Value {
        Value::Str(Rc::new(Text::new(s.into_boxed_str())))
    }

    /// Python's type name.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::None => "NoneType",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::Tuple(_) => "tuple",
            Value::List(_) => "list",
            Value::Dict(_) => "dict",
            Value::Range(..) => "range",
            Value::Func(_) => "function",
            Value::Builtin(b) => b.type_name(),
            Value::Method(_) => "builtin_function_or_method",
            Value::Math => "module",
            Value::View(_, 0) => "dict_keys",
            Value::View(_, 1) => "dict_values",
            Value::View(..) => "dict_items",
            Value::Iter(it) => it.kind,
        }
    }

    /// Python's `repr()`, for messages, whole: what Python would show
    /// otherwise (an object's address, the project's order of the
    /// parameters, a character whose printing is not known here) or that
    /// is nested too deeply or too long to show is left to Python.
    pub fn repr(&self) -> Result<String, Fault> {
        let mut out = String::new();
        repr_into(self, &mut out, 0)?;
        Ok(out)
    }

    /// Python's `str()`, for messages: a string's own text, else its repr.
    pub fn text(&self) -> Result<String, Fault> {
        match self {
            Value::Str(s) => Ok(s.as_str().to_string()),
            _ => self.repr(),
        }
    }

    /// Python's truth value.
    pub fn truthy(&self) -> bool {
        match self {
            Value::None => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            // NaN is true, as in Python
            Value::Float(x) => *x != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Tuple(t) => !t.is_empty(),
            Value::List(l) => !l.borrow().is_empty(),
            Value::Dict(d) | Value::View(d, _) => !d.borrow().entries.is_empty(),
            Value::Range(a, b, s) => range_len(*a, *b, *s) > 0,
            _ => true,
        }
    }

    /// The value as an integer, if it is one (bools are).
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Bool(b) => Some(*b as i64),
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The value as a float, if it is a number (Python's int-to-float
    /// conversion: correctly rounded, ties to even, as Rust's `as`).
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Bool(b) => Some(*b as i64 as f64),
            Value::Int(i) => Some(*i as f64),
            Value::Float(x) => Some(*x),
            _ => None,
        }
    }

    fn is_number(&self) -> bool {
        matches!(self, Value::Bool(_) | Value::Int(_) | Value::Float(_))
    }
}

/// The longest quotation of a value a message gives here.
const REPR_MAX: usize = 100_000;

/// Whether a dict's order is the one Python would have, when it matters
/// (two entries or more).
fn ordered(d: &DictCell) -> Result<(), Fault> {
    let dm = d.borrow();
    if dm.unordered && dm.entries.len() > 1 {
        return Err(Outside("quoting the parameters (Python keeps the project's order)".into()));
    }
    Ok(())
}

/// Items, each quoted, separated by commas.
fn items_into<'a>(
    items: impl Iterator<Item = &'a Value>,
    out: &mut String,
    depth: usize,
) -> Result<(), Fault> {
    for (k, x) in items.enumerate() {
        if k > 0 {
            out.push_str(", ");
        }
        repr_into(x, out, depth + 1)?;
    }
    Ok(())
}

fn repr_into(v: &Value, out: &mut String, depth: usize) -> Result<(), Fault> {
    if depth > MAX_NESTING {
        return Err(Outside("quoting containers nested this deeply".into()));
    }
    if out.len() > REPR_MAX {
        return Err(Outside("quoting a value this long".into()));
    }
    if depth > 0 {
        stack_check()?;
    }
    charge(1)?;
    match v {
        Value::None => out.push_str("None"),
        Value::Bool(b) => out.push_str(if *b { "True" } else { "False" }),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Float(x) => out.push_str(&float_repr(*x)),
        Value::Str(s) => {
            charge(s.len() as u64)?;
            str_repr(s, out)?
        }
        Value::Tuple(t) => {
            out.push('(');
            items_into(t.iter(), out, depth)?;
            if t.len() == 1 {
                out.push(',');
            }
            out.push(')');
        }
        Value::List(l) => {
            out.push('[');
            items_into(l.borrow().iter(), out, depth)?;
            out.push(']');
        }
        Value::Dict(d) => {
            ordered(d)?;
            out.push('{');
            for (k, (key, x)) in d.borrow().entries.iter().enumerate() {
                if k > 0 {
                    out.push_str(", ");
                }
                repr_into(key, out, depth + 1)?;
                out.push_str(": ");
                repr_into(x, out, depth + 1)?;
            }
            out.push('}');
        }
        Value::View(d, kind) => {
            ordered(d)?;
            let dm = d.borrow();
            match kind {
                0 => {
                    out.push_str("dict_keys([");
                    items_into(dm.entries.iter().map(|(k, _)| k), out, depth)?;
                }
                1 => {
                    out.push_str("dict_values([");
                    items_into(dm.entries.iter().map(|(_, x)| x), out, depth)?;
                }
                _ => {
                    out.push_str("dict_items([");
                    for (k, (key, x)) in dm.entries.iter().enumerate() {
                        out.push_str(if k > 0 { ", (" } else { "(" });
                        repr_into(key, out, depth + 1)?;
                        out.push_str(", ");
                        repr_into(x, out, depth + 1)?;
                        out.push(')');
                    }
                }
            }
            out.push_str("])");
        }
        Value::Range(a, b, 1) => out.push_str(&format!("range({a}, {b})")),
        Value::Range(a, b, s) => out.push_str(&format!("range({a}, {b}, {s})")),
        Value::Builtin(b @ (Builtin::Clamp | Builtin::Interp)) => {
            return Err(Outside(format!("quoting {} (a Python function: its address)", b.name())));
        }
        Value::Builtin(b) if b.type_name() == "type" => {
            out.push_str(&format!("<class '{}'>", b.name()))
        }
        Value::Builtin(b) => out.push_str(&format!("<built-in function {}>", b.name())),
        Value::Math => out.push_str("<module 'math' (built-in)>"),
        Value::Func(_) | Value::Method(_) | Value::Iter(_) => {
            let what = match v {
                Value::Func(_) => "a function",
                Value::Method(_) => "a bound method",
                _ => "an iterator object",
            };
            return Err(Outside(format!("quoting {what} (Python gives its address)")));
        }
    }
    Ok(())
}

/// Python's `repr()` of a float: the shortest digits that read back as
/// `x`, written with an exponent below 1e-4 and from 1e16 on.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    // Rust's shortest digits are Python's (both the shortest that read
    // back, the nearer of two)
    let sci = format!("{:e}", x.abs());
    let (mant, exp) = sci.split_once('e').expect("an exponent");
    let exp: i32 = exp.parse().expect("an integer exponent");
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let (n, point) = (digits.len() as i32, exp + 1);
    let body = if !(-4 < point && point <= 16) {
        let rest = &digits[1..];
        let dot = if rest.is_empty() { "" } else { "." };
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}{dot}{rest}e{sign}{:02}", &digits[..1], exp.abs())
    } else if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point >= n {
        format!("{digits}{}.0", "0".repeat((point - n) as usize))
    } else {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    };
    format!("{}{body}", if x < 0.0 { "-" } else { "" })
}

/// Python's `repr()` of a string: in single quotes unless it holds one and
/// no double quote, with backslash escapes for the quote, the backslash
/// and the characters Python does not print. Beyond ASCII, Python's
/// printing (Unicode 14's) is known here for the common scripts and
/// symbols ([`PRINTED`]); a string with other characters is left to
/// Python.
fn str_repr(s: &str, out: &mut String) -> Result<(), Fault> {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            c if c.is_ascii() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => match printed(c) {
                Some(true) => out.push(c),
                Some(false) if (c as u32) < 0x100 => out.push_str(&format!("\\x{:02x}", c as u32)),
                Some(false) => out.push_str(&format!("\\u{:04x}", c as u32)),
                None => {
                    return Err(Outside(format!(
                        "quoting the character U+{:04X} (whether Python prints it is not known here)",
                        c as u32
                    )));
                }
            },
        }
    }
    out.push(quote);
    Ok(())
}

/// The blocks beyond ASCII whose printing is known here: the C1 controls,
/// Latin, Greek, Cyrillic, Armenian and Hebrew; punctuation and the
/// symbols from U+2000; CJK punctuation and the kana; the CJK
/// ideographs; the full-width forms.
const KNOWN: [(u32, u32); 5] =
    [(0x80, 0x5ff), (0x2000, 0x2bff), (0x3000, 0x30ff), (0x4e00, 0x9fff), (0xff00, 0xffef)];

/// The characters of [`KNOWN`] that CPython 3.11 prints in a repr
/// (`str.isprintable()`, Unicode 14.0); the others it escapes.
const PRINTED: [(u32, u32); 37] = [
    (0xa1, 0xac),
    (0xae, 0x377),
    (0x37a, 0x37f),
    (0x384, 0x38a),
    (0x38c, 0x38c),
    (0x38e, 0x3a1),
    (0x3a3, 0x52f),
    (0x531, 0x556),
    (0x559, 0x58a),
    (0x58d, 0x58f),
    (0x591, 0x5c7),
    (0x5d0, 0x5ea),
    (0x5ef, 0x5f4),
    (0x2010, 0x2027),
    (0x2030, 0x205e),
    (0x2070, 0x2071),
    (0x2074, 0x208e),
    (0x2090, 0x209c),
    (0x20a0, 0x20c0),
    (0x20d0, 0x20f0),
    (0x2100, 0x218b),
    (0x2190, 0x2426),
    (0x2440, 0x244a),
    (0x2460, 0x2b73),
    (0x2b76, 0x2b95),
    (0x2b97, 0x2bff),
    (0x3001, 0x303f),
    (0x3041, 0x3096),
    (0x3099, 0x30ff),
    (0x4e00, 0x9fff),
    (0xff01, 0xffbe),
    (0xffc2, 0xffc7),
    (0xffca, 0xffcf),
    (0xffd2, 0xffd7),
    (0xffda, 0xffdc),
    (0xffe0, 0xffe6),
    (0xffe8, 0xffee),
];

/// Whether Python prints `c` (beyond ASCII) as it is in a repr; None
/// outside the blocks known here.
fn printed(c: char) -> Option<bool> {
    let c = c as u32;
    if !KNOWN.iter().any(|(lo, hi)| (*lo..=*hi).contains(&c)) {
        return None;
    }
    Some(PRINTED.iter().any(|(lo, hi)| (*lo..=*hi).contains(&c)))
}

/// The number of items of `range(a, b, s)`, exactly.
pub fn range_len(a: i64, b: i64, s: i64) -> i128 {
    let (a, b, s) = (a as i128, b as i128, s as i128);
    if s > 0 && a < b {
        (b - a - 1) / s + 1
    } else if s < 0 && a > b {
        (a - b - 1) / (-s) + 1
    } else {
        0
    }
}

fn type_error(what: String) -> Fault {
    Raise(what)
}

fn overflow() -> Fault {
    Outside("an integer beyond 64 bits".into())
}

/// An integer and a float compared exactly (as Python does).
fn cmp_int_float(i: i64, x: f64) -> Option<Ordering> {
    if x.is_nan() {
        return None;
    }
    if x.is_infinite() {
        return Some(if x > 0.0 { Ordering::Less } else { Ordering::Greater });
    }
    if i.unsigned_abs() <= EXACT {
        return (i as f64).partial_cmp(&x);
    }
    // |i| > 2^53: compare with x's integral part exactly
    let xf = x.trunc();
    if xf >= TWO_63 {
        return Some(Ordering::Less);
    }
    if xf < -TWO_63 {
        return Some(Ordering::Greater);
    }
    // -2^63 <= xf < 2^63: an exact 64-bit integer
    let xi = xf as i64;
    match i.cmp(&xi) {
        Ordering::Equal => {
            let frac = x - xf;
            Some(if frac > 0.0 {
                Ordering::Less
            } else if frac < 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            })
        }
        o => Some(o),
    }
}

/// Two numbers compared (None when a NaN is involved).
fn cmp_num(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Float(x), _) => {
            cmp_int_float(b.as_int().expect("a number"), *x).map(Ordering::reverse)
        }
        (_, Value::Float(y)) => cmp_int_float(a.as_int().expect("a number"), *y),
        _ => Some(a.as_int().expect("a number").cmp(&b.as_int().expect("a number"))),
    }
}

/// A container's identity (for the visited sets).
fn ptr(v: &Value) -> Option<*const ()> {
    match v {
        Value::Tuple(t) => Some(Rc::as_ptr(t) as *const ()),
        Value::List(l) => Some(Rc::as_ptr(l) as *const ()),
        Value::Dict(d) | Value::View(d, _) => Some(Rc::as_ptr(d) as *const ()),
        Value::Iter(i) => Some(Rc::as_ptr(i) as *const ()),
        Value::Method(m) => Some(Rc::as_ptr(m) as *const ()),
        Value::Func(f) => Some(Rc::as_ptr(f) as *const ()),
        _ => None,
    }
}

/// Every container `v` holds directly (see [`each_child`]), each item
/// looked at charged as work.
fn containers_in(v: &Value, out: &mut Vec<Value>) -> Result<(), Fault> {
    let mut n = 0u64;
    each_child(v, &mut |x| {
        n += 1;
        if ptr(x).is_some() {
            out.push(x.clone());
        }
    });
    charge(n)
}

/// Calls `f` on every value `v` holds directly (a container's items, a
/// dict's keys and values, a method's object, an iterator's sources, a
/// function's defaults), without copying them all first.
fn each_child(v: &Value, f: &mut dyn FnMut(&Value)) {
    match v {
        Value::Tuple(t) => t.iter().for_each(f),
        Value::List(l) => l.borrow().iter().for_each(f),
        Value::Dict(d) | Value::View(d, _) => {
            for (k, x) in &d.borrow().entries {
                f(k);
                f(x);
            }
        }
        Value::Method(m) => f(&m.0),
        Value::Iter(i) => {
            let mut sources = vec![];
            i.borrow().sources(&mut sources);
            sources.iter().for_each(f);
        }
        Value::Func(c) => c.defaults.iter().for_each(f),
        _ => {}
    }
}

/// Whether `x` is a NaN; a container is put on `stack` to be looked into.
fn nan_or_push(x: &Value, stack: &mut Vec<Value>, walk: &mut Walk) -> Result<bool, Fault> {
    walk.step(0)?;
    Ok(match x {
        Value::Float(f) => f.is_nan(),
        Value::Tuple(_) | Value::List(_) | Value::Dict(_) => {
            stack.push(x.clone());
            false
        }
        _ => false,
    })
}

/// Whether one of `items` is a NaN (the containers among them put on
/// `stack`).
fn any_nan<'a>(
    items: impl Iterator<Item = &'a Value>,
    stack: &mut Vec<Value>,
    walk: &mut Walk,
) -> Result<bool, Fault> {
    for y in items {
        if nan_or_push(y, stack, walk)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether `v` is or holds a NaN. Python finds an item of a container by
/// identity before equality (`x is y or x == y`), so the very same NaN is
/// found while another NaN equals nothing: values here have no identity,
/// so such a search is left to Python. Each container is looked into once
/// (a dict's values: no key holds a NaN).
pub fn has_nan(v: &Value) -> Result<bool, Fault> {
    let mut seen = std::collections::HashSet::new();
    let mut walk = Walk::default();
    let mut stack = vec![];
    if nan_or_push(v, &mut stack, &mut walk)? {
        return Ok(true);
    }
    while let Some(x) = stack.pop() {
        if !seen.insert(ptr(&x)) {
            continue;
        }
        let found = match &x {
            Value::Dict(d) => {
                any_nan(d.borrow().entries.iter().map(|(_, y)| y), &mut stack, &mut walk)?
            }
            Value::List(l) => any_nan(l.borrow().iter(), &mut stack, &mut walk)?,
            Value::Tuple(t) => any_nan(t.iter(), &mut stack, &mut walk)?,
            _ => false,
        };
        if found {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Checks that a search for `x` among a container's items is decided by
/// equality alone (see [`has_nan`]).
pub fn searchable(x: &Value) -> Result<(), Fault> {
    if has_nan(x)? {
        return Err(Outside(
            "looking for a NaN in a container (Python matches it by identity)".into(),
        ));
    }
    Ok(())
}

/// Checks that putting `item` into `container` makes no cycle (a container
/// that holds itself; Python allows it, its comparisons and printing then
/// recurse until they fail): left to Python.
pub fn insertable(container: &Value, item: &Value) -> Result<(), Fault> {
    insertable_all(container, std::slice::from_ref(item))
}

/// [`insertable`] for each of `items`, each container among them looked
/// into once (`l.extend(items)`, `l += items`).
pub fn insertable_all(container: &Value, items: &[Value]) -> Result<(), Fault> {
    let Some(target) = ptr(container) else { return Ok(()) };
    let mut stack: Vec<Value> = items.iter().filter(|x| ptr(x).is_some()).cloned().collect();
    if stack.is_empty() {
        return Ok(());
    }
    let mut seen = std::collections::HashSet::new();
    let mut walk = Walk::default();
    while let Some(x) = stack.pop() {
        walk.step(0)?;
        let Some(p) = ptr(&x) else { continue };
        if p == target {
            return Err(Outside("a container that holds itself".into()));
        }
        if seen.insert(p) {
            containers_in(&x, &mut stack)?;
        }
    }
    Ok(())
}

/// `a == b`.
pub fn eq(a: &Value, b: &Value) -> Result<bool, Fault> {
    eq_w(a, b, &mut Walk::default(), 0)
}

fn eq_w(a: &Value, b: &Value, w: &mut Walk, depth: usize) -> Result<bool, Fault> {
    w.step(depth)?;
    Ok(match (a, b) {
        _ if a.is_number() && b.is_number() => cmp_num(a, b) == Some(Ordering::Equal),
        (Value::None, Value::None) => true,
        (Value::Str(x), Value::Str(y)) => {
            charge(x.len().min(y.len()) as u64)?;
            x.as_str() == y.as_str()
        }
        (Value::Tuple(x), Value::Tuple(y)) => {
            if Rc::ptr_eq(x, y) {
                return Ok(true);
            }
            if x.len() != y.len() {
                return Ok(false);
            }
            for (p, q) in x.iter().zip(y.iter()) {
                if !eq_w(p, q, w, depth + 1)? {
                    return Ok(false);
                }
            }
            true
        }
        (Value::List(x), Value::List(y)) => {
            if Rc::ptr_eq(x, y) {
                return Ok(true);
            }
            // nothing a comparison does changes a list
            let (x, y) = (x.borrow(), y.borrow());
            if x.len() != y.len() {
                return Ok(false);
            }
            for (p, q) in x.iter().zip(y.iter()) {
                if !eq_w(p, q, w, depth + 1)? {
                    return Ok(false);
                }
            }
            true
        }
        (Value::Dict(x), Value::Dict(y)) => {
            if Rc::ptr_eq(x, y) {
                return Ok(true);
            }
            let (dx, dy) = (x.borrow(), y.borrow());
            if dx.entries.len() != dy.entries.len() {
                return Ok(false);
            }
            for (k, v) in &dx.entries {
                let mut found = None;
                for (k2, v2) in &dy.entries {
                    if eq_w(k, k2, w, depth + 1)? {
                        found = Some(v2);
                        break;
                    }
                }
                match found {
                    Some(v2) if eq_w(v, v2, w, depth + 1)? => {}
                    _ => return Ok(false),
                }
            }
            true
        }
        (Value::Range(a1, b1, s1), Value::Range(a2, b2, s2)) => {
            let (n1, n2) = (range_len(*a1, *b1, *s1), range_len(*a2, *b2, *s2));
            n1 == n2 && (n1 == 0 || (a1 == a2 && (n1 == 1 || s1 == s2)))
        }
        (Value::Builtin(x), Value::Builtin(y)) => x == y,
        (Value::Func(x), Value::Func(y)) => Rc::ptr_eq(x, y),
        (Value::Iter(x), Value::Iter(y)) => Rc::ptr_eq(x, y),
        (Value::Math, Value::Math) => true,
        (Value::Method(x), Value::Method(y)) => {
            // bound methods are equal when bound to the same object
            let same = match (&x.0, &y.0) {
                (Value::Dict(p), Value::Dict(q)) => Rc::ptr_eq(p, q),
                (Value::List(p), Value::List(q)) => Rc::ptr_eq(p, q),
                _ => return Err(Outside("comparing methods of tuples".into())),
            };
            same && x.1 == y.1
        }
        (Value::View(..), _) | (_, Value::View(..)) => {
            return Err(Outside("comparing a dict view".into()));
        }
        _ => false,
    })
}

/// `a is b`, for the values where Python's identity is predictable
/// (`None`, `True`, `False`, and the same container).
pub fn is(a: &Value, b: &Value) -> Result<bool, Fault> {
    Ok(match (a, b) {
        (Value::None, Value::None) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::None | Value::Bool(_), _) | (_, Value::None | Value::Bool(_)) => false,
        (Value::List(x), Value::List(y)) => Rc::ptr_eq(x, y),
        (Value::Dict(x), Value::Dict(y)) => Rc::ptr_eq(x, y),
        (Value::Func(x), Value::Func(y)) => Rc::ptr_eq(x, y),
        (Value::Iter(x), Value::Iter(y)) => Rc::ptr_eq(x, y),
        _ => return Err(Outside("'is' between numbers, strings or tuples".into())),
    })
}

/// `a < b` and the like: the ordering of two values, or Python's
/// TypeError.
pub fn order(a: &Value, b: &Value, op: &str) -> Result<Option<Ordering>, Fault> {
    order_w(a, b, op, &mut Walk::default(), 0)
}

fn order_w(
    a: &Value,
    b: &Value,
    op: &str,
    w: &mut Walk,
    depth: usize,
) -> Result<Option<Ordering>, Fault> {
    w.step(depth)?;
    if a.is_number() && b.is_number() {
        return Ok(cmp_num(a, b));
    }
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => {
            charge(x.len().min(y.len()) as u64)?;
            Ok(Some(x.as_str().cmp(y.as_str())))
        }
        (Value::Tuple(x), Value::Tuple(y)) => seq_order(x, y, op, w, depth),
        (Value::List(x), Value::List(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            seq_order(&x, &y, op, w, depth)
        }
        _ => Err(type_error(format!(
            "'{op}' not supported between instances of '{}' and '{}'",
            a.type_name(),
            b.type_name()
        ))),
    }
}

fn seq_order(
    x: &[Value],
    y: &[Value],
    op: &str,
    w: &mut Walk,
    depth: usize,
) -> Result<Option<Ordering>, Fault> {
    for v in x.iter().chain(y) {
        if has_nan(v)? {
            return Err(Outside("ordering sequences that hold a NaN".into()));
        }
    }
    for (p, q) in x.iter().zip(y) {
        if !eq_w(p, q, w, depth + 1)? {
            return order_w(p, q, op, w, depth + 1);
        }
    }
    Ok(Some(x.len().cmp(&y.len())))
}

/// Checks that `k` can be a dict key here.
pub fn check_key(k: &Value) -> Result<(), Fault> {
    check_key_w(k, &mut Walk::default(), 0)
}

fn check_key_w(k: &Value, w: &mut Walk, depth: usize) -> Result<(), Fault> {
    w.step(depth)?;
    match k {
        Value::List(_) | Value::Dict(_) => {
            Err(type_error(format!("unhashable type: '{}'", k.type_name())))
        }
        Value::View(..)
        | Value::Iter(_)
        | Value::Func(_)
        | Value::Builtin(_)
        | Value::Method(_)
        | Value::Math
        | Value::Range(..) => Err(Outside(format!("a {} as a dict key", k.type_name()))),
        Value::Float(x) if x.is_nan() => Err(Outside("a NaN as a dict key".into())),
        Value::Tuple(t) => t.iter().try_for_each(|x| check_key_w(x, w, depth + 1)),
        _ => Ok(()),
    }
}

/// The entry of key `k` in `d`, if any.
fn find(d: &Dict, k: &Value) -> Result<Option<usize>, Fault> {
    let len = d.entries.len();
    if len < INDEX_FROM {
        for (i, (k2, _)) in d.entries.iter().enumerate() {
            charge(1)?;
            if eq(k, k2)? {
                return Ok(Some(i));
            }
        }
        return Ok(None);
    }
    let mut ix = d.index.borrow_mut();
    if ix.hashes.len() < len {
        room(&ix.hashes, len - ix.hashes.len())?;
        while ix.hashes.len() < len {
            let h = hash_of(&d.entries[ix.hashes.len()].0)?;
            ix.hashes.push(h);
        }
    }
    // at most half full: a larger table, every entry placed again
    if ix.slots.len() < 2 * len {
        let n = (2 * len).next_power_of_two().max(2 * INDEX_FROM);
        fits(heap(n * 4))?;
        ix.slots.clear();
        ix.slots.resize(n, 0);
        ix.n = 0;
    }
    while ix.n < len {
        charge(1)?;
        let (i, h) = (ix.n, ix.hashes[ix.n]);
        ix.place(i, h);
        ix.n += 1;
    }
    ix.account();
    let want = hash_of(k)?;
    let mask = ix.slots.len() - 1;
    let mut at = want as usize & mask;
    loop {
        let slot = ix.slots[at];
        if slot == 0 {
            return Ok(None);
        }
        let i = slot as usize - 1;
        charge(1)?;
        if ix.hashes[i] == want && eq(k, &d.entries[i].0)? {
            return Ok(Some(i));
        }
        at = (at + 1) & mask;
    }
}

/// `d[k]`, if present.
pub fn dict_get(d: &Rc<DictCell>, k: &Value) -> Result<Option<Value>, Fault> {
    check_key(k)?;
    let dm = d.borrow();
    Ok(find(&dm, k)?.map(|i| dm.entries[i].1.clone()))
}

/// The position of key `k` in `d`.
pub fn dict_find(d: &Rc<DictCell>, k: &Value) -> Result<Option<usize>, Fault> {
    check_key(k)?;
    find(&d.borrow(), k)
}

/// `d[k] = v`, where `v` cannot hold `d` (a dict being built).
fn dict_insert(d: &Rc<DictCell>, k: Value, v: Value) -> Result<(), Fault> {
    check_key(&k)?;
    let at = find(&d.borrow(), &k)?;
    let mut dm = d.borrow_mut();
    match at {
        Some(i) => dm.entries[i].1 = v,
        None => {
            if dm.entries.len() >= crate::MAX_ITEMS {
                return Err(Outside("a dict this large".into()));
            }
            room(&dm.entries, 1)?;
            dm.keys += 1;
            dm.entries.push((k, v));
            drop(dm);
            d.account();
        }
    }
    Ok(())
}

/// `d[k] = v`.
pub fn dict_set(d: &Rc<DictCell>, k: Value, v: Value) -> Result<(), Fault> {
    insertable(&Value::Dict(d.clone()), &v)?;
    dict_insert(d, k, v)
}

/// `-a`.
pub fn neg(a: &Value) -> Result<Value, Fault> {
    match a {
        Value::Float(x) => Ok(Value::Float(-x)),
        _ => match a.as_int() {
            Some(i) => i.checked_neg().map(Value::Int).ok_or_else(overflow),
            None => Err(type_error(format!("bad operand type for unary -: '{}'", a.type_name()))),
        },
    }
}

/// `+a`.
pub fn pos(a: &Value) -> Result<Value, Fault> {
    match a {
        Value::Float(x) => Ok(Value::Float(*x)),
        _ => match a.as_int() {
            Some(i) => Ok(Value::Int(i)),
            None => Err(type_error(format!("bad operand type for unary +: '{}'", a.type_name()))),
        },
    }
}

fn unsupported_operands(op: &str, a: &Value, b: &Value) -> Fault {
    type_error(format!(
        "unsupported operand type(s) for {op}: '{}' and '{}'",
        a.type_name(),
        b.type_name()
    ))
}

/// CPython's float floor division and modulo (`_float_div_mod`).
fn float_div_mod(vx: f64, wx: f64) -> (f64, f64) {
    let mut m = vx % wx; // fmod: exact
    let mut div = (vx - m) / wx;
    if m != 0.0 {
        if (wx < 0.0) != (m < 0.0) {
            m += wx;
            div -= 1.0;
        }
    } else {
        m = 0.0f64.copysign(wx);
    }
    let floordiv = if div != 0.0 {
        let mut f = div.floor();
        if div - f > 0.5 {
            f += 1.0;
        }
        f
    } else {
        0.0f64.copysign(vx / wx)
    };
    (floordiv, m)
}

/// CPython's float modulo (`float_rem`).
fn float_rem(vx: f64, wx: f64) -> f64 {
    let mut m = vx % wx;
    if m != 0.0 {
        if (wx < 0.0) != (m < 0.0) {
            m += wx;
        }
    } else {
        m = 0.0f64.copysign(wx);
    }
    m
}

fn concat_len(a: usize, b: usize) -> Result<usize, Fault> {
    let n = a + b;
    if n > crate::MAX_ITEMS {
        return Err(Outside("a sequence this long".into()));
    }
    Ok(n)
}

/// `a op b` for `+ - * / // % **`.
pub fn binary(op: crate::ast::BinOp, a: &Value, b: &Value) -> Result<Value, Fault> {
    binary_op(op, a, b, false)
}

/// `a op= b`, where `a` is not a list (whose `+=` extends it): as `a op
/// b`, Python's message naming the augmented operator.
pub fn binary_inplace(op: crate::ast::BinOp, a: &Value, b: &Value) -> Result<Value, Fault> {
    binary_op(op, a, b, true)
}

fn binary_op(op: crate::ast::BinOp, a: &Value, b: &Value, inplace: bool) -> Result<Value, Fault> {
    use crate::ast::BinOp::*;
    let sym = match (op, inplace) {
        (Add, false) => "+",
        (Sub, false) => "-",
        (Mul, false) => "*",
        (Div, false) => "/",
        (FloorDiv, false) => "//",
        (Mod, false) => "%",
        (Pow, false) => "** or pow()",
        (Add, true) => "+=",
        (Sub, true) => "-=",
        (Mul, true) => "*=",
        (Div, true) => "/=",
        (FloorDiv, true) => "//=",
        (Mod, true) => "%=",
        (Pow, true) => "**=",
    };
    // integers (bools are integers)
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return match op {
            Add => x.checked_add(y).map(Value::Int).ok_or_else(overflow),
            Sub => x.checked_sub(y).map(Value::Int).ok_or_else(overflow),
            Mul => x.checked_mul(y).map(Value::Int).ok_or_else(overflow),
            Div => {
                if y == 0 {
                    return Err(Raise("division by zero".into()));
                }
                if x.unsigned_abs() > EXACT || y.unsigned_abs() > EXACT {
                    return Err(Outside("dividing integers beyond 2**53".into()));
                }
                // both exact as floats: IEEE division is Python's correctly
                // rounded true division
                Ok(Value::Float(x as f64 / y as f64))
            }
            FloorDiv | Mod => {
                if y == 0 {
                    return Err(Raise(
                        if op == Mod {
                            "integer modulo by zero"
                        } else {
                            "integer division or modulo by zero"
                        }
                        .into(),
                    ));
                }
                let q = x.checked_div(y).ok_or_else(overflow)?;
                let r = x - q * y;
                let (q, r) = if r != 0 && ((r < 0) != (y < 0)) { (q - 1, r + y) } else { (q, r) };
                Ok(Value::Int(if op == FloorDiv { q } else { r }))
            }
            Pow => {
                if y < 0 {
                    return Err(Outside("a negative integer power".into()));
                }
                let e = u32::try_from(y).map_err(|_| overflow())?;
                x.checked_pow(e).map(Value::Int).ok_or_else(overflow)
            }
        };
    }
    // floats, with an integer converted
    if let (Some(x), Some(y)) = (a.as_float(), b.as_float()) {
        return match op {
            Add => Ok(Value::Float(x + y)),
            Sub => Ok(Value::Float(x - y)),
            Mul => Ok(Value::Float(x * y)),
            Div => {
                if y == 0.0 {
                    return Err(Raise("float division by zero".into()));
                }
                Ok(Value::Float(x / y))
            }
            FloorDiv => {
                if y == 0.0 {
                    return Err(Raise("float floor division by zero".into()));
                }
                Ok(Value::Float(float_div_mod(x, y).0))
            }
            Mod => {
                if y == 0.0 {
                    // CPython 3.11's words (3.12 added "by zero")
                    return Err(Raise("float modulo".into()));
                }
                Ok(Value::Float(float_rem(x, y)))
            }
            // CPython calls the C runtime's pow: not the same bits everywhere
            Pow => Err(Outside("powers of floats".into())),
        };
    }
    // sequences
    match (op, a, b) {
        (Add, Value::Str(x), Value::Str(y)) => {
            let n = concat_len(x.len(), y.len())?;
            charge(n as u64)?;
            fits(Text::bytes(n))?;
            let mut s = String::with_capacity(n);
            s.push_str(x);
            s.push_str(y);
            Ok(Value::string(s))
        }
        (Add, Value::Tuple(x), Value::Tuple(y)) => {
            let n = concat_len(x.len(), y.len())?;
            charge(n as u64)?;
            fits(heap(n * VALUE))?;
            Ok(Value::tuple(x.iter().chain(y.iter()).cloned().collect()))
        }
        (Add, Value::List(x), Value::List(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            let n = concat_len(x.len(), y.len())?;
            charge(n as u64)?;
            fits(heap(n * VALUE))?;
            Ok(Value::list(x.iter().chain(y.iter()).cloned().collect()))
        }
        (Mod, Value::Str(_), _) => Err(Outside("string formatting with '%'".into())),
        (Mul, Value::Str(_) | Value::Tuple(_) | Value::List(_), _)
        | (Mul, _, Value::Str(_) | Value::Tuple(_) | Value::List(_)) => {
            Err(Outside("repeating a sequence with '*'".into()))
        }
        // a sequence's own concatenation says what it cannot add
        (Add, Value::Str(_) | Value::Tuple(_) | Value::List(_), _) => Err(type_error(format!(
            "can only concatenate {t} (not \"{}\") to {t}",
            b.type_name(),
            t = a.type_name()
        ))),
        _ => Err(unsupported_operands(sym, a, b)),
    }
}

/// Counts `n` more items of ranges the current call has CPython compare
/// with a value that is no integer: whether they are a million or fewer in
/// all.
fn scanned(n: u64) -> bool {
    SCANNED.with(|s| {
        let all = s.get().saturating_add(n);
        s.set(all);
        all <= crate::MAX_ITEMS as u64
    })
}

/// `x in c`.
pub fn contains(c: &Value, x: &Value) -> Result<bool, Fault> {
    match c {
        Value::Dict(d) => Ok(dict_get(d, x)?.is_some()),
        Value::List(_) | Value::Tuple(_) => {
            searchable(x)?;
            let found = |items: &[Value]| -> Result<bool, Fault> {
                let mut w = Walk::default();
                for v in items {
                    if eq_w(v, x, &mut w, 0)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            };
            match c {
                Value::List(l) => found(&l.borrow()),
                Value::Tuple(t) => found(t),
                _ => unreachable!("a sequence"),
            }
        }
        Value::Str(s) => match x {
            Value::Str(sub) => {
                charge((s.len() + sub.len()) as u64)?;
                Ok(s.contains(sub.as_str()))
            }
            _ => Err(type_error(format!(
                "'in <string>' requires string as left operand, not {}",
                x.type_name()
            ))),
        },
        Value::Range(a, b, st) => match x.as_int() {
            Some(i) => {
                let n = range_len(*a, *b, *st);
                let off = i as i128 - *a as i128;
                Ok(n > 0 && off % *st as i128 == 0 && {
                    let k = off / *st as i128;
                    k >= 0 && k < n
                })
            }
            // CPython compares any other value with every item (in C, which
            // no time limit stops, some 20 ms a million) where nothing is
            // done here: no item equals it, but ranges of more than a
            // million items in all in a call are left to Python
            None => match x {
                Value::Float(_) => Err(Outside("a float 'in' a range".into())),
                _ => match u64::try_from(range_len(*a, *b, *st)) {
                    Ok(n) if scanned(n) => {
                        charge(n)?;
                        Ok(false)
                    }
                    _ => Err(Outside(format!(
                        "a {} 'in' ranges of more than a million items in a call",
                        x.type_name()
                    ))),
                },
            },
        },
        _ => Err(type_error(format!("argument of type '{}' is not iterable", c.type_name()))),
    }
}

/// A sequence index `i` of a sequence of `n` items (negative from the end).
fn seq_index(n: i128, i: &Value, what: &str) -> Result<i128, Fault> {
    let Some(k) = i.as_int() else {
        let t = i.type_name();
        return Err(type_error(match what {
            "string" => format!("string indices must be integers, not '{t}'"),
            "range object" => format!("range indices must be integers or slices, not {t}"),
            _ => format!("{what} indices must be integers or slices, not {t}"),
        }));
    };
    let k = k as i128;
    let k = if k < 0 { k + n } else { k };
    if k < 0 || k >= n {
        return Err(Raise(format!("{what} index out of range")));
    }
    Ok(k)
}

/// `c[i]`.
pub fn index(c: &Value, i: &Value) -> Result<Value, Fault> {
    match c {
        Value::Dict(d) => match dict_get(d, i)? {
            Some(v) => Ok(v),
            None => Err(Raise(i.repr()?)),
        },
        Value::Tuple(t) => Ok(t[seq_index(t.len() as i128, i, "tuple")? as usize].clone()),
        Value::List(l) => {
            let l = l.borrow();
            Ok(l[seq_index(l.len() as i128, i, "list")? as usize].clone())
        }
        Value::Str(s) => {
            charge(s.len() as u64)?;
            let k = seq_index(s.chars().count() as i128, i, "string")?;
            let c = s.chars().nth(k as usize).expect("an index inside the string");
            Ok(Value::str(c.encode_utf8(&mut [0; 4])))
        }
        Value::Range(a, b, st) => {
            let k = seq_index(range_len(*a, *b, *st), i, "range object")?;
            // inside the range: a 64-bit integer
            Ok(Value::Int((*a as i128 + *st as i128 * k) as i64))
        }
        // `list[float]` is a generic alias, `float[int]` Python's own
        // TypeError: left to Python
        Value::Builtin(_) => Err(Outside("subscripting a builtin".into())),
        _ => Err(type_error(format!("'{}' object is not subscriptable", c.type_name()))),
    }
}

/// `c[i] = v`.
pub fn set_index(c: &Value, i: Value, v: Value) -> Result<(), Fault> {
    match c {
        Value::Dict(d) => dict_set(d, i, v),
        Value::List(l) => {
            insertable(c, &v)?;
            let mut l = l.borrow_mut();
            let k = match seq_index(l.len() as i128, &i, "list") {
                Ok(k) => k,
                Err(Raise(m)) if m.ends_with("index out of range") => {
                    return Err(Raise("list assignment index out of range".into()));
                }
                Err(e) => return Err(e),
            };
            l[k as usize] = v;
            Ok(())
        }
        _ => {
            Err(type_error(format!("'{}' object does not support item assignment", c.type_name())))
        }
    }
}

/// `float(x)` as Python converts.
pub fn to_float(x: &Value) -> Result<f64, Fault> {
    match x {
        Value::Str(s) => {
            charge(s.len() as u64)?;
            if !plain_ascii(s) {
                return Err(Outside("a number written with other than plain ASCII".into()));
            }
            match parse_float(s) {
                Some(v) => Ok(v),
                None => Err(Raise(format!("could not convert string to float: {}", x.repr()?))),
            }
        }
        _ => x.as_float().ok_or_else(|| {
            type_error(format!(
                "float() argument must be a string or a real number, not '{}'",
                x.type_name()
            ))
        }),
    }
}

/// Python's `float(str)` of an ASCII string: surrounding whitespace, a
/// sign, digits with single underscores between them, an exponent, `inf`,
/// `infinity`, `nan` (the sign kept, as Python keeps it).
pub fn parse_float(s: &str) -> Option<f64> {
    let t = s.trim_matches(|c: char| c.is_ascii_whitespace());
    let (sign, body) = match t.as_bytes().first() {
        Some(b'+') => (1.0, &t[1..]),
        Some(b'-') => (-1.0, &t[1..]),
        _ => (1.0, t),
    };
    let lower = body.to_ascii_lowercase();
    match lower.as_str() {
        "inf" | "infinity" => return Some(sign * f64::INFINITY),
        "nan" => return Some(f64::NAN.copysign(sign)),
        _ => {}
    }
    // digits, '.', 'e', sign after 'e', underscores only between digits
    let b = body.as_bytes();
    let mut clean = String::with_capacity(b.len());
    for (k, &ch) in b.iter().enumerate() {
        match ch {
            b'0'..=b'9' | b'.' | b'e' | b'E' => clean.push(ch as char),
            b'+' | b'-' if k > 0 && matches!(b[k - 1], b'e' | b'E') => clean.push(ch as char),
            b'_' if k > 0
                && k + 1 < b.len()
                && b[k - 1].is_ascii_digit()
                && b[k + 1].is_ascii_digit() => {}
            _ => return None,
        }
    }
    if !clean.bytes().any(|c| c.is_ascii_digit()) {
        return None;
    }
    // a mantissa needs a digit before an exponent (".e1" is not a number)
    let mant = clean.split(['e', 'E']).next().unwrap_or("");
    if !mant.bytes().any(|c| c.is_ascii_digit()) {
        return None;
    }
    clean.parse::<f64>().ok().map(|v| sign * v)
}

/// `int(x)` as Python converts (towards zero).
pub fn to_int(x: &Value) -> Result<i64, Fault> {
    match x {
        Value::Float(f) => {
            if f.is_nan() {
                return Err(Raise("cannot convert float NaN to integer".into()));
            }
            if f.is_infinite() {
                return Err(Raise("cannot convert float infinity to integer".into()));
            }
            let t = f.trunc();
            if !(-TWO_63..TWO_63).contains(&t) {
                return Err(overflow());
            }
            Ok(t as i64)
        }
        Value::Str(s) => {
            charge(s.len() as u64)?;
            if !plain_ascii(s) {
                return Err(Outside("a number written with other than plain ASCII".into()));
            }
            // (CPython shows at most 200 characters of the repr)
            let bad = || -> Fault {
                match x.repr() {
                    Ok(r) => {
                        let r: String = r.chars().take(200).collect();
                        Raise(format!("invalid literal for int() with base 10: {r}"))
                    }
                    Err(f) => f,
                }
            };
            let t = s.trim_matches(|c: char| c.is_ascii_whitespace());
            let (neg, body) = match t.as_bytes().first() {
                Some(b'+') => (false, &t[1..]),
                Some(b'-') => (true, &t[1..]),
                _ => (false, t),
            };
            // digits with single underscores between them
            let b = body.as_bytes();
            if b.is_empty()
                || !b.iter().all(|c| c.is_ascii_digit() || *c == b'_')
                || b[0] == b'_'
                || b[b.len() - 1] == b'_'
                || body.contains("__")
            {
                return Err(bad());
            }
            let digits: String = body.chars().filter(|c| *c != '_').collect();
            // Python refuses more digits than sys.get_int_max_str_digits()
            // (4300 unless set, never below 640)
            if digits.len() > MAX_SURE_DIGITS {
                return Err(Outside("int() of more than 640 digits".into()));
            }
            let v: i128 = digits.parse().map_err(|_| overflow())?;
            let v = if neg { -v } else { v };
            i64::try_from(v).map_err(|_| overflow())
        }
        _ => x.as_int().ok_or_else(|| {
            type_error(format!(
                "int() argument must be a string, a bytes-like object or a real number, not '{}'",
                x.type_name()
            ))
        }),
    }
}

/// The most digits `int(str)` converts on every configuration of Python
/// (the lowest limit `sys.set_int_max_str_digits` takes).
const MAX_SURE_DIGITS: usize = 640;

/// ASCII without the control characters Python counts as white space and
/// Rust does not (vertical tab, the separators 0x1c to 0x1f).
fn plain_ascii(s: &str) -> bool {
    s.is_ascii() && !s.bytes().any(|b| b == 0x0b || (0x1c..=0x1f).contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::BinOp;

    fn f(x: f64) -> Value {
        Value::Float(x)
    }

    fn fl(v: Result<Value, Fault>) -> f64 {
        match v.unwrap() {
            Value::Float(x) => x,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn float_floor_division_and_modulo_are_cpythons() {
        // values checked against CPython 3.13
        assert_eq!(fl(binary(BinOp::FloorDiv, &f(7.5), &f(2.0))), 3.0);
        assert_eq!(fl(binary(BinOp::FloorDiv, &f(-7.5), &f(2.0))), -4.0);
        assert_eq!(fl(binary(BinOp::Mod, &f(-7.5), &f(2.0))), 0.5);
        assert_eq!(fl(binary(BinOp::Mod, &f(7.5), &f(-2.0))), -0.5);
        let z = fl(binary(BinOp::Mod, &f(4.0), &f(-2.0)));
        assert!(z == 0.0 && z.is_sign_negative());
        let z = fl(binary(BinOp::FloorDiv, &f(-0.0), &f(5.0)));
        assert!(z == 0.0 && z.is_sign_negative());
        assert_eq!(fl(binary(BinOp::FloorDiv, &f(1.0), &f(0.1))), 9.0);
        assert_eq!(fl(binary(BinOp::Mod, &f(1.0), &f(0.1))), 0.09999999999999995);
        assert!(binary(BinOp::Div, &f(1.0), &f(0.0)).is_err());
        assert!(binary(BinOp::Div, &Value::Int(1), &Value::Int(0)).is_err());
    }

    #[test]
    fn integers_follow_python() {
        let i = Value::Int;
        assert!(matches!(binary(BinOp::FloorDiv, &i(-7), &i(2)), Ok(Value::Int(-4))));
        assert!(matches!(binary(BinOp::Mod, &i(-7), &i(2)), Ok(Value::Int(1))));
        assert!(matches!(binary(BinOp::Mod, &i(7), &i(-2)), Ok(Value::Int(-1))));
        assert!(matches!(binary(BinOp::Add, &Value::Bool(true), &i(1)), Ok(Value::Int(2))));
        assert_eq!(fl(binary(BinOp::Div, &i(1), &i(3))), 1.0 / 3.0);
        assert!(matches!(binary(BinOp::Mul, &i(i64::MAX), &i(2)), Err(Outside(_))));
        assert!(matches!(binary(BinOp::Pow, &f(2.0), &i(2)), Err(Outside(_))));
        assert!(matches!(binary(BinOp::Pow, &i(2), &i(10)), Ok(Value::Int(1024))));
    }

    #[test]
    fn comparisons_between_ints_and_floats_are_exact() {
        let big = (1i64 << 53) + 1;
        assert!(!eq(&Value::Int(big), &f(big as f64)).unwrap(), "2^53 + 1 is not the float 2^53");
        assert!(eq(&Value::Int(3), &f(3.0)).unwrap());
        assert!(eq(&Value::Int(i64::MIN), &f(-TWO_63)).unwrap());
        assert!(eq(&Value::Bool(true), &Value::Int(1)).unwrap());
        assert_eq!(order(&Value::Int(big), &f(big as f64), "<").unwrap(), Some(Ordering::Greater));
        assert_eq!(order(&f(f64::NAN), &Value::Int(1), "<").unwrap(), None);
        assert!(order(&Value::str("a"), &Value::Int(1), "<").is_err());
        let d = Value::dict(vec![(Value::Int(1), Value::str("x"))]).unwrap();
        let Value::Dict(d) = d else { panic!() };
        assert!(dict_get(&d, &f(1.0)).unwrap().is_some(), "1.0 finds the key 1");
        assert!(dict_get(&d, &Value::Bool(true)).unwrap().is_some());
    }

    #[test]
    fn deep_and_shared_containers_neither_crash_nor_hang() {
        // a million deep: dropped without recursion
        let mut x = Value::list(vec![]);
        for _ in 0..1_000_000 {
            x = Value::list(vec![x]);
        }
        assert!(matches!(eq(&x, &x.clone()), Ok(true)));
        let y = Value::list(vec![Value::list(vec![])]);
        assert!(matches!(eq(&x, &y), Err(Outside(_)) | Ok(false)));
        drop(x);
        // shared 2^80 ways: each container looked into once
        let mut a = Value::list(vec![Value::Float(0.0)]);
        for _ in 0..80 {
            a = Value::list(vec![a.clone(), a]);
        }
        assert!(!has_nan(&a).unwrap());
        // a container that would hold itself
        let l = Value::list(vec![]);
        assert!(matches!(insertable(&l, &Value::list(vec![l.clone()])), Err(Outside(_))));
    }

    #[test]
    fn conversions_follow_python() {
        assert_eq!(parse_float(" 1_000.5 "), Some(1000.5));
        assert_eq!(parse_float("-Infinity"), Some(f64::NEG_INFINITY));
        assert_eq!(parse_float("1__0"), None);
        assert_eq!(parse_float("_1"), None);
        assert_eq!(parse_float(".e1"), None);
        assert_eq!(parse_float("5."), Some(5.0));
        assert_eq!(to_int(&f(-2.9)).unwrap(), -2);
        assert!(to_int(&f(f64::NAN)).is_err());
        assert_eq!(to_int(&Value::str(" 42 ")).unwrap(), 42);
        assert!(to_int(&Value::str("+_1")).is_err());
        assert!(matches!(to_int(&Value::str("\u{661}")), Err(Outside(_))));
        assert!(parse_float("-nan").unwrap().is_sign_negative());
        assert_eq!(
            range_len(-5_000_000_000_000_000_000, 5_000_000_000_000_000_000, 1),
            10_000_000_000_000_000_000
        );
    }
}
