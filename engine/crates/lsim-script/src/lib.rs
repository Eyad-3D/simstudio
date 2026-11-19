//! # lsim-script: Script blocks without the Python interpreter
//!
//! A Script block's code (`def step(t, dt, inputs, state, params)`,
//! returning a dict of outputs) is run here by an interpreter of a strict
//! subset of Python whose every operation has CPython's semantics to the
//! bit: integers, IEEE floats with CPython's floor division and modulo,
//! exact comparisons between integers and floats, dicts, lists, tuples,
//! the builtins scripts use, `clamp`, and `interp` as app/solver/maps.py
//! computes it. The example cars' controllers (the P2 hybrid's Hybrid
//! Control Unit) are in the subset, so they run without the interpreter in
//! the loop and give the same numbers as the app.
//!
//! **The reference** is the app's own Script runner: the locked-down
//! worker (backend/app/solver/sandbox_worker.py's `_run_step`, on CPython
//! 3.11) called as the app calls it. Its parameters are typed as the
//! app's project model types them (schemas.py's `ParamValue`: a table's
//! values are floats, whatever the file writes) and each call gets a fresh
//! shallow copy of them; an input of -0.0 is passed as +0.0. A NaN result
//! is a NaN, but its sign and payload are not part of the promise (IEEE
//! 754 leaves them open, and CPython's own vary).
//!
//! **What is left to Python.** A script is refused when it is first read
//! ([`check`]: [`Unsupported`], with the line and what it is) if it uses
//! anything outside the subset: other imports, the math module's
//! transcendental functions (CPython calls the C runtime's, whose last
//! bit differs between platforms; these would need the engine's own
//! correctly rounded library and so would not match the Python runner),
//! powers of floats (the C runtime's `pow`), string formatting, classes,
//! exceptions, comprehensions, slices, closures, and any name that is not
//! the script's own or one of the builtins here. So are parameters the app
//! would read otherwise or refuse (a null, a list, a table holding other
//! than numbers) or that JSON cannot tell apart (a number written `-0`,
//! Python's integer 0, from `-0.0`). While it runs, an operation Python
//! supports but the subset does not (an integer beyond 64 bits, a method
//! of a string, copying the sign of a NaN, a builtin subscripted, `int()`
//! of more than 640 digits) stops it with [`Fault::Outside`], so the caller
//! can run the case again on Python: a result never differs from the app's
//! runner's. An exception Python would raise (division by zero, a missing
//! key) is [`Fault::Raise`] with CPython 3.11's message, the values it
//! quotes whole as Python's repr shows them; a message whose words are not
//! known here is left to Python instead: one that would quote an object's
//! address (a function, a bound method, an iterator object, the scripts'
//! `clamp` and `interp`), the parameters in the project's order (the
//! importer keeps them sorted), a character beyond the common scripts and
//! symbols, or a value nested or long beyond what is quoted here. So is
//! what that order decides otherwise: outputs returned in the parameters'
//! order (the parameters, or a copy of them or of a table among them),
//! and which of a table parameter's keys 0 and -0 is named the duplicate.
//! So are an evaluation order or a cost that CPython's compiler and C code
//! decide: an unhashable key in a dict display of more than 15 pairs
//! (CPython 3.11 builds up to 15 from all their keys and values evaluated
//! first, as here, and longer ones in runs), `x in range(n)` of an x
//! that is no integer over more than a million items in a call (CPython
//! compares it with each, past any time limit), and what a call prints
//! beyond 1024 writes or 64 KiB (nothing is printed here; CPython writes
//! each argument, the separator before it and the end of the line, past
//! any time limit) or other than numbers and strings of ASCII characters
//! (Python writes text in its standard output's encoding, on Windows the
//! ANSI code page, which may not have them). The worker's frames give the
//! number of inputs and outputs of a call and the length of each output
//! key in 16 bits: more than 65 535 of them, or a key of more than 65 535
//! bytes, stop the worker and fail the app's run, and are left to Python
//! here.
//!
//! **Limits.** As the app's worker: a call that does not return within two
//! seconds stops (timed from its inputs and its copy of the parameters
//! made to its outputs read, as the worker is from the request to its
//! reply; the clock is read every 1024 statements, loop rounds, calls and
//! operations of expressions, and as every operation whose work grows with
//! the size of what it handles goes: a call's arguments, keywords and
//! frame, displays, unpacking, concatenation, copies, comparisons and
//! searches of containers, an item of a `zip`, indexing and converting
//! text, reading the outputs), and a runner may hold at most 512 MB: every
//! heap block its values make counts, as the heap keeps it, from before it
//! is made (strings, lists, tuples, dicts and their indexes, iterator
//! objects and what they keep of their sources, bound methods, functions
//! and their defaults, `interp`'s parsed tables, a call's frame while it
//! runs), and the syntax tree 128 bytes for each byte of code (it holds
//! at most about 100); a script of more than 1 MiB is not read, and
//! reading one costs about its length (a MiB in about 0.2 s). Past
//! either limit the script is left to Python, whose own limit then
//! decides. They are the worker's limits in size, not in how they are
//! measured (Python is slower and its values larger): a call close to
//! either can be stopped by the worker and return here. On top: containers
//! of at most a million items, calls nested at most 50 deep, code nested
//! at most 100 deep (each `elif` of a chain a level, as in Python's tree:
//! CPython refuses some 990 in a row), and code and data nested no deeper
//! than [`STACK_BUDGET`] of the caller's stack allows
//! (measured where the interpreter nests, so such a script is left to
//! Python instead of overflowing a thread of 1 MiB or more, in every
//! build) are outside the subset; Python may go on. The interpreter
//! reaches nothing outside the script: no files, processes, network or
//! memory but its own values.

mod ast;
mod interp;
mod lexer;
mod parser;
mod value;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use value::{Builtin, Value};

pub use value::{MAX_LIVE_BYTES, STACK_BUDGET};

/// The most items a list, tuple, dict or string may hold here.
pub const MAX_ITEMS: usize = 1_000_000;

/// The longest script read here, bytes (reading one takes up to some 300
/// bytes for each byte of code for a moment).
pub const MAX_CODE_BYTES: usize = 1 << 20;

/// What a script's syntax tree, held all the run, counts towards its
/// runner's memory for each byte of its code: more than it holds of
/// every kind of statement and expression (about 100 at the most,
/// `tests/memory.rs`).
const TREE_BYTES_PER_BYTE: usize = 128;

/// The wall-clock budget of one call into the script, s (the Python
/// runner's).
pub const TIME_LIMIT_S: f64 = 2.0;

/// The budget runners take: [`TIME_LIMIT_S`] (0 here), or what the
/// engine's own tests of the other limits set ([`set_time_limit_for_tests`]).
static TIME_LIMIT_SET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn time_limit() -> f64 {
    match TIME_LIMIT_SET.load(std::sync::atomic::Ordering::Relaxed) {
        0 => TIME_LIMIT_S,
        bits => f64::from_bits(bits),
    }
}

/// For the engine's own tests of the memory and depth limits only: runners
/// started after it get `seconds` a call instead of [`TIME_LIMIT_S`], so
/// that a slow machine (a debug build on a CI runner) meets the limit
/// under test before the clock. Nothing else calls it.
#[doc(hidden)]
pub fn set_time_limit_for_tests(seconds: f64) {
    TIME_LIMIT_SET.store(seconds.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// The most inputs and outputs of one call, and the longest output key in
/// UTF-8 bytes, that the app's Script worker sends: its frames give each
/// in 16 bits (sandbox_worker.py's `_pack_step` and `_pack_outputs`), and
/// past them the worker stops and the app's run fails.
pub const MAX_FRAME_FIELD: usize = 65_535;

/// Why a script is outside the subset, found when it is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsupported {
    /// the line (1-based)
    pub line: usize,
    /// what it uses
    pub what: String,
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.what)
    }
}

/// How a call into a script went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// an exception Python raises too: its type and message
    Raise(String),
    /// an operation Python supports but this subset does not: run the
    /// script on Python instead
    Outside(String),
    /// the call took longer than [`TIME_LIMIT_S`]
    Limit,
}

/// Why a script cannot run here, or stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub enum ScriptError {
    /// outside the subset: run it on Python (the Python runner's results)
    Unsupported(String),
    /// the script failed, as it would on Python: the message
    Failed(String),
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptError::Unsupported(m) | ScriptError::Failed(m) => f.write_str(m),
        }
    }
}

/// Whether `code` is in the subset: it parses, every name it reads is its
/// own, one of the builtins here or one every script is given (`math`,
/// `clamp`, `interp`), and it defines `step`.
pub fn check(code: &str) -> Result<(), Unsupported> {
    let m = parser::parse(code)?;
    names_check(&m)
}

fn names_check(m: &ast::Module) -> Result<(), Unsupported> {
    use ast::{Name, StmtKind, Target};
    // every global the script binds anywhere
    let mut bound: std::collections::HashSet<usize> = std::collections::HashSet::new();
    fn target(t: &Target, bound: &mut std::collections::HashSet<usize>) {
        match t {
            Target::Name(Name::Global(k, _)) => {
                bound.insert(*k);
            }
            Target::Tuple(ts) => ts.iter().for_each(|t| target(t, bound)),
            _ => {}
        }
    }
    fn walk(b: &[ast::Stmt], bound: &mut std::collections::HashSet<usize>) {
        for s in b {
            match &s.kind {
                StmtKind::Assign(ts, _) => ts.iter().for_each(|t| target(t, bound)),
                StmtKind::AugAssign(t, ..) => target(t, bound),
                StmtKind::For(t, _, body, orelse) => {
                    target(t, bound);
                    walk(body, bound);
                    walk(orelse, bound);
                }
                StmtKind::Def(n, f, _) => {
                    if let Name::Global(k, _) = n {
                        bound.insert(*k);
                    }
                    walk(&f.body, bound);
                }
                StmtKind::Import(Name::Global(k, _), _) => {
                    bound.insert(*k);
                }
                StmtKind::If(branches, orelse) => {
                    branches.iter().for_each(|(_, b)| walk(b, bound));
                    walk(orelse, bound);
                }
                StmtKind::While(_, body, orelse) => {
                    walk(body, bound);
                    walk(orelse, bound);
                }
                _ => {}
            }
        }
    }
    walk(&m.body, &mut bound);
    for (k, n) in m.globals.iter().enumerate() {
        let given = matches!(&**n, "math" | "clamp" | "interp") || Builtin::of(n).is_some();
        if !bound.contains(&k) && !given {
            return Err(Unsupported { line: 0, what: format!("the name '{n}'") });
        }
    }
    let step = m.globals.iter().position(|n| &**n == "step");
    if !step.is_some_and(|k| bound.contains(&k)) {
        return Err(Unsupported { line: 0, what: "a script without step()".into() });
    }
    Ok(())
}

/// A number that is zero with its sign set: JSON's `-0` (Python's
/// integer 0) or `-0.0` (a float), which serde_json cannot tell apart.
fn minus_zero(x: f64) -> Result<f64, Fault> {
    if x == 0.0 && x.is_sign_negative() {
        return Err(Fault::Outside(
            "a parameter written -0 or -0.0 (Python reads the one as an integer)".into(),
        ));
    }
    Ok(x)
}

/// A scalar parameter's number, as Python's `json.loads` reads it.
fn number(n: &serde_json::Number) -> Result<Value, Fault> {
    if let Some(i) = n.as_i64() {
        return Ok(Value::Int(i));
    }
    if n.is_u64() {
        return Err(Fault::Outside("an integer parameter beyond 64 bits".into()));
    }
    let x = minus_zero(n.as_f64().ok_or_else(|| Fault::Outside("a number".into()))?)?;
    // an integer written beyond 64 bits arrives here as a float (Python
    // reads it as an integer)
    if x.fract() == 0.0 && x.abs() >= 9_007_199_254_740_992.0 {
        return Err(Fault::Outside("a whole-number parameter beyond 2**53".into()));
    }
    Ok(Value::Float(x))
}

/// A table of `{key: number}` with its values as floats (Rust's `as`
/// rounds an integer as Python's `float()` does), in the importer's order.
fn float_table<'a>(
    t: impl Iterator<Item = (&'a String, &'a serde_json::Value)>,
    value: impl Fn(&'a serde_json::Value) -> Result<Value, Fault>,
) -> Result<Value, Fault> {
    let d = Value::dict(t.map(|(k, v)| Ok((Value::str(k), value(v)?))).collect::<Result<_, _>>()?)?;
    unordered(&d);
    Ok(d)
}

/// The importer's map is sorted, Python keeps the project's order:
/// iterating such a dict is left to Python.
fn unordered(d: &Value) {
    if let Value::Dict(cell) = d {
        cell.borrow_mut().unordered = true;
    }
}

/// A Script block's parameters as the app hands them to it: the JSON
/// object read as Python's `json.loads` reads it, each value typed as the
/// app's project model types a parameter (schemas.py's `ParamValue`: a
/// bool, int, float or string, or a table of floats, `{x: value}` or
/// `{x: {y: value}}`). What the app would read otherwise or refuse is
/// left to Python.
fn params_of(o: &serde_json::Map<String, serde_json::Value>) -> Result<Value, Fault> {
    use serde_json::Value as J;
    let leaf = |v: &J| match v {
        J::Number(n) => Ok(Value::Float(minus_zero(n.as_f64().unwrap_or(f64::NAN))?)),
        _ => unreachable!("a table of numbers"),
    };
    let numbers = |t: &serde_json::Map<String, J>| t.values().all(J::is_number);
    let mut entries = Vec::with_capacity(o.len());
    for (k, v) in o {
        let v = match v {
            J::Bool(b) => Value::Bool(*b),
            J::String(s) => Value::str(s),
            J::Number(n) => number(n)?,
            J::Object(t) if numbers(t) => float_table(t.iter(), leaf)?,
            J::Object(t) if t.values().all(|x| x.as_object().is_some_and(numbers)) => {
                float_table(t.iter(), |x| {
                    float_table(x.as_object().expect("a table").iter(), leaf)
                })?
            }
            // pydantic makes a table's booleans and numeric text floats
            // and refuses the rest
            J::Object(_) => {
                return Err(Fault::Outside(format!(
                    "the table parameter '{k}' holding other than numbers"
                )));
            }
            J::Null | J::Array(_) => {
                return Err(Fault::Outside(format!(
                    "the parameter '{k}', which the app does not read (a null or a list)"
                )));
            }
        };
        entries.push((Value::str(k), v));
    }
    let d = Value::dict(entries)?;
    unordered(&d);
    Ok(d)
}

/// A running script: its values between calls, and the memory they hold
/// (counted while it is called, see [`MAX_LIVE_BYTES`]).
pub struct Runner {
    script: Option<Script>,
    live: usize,
}

/// `step` and its `state` and `params` between calls.
struct Script {
    /// what its syntax tree counts (see [`TREE_BYTES_PER_BYTE`])
    tree: usize,
    module: ast::Module,
    machine: interp::Machine,
    step: Value,
    state: Value,
    params: Value,
    label: String,
    /// the names of the inputs of the last call, as values
    keys: Vec<(String, Value)>,
}

// SAFETY: a Runner owns every reference-counted value it can reach: it
// parses its own copy of the script (`Runner::start` takes the source
// text, not a shared tree), creates every value while it runs, and hands
// out only plain numbers and strings (`Runner::step` returns owned
// `String`s and `f64`s; errors are owned `String`s). No `Rc` is ever
// shared with anything outside it, so moving the whole Runner to another
// thread moves every count with it, and nothing on the old thread can
// touch them. The memory its values hold travels with it too (`live`,
// put in place on whatever thread calls it).
unsafe impl Send for Runner {}

/// Python's `format(x, "g")`: six significant digits, fixed or with an
/// exponent, trailing zeros dropped.
pub fn fmt_g(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let sci = format!("{x:.5e}");
    let (mant, exp) = sci.split_once('e').expect("an exponent");
    let exp: i32 = exp.parse().expect("an integer exponent");
    let trim = |s: String| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
    };
    if (-4..6).contains(&exp) {
        trim(format!("{x:.*}", (5 - exp) as usize))
    } else {
        let m = trim(mant.to_string());
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

fn failed(label: &str, t: Option<f64>, f: Fault) -> ScriptError {
    match f {
        Fault::Outside(what) => ScriptError::Unsupported(format!(
            "Script '{label}': {what} is outside what runs without Python"
        )),
        Fault::Raise(msg) => ScriptError::Failed(match t {
            Some(t) => format!("Script '{label}' raised at t = {} s: {msg}", fmt_g(t)),
            None => format!("Script '{label}' failed to compile: {msg}"),
        }),
        // Python may be faster at some of it (a large dict): Python's own
        // limit then decides
        Fault::Limit => ScriptError::Unsupported(match t {
            Some(t) => format!(
                "Script '{label}' took longer than {TIME_LIMIT_S} s at t = {} s without Python",
                fmt_g(t)
            ),
            None => format!(
                "Script '{label}': top-level code took longer than {TIME_LIMIT_S} s without Python"
            ),
        }),
    }
}

impl Runner {
    /// Reads `code` and runs its top-level code, as the app's runner does
    /// at the start of a run; `params` is the block's parameters (a JSON
    /// object, typed as the app types them).
    pub fn start(
        code: &str,
        label: &str,
        params: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Runner, ScriptError> {
        let outer = value::memory_enter(0);
        let script = Script::start(code, label, params);
        // (what a failed start made is gone again)
        let live = value::memory_leave(outer);
        Ok(Runner { script: Some(script?), live })
    }

    /// One call `step(t, dt, inputs, state, params)`: the outputs it
    /// returns, as the app's runner converts them (`float(value)` under
    /// the key's text), in the order of the returned dict.
    pub fn step(
        &mut self,
        t: f64,
        dt: f64,
        inputs: &[(&str, f64)],
    ) -> Result<Vec<(String, f64)>, ScriptError> {
        let outer = value::memory_enter(self.live);
        let out = self.script.as_mut().expect("a started script").step(t, dt, inputs);
        self.live = value::memory_leave(outer);
        out
    }
}

impl Runner {
    /// The bytes its values are counted as holding: what
    /// [`MAX_LIVE_BYTES`] caps.
    pub fn memory(&self) -> usize {
        self.live
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        // its values give their memory back to its own count
        let outer = value::memory_enter(self.live);
        self.script = None;
        value::memory_leave(outer);
    }
}

impl Drop for Script {
    fn drop(&mut self) {
        value::memory_sub(self.tree);
    }
}

impl Script {
    fn start(
        code: &str,
        label: &str,
        params: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Script, ScriptError> {
        if code.len() > MAX_CODE_BYTES {
            return Err(ScriptError::Unsupported(format!(
                "Script '{label}': a script of more than {} MiB is outside what runs without Python",
                MAX_CODE_BYTES >> 20
            )));
        }
        // the tree counts from before it is read (a failed start gives
        // back all it counted)
        let tree = code.len() * TREE_BYTES_PER_BYTE;
        value::memory_add(tree);
        let module = parser::parse(code)
            .map_err(|u| ScriptError::Unsupported(format!("Script '{label}', {u}")))?;
        names_check(&module)
            .map_err(|u| ScriptError::Unsupported(format!("Script '{label}': {}", u.what)))?;
        let mut machine = interp::Machine::new(&module, time_limit());
        machine.run_module(&module).map_err(|f| failed(label, None, f))?;
        let step = match machine.global("step") {
            Some(f @ Value::Func(_)) => f,
            _ => {
                return Err(ScriptError::Unsupported(format!(
                    "Script '{label}': step is not a plain function after the top-level code"
                )));
            }
        };
        // reading the parameters is timed as a call is
        value::start_meter(Some(
            std::time::Instant::now() + std::time::Duration::from_secs_f64(time_limit()),
        ));
        let params = params_of(params)
            .and_then(|p| value::memory_check().map(|_| p))
            .map_err(|f| failed(label, None, f))?;
        value::start_meter(None);
        Ok(Script {
            tree,
            module,
            machine,
            step,
            state: Value::dict(vec![]).expect("an empty dict"),
            params,
            label: label.to_string(),
            keys: vec![],
        })
    }

    fn step(
        &mut self,
        t: f64,
        dt: f64,
        inputs: &[(&str, f64)],
    ) -> Result<Vec<(String, f64)>, ScriptError> {
        let _ = &self.module;
        // the call's clock: the app's worker is timed from the request it is
        // sent to its reply, the inputs and the parameters' copy made, the
        // outputs read and sent
        self.machine.start_call();
        let label = self.label.clone();
        let fail = |f| failed(&label, Some(t), f);
        if inputs.len() > MAX_FRAME_FIELD {
            return Err(fail(Fault::Outside(format!(
                "more than {MAX_FRAME_FIELD} inputs (the app's worker cannot be sent them)"
            ))));
        }
        if self.keys.len() != inputs.len()
            || self.keys.iter().zip(inputs).any(|((k, _), (name, _))| k != name)
        {
            self.keys = inputs.iter().map(|(k, _)| (k.to_string(), Value::str(k))).collect();
        }
        // as the app passes them: an input of -0.0 as +0.0 (sandbox.py's
        // `float(x or 0.0)`), a NaN as it is
        let ins = Value::dict(
            self.keys
                .iter()
                .zip(inputs)
                .map(|((_, k), (_, v))| (k.clone(), Value::Float(if *v == 0.0 { 0.0 } else { *v })))
                .collect(),
        )
        .map_err(fail)?;
        // and a fresh shallow copy of the parameters every call
        // (sandbox_worker.py's `dict(script.params)`): a key a call sets is
        // gone at the next, a table changed in place stays changed
        let Value::Dict(params) = &self.params else { unreachable!("the parameters' dict") };
        let params = Value::dict_copy(params).map_err(fail)?;
        let args = vec![Value::Float(t), Value::Float(dt), ins, self.state.clone(), params];
        let out = self.machine.call_top(&self.step, args).map_err(fail)?;
        // reading the outputs is part of the call: timed, and the values
        // it leaves held within the memory allowed
        let entries = match out {
            Value::None => return value::memory_check().map(|_| vec![]).map_err(fail),
            Value::Dict(d) => {
                let d = d.borrow();
                if d.entries.len() > MAX_FRAME_FIELD {
                    return Err(fail(Fault::Outside(format!(
                        "more than {MAX_FRAME_FIELD} outputs (the app's worker cannot send them)"
                    ))));
                }
                // the outputs are in the returned dict's order: the
                // parameters' (or a copy's) is the importer's
                if d.unordered && d.entries.len() > 1 {
                    return Err(fail(Fault::Outside(
                        "outputs in the parameters' order (Python keeps the project's, the import \
                         does not)"
                            .into(),
                    )));
                }
                value::charge(d.entries.len() as u64).map_err(fail)?;
                d.entries.clone()
            }
            other => {
                return Err(ScriptError::Failed(format!(
                    "Script '{label}' must return a dict of output values, got {}.",
                    other.type_name()
                )));
            }
        };
        let mut res = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            let Value::Str(key) = &k else {
                return Err(ScriptError::Unsupported(format!(
                    "Script '{label}': an output key that is not a string is outside what runs \
                     without Python"
                )));
            };
            value::charge(key.len() as u64).map_err(fail)?;
            if key.len() > MAX_FRAME_FIELD {
                return Err(fail(Fault::Outside(format!(
                    "an output key of more than {MAX_FRAME_FIELD} bytes (the app's worker cannot send it)"
                ))));
            }
            match value::to_float(&v) {
                Ok(x) => res.push((key.as_str().to_string(), x)),
                Err(f @ (Fault::Outside(_) | Fault::Limit)) => return Err(fail(f)),
                Err(_) => {
                    let shown = v.repr().map_err(fail)?;
                    return Err(ScriptError::Failed(format!(
                        "Script '{label}' output '{}' is not numeric ({shown}).",
                        key.as_str()
                    )));
                }
            }
        }
        value::memory_check().map_err(fail)?;
        Ok(res)
    }
}

/// A Script block run here, as a sampled block of the run loop, as the
/// app runs one (and the golden comparison's host): `step` runs at the
/// start and then every period, with `dt` the period; an output the
/// script does not return holds its value.
pub struct ScriptBlock {
    runner: Runner,
    name: String,
    period: f64,
    inputs: Vec<String>,
    outputs: Vec<String>,
    outside: Arc<AtomicBool>,
}

impl ScriptBlock {
    /// A block for a Script part: its code, label, period, the names of its
    /// inputs and outputs in the run loop's order, and its parameters.
    pub fn new(
        code: &str,
        label: &str,
        period: f64,
        inputs: Vec<String>,
        outputs: Vec<String>,
        params: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<ScriptBlock, ScriptError> {
        Ok(ScriptBlock {
            runner: Runner::start(code, label, params)?,
            name: label.to_string(),
            period,
            inputs,
            outputs,
            outside: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Set when a call stopped because the script went outside the subset:
    /// the run failed, and the case must run again on Python.
    pub fn outside_flag(&self) -> Arc<AtomicBool> {
        self.outside.clone()
    }

    fn call(&mut self, t: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        let ins: Vec<(&str, f64)> =
            self.inputs.iter().map(String::as_str).zip(inputs.iter().copied()).collect();
        match self.runner.step(t, self.period, &ins) {
            Ok(res) => {
                // each of the block's outputs by its key (among many, through
                // a map: a call's cost is not the outputs times the returned)
                let mut by_key = std::collections::HashMap::new();
                if res.len() > 8 {
                    by_key = res.iter().map(|(k, v)| (k.as_str(), *v)).collect();
                }
                for (k, o) in self.outputs.iter().zip(outputs.iter_mut()) {
                    let v = if by_key.is_empty() {
                        res.iter().find(|(rk, _)| rk == k).map(|(_, v)| *v)
                    } else {
                        by_key.get(k.as_str()).copied()
                    };
                    if let Some(v) = v {
                        *o = v;
                    }
                }
                Ok(())
            }
            Err(ScriptError::Unsupported(m)) => {
                self.outside.store(true, Ordering::SeqCst);
                Err(m)
            }
            Err(ScriptError::Failed(m)) => Err(m),
        }
    }
}

impl lsim_ir::runtime::DiscreteBlock for ScriptBlock {
    fn name(&self) -> &str {
        &self.name
    }
    fn period(&self) -> f64 {
        self.period
    }
    // today runs a Script at the start and then every period
    fn offset(&self) -> f64 {
        self.period
    }
    fn init(&mut self, t0: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.call(t0, inputs, outputs)
    }
    fn tick(&mut self, t: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.call(t, inputs, outputs)
    }
}

#[cfg(test)]
mod tests {
    use super::{Runner, ScriptError, fmt_g};

    #[test]
    fn an_endless_loop_stops_at_the_time_limit() {
        let code = "def step(t, dt, inputs, state, params):\n    while True:\n        pass\n";
        let mut r = Runner::start(code, "Loop", &Default::default()).unwrap();
        let started = std::time::Instant::now();
        let e = r.step(1.5, 0.01, &[]).unwrap_err();
        let s = started.elapsed().as_secs_f64();
        assert!((1.9..3.0).contains(&s), "{s} s");
        let ScriptError::Unsupported(m) = e else { panic!("{e:?}") };
        assert!(m.contains("took longer than 2 s at t = 1.5 s"), "{m}");
    }

    #[test]
    fn what_is_outside_the_subset_is_told() {
        let deep = "def f(n):\n    return f(n + 1)\ndef step(t, dt, inputs, state, params):\n    return {'y': f(0)}\n";
        let mut r = Runner::start(deep, "Deep", &Default::default()).unwrap();
        assert!(matches!(r.step(0.0, 0.01, &[]), Err(ScriptError::Unsupported(_))));
        let big = "def step(t, dt, inputs, state, params):\n    return {'y': 2 ** 70}\n";
        let mut r = Runner::start(big, "Big", &Default::default()).unwrap();
        let e = r.step(0.0, 0.01, &[]).unwrap_err();
        assert!(matches!(&e, ScriptError::Unsupported(m) if m.contains("line 2")), "{e:?}");
        for code in [
            "import os\ndef step(t, dt, inputs, state, params):\n    pass\n",
            "def step(t, dt, inputs, state, params):\n    return {'y': math.sin(t)}\n",
            "def step(t, dt, inputs, state, params):\n    return {'y': t ** 2.0}\n",
            "def step(t, dt, inputs, state, params):\n    return {'y': open('x')}\n",
            "x = 1\n",
        ] {
            let r = Runner::start(code, "S", &Default::default());
            match code {
                c if c.contains("**") => {
                    let mut r = r.unwrap();
                    assert!(matches!(r.step(1.0, 0.01, &[]), Err(ScriptError::Unsupported(_))));
                }
                _ => assert!(matches!(r, Err(ScriptError::Unsupported(_))), "{code}"),
            }
        }
    }

    #[test]
    fn a_block_takes_its_outputs_by_their_keys() {
        use lsim_ir::runtime::DiscreteBlock;
        for n in [3, 40] {
            let more: Vec<String> = (0..n).map(|k| format!("'k{k}': {k}")).collect();
            let code = format!(
                "def step(t, dt, inputs, state, params):\n    return {{'b': inputs['a'] * 2, {}}}\n",
                more.join(", ")
            );
            let outputs: Vec<String> = ["k2", "b", "missing"].map(String::from).to_vec();
            let mut b = super::ScriptBlock::new(
                &code,
                "Block",
                0.01,
                vec!["a".into()],
                outputs,
                &Default::default(),
            )
            .unwrap();
            let mut out = [9.0; 3];
            b.init(0.0, &[1.5], &mut out).unwrap();
            assert_eq!(out, [2.0, 3.0, 9.0], "{n} outputs returned");
        }
    }

    #[test]
    fn times_are_formatted_as_python_formats_them() {
        for (x, s) in [
            (0.0, "0"),
            (12.34, "12.34"),
            (0.01, "0.01"),
            (1e-5, "1e-05"),
            (123456.0, "123456"),
            (1234567.0, "1.23457e+06"),
            (57.000000001, "57"),
            (0.1 + 0.2, "0.3"),
            (-2.5, "-2.5"),
        ] {
            assert_eq!(fmt_g(x), s, "{x}");
        }
    }
}
