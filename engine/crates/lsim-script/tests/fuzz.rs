//! Differential fuzzing: random whole scripts, run on the native runner and
//! on the app's own Script runner (CPython 3.11, as the app's worker calls
//! it: `tests/parity.py`), call by call. Each script has module constants,
//! tables, a module list, helper functions (defaults, keywords, `global`),
//! and a `step()` of assignments, `if`/`elif`/`else`, `for` over ranges,
//! lists, tuples, `enumerate`, `zip` and dicts, bounded `while` loops,
//! `break` and `continue`, early returns, list and dict methods and the
//! block's `state` kept across calls; its expressions use every builtin and
//! math function of the subset, the scripts' `clamp` and `interp`, and now
//! and then a value of the wrong type, so Python's errors are met too. The
//! inputs take the special values (signed zeros, subnormals, the largest
//! floats, infinities, NaN).
//!
//! A divergence is anything but: the same 64 bits of every output (two
//! NaNs equal) under the same names in the same order, the same message
//! where Python raises, both refusing a script, or the native runner
//! leaving a script or a call to Python (which then gives Python's result
//! by construction).
//!
//! By default a short run (a guard: a few hundred scripts). For the long
//! runs of the Script decision record (docs/SCRIPT-RUNNER-DECISION.md):
//!
//!   LSIM_FUZZ_SECONDS=3600 LSIM_FUZZ_SEED=7 LSIM_FUZZ_OUT=dir \
//!     cargo test -p lsim-script --release --test fuzz -- --nocapture
//!
//! writes `dir/summary.json` after each batch (seconds, scripts, calls,
//! the tally, divergences by kind) and the first divergences found, each
//! with its script and call, to `dir/divergences.jsonl`.

mod common;

use common::*;
use lsim_script::{Runner, ScriptError};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

/// The names an expression may read, where it is.
struct Scope {
    nums: Vec<&'static str>,
    lists: Vec<&'static str>,
    dicts: Vec<&'static str>,
    /// helper functions it may call (h0 .. h{helpers-1})
    helpers: usize,
    /// `from math import floor` and `sqrt` were written
    from_math: bool,
    /// in step(), where t, dt, inputs, state and params are
    in_step: bool,
}

const INTS: [&str; 13] = [
    "0",
    "1",
    "2",
    "3",
    "-1",
    "-7",
    "10",
    "255",
    "9007199254740993",
    "1000000000000000000",
    "9223372036854775807",
    "(-9223372036854775807 - 1)",
    "True",
];

fn lit_f(x: f64) -> String {
    if x.is_nan() {
        "float('nan')".into()
    } else if x == f64::INFINITY {
        "float('inf')".into()
    } else if x == f64::NEG_INFINITY {
        "-float('inf')".into()
    } else {
        format!("{x:?}")
    }
}

fn lit(r: &mut Rng) -> String {
    match r.below(7) {
        0 | 1 => INTS[r.below(INTS.len())].into(),
        2 | 3 => lit_f(SPECIAL[r.below(SPECIAL.len())]),
        // a large float with a fraction: often exactly halfway between its
        // two shortest readings
        4 => lit_f(((r.range(1e14, 9e15)) * 8.0).round() / 8.0),
        _ => lit_f((r.range(-50.0, 50.0) * 8.0).round() / 8.0),
    }
}

fn pick<'a>(r: &mut Rng, v: &[&'a str]) -> &'a str {
    v[r.below(v.len())]
}

/// A value of another type than a number, now and then.
const WILD: [&str; 9] = ["S", "None", "TP", "'x'", "LG", "[]", "{}", "(1, 2)", "''"];

/// An index into a list: mostly one its items have (lists hold one to
/// eight), past them now and then.
fn index(r: &mut Rng) -> i64 {
    if r.below(5) == 0 { r.below(9) as i64 - 4 } else { [0, -1][r.below(2)] }
}

/// `e`, which reads list `l`, mostly only where `l` holds something (a
/// call that raises stops there: most calls should run to their end).
fn guarded(r: &mut Rng, l: &str, e: String) -> String {
    if r.below(5) == 0 { e } else { format!("({e} if {l} else 0.0)") }
}

fn leaf(r: &mut Rng, s: &Scope) -> String {
    match r.below(24) {
        0..=5 => pick(r, &s.nums).into(),
        6..=7 | 11 | 12 | 16 if !s.in_step => pick(r, &s.nums).into(),
        6 => format!("inputs['{}']", ["a", "b", "c"][r.below(3)]),
        7 => ["t", "dt", "K", "CNT", "math.pi", "math.e"][r.below(6)].into(),
        8..=10 => lit(r),
        11 => "params['gain']".into(),
        12 => format!("params['tbl']['{}']", ["0", "10", "20"][r.below(3)]),
        13 => format!("len({})", pick(r, &s.lists)),
        14 => {
            let l = pick(r, &s.lists);
            let e = format!("{l}[{}]", index(r));
            guarded(r, l, e)
        }
        15 if !s.dicts.is_empty() => {
            let d = pick(r, &s.dicts);
            match r.below(3) {
                0 => format!("{d}['p']"),
                1 => format!("{d}.get('{}', 0.5)", ["p", "q", "z"][r.below(3)]),
                _ => format!("len({d})"),
            }
        }
        16 => "state.get('s', 0.0)".into(),
        17 => format!("TP[{}]", r.below(7) as i64 - 3),
        18 if r.below(4) == 0 => WILD[r.below(WILD.len())].into(),
        _ => pick(r, &s.nums).into(),
    }
}

fn num(r: &mut Rng, s: &Scope, d: usize) -> String {
    if d == 0 || r.below(3) == 0 {
        return leaf(r, s);
    }
    let d = d - 1;
    match r.below(40) {
        0..=7 => {
            let op = ["+", "-", "*", "/", "//", "%"][r.below(6)];
            let (x, y) = (num(r, s, d), num(r, s, d));
            if matches!(op, "/" | "//" | "%") && r.below(4) > 0 {
                format!("({x} {op} ({y} or 1.5))")
            } else {
                format!("({x} {op} {y})")
            }
        }
        8 => format!("(-{})", num(r, s, d)),
        9 => format!("abs({})", num(r, s, d)),
        10 => format!("min({}, {})", num(r, s, d), num(r, s, d)),
        11 => format!("max({}, {}, {})", num(r, s, d), num(r, s, d), num(r, s, d)),
        12 => {
            let l = pick(r, &s.lists);
            let e = format!("{}({l})", ["max", "min"][r.below(2)]);
            guarded(r, l, e)
        }
        13 => format!("clamp({}, {}, {})", num(r, s, d), num(r, s, d), num(r, s, d)),
        14 => format!("round({})", num(r, s, d)),
        15 => format!("int({})", num(r, s, d)),
        16 => format!("float({})", num(r, s, d)),
        17 => format!("({} if {} else {})", num(r, s, d), cond(r, s, d), num(r, s, d)),
        18 => {
            let f = ["floor", "ceil", "trunc", "sqrt", "fabs", "radians", "degrees"][r.below(7)];
            if s.from_math && (f == "floor" || f == "sqrt") && r.below(2) == 0 {
                format!("{f}({})", num(r, s, d))
            } else {
                format!("math.{f}({})", num(r, s, d))
            }
        }
        19 => format!("math.copysign({}, {})", num(r, s, d), num(r, s, d)),
        20 => format!("interp(T1, {})", num(r, s, d)),
        21 => format!("interp(T2, {}, {})", num(r, s, d), num(r, s, d)),
        22 if s.in_step => format!("interp(params['tbl'], {})", num(r, s, d)),
        23 | 24 if s.helpers > 0 => {
            let h = r.below(s.helpers);
            match r.below(3) {
                0 => format!("h{h}({})", num(r, s, d)),
                1 => format!("h{h}({}, {})", num(r, s, d), num(r, s, d)),
                _ => format!("h{h}({}, y={})", num(r, s, d), num(r, s, d)),
            }
        }
        25 => format!("({} ** {})", ["2", "3", "-2", "10", "7"][r.below(5)], r.below(5)),
        26 => format!("len(range({}, {}, {}))", r.below(5), r.below(30), 1 + r.below(4)),
        27 => {
            let n = 1 + r.below(9);
            format!("range({n})[{}]", r.below(n + n / 4 + 1) as i64 - (n / 2) as i64)
        }
        28 => format!("{}({})", ["any", "all"][r.below(2)], pick(r, &s.lists)),
        29 => {
            let l = pick(r, &s.lists);
            let x = if r.below(2) == 0 { format!("{l}[{}]", index(r)) } else { num(r, s, d) };
            format!("{l}.index({x})")
        }
        30 => format!("{}.count({})", pick(r, &s.lists), num(r, s, d)),
        31 => {
            let f = ["isnan", "isinf", "isfinite"][r.below(3)];
            format!("math.{f}({})", num(r, s, d))
        }
        32 => {
            let n = if r.below(4) == 0 { 9 } else { 7 };
            format!(
                "float('{}')",
                ["1.5", " -2e3 ", "inf", "-nan", "1_000", "7", "1e-320", "0x10", ""][r.below(n)]
            )
        }
        33 => format!("len([{}, {}])", num(r, s, d), num(r, s, d)),
        // containers compared, built and read
        34 => {
            let cmp = ["<", "<=", "==", "!="][r.below(4)];
            let (a, b, c, e) = (num(r, s, d), num(r, s, d), num(r, s, d), num(r, s, d));
            if r.below(2) == 0 {
                format!("(({a}, {b}) {cmp} ({c}, {e}))")
            } else {
                format!("([{a}, {b}] {cmp} [{c}, {e}])")
            }
        }
        35 => format!("dict([(1, 2.5), ({}, 3.0)], p={})[1]", num(r, s, d), num(r, s, d)),
        36 if !s.dicts.is_empty() => {
            let d = pick(r, &s.dicts);
            format!("(list({d}.values()) or [0.0])[{}]", index(r).clamp(-1, 0))
        }
        37 => {
            let (k, v) = (num(r, s, d), num(r, s, d));
            format!("{{{k}: {v}, 1: 2.5, 1.0: -1}}.get({}, 0.25)", num(r, s, d))
        }
        _ => cond(r, s, d),
    }
}

fn cond(r: &mut Rng, s: &Scope, d: usize) -> String {
    let cmp = ["<", "<=", ">", ">=", "==", "!="];
    match r.below(10) {
        0..=3 => format!("({} {} {})", num(r, s, d), cmp[r.below(6)], num(r, s, d)),
        4 => format!(
            "({} {} {} {} {})",
            num(r, s, d),
            cmp[r.below(4)],
            num(r, s, d),
            cmp[r.below(4)],
            num(r, s, d)
        ),
        5 => format!("({} and {})", cond(r, s, d.saturating_sub(1)), num(r, s, d)),
        6 => format!("({} or {})", num(r, s, d), cond(r, s, d.saturating_sub(1))),
        7 => format!("(not {})", num(r, s, d)),
        8 => format!("({} in {})", num(r, s, d), pick(r, &s.lists)),
        _ if s.in_step => format!("('{}' in state)", ["s", "h", "n"][r.below(3)]),
        _ => format!("({} in TP)", num(r, s, d)),
    }
}

/// One statement (and the ones it holds) at `ind` spaces.
fn stmt(
    r: &mut Rng,
    s: &Scope,
    ind: usize,
    depth: usize,
    in_loop: bool,
    out: &mut Vec<String>,
    n: &mut usize,
) {
    let p = " ".repeat(ind);
    let e = |r: &mut Rng| num(r, s, 3);
    let k = r.below(if depth > 0 { 30 } else { 18 });
    match k {
        0..=4 => {
            let v = pick(r, &s.nums);
            out.push(format!("{p}{v} = {}", e(r)));
        }
        5 | 6 => {
            let v = pick(r, &s.nums);
            let op = ["+=", "-=", "*=", "/=", "//=", "%="][r.below(6)];
            out.push(format!("{p}{v} {op} {}", e(r)));
        }
        7 => {
            let l = pick(r, &s.lists);
            if l != "TP" {
                out.push(format!("{p}if len({l}) < 8:"));
                out.push(format!("{p}    {l}.append({})", e(r)));
            }
        }
        8 => {
            let l = pick(r, &s.lists);
            if l != "TP" {
                let m = match r.below(5) {
                    0 => "pop()".to_string(),
                    1 => format!("pop({})", index(r)),
                    2 => format!("insert({}, {})", r.below(5) as i64 - 2, e(r)),
                    3 => format!("remove({l}[{}])", index(r)),
                    _ => format!("extend([{}])", e(r)),
                };
                if m.starts_with("extend") || r.below(5) == 0 {
                    out.push(format!("{p}{l}.{m}"));
                } else {
                    out.push(format!("{p}if {l}:"));
                    out.push(format!("{p}    {l}.{m}"));
                }
            }
        }
        9 => {
            let l = pick(r, &s.lists);
            if l != "TP" || r.below(8) == 0 {
                out.push(format!("{p}{l}[{}] = {}", index(r), e(r)));
            }
        }
        10 if !s.dicts.is_empty() => {
            let d = pick(r, &s.dicts);
            let key = ["p", "q", "z"][r.below(3)];
            out.push(match r.below(4) {
                0 => format!("{p}{d}['{key}'] = {}", e(r)),
                1 => format!("{p}{d}.setdefault('{key}', {})", e(r)),
                2 => format!("{p}{d}.update({key}={})", e(r)),
                _ => format!("{p}{d}.pop('{key}', None)"),
            });
        }
        11 if s.in_step => out.push(format!("{p}state['s'] = state.get('s', 0.0) + {}", e(r))),
        12 if s.lists.contains(&"H") => {
            out.push(format!("{p}H.append({})", e(r)));
            out.push(format!("{p}if len(H) > 6:"));
            out.push(format!("{p}    H.pop(0)"));
        }
        13 => {
            let (a, b) = (pick(r, &s.nums), pick(r, &s.nums));
            out.push(format!("{p}{a}, {b} = {b}, {a}"));
        }
        14 if r.below(4) == 0 => out.push(format!("{p}print({}, 'x')", e(r))),
        15 if in_loop => out.push(format!("{p}{}", ["break", "continue"][r.below(2)])),
        16 if s.dicts.contains(&"D") && r.below(3) == 0 => {
            out.push(format!("{p}return {{'y0': {}}}", e(r)));
        }
        17 if s.helpers > 0 && r.below(3) == 0 => {
            let v = pick(r, &s.nums);
            out.push(format!("{p}{v} = h{}({})", r.below(s.helpers), e(r)));
        }
        18..=20 => {
            out.push(format!("{p}if {}:", cond(r, s, 3)));
            block(r, s, ind + 4, depth - 1, in_loop, out, n);
            if r.below(2) == 0 {
                out.push(format!("{p}elif {}:", cond(r, s, 2)));
                block(r, s, ind + 4, depth - 1, in_loop, out, n);
            }
            if r.below(2) == 0 {
                out.push(format!("{p}else:"));
                block(r, s, ind + 4, depth - 1, in_loop, out, n);
            }
        }
        21..=25 => {
            let v = pick(r, &s.nums);
            let l = pick(r, &s.lists);
            let head = match r.below(6) {
                0 => format!("for {v} in range({}):", r.below(6)),
                1 => format!("for {v} in list({l}):"),
                2 => {
                    let w = pick(r, &s.nums);
                    format!("for {w}, {v} in enumerate(list({l})):")
                }
                3 => {
                    let w = pick(r, &s.nums);
                    format!("for {w}, {v} in zip(list({l}), TP):")
                }
                4 if !s.dicts.is_empty() => {
                    let d = pick(r, &s.dicts);
                    format!("for kk, {v} in list({d}.items()):")
                }
                _ => format!("for {v} in range({}, {}):", r.below(3) as i64 - 1, r.below(6)),
            };
            out.push(format!("{p}{head}"));
            block(r, s, ind + 4, depth - 1, true, out, n);
            if r.below(5) == 0 {
                out.push(format!("{p}else:"));
                block(r, s, ind + 4, depth - 1, in_loop, out, n);
            }
        }
        26..=27 => {
            *n += 1;
            let c = format!("n{n}");
            out.push(format!("{p}{c} = 0"));
            out.push(format!("{p}while {c} < {} and {}:", 1 + r.below(5), cond(r, s, 2)));
            out.push(format!("{p}    {c} += 1"));
            block(r, s, ind + 4, depth - 1, true, out, n);
        }
        _ => {
            let v = pick(r, &s.nums);
            out.push(format!("{p}{v} = {}", e(r)));
        }
    }
}

fn block(
    r: &mut Rng,
    s: &Scope,
    ind: usize,
    depth: usize,
    in_loop: bool,
    out: &mut Vec<String>,
    n: &mut usize,
) {
    let before = out.len();
    for _ in 0..1 + r.below(3) {
        stmt(r, s, ind, depth, in_loop, out, n);
    }
    if out.len() == before {
        out.push(format!("{}pass", " ".repeat(ind)));
    }
}

fn table1(r: &mut Rng) -> String {
    let mut keys: Vec<f64> =
        (0..2 + r.below(4)).map(|_| (r.range(-5.0, 30.0) * 2.0).round() / 2.0).collect();
    keys.sort_by(f64::total_cmp);
    keys.dedup();
    let pairs: Vec<String> = keys
        .iter()
        .map(|k| {
            let key = match r.below(3) {
                0 if k.fract() == 0.0 => format!("{}", *k as i64),
                1 => format!("'{k}'"),
                _ => format!("{k:?}"),
            };
            format!("{key}: {}", lit_f((r.range(-10.0, 10.0) * 4.0).round() / 4.0))
        })
        .collect();
    format!("{{{}}}", pairs.join(", "))
}

/// A whole random script.
fn script(r: &mut Rng) -> String {
    let mut out = vec!["import math".to_string()];
    let from_math = r.below(3) == 0;
    if from_math {
        out.push("from math import floor".into());
        out.push("from math import sqrt".into());
    }
    out.push(format!("K = {}", lit(r)));
    out.push("CNT = 0".into());
    out.push("S = 'ab'".into());
    out.push(format!("TP = (1.0, {}, 3)", lit(r)));
    out.push(format!("LG = [{}, 2.0]", lit(r)));
    out.push(format!("T1 = {}", table1(r)));
    out.push(format!("T2 = {{0: {}, 5.5: {}}}", table1(r), table1(r)));
    let helpers = r.below(3);
    let mut n = 0;
    for h in 0..helpers {
        let s = Scope {
            nums: vec!["x", "y", "w0", "w1"],
            lists: vec!["LG", "TP"],
            dicts: vec![],
            helpers: h,
            from_math,
            in_step: false,
        };
        out.push(format!("def h{h}(x, y={}):", lit(r)));
        if r.below(2) == 0 {
            out.push("    global CNT".into());
            out.push("    CNT += 1".into());
        }
        out.push("    w0 = x * 0.5".into());
        out.push("    w1 = 0.0".into());
        block(r, &s, 4, 2, false, &mut out, &mut n);
        out.push(format!("    return {}", num(r, &s, 3)));
    }
    let s = Scope {
        nums: vec!["v0", "v1", "v2", "v3", "v4"],
        lists: vec!["L", "H", "LG", "TP"],
        dicts: vec!["D"],
        helpers,
        from_math,
        in_step: true,
    };
    out.push("def step(t, dt, inputs, state, params):".into());
    out.push("    v0 = inputs['a']".into());
    out.push("    v1 = inputs['b']".into());
    out.push("    v2 = inputs['c']".into());
    out.push("    v3 = 0.0".into());
    out.push(format!("    v4 = {}", lit(r)));
    out.push(format!("    L = [{}, {}, v0]", lit(r), lit(r)));
    out.push(format!("    D = {{'p': 1.0, 'q': {}}}", lit(r)));
    out.push("    H = state.setdefault('h', [])".into());
    for _ in 0..2 + r.below(6) {
        stmt(r, &s, 4, 3, false, &mut out, &mut n);
    }
    let outs: Vec<String> =
        (0..1 + r.below(3)).map(|k| format!("'y{k}': {}", num(r, &s, 4))).collect();
    out.push(format!("    return {{{}}}", outs.join(", ")));
    out.join("\n") + "\n"
}

fn params(r: &mut Rng) -> String {
    format!(
        r#"{{"gain": {}, "tbl": {{"0": {}, "10": {}, "20": {}}}}}"#,
        [0.5, 2.0, -1.25, 123.456, 0.0][r.below(5)],
        r.below(9) as i64 - 4,
        (r.range(-10.0, 10.0) * 4.0).round() / 4.0,
        ["1", "2.5", "-3", "1e-300"][r.below(4)]
    )
}

fn calls(r: &mut Rng, n: usize) -> Vec<Call> {
    (0..n)
        .map(|k| Call {
            t: k as f64 * 0.01,
            dt: 0.01,
            inputs: ["a", "b", "c"].iter().map(|nm| (nm.to_string(), special(r))).collect(),
        })
        .collect()
}

#[derive(Default, Debug)]
struct Tally {
    scripts: usize,
    calls: usize,
    equal: usize,
    both_failed: usize,
    both_refused: usize,
    left_start: usize,
    left_calls: usize,
    python_crashed: usize,
    /// scripts Python did not finish within the worker's time limit (a
    /// call or the start): the app's run fails on them
    python_timed_out: usize,
    /// why scripts and calls were left to Python (the reason, its numbers
    /// taken out)
    left: BTreeMap<String, usize>,
    /// the messages both runners raised (numbers taken out)
    raised: BTreeMap<String, usize>,
}

/// A message with its numbers and the script's label taken out.
fn shape(m: &str) -> String {
    let m = m
        .strip_prefix("Script '")
        .and_then(|m| m.split_once("': ").or_else(|| m.split_once("' ")))
        .map_or(m, |(_, b)| b);
    let m = m.split_once(" s: ").map_or(m, |(_, b)| b);
    m.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).take(90).collect()
}

impl Tally {
    fn leave(&mut self, why: &str) {
        *self.left.entry(shape(why)).or_default() += 1;
    }
}

/// One script's calls on both runners: its divergences (kind, call, what each gave).
fn diff(case: &Case, py: &Value, tally: &mut Tally) -> Vec<(String, Value)> {
    let mut found = vec![];
    let params: serde_json::Map<String, Value> =
        serde_json::from_str(&case.params_text).expect("an object");
    let mine =
        match catch_unwind(AssertUnwindSafe(|| Runner::start(&case.code, &case.label, &params))) {
            Ok(m) => m,
            Err(_) => return vec![("native-panic-start".into(), json!({}))],
        };
    if let Some(e) = py.get("compile_error") {
        match mine {
            Err(_) => tally.both_refused += 1,
            Ok(_) => found.push(("python-refused-native-ran".into(), json!({"python": e}))),
        }
        return found;
    }
    let mut runner = match mine {
        Ok(r) => r,
        Err(ScriptError::Unsupported(m)) => {
            tally.left_start += 1;
            tally.leave(&m);
            return found;
        }
        Err(ScriptError::Failed(m)) => {
            return vec![("native-refused-python-ran".into(), json!({"native": m}))];
        }
    };
    let results = py["results"].as_array().expect("results");
    for (k, (call, want)) in case.calls.iter().zip(results).enumerate() {
        tally.calls += 1;
        let ins: Vec<(&str, f64)> = call.inputs.iter().map(|(n, v)| (n.as_str(), *v)).collect();
        let got = match catch_unwind(AssertUnwindSafe(|| runner.step(call.t, call.dt, &ins))) {
            Ok(g) => g,
            Err(_) => {
                found.push(("native-panic-call".into(), json!({"call": k})));
                break;
            }
        };
        let at = |kind: &str, native: Value, python: &Value| {
            (
                kind.to_string(),
                json!({"call": k, "inputs": call.inputs.iter().map(|(n, v)| json!([n, format!("{v:e}")])).collect::<Vec<_>>(),
                       "native": native, "python": python}),
            )
        };
        match (got, want.get("ok"), want.get("err")) {
            (Ok(res), Some(ok), None) => {
                let ok = ok.as_array().expect("pairs");
                let same = res.len() == ok.len()
                    && res.iter().zip(ok).all(|((name, v), pair)| {
                        let pb = pair[1].as_u64().expect("bits");
                        name == pair[0].as_str().expect("a name")
                            && (v.to_bits() == pb || (v.is_nan() && f64::from_bits(pb).is_nan()))
                    });
                if same {
                    tally.equal += 1;
                } else {
                    let mine: Vec<Value> = res
                        .iter()
                        .map(|(n, v)| json!([n, format!("{v:e}"), v.to_bits()]))
                        .collect();
                    let theirs: Vec<Value> = ok
                        .iter()
                        .map(|p| {
                            let b = p[1].as_u64().unwrap();
                            json!([p[0], format!("{:e}", f64::from_bits(b)), b])
                        })
                        .collect();
                    found.push(at("outputs", json!(mine), &json!(theirs)));
                }
            }
            (Err(ScriptError::Failed(m)), None, Some(e)) => {
                if m == e.as_str().unwrap_or_default() {
                    tally.both_failed += 1;
                    *tally.raised.entry(shape(&m)).or_default() += 1;
                } else {
                    found.push(at("message", json!(m), e));
                }
            }
            (Err(ScriptError::Unsupported(m)), _, _) => {
                tally.left_calls += 1;
                tally.leave(&m);
                break; // a real run goes on Python from the start
            }
            (Ok(res), None, Some(e)) => {
                found.push(at("python-raised-native-ran", json!(format!("{res:?}")), e));
            }
            (Err(ScriptError::Failed(m)), Some(ok), _) => {
                found.push(at("native-raised-python-ran", json!(m), ok))
            }
            (other, _, _) => found.push(at("unexpected", json!(format!("{other:?}")), want)),
        }
    }
    found
}

/// The app's worker's time limit for one call (the engine stops it past
/// it): Python's runs are held to it too.
const CALL_LIMIT_S: f64 = lsim_script::TIME_LIMIT_S;

/// What the native runner does with a script Python could not finish:
/// each call's outcome in a word, the time it took.
fn native_alone(case: &Case) -> Value {
    let started = std::time::Instant::now();
    let params: serde_json::Map<String, Value> =
        serde_json::from_str(&case.params_text).expect("an object");
    let mut calls = vec![];
    match catch_unwind(AssertUnwindSafe(|| Runner::start(&case.code, &case.label, &params))) {
        Err(_) => calls.push("panic".to_string()),
        Ok(Err(e)) => calls.push(format!("start: {e}")),
        Ok(Ok(mut r)) => {
            for call in &case.calls {
                let ins: Vec<(&str, f64)> =
                    call.inputs.iter().map(|(n, v)| (n.as_str(), *v)).collect();
                let got = catch_unwind(AssertUnwindSafe(|| r.step(call.t, call.dt, &ins)));
                calls.push(match got {
                    Ok(Ok(_)) => "ok".into(),
                    Ok(Err(ScriptError::Failed(m))) => format!("raised: {m}"),
                    Ok(Err(ScriptError::Unsupported(m))) => format!("left: {m}"),
                    Err(_) => "panic".into(),
                });
                if !calls.last().is_some_and(|c| c == "ok" || c.starts_with("raised")) {
                    break;
                }
            }
        }
    }
    json!({"calls": calls, "seconds": started.elapsed().as_secs_f64()})
}

fn env_num(name: &str, default: f64) -> f64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[test]
fn the_native_runner_against_python_on_random_scripts() {
    let seconds = env_num("LSIM_FUZZ_SECONDS", 0.0);
    let seed = env_num("LSIM_FUZZ_SEED", 1.0) as u64;
    let out = std::env::var("LSIM_FUZZ_OUT").ok().map(PathBuf::from);
    if let Some(o) = &out {
        std::fs::create_dir_all(o).expect("the output folder");
    }
    let mut r = Rng(0x9e37_79b9_7f4a_7c15 ^ seed.wrapping_mul(0x1000_0000_01b3));
    let started = std::time::Instant::now();
    let (batch, guard_batches) = (100, 3);
    let mut tally = Tally::default();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut first: Vec<Value> = vec![];
    let mut batches = 0usize;
    loop {
        let cases: Vec<Case> = (0..batch)
            .map(|k| Case {
                label: format!("fuzz {seed}:{}", batches * batch + k),
                code: script(&mut r),
                params_text: params(&mut r),
                calls: calls(&mut r, 12),
                may_leave: true,
            })
            .collect();
        // one Python for the batch; should it stop (a call past the time
        // limit, a crash), one for each script, those it cannot run apart
        let py: Vec<Result<Value, String>> = match try_on_python(&cases, Some(CALL_LIMIT_S)) {
            Ok(v) => v.into_iter().map(Ok).collect(),
            Err(_) => cases
                .iter()
                .map(|c| {
                    try_on_python(std::slice::from_ref(c), Some(CALL_LIMIT_S))
                        .map(|mut v| v.remove(0))
                })
                .collect(),
        };
        for (case, p) in cases.iter().zip(py) {
            tally.scripts += 1;
            let p = match p {
                Ok(p) => p,
                Err(e) => {
                    // the app's run fails there; what the native runner does
                    let timed_out = e.contains(TIMED_OUT);
                    let rec = json!({"kind": if timed_out { "python-timed-out" } else { "python-crashed" },
                                     "label": case.label, "code": case.code, "params": case.params_text,
                                     "python": e.chars().rev().take(1500).collect::<String>().chars().rev().collect::<String>(),
                                     "native": native_alone(case)});
                    if timed_out {
                        tally.python_timed_out += 1;
                    } else {
                        tally.python_crashed += 1;
                    }
                    if let Some(o) = &out {
                        let mut f = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(o.join("python-stopped.jsonl"))
                            .expect("the file of scripts Python stopped on");
                        writeln!(f, "{rec}").expect("written");
                    }
                    continue;
                }
            };
            let p = &p;
            for (kind, detail) in diff(case, p, &mut tally) {
                *kinds.entry(kind.clone()).or_default() += 1;
                if first.len() < 300 {
                    let rec = json!({"kind": kind, "label": case.label, "code": case.code,
                                     "params": case.params_text, "detail": detail});
                    if let Some(o) = &out {
                        let mut f = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(o.join("divergences.jsonl"))
                            .expect("the divergences file");
                        writeln!(f, "{rec}").expect("written");
                    }
                    first.push(rec);
                }
            }
        }
        batches += 1;
        let took = started.elapsed().as_secs_f64();
        if let Some(o) = &out {
            let summary = json!({"seed": seed, "seconds": took, "scripts": tally.scripts, "calls": tally.calls,
                                 "equal": tally.equal, "both_failed": tally.both_failed,
                                 "both_refused": tally.both_refused, "left_start": tally.left_start,
                                 "left_calls": tally.left_calls, "python_crashed": tally.python_crashed,
                                 "python_timed_out": tally.python_timed_out,
                                 "divergences": kinds.values().sum::<usize>(), "by_kind": kinds,
                                 "left": tally.left, "raised": tally.raised});
            std::fs::write(o.join("summary.json"), summary.to_string()).expect("the summary");
        }
        if (seconds <= 0.0 && batches >= guard_batches) || (seconds > 0.0 && took >= seconds) {
            break;
        }
    }
    println!("{tally:?}; divergences {kinds:?}; {:.1} s", started.elapsed().as_secs_f64());
    assert!(
        first.is_empty(),
        "{} divergences, the first:\n{:#}",
        kinds.values().sum::<usize>(),
        first[0]
    );
    assert_eq!(tally.python_crashed, 0, "{tally:?}");
    // the scripts are mostly in the subset: the fuzzing reaches the runner
    assert!(tally.left_start * 5 < tally.scripts, "{tally:?}");
    assert!(tally.equal * 3 > tally.calls, "{tally:?}");
}
