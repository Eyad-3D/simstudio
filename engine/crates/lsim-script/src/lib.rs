//! # lsim-script: Script blocks without the Python interpreter
//!
//! A Script block's code (`def step(t, dt, inputs, state, params)`,
//! returning a dict of outputs; backend/app/solver/scripting.py) is run
//! here by an interpreter of a strict subset of Python whose every
//! operation has CPython's semantics to the bit: integers, IEEE floats
//! with CPython's floor division and modulo, exact comparisons between
//! integers and floats, dicts, lists, tuples, the builtins scripts use,
//! `clamp`, and `interp` as app/solver/maps.py computes it. The example
//! cars' controllers (the P2 hybrid's Hybrid Control Unit) are in the
//! subset, so they run without the interpreter in the loop and give the
//! same numbers as the Python runner.
//!
//! **What is left to Python.** A script is refused when it is first read
//! ([`check`]: [`Unsupported`], with the line and what it is) if it uses
//! anything outside the subset: other imports, the math module's
//! transcendental functions (CPython calls the C runtime's, whose last
//! bit differs between platforms; these would need the engine's own
//! correctly rounded library and so would not match the Python runner),
//! powers of floats (the C runtime's `pow`), string formatting, classes,
//! exceptions, comprehensions, slices, closures, and any name that is not
//! the script's own or one of the builtins here. While it runs, an
//! operation Python supports but the subset does not (an integer beyond
//! 64 bits, a method of a string) stops it with [`Fault::Outside`], so the
//! caller can run the case again on Python: a result never differs from
//! the Python runner's. An exception Python would raise (division by zero,
//! a missing key) is [`Fault::Raise`] with Python's message.
//!
//! **Limits.** As the Python runner: a call that does not return within
//! two seconds stops the run (the clock is read every 1024 statements,
//! loop rounds and calls). Beyond it: containers of at most a million
//! items and calls nested at most 50 deep are outside the subset (the
//! Python runner may go on). The interpreter reaches nothing outside the
//! script: no files, processes, network or the host's memory.

mod ast;
mod interp;
mod lexer;
mod parser;
mod value;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use value::{Builtin, Value};

/// The most items a list, tuple, dict or string may hold here.
pub const MAX_ITEMS: usize = 1_000_000;

/// The wall-clock budget of one call into the script, s (the Python
/// runner's).
pub const TIME_LIMIT_S: f64 = 2.0;

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

/// A JSON value as Python's `json.loads` gives it.
fn from_json(v: &serde_json::Value) -> Result<Value, Fault> {
    Ok(match v {
        serde_json::Value::Null => Value::None,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if n.is_u64() {
                return Err(Fault::Outside("an integer parameter beyond 64 bits".into()));
            } else {
                let x = n.as_f64().ok_or_else(|| Fault::Outside("a number".into()))?;
                // an integer written beyond 64 bits arrives here as a float
                // (Python reads it as an integer)
                if x.fract() == 0.0 && x.abs() >= 9_007_199_254_740_992.0 {
                    return Err(Fault::Outside("a whole-number parameter beyond 2**53".into()));
                }
                Value::Float(x)
            }
        }
        serde_json::Value::String(s) => Value::str(s),
        serde_json::Value::Array(a) => {
            Value::list(a.iter().map(from_json).collect::<Result<Vec<_>, _>>()?)
        }
        serde_json::Value::Object(o) => {
            let d = Value::dict(
                o.iter()
                    .map(|(k, v)| Ok((Value::str(k), from_json(v)?)))
                    .collect::<Result<Vec<_>, Fault>>()?,
            )?;
            // the importer's map is sorted, Python keeps the project's order
            if let Value::Dict(cell) = &d {
                cell.borrow_mut().unordered = true;
            }
            d
        }
    })
}

/// A running script: `step` and its `state` and `params` between calls.
pub struct Runner {
    module: ast::Module,
    machine: interp::Machine,
    step: Value,
    state: Value,
    params: Value,
    label: String,
}

// SAFETY: a Runner owns every reference-counted value it can reach: it
// parses its own copy of the script (`Runner::start` takes the source
// text, not a shared tree), creates every value while it runs, and hands
// out only plain numbers and strings (`Runner::step` returns owned
// `String`s and `f64`s; errors are owned `String`s). No `Rc` is ever
// shared with anything outside it, so moving the whole Runner to another
// thread moves every count with it, and nothing on the old thread can
// touch them.
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
    /// Reads `code` and runs its top-level code, as the Python runner does
    /// at the start of a run; `params` is the block's parameters (a JSON
    /// object, as Python's `json.loads` reads it).
    pub fn start(
        code: &str,
        label: &str,
        params: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Runner, ScriptError> {
        let module = parser::parse(code)
            .map_err(|u| ScriptError::Unsupported(format!("Script '{label}', {u}")))?;
        names_check(&module)
            .map_err(|u| ScriptError::Unsupported(format!("Script '{label}': {}", u.what)))?;
        let mut machine = interp::Machine::new(&module, TIME_LIMIT_S);
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
            std::time::Instant::now() + std::time::Duration::from_secs_f64(TIME_LIMIT_S),
        ));
        let params = from_json(&serde_json::Value::Object(params.clone()))
            .map_err(|f| failed(label, None, f))?;
        value::start_meter(None);
        Ok(Runner {
            module,
            machine,
            step,
            state: Value::dict(vec![]).expect("an empty dict"),
            params,
            label: label.to_string(),
        })
    }

    /// One call `step(t, dt, inputs, state, params)`: the outputs it
    /// returns, as the Python runner converts them (`float(value)` under
    /// the key's text), in the order of the returned dict.
    pub fn step(
        &mut self,
        t: f64,
        dt: f64,
        inputs: &[(&str, f64)],
    ) -> Result<Vec<(String, f64)>, ScriptError> {
        let _ = &self.module;
        // no deadline left over from another call (the call sets its own)
        value::start_meter(None);
        let label = self.label.clone();
        let fail = |f| failed(&label, Some(t), f);
        let ins =
            Value::dict(inputs.iter().map(|(k, v)| (Value::str(k), Value::Float(*v))).collect())
                .map_err(fail)?;
        let args =
            vec![Value::Float(t), Value::Float(dt), ins, self.state.clone(), self.params.clone()];
        let out = self.machine.call_top(&self.step, args).map_err(fail)?;
        let entries = match out {
            Value::None => return Ok(vec![]),
            Value::Dict(d) => d.borrow().entries.clone(),
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
            match value::to_float(&v) {
                Ok(x) => res.push((key.to_string(), x)),
                Err(f @ (Fault::Outside(_) | Fault::Limit)) => return Err(fail(f)),
                Err(_) => {
                    return Err(ScriptError::Failed(format!(
                        "Script '{label}' output '{key}' is not numeric ({}).",
                        v.repr()
                    )));
                }
            }
        }
        Ok(res)
    }
}

/// A Script block run here, as a sampled block of the run loop: today's
/// semantics (backend/app/solver/scripting.py, and the golden comparison's
/// host): `step` runs at the start and then every period, with `dt` the
/// period; an output the script does not return holds its value.
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
                for (k, o) in self.outputs.iter().zip(outputs.iter_mut()) {
                    if let Some((_, v)) = res.iter().find(|(rk, _)| rk == k) {
                        *o = *v;
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
