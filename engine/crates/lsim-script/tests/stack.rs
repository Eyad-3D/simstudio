//! No script inside the subset's limits overflows the stack of the thread
//! that calls the runner, down to a thread of 1 MiB (a sweep's threads
//! have 2 MiB, python.exe's main thread on Windows 2 MB): it gives
//! Python's result or is left to Python. In its own test binary, as a
//! stack overflow ends the whole process.
//!
//! Run in every profile: `cargo test -p lsim-script --test stack` (the
//! test profile), with `--profile dev` (no optimisation: the largest
//! frames) and with `--release`.

use lsim_script::{Runner, ScriptError};

const MIB: usize = 1024 * 1024;

/// Starts `code` and makes one call, on a thread of `stack` bytes.
fn on_thread(stack: usize, code: &str) -> Result<Vec<(String, f64)>, ScriptError> {
    let code = code.to_string();
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || {
            let mut r = Runner::start(&code, "deep", &serde_json::Map::new())?;
            r.step(0.0, 0.01, &[])
        })
        .expect("a thread")
        .join()
        .expect("no panic")
}

/// The script `make(n)` for the largest `n` the parser takes.
fn deepest(make: impl Fn(usize) -> String) -> (usize, String) {
    (1..=200)
        .rev()
        .map(|n| (n, make(n)))
        .find(|(_, code)| lsim_script::check(code).is_ok())
        .expect("a depth the parser takes")
}

/// Python's result on threads of 1 and 2 MiB, or left to Python.
fn runs_or_is_left(code: &str, want: f64, what: &str) {
    for stack in [MIB, 2 * MIB] {
        let ran = match on_thread(stack, code) {
            Ok(out) => {
                assert_eq!(out, vec![("y".to_string(), want)], "{what}");
                "ran"
            }
            Err(ScriptError::Unsupported(_)) => "left to Python",
            Err(e) => panic!("{what}: {e}"),
        };
        eprintln!("{what} on {} MiB: {ran}", stack / MIB);
    }
}

/// A kind of nesting: its name and the script nested `k` deep.
type Nesting<'a> = (&'static str, Box<dyn Fn(usize) -> String + 'a>);

fn recursive(expr: &str, calls: usize) -> String {
    format!(
        "def f(n):\n    if n <= 0:\n        return 0\n    return {expr}\ndef step(t, dt, inputs, state, params):\n    return {{'y': f({calls})}}\n"
    )
}

/// The review's cases: calls up to 49 deep (the subset allows 50), each
/// through an expression nested as deeply as the parser allows (unary
/// minus signs, or a chain of additions).
#[test]
fn deep_calls_through_deep_expressions_fit_a_small_stack() {
    let (neg, _) = deepest(|k| recursive(&format!("{}f(n - 1)", "- ".repeat(k)), 1));
    let (add, _) = deepest(|k| recursive(&format!("f(n - 1){}", " + 1".repeat(k)), 1));
    eprintln!("{neg} minus signs, {add} additions");
    assert!(neg >= 90 && add >= 90);
    for calls in [10, 30, 48, 49] {
        let code = recursive(&format!("{}f(n - 1)", "- ".repeat(neg)), calls);
        runs_or_is_left(&code, 0.0, &format!("{neg} minus signs, f({calls})"));
        let code = recursive(&format!("f(n - 1){}", " + 1".repeat(add)), calls);
        runs_or_is_left(&code, (add * calls) as f64, &format!("{add} additions, f({calls})"));
    }
}

/// Every kind of nesting at the parser's depth: brackets, unary operators,
/// conditional expressions, subscripts, calls and blocks.
#[test]
fn code_nested_as_deeply_as_the_parser_allows_fits_a_small_stack() {
    let step = |body: String| format!("def step(t, dt, inputs, state, params):\n{body}");
    let blocks = |k: usize| {
        let mut b = String::new();
        for j in 0..k {
            b.push_str(&format!("{}if t >= 0:\n", "    ".repeat(j + 1)));
        }
        b.push_str(&format!("{}return {{'y': 1.0}}\n", "    ".repeat(k + 1)));
        step(b)
    };
    let kinds: [Nesting<'_>; 8] = [
        (
            "brackets",
            Box::new(|k| {
                step(format!("    return {{'y': {}1.0{}}}\n", "(".repeat(k), ")".repeat(k)))
            }),
        ),
        (
            "lists and subscripts",
            Box::new(|k| {
                step(format!(
                    "    return {{'y': {}1.0{}{}}}\n",
                    "[".repeat(k),
                    "]".repeat(k),
                    "[0]".repeat(k)
                ))
            }),
        ),
        (
            "minus signs",
            Box::new(|k| step(format!("    return {{'y': {}1.0}}\n", "- ".repeat(2 * k)))),
        ),
        (
            "nots",
            Box::new(|k| {
                step(format!("    return {{'y': 1.0 if {}True else 0.0}}\n", "not ".repeat(2 * k)))
            }),
        ),
        (
            "calls",
            Box::new(|k| {
                step(format!("    return {{'y': {}1.0{}}}\n", "abs(".repeat(k), ")".repeat(k)))
            }),
        ),
        (
            "conditional expressions",
            Box::new(|k| {
                step(format!("    return {{'y': {}1.0}}\n", "0.0 if t < 0 else ".repeat(k)))
            }),
        ),
        (
            "additions",
            Box::new(|k| step(format!("    return {{'y': 1.0{}}}\n", " + 0.0".repeat(k)))),
        ),
        ("blocks", Box::new(blocks)),
    ];
    for (what, make) in kinds {
        let (k, code) = deepest(make);
        runs_or_is_left(&code, 1.0, &format!("{k} {what}"));
    }
}

/// The comparisons, searches and keys that walk nested data, at the
/// bottom of calls 48 deep through deep expressions.
#[test]
fn walks_through_deeply_nested_data_at_the_bottom_of_deep_calls_fit_a_small_stack() {
    let script = |test: &str, k: usize| {
        format!(
            "A = [1.0]\nB = [1.0]\nK = (1,)\nfor i in range(95):\n    A = [A]\n    B = [B]\n    K = (K,)\nD = {{K: 1.0}}\ndef f(n):\n    if n <= 0:\n        return 1.0 if {test} else 0.0\n    return {}f(n - 1)\ndef step(t, dt, inputs, state, params):\n    return {{'y': f(48)}}\n",
            "+ ".repeat(k)
        )
    };
    for test in ["A == B", "A <= B", "B in [0, A]", "K in D", "[K] == [K]", "D[K] == 1.0"] {
        let (k, code) = deepest(|k| script(test, k));
        runs_or_is_left(&code, 1.0, &format!("{test} under {k} unary plus signs"));
    }
}
