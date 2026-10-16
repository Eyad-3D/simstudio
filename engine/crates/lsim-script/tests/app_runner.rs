//! The native runner against what the app's own Script runner gives
//! (backend/app/solver/sandbox_worker.py's `_run_step`, with the
//! parameters as the app types them and the inputs as it passes them),
//! and its limits: the cases a review found, each either giving the app's
//! result or left to Python, never a different result, and every call
//! held to the time and memory the app's worker allows.
//!
//! The bitwise comparison with Python itself is `parity.rs`; these are the
//! cases whose answer is known without running Python.

use lsim_script::{Runner, ScriptError};
use std::time::Instant;

fn params(text: &str) -> serde_json::Map<String, serde_json::Value> {
    serde_json::from_str(text).expect("a JSON object")
}

fn step_code(body: &str) -> String {
    format!("import math\ndef step(t, dt, inputs, state, params):\n{body}")
}

fn start(body: &str, params_text: &str) -> Result<Runner, ScriptError> {
    Runner::start(&step_code(body), "probe", &params(params_text))
}

fn outputs(r: Result<Vec<(String, f64)>, ScriptError>) -> Vec<(String, u64)> {
    r.expect("the call runs").into_iter().map(|(k, v)| (k, v.to_bits())).collect()
}

fn is_outside<T>(r: &Result<T, ScriptError>) -> bool {
    matches!(r, Err(ScriptError::Unsupported(_)))
}

fn failed_with(r: Result<Vec<(String, f64)>, ScriptError>) -> String {
    match r {
        Err(ScriptError::Failed(m)) => m,
        other => panic!("expected Python's exception, got {other:?}"),
    }
}

/// The app passes each call a fresh shallow copy of the parameters
/// (`dict(script.params)`): a top-level key a call sets is gone at the
/// next call, a table changed in place stays changed.
#[test]
fn each_call_gets_a_fresh_copy_of_the_parameters() {
    let mut r = start(
        "    params['n'] = params.get('n', 0) + 1\n    params['t']['k'] = params['t'].get('k', 0.0) + 1.0\n    return {'y': params['n'], 'z': params['t']['k']}\n",
        r#"{"t": {"0": 1.5}}"#,
    )
    .unwrap();
    for k in 1..=3 {
        let got = outputs(r.step(0.0, 0.01, &[]));
        assert_eq!(
            got,
            vec![("y".into(), 1.0f64.to_bits()), ("z".into(), (k as f64).to_bits())],
            "call {k}"
        );
    }
}

/// The app passes an input of -0.0 as +0.0 (`... or 0.0`); a NaN as it is.
#[test]
fn an_input_of_minus_zero_arrives_as_plus_zero() {
    let mut r = start(
        "    a = inputs['a']\n    return {'s': math.copysign(1.0, a), 'y': a, 'n': a * 0.0}\n",
        "{}",
    )
    .unwrap();
    let got = outputs(r.step(0.0, 0.01, &[("a", -0.0)]));
    assert_eq!(
        got,
        vec![("s".into(), 1.0f64.to_bits()), ("y".into(), 0), ("n".into(), 0)],
        "-0.0 is passed as +0.0"
    );
    let mut r = start("    a = inputs['a']\n    return {'y': a, 'n': a * 0.0}\n", "{}").unwrap();
    let got = r.step(0.0, 0.01, &[("a", f64::NAN)]).unwrap();
    assert!(got[0].1.is_nan() && got[1].1.is_nan(), "a NaN stays one: {got:?}");
}

/// The app reads a table's values as floats (pydantic: Table1D and
/// Table2D are dicts of floats), whatever the file writes.
#[test]
fn table_parameters_are_floats_as_the_app_reads_them() {
    let body = "    v = params['t']['0']\n    w = params['m']['1']['2']\n    return {'neg': -v, 'mul': v * -2, 'sign': math.copysign(1.0, -v), 'w': w * 1, 'big': params['t']['b'] - 9007199254740992}\n";
    let mut r =
        start(body, r#"{"t": {"0": 0, "1": 5, "b": 9007199254740993}, "m": {"1": {"2": 3}}}"#)
            .unwrap();
    let got = outputs(r.step(0.0, 0.01, &[]));
    let want: Vec<(String, u64)> = [
        ("neg", (-0.0f64).to_bits()),
        ("mul", (-0.0f64).to_bits()),
        ("sign", (-1.0f64).to_bits()),
        ("w", 3.0f64.to_bits()),
        // 2**53 + 1 read as a float is 2**53
        ("big", 0.0f64.to_bits()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    assert_eq!(got, want);
    // a float is no integer: Python's TypeError
    let mut r =
        start("    return {'y': len(range(params['t']['1']))}\n", r#"{"t": {"1": 5}}"#).unwrap();
    let m = failed_with(r.step(0.0, 0.01, &[]));
    assert!(m.ends_with("'float' object cannot be interpreted as an integer"), "{m}");
    // scalars keep their JSON types
    let mut r = start(
        "    return {'y': len(range(params['n'])) + params['f'] + params['b']}\n",
        r#"{"n": 3, "f": 0.5, "b": true}"#,
    )
    .unwrap();
    assert_eq!(outputs(r.step(0.0, 0.01, &[])), vec![("y".into(), 4.5f64.to_bits())]);
}

/// What the app's typing changes or refuses, and what JSON cannot tell
/// apart, is left to Python.
#[test]
fn parameters_the_app_reads_otherwise_are_left_to_python() {
    for text in [
        // `-0` is the integer 0 in Python, `-0.0` a float: serde_json
        // reads both as -0.0
        r#"{"k": -0}"#,
        r#"{"t": {"0": -0}}"#,
        // pydantic makes these floats or refuses them
        r#"{"t": {"0": true}}"#,
        r#"{"t": {"0": "1.5"}}"#,
        r#"{"t": {"0": null}}"#,
        r#"{"t": {"0": {"1": 2}, "3": 4}}"#,
        r#"{"t": {"0": {"1": {"2": 3}}}}"#,
        r#"{"x": null}"#,
        r#"{"x": [1, 2.5]}"#,
    ] {
        let r = start("    return {'y': 1.0}\n", text);
        assert!(is_outside(&r), "{text}: {:?}", r.as_ref().err());
    }
}

/// The sign of a NaN is not part of the bitwise promise (CPython's own
/// differs between its specialised and plain additions): copysign of a
/// NaN's sign is left to Python.
#[test]
fn the_sign_of_a_nan_is_left_to_python() {
    let mut r = start(
        "    a = inputs['a']; b = inputs['b']\n    return {'y': math.copysign(1.0, -a + b)}\n",
        "{}",
    )
    .unwrap();
    let got = r.step(0.0, 0.01, &[("a", f64::NAN), ("b", f64::NAN)]);
    assert!(is_outside(&got), "{got:?}");
    // the NaN as the value whose sign is set is fine: the result is a NaN
    let mut r = start("    return {'y': math.copysign(inputs['a'], -1.0)}\n", "{}").unwrap();
    assert!(r.step(0.0, 0.01, &[("a", f64::NAN)]).unwrap()[0].1.is_nan());
}

/// `list[float]` is a types.GenericAlias in Python 3.9 and later.
#[test]
fn subscripting_a_builtin_is_left_to_python() {
    for e in ["list[float]", "dict[int, float]", "tuple[int]", "enumerate[int]", "float[int]"] {
        let mut r = start(&format!("    hint = {e}\n    return {{'y': 1.0}}\n"), "{}").unwrap();
        let got = r.step(0.0, 0.01, &[]);
        assert!(is_outside(&got), "{e}: {got:?}");
    }
}

/// Python refuses to convert a string of more than 4300 digits (its
/// default `sys.get_int_max_str_digits()`, which can be set as low as
/// 640): a string above 640 digits is left to Python.
#[test]
fn int_of_a_long_digit_string_is_left_to_python() {
    // `z` is 639 zeros
    let zeros = "    z = ''\n    for i in range(639):\n        z = z + '0'\n";
    let mut r = start(&format!("{zeros}    return {{'y': int(z + '1')}}\n"), "{}").unwrap();
    assert_eq!(outputs(r.step(0.0, 0.01, &[])), vec![("y".into(), 1.0f64.to_bits())], "640");
    for digits in ["z + '01'", "'-' + z + '_0' + z + '7'", "z + z + z + z + z + z + z + '7'"] {
        let mut r = start(&format!("{zeros}    return {{'y': int({digits})}}\n"), "{}").unwrap();
        let got = r.step(0.0, 0.01, &[]);
        assert!(is_outside(&got), "{digits}: {got:?}");
    }
}

/// CPython 3.11's words for the errors scripts meet, the values they
/// quote as Python's repr shows them.
#[test]
fn messages_are_cpython_311s() {
    let cases: Vec<(&str, &str, &str)> = vec![
        ("    return {'y': 7 % 0}\n", "{}", "integer modulo by zero"),
        ("    return {'y': 7 // 0}\n", "{}", "integer division or modulo by zero"),
        ("    return {'y': 1.0 % 0.0}\n", "{}", "float modulo"),
        ("    return {'y': 7 % 0.0}\n", "{}", "float modulo"),
        (
            "    return {'y': interp({'1000000': 1.0, '1000000.0': 2.0}, 1.0)}\n",
            "{}",
            "table1d has duplicate key 1e+06",
        ),
        (
            "    return {'y': interp({'abc': 1}, 1.0)}\n",
            "{}",
            "table1d entry 'abc: 1' is not numeric",
        ),
        (
            "    return {'y': interp({1e16: 'x'}, 1.0)}\n",
            "{}",
            "table1d entry '1e+16: x' is not numeric",
        ),
        // a sequence's own concatenation, and the operator of an
        // augmented assignment
        ("    return {'y': 'a' + 1}\n", "{}", "can only concatenate str (not \"int\") to str"),
        (
            "    return {'y': [1] + (1,)}\n",
            "{}",
            "can only concatenate list (not \"tuple\") to list",
        ),
        (
            "    x = (1,)\n    x += [1]\n",
            "{}",
            "can only concatenate tuple (not \"list\") to tuple",
        ),
        ("    x = 1\n    x -= 'a'\n", "{}", "unsupported operand type(s) for -=: 'int' and 'str'"),
        (
            "    x = 2\n    x **= 'a'\n",
            "{}",
            "unsupported operand type(s) for **=: 'int' and 'str'",
        ),
        (
            "    x = None\n    x //= 1\n",
            "{}",
            "unsupported operand type(s) for //=: 'NoneType' and 'int'",
        ),
        ("    return {'y': min([])}\n", "{}", "min() arg is an empty sequence"),
        ("    return {'y': max(())}\n", "{}", "max() arg is an empty sequence"),
        ("    return {'y': math.trunc('a')}\n", "{}", "type str doesn't define __trunc__ method"),
        (
            "    return {'y': math.sqrt()}\n",
            "{}",
            "math.sqrt() takes exactly one argument (0 given)",
        ),
        ("    return {'y': math.copysign(1)}\n", "{}", "copysign expected 2 arguments, got 1"),
        // a call's arguments, checked in CPython's order
        (
            "    return {'y': f(1, 2, 3, 4)}\n",
            "{}",
            "f() takes from 1 to 3 positional arguments but 4 were given",
        ),
        ("    return {'y': h(1, 2)}\n", "{}", "h() takes 1 positional argument but 2 were given"),
        ("    return {'y': z(1)}\n", "{}", "z() takes 0 positional arguments but 1 was given"),
        (
            "    return {'y': g()}\n",
            "{}",
            "g() missing 2 required positional arguments: 'a' and 'b'",
        ),
        ("    return {'y': f(b=1)}\n", "{}", "f() missing 1 required positional argument: 'a'"),
        (
            "    return {'y': f(1, 2, 3, 4, a=5)}\n",
            "{}",
            "f() got multiple values for argument 'a'",
        ),
        (
            "    return {'y': f(1, 2, 3, 4, d=5)}\n",
            "{}",
            "f() got an unexpected keyword argument 'd'",
        ),
        // the types of iterator objects
        ("    return {'y': len(enumerate([]))}\n", "{}", "object of type 'enumerate' has no len()"),
        ("    return {'y': zip()[0]}\n", "{}", "'zip' object is not subscriptable"),
        ("    return enumerate([])\n", "{}", "must return a dict of output values, got enumerate."),
        // what is quoted: builtins, types, the module, views
        ("    return {'y': abs}\n", "{}", "output 'y' is not numeric (<built-in function abs>)."),
        (
            "    return {'y': [float, math.sqrt, math]}\n",
            "{}",
            "output 'y' is not numeric ([<class 'float'>, <built-in function sqrt>, <module 'math' (built-in)>]).",
        ),
        (
            "    return {'y': {1: 'a'}.items()}\n",
            "{}",
            "output 'y' is not numeric (dict_items([(1, 'a')])).",
        ),
        ("    return {'y': [1].index(len)}\n", "{}", "<built-in function len> is not in list"),
        // a character Python does not print (unassigned in Unicode 14)
        ("    return {'y': {}['\u{378}']}\n", "{}", "'\\u0378'"),
        ("    return {'y': {}['\u{e9}\u{2013}\u{4e00}']}\n", "{}", "'\u{e9}\u{2013}\u{4e00}'"),
        // the comparison Python makes first in a 2-D lookup
        (
            "    return {'y': interp({'0': {'0': 1}}, 'x', 0)}\n",
            "{}",
            "'<' not supported between instances of 'float' and 'str'",
        ),
        (
            "    return {'y': interp({'0': {'0': 1}}, 0, 'y')}\n",
            "{}",
            "'<=' not supported between instances of 'str' and 'float'",
        ),
        // indexing, unpacking and dict()
        ("    return {'y': 'abc'['a']}\n", "{}", "string indices must be integers, not 'str'"),
        (
            "    return {'y': range(3)['a']}\n",
            "{}",
            "range indices must be integers or slices, not str",
        ),
        ("    return {'y': range(3)[5]}\n", "{}", "range object index out of range"),
        ("    a, b = 1\n", "{}", "cannot unpack non-iterable int object"),
        (
            "    return {'y': dict([1])}\n",
            "{}",
            "cannot convert dictionary update sequence element #0 to a sequence",
        ),
        (
            "    return {'y': dict([(1, 2), (1, 2, 3)])}\n",
            "{}",
            "dictionary update sequence element #1 has length 3; 2 is required",
        ),
        // a parameter table with one entry that is no number: its order
        // does not matter
        (
            "    return {'y': interp(params['t'], 0.5)}\n",
            r#"{"t": {"zz": 1}}"#,
            "table1d entry 'zz: 1.0' is not numeric",
        ),
    ];
    let defs = "def f(a, b=2, c=3):\n    return a\ndef g(a, b):\n    return a\ndef h(a):\n    return a\ndef z():\n    return 1\n";
    for (body, p, want) in cases {
        let code = format!("import math\n{defs}def step(t, dt, inputs, state, params):\n{body}");
        let mut r = Runner::start(&code, "probe", &params(p)).unwrap();
        let m = failed_with(r.step(0.5, 0.01, &[]));
        let raised = format!("Script 'probe' raised at t = 0.5 s: {want}");
        let other = format!("Script 'probe' {want}");
        assert!(m == raised || m == other, "{body}:\n  {m}\nnot\n  {raised}");
    }
    let mut r = start("    return {'y': interp({0: {0: 1.0}, '0.0': {1: 2.0}}, 1.0, 2.0)}\n", "{}")
        .unwrap();
    let m = failed_with(r.step(0.5, 0.01, &[]));
    assert!(m.ends_with("table2d has duplicate outer key 0"), "{m}");
    let mut r = start("    return {'y': interp({'q': {0: 1.0}}, 1.0, 2.0)}\n", "{}").unwrap();
    let m = failed_with(r.step(0.5, 0.01, &[]));
    assert!(m.ends_with("table2d outer key 'q' is not numeric"), "{m}");
}

/// A message whose words the native runner cannot know is left to
/// Python: what quotes an object's address (a function, a bound method,
/// an iterator object, the scripts' own `clamp` and `interp`), the
/// parameters' order (the importer's is not the project's) or a
/// character beyond the scripts Python's table is known here for.
#[test]
fn messages_python_words_otherwise_are_left_to_python() {
    for (body, p) in [
        ("    return {'y': step}\n", "{}"),
        ("    return {'y': enumerate([])}\n", "{}"),
        ("    return {'y': [[].append]}\n", "{}"),
        ("    return {'y': (clamp, 1)}\n", "{}"),
        ("    return {'y': interp}\n", "{}"),
        ("    return {'y': [1].index(step)}\n", "{}"),
        ("    return {'y': interp({'1': [zip()]}, 1)}\n", "{}"),
        // the first entry that is no number, in the project's order
        ("    return {'y': interp(params['t'], 0.5)}\n", r#"{"t": {"zz": 1, "aa": 5}}"#),
        (
            "    return {'y': interp(params['m'], 0.5, 1)}\n",
            r#"{"m": {"zz": {"0": 1}, "aa": {"0": 5}}}"#,
        ),
        ("    return {'y': params}\n", r#"{"b": 1, "a": 2}"#),
        ("    return {'y': params['t'].keys()}\n", r#"{"t": {"1": 1, "0": 5}}"#),
        // a character beyond the common scripts
        ("    return {'y': {}['\u{1f600}']}\n", "{}"),
        ("    return {'y': {}['\u{e0001}']}\n", "{}"),
    ] {
        let mut r = start(body, p).unwrap();
        let got = r.step(0.0, 0.01, &[]);
        assert!(is_outside(&got), "{body}: {got:?}");
    }
}

/// The app's worker sends a call's outputs in one frame that gives their
/// number and each key's length in UTF-8 bytes in 16 bits
/// (sandbox_worker.py's `_pack_outputs`): past them the worker stops and
/// the app's run fails. Such outputs are left to Python, as are more than
/// 65 535 inputs (the frame of a call).
#[test]
fn outputs_the_apps_worker_cannot_send_are_left_to_python() {
    // `s` is 65 535 bytes, `e` 32 768 two-byte characters
    let long =
        "    s = ''\n    p = 'k'\n    for i in range(16):\n        s = s + p\n        p = p + p\n";
    let wide = "    e = '\u{e9}'\n    for i in range(15):\n        e = e + e\n";
    let mut r = start(&format!("{long}    return {{'y': 1.0, s: 2.0}}\n"), "{}").unwrap();
    let got = r.step(0.0, 0.01, &[]).expect("a key of 65 535 bytes is sent");
    assert_eq!((got[1].0.len(), got[1].1), (65_535, 2.0));
    for body in [
        format!("{long}    return {{'y': 1.0, s + 'k': 2.0}}\n"),
        format!("{wide}    return {{e: 1.0}}\n"),
    ] {
        let mut r = start(&body, "{}").unwrap();
        let got = r.step(0.0, 0.01, &[]);
        assert!(is_outside(&got), "{got:?}");
    }
    // 16**4 = 65 536 outputs, and one fewer
    let many = "    out = {}\n    for a in 'abcdefghijklmnop':\n        for b in 'abcdefghijklmnop':\n            for c in 'abcdefghijklmnop':\n                for d in 'abcdefghijklmnop':\n";
    let mut r = start(
        &format!("{many}                    out[a + b + c + d] = 1.0\n    return out\n"),
        "{}",
    )
    .unwrap();
    assert!(is_outside(&r.step(0.0, 0.01, &[])));
    let mut r = start(
        &format!(
            "{many}                    if a + b + c + d != 'pppp':\n                        out[a + b + c + d] = 1.0\n    return out\n"
        ),
        "{}",
    )
    .unwrap();
    assert_eq!(r.step(0.0, 0.01, &[]).expect("65 535 outputs are sent").len(), 65_535);
    let names: Vec<String> = (0..65_536).map(|k| format!("u{k}")).collect();
    let ins: Vec<(&str, f64)> = names.iter().map(|n| (n.as_str(), 1.0)).collect();
    let mut r = start("    return {'y': len(inputs)}\n", "{}").unwrap();
    assert!(is_outside(&r.step(0.0, 0.01, &ins)));
    assert_eq!(outputs(r.step(0.0, 0.01, &ins[1..])), vec![("y".into(), 65_535f64.to_bits())]);
}

/// One statement of a lot of work still stops at the time limit: every
/// operation whose work grows with the size of what it handles is
/// charged as it goes (here the review's case, a list of a million items
/// joined to itself a thousand times in one statement, about 10 s
/// without the charge).
#[test]
fn one_statement_of_long_operations_stops_at_the_time_limit() {
    let many = |e: &str, n: usize| vec![e; n].join(", ");
    let bodies = [
        format!("    l = list(range(500000))\n    x = [{}]\n", many("not (l + l)", 1000)),
        format!("    l = list(range(500000))\n    x = [{}]\n", many("len(list(l))", 3000)),
        format!("    l = list(range(500000))\n    x = [{}]\n", many("len(l.copy())", 3000)),
        format!(
            "    s = 'abc'\n    for i in range(18):\n        s = s + s\n    x = [{}]\n",
            many("s[-1]", 20000)
        ),
        format!(
            "    s = '123'\n    for i in range(18):\n        s = s + s\n    x = [{}]\n",
            many("float(s)", 3000)
        ),
    ];
    for body in bodies {
        let mut r = start(&format!("{body}    return {{'y': 1.0}}\n"), "{}").unwrap();
        let started = Instant::now();
        let got = r.step(0.0, 0.01, &[]);
        let took = started.elapsed().as_secs_f64();
        assert!(
            matches!(&got, Err(ScriptError::Unsupported(m)) if m.contains("took longer than 2 s")),
            "{got:?}\n{}",
            &body[..body.len().min(120)]
        );
        assert!((1.9..3.0).contains(&took), "stopped after {took:.2} s: {}", &body[..60]);
    }
}

/// The memory a runner's values hold is capped at what the app's worker
/// allows (512 MB): a script that keeps adding to its state is left to
/// Python once it holds more, instead of taking the host's memory.
#[test]
fn state_that_keeps_growing_stops_at_the_memory_cap() {
    let mut r = start(
        "    keep = state.setdefault('keep', [])\n    keep.append(list(range(1000000)))\n    return {'n': len(keep)}\n",
        "{}",
    )
    .unwrap();
    let mut stopped = None;
    for k in 1..=24 {
        match r.step(k as f64, 0.01, &[]) {
            Ok(out) => assert_eq!(out, vec![("n".to_string(), k as f64)]),
            Err(ScriptError::Unsupported(m)) => {
                assert!(m.contains("512 MB"), "{m}");
                stopped = Some(k);
                break;
            }
            Err(e) => panic!("{e}"),
        }
    }
    // a million integers take 32 MB here (36 MB in CPython)
    let k = stopped.expect("the state's growth was stopped");
    assert!((12..=20).contains(&k), "stopped at call {k}");
    drop(r);
    // what a call lets go of is given back: a runner that makes and
    // drops a lot every call goes on
    let mut r = start(
        "    x = list(range(1000000))\n    state['y'] = list(range(1000000))\n    return {'n': len(x)}\n",
        "{}",
    )
    .unwrap();
    for k in 0..40 {
        assert!(r.step(k as f64, 0.01, &[]).is_ok(), "call {k}");
    }
}
