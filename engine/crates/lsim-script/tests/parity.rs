//! The native runner against today's Python runner, bit for bit.
//!
//! The same scripts get the same calls on both: today's own runner
//! (backend/app/solver/scripting.py, run by `tests/parity.py` with only the
//! standard library) and lsim-script. Every output must have the same 64
//! bits (two NaNs count as equal), every call that raises on Python must
//! fail here too, and no call of these scripts may leave the subset. The
//! scripts: the P2 hybrid's Hybrid Control Unit from
//! backend/projects/hybrid-car.json on long drives of realistic inputs,
//! hand-written scripts that exercise each feature of the subset, and a
//! few hundred random expressions over special values (signed zeros,
//! subnormals, the largest floats, infinities, NaN).
//!
//! Needs Python 3.11 or newer (`LSIM_PYTHON`, else `python3` or `python`
//! on the PATH), as engine/check.sh does.

use lsim_script::{Runner, ScriptError};
use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn python() -> String {
    if let Ok(p) = std::env::var("LSIM_PYTHON") {
        return p;
    }
    for p in ["python3", "python"] {
        if Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()) {
            return p.into();
        }
    }
    panic!(
        "the parity test needs Python 3.11 or newer: set LSIM_PYTHON or put python3 on the PATH"
    );
}

/// A deterministic random source (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

struct Call {
    t: f64,
    dt: f64,
    inputs: Vec<(String, f64)>,
}

struct Case {
    label: String,
    code: String,
    /// the parameters as JSON text: Python reads them in their own order,
    /// the native runner as the importer hands them over (sorted)
    params_text: String,
    calls: Vec<Call>,
    /// whether a call may leave the subset (random scripts may)
    may_leave: bool,
}

fn bits(x: f64) -> u64 {
    x.to_bits()
}

/// Every case's results on Python.
fn on_python(cases: &[Case]) -> Vec<Value> {
    let doc: Vec<Value> = cases
        .iter()
        .map(|c| {
            json!({
                "code": c.code,
                "label": c.label,
                "params_text": c.params_text,
                "calls": c.calls.iter().map(|k| json!({
                    "t": bits(k.t),
                    "dt": bits(k.dt),
                    "inputs": k.inputs.iter().map(|(n, v)| (n.clone(), json!(bits(*v)))).collect::<serde_json::Map<_, _>>(),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut child = Command::new(python())
        .arg(here.join("tests/parity.py"))
        .arg(repo().join("backend/app/solver"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Python starts");
    let text = serde_json::to_string(&doc).expect("JSON");
    let mut stdin = child.stdin.take().expect("stdin");
    let writer = std::thread::spawn(move || stdin.write_all(text.as_bytes()));
    let out = child.wait_with_output().expect("Python finishes");
    writer.join().expect("the writer").expect("written");
    assert!(
        out.status.success(),
        "the Python side failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Vec<Value>>(&out.stdout).expect("Python's results")
}

#[derive(Default, Debug)]
struct Tally {
    calls: usize,
    equal: usize,
    both_failed: usize,
    left: usize,
}

fn compare(cases: &[Case], py: &[Value]) -> Tally {
    let mut tally = Tally::default();
    for (case, py) in cases.iter().zip(py) {
        let params: serde_json::Map<String, Value> =
            serde_json::from_str(&case.params_text).expect("a JSON object");
        let mine = Runner::start(&case.code, &case.label, &params);
        if let Some(e) = py.get("compile_error") {
            assert!(
                mine.is_err(),
                "{}: Python refused it ({e}) but the native runner read it",
                case.label
            );
            continue;
        }
        let mut runner = match mine {
            Ok(r) => r,
            Err(ScriptError::Unsupported(m)) if case.may_leave => {
                tally.left += case.calls.len();
                eprintln!("LEFT-START {}: {m}", case.label);
                continue;
            }
            Err(e) => {
                panic!("{}: the native runner does not start: {e}\n{}", case.label, case.code)
            }
        };
        let results = py["results"].as_array().expect("results");
        assert_eq!(results.len(), case.calls.len());
        for (k, (call, want)) in case.calls.iter().zip(results).enumerate() {
            tally.calls += 1;
            let ins: Vec<(&str, f64)> = call.inputs.iter().map(|(n, v)| (n.as_str(), *v)).collect();
            let got = runner.step(call.t, call.dt, &ins);
            let ctx = || {
                format!(
                    "{} call {k} at t = {} with {:?}\n{}",
                    case.label, call.t, call.inputs, case.code
                )
            };
            match (got, want.get("ok"), want.get("err")) {
                (Ok(res), Some(ok), None) => {
                    let ok = ok.as_array().expect("pairs");
                    assert_eq!(res.len(), ok.len(), "outputs differ: {res:?} vs {ok:?}\n{}", ctx());
                    for ((name, v), pair) in res.iter().zip(ok) {
                        let (pn, pb) =
                            (pair[0].as_str().expect("a name"), pair[1].as_u64().expect("bits"));
                        let pv = f64::from_bits(pb);
                        assert_eq!(name, pn, "output order differs\n{}", ctx());
                        assert!(
                            v.to_bits() == pb,
                            "output '{name}': native {v:e} ({:#x}), Python {pv:e} ({pb:#x})\n{}",
                            v.to_bits(),
                            ctx()
                        );
                    }
                    tally.equal += 1;
                }
                (Err(ScriptError::Failed(_)), None, Some(_)) => tally.both_failed += 1,
                (Err(ScriptError::Unsupported(m)), _, _) => {
                    assert!(case.may_leave, "left the subset: {m}\n{}", ctx());
                    tally.left += 1;
                    eprintln!("LEFT-CALL {}: {m}", case.label);
                    // a real run would go on Python from the start
                    break;
                }
                (Ok(res), None, Some(e)) => {
                    panic!("Python raised ({e}), native gave {res:?}\n{}", ctx())
                }
                (Err(ScriptError::Failed(m)), Some(ok), _) => {
                    panic!("native failed ({m}), Python gave {ok}\n{}", ctx())
                }
                other => panic!("unexpected {other:?}\n{}", ctx()),
            }
        }
    }
    tally
}

fn hcu_code() -> String {
    let text = std::fs::read_to_string(repo().join("backend/projects/hybrid-car.json"))
        .expect("the project");
    let p: Value = serde_json::from_str(&text).expect("JSON");
    for s in p["systems"].as_array().expect("systems") {
        for e in s["elements"].as_array().expect("elements") {
            if e["id"] == "el-hcu" {
                return e["parameterOverrides"]["code"].as_str().expect("code").to_string();
            }
        }
    }
    panic!("no Hybrid Control Unit");
}

/// A drive of realistic inputs: a slowly moving charge, speed, pedal and
/// shaft speeds, with exact values at the script's thresholds now and then.
fn hcu_drive(seed: u64, n: usize, cumulative_t: bool) -> Vec<Call> {
    let mut r = Rng(seed);
    let (mut soc, mut v, mut trac) = (r.range(45.0, 65.0), 0.0f64, 0.0f64);
    let mut t = 0.0f64;
    let mut calls = vec![];
    for k in 0..n {
        soc = (soc + r.range(-0.02, 0.02)).clamp(20.0, 90.0);
        trac = (trac + r.range(-0.08, 0.08)).clamp(-1.0, 1.0);
        if r.below(50) == 0 {
            trac = [0.0, -0.0, 1.0, -1.0][r.below(4)];
        }
        v = (v + trac * 0.4 + r.range(-0.1, 0.1)).clamp(0.0, 140.0);
        if r.below(80) == 0 {
            v = [0.0, 1.0, 18.0, 30.0, 45.0, 58.0, 70.0, 12.0, 24.0][r.below(9)];
        }
        let mut n_in = v * r.range(40.0, 120.0);
        if r.below(60) == 0 {
            n_in = [0.0, 1000.0, 1100.0, 800.0, 999.9999999999999][r.below(5)];
        }
        let n_eng =
            if r.below(3) == 0 { n_in + r.range(-150.0, 150.0) } else { r.range(0.0, 6500.0) };
        let tk = if cumulative_t { t } else { k as f64 * 0.01 };
        calls.push(Call {
            t: tk,
            dt: 0.01,
            inputs: vec![
                ("soc".into(), soc),
                ("speed".into(), v),
                ("traction".into(), trac),
                ("motor_rpm".into(), n_in),
                ("engine_rpm".into(), n_eng),
            ],
        });
        t += 0.01;
    }
    calls
}

const SPECIAL: [f64; 22] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    2.5,
    -3.5,
    0.1,
    0.3,
    3.0,
    7.0,
    -7.0,
    123.456,
    1e-300,
    5e-324,
    1e300,
    1.797_693_134_862_315_7e308,
    -1e16,
    9_007_199_254_740_993.0,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NAN,
];

fn special(r: &mut Rng) -> f64 {
    if r.below(3) == 0 { r.range(-1000.0, 1000.0) } else { SPECIAL[r.below(SPECIAL.len())] }
}

fn special_calls(r: &mut Rng, names: &[&str], n: usize) -> Vec<Call> {
    (0..n)
        .map(|k| Call {
            t: k as f64 * 0.01,
            dt: 0.01,
            inputs: names.iter().map(|nm| (nm.to_string(), special(r))).collect(),
        })
        .collect()
}

/// Hand-written scripts, one per group of features.
const FEATURES: [&str; 12] = [
    // arithmetic, floor division and modulo of floats and ints, comparisons
    r#"
def step(t, dt, inputs, state, params):
    a = inputs['a']; b = inputs['b']; c = inputs['c']
    n = 7
    return {
        'add': a + b * c, 'sub': a - b - c, 'div': a / (b if b else 1.0),
        'fdiv': a // 2.5, 'mod': a % -3.0, 'imod': n % 3 - (-n) % 3 + n // -2,
        'cmp': (a < b) + 2 * (b <= c) + 4 * (a == c) + 8 * (a != b) + 16 * (1 < a <= 3 < c),
        'neg': -a + +b, 'ints': n * 3 // 2 + n / 2,
    }
"#,
    // builtins: abs, min, max, clamp, round, int, float, bool, len
    r#"
def step(t, dt, inputs, state, params):
    a = inputs['a']; b = inputs['b']
    r = {}
    r['abs'] = abs(a) - abs(-3)
    r['min'] = min(a, b, 0.0)
    r['max'] = max((a, b, 1))
    r['clamp'] = clamp(a, -2.0, b)
    r['bool'] = bool(a) + bool(0.0) * 2 + bool([]) * 4 + bool({'k': 1}) * 8
    r['len'] = len('héllo') + len((1, 2)) + len([1]) + len({1: 2}) + len(range(2, 11, 3))
    if a == a and abs(a) < 1e15:
        r['round'] = round(a)
        r['int'] = int(a)
    r['float'] = float('  1_5.25 ') + float(3) + float(True)
    return r
"#,
    // control flow, loops, functions with defaults and globals, unpacking
    r#"
COUNT = 0
def helper(x, k=2, scale=1.5):
    global COUNT
    COUNT += 1
    total = 0.0
    for i in range(k):
        if i % 2:
            continue
        total += x * scale + i
    else:
        total += 0.25
    while 100.0 < total < 1e300:
        total = total / 3.0
        if total < 50:
            break
    return total

def step(t, dt, inputs, state, params):
    a = inputs['a']
    p, q = (a, 2.0)
    [u, w] = [q, p]
    acc = 0.0
    for i, x in enumerate((1.0, 2.0, a)):
        acc += i * x
    for x, y in zip([1, 2, 3], (a, a)):
        acc -= x * y
    return {'h': helper(a), 'h2': helper(a, 5, scale=-0.5), 'acc': acc, 'n': COUNT, 'u': u - w}
"#,
    // dicts and lists: state across calls, methods, membership, iteration
    r#"
def step(t, dt, inputs, state, params):
    a = inputs['a']
    if not state:
        state.update(n=0, hist=[], seen={})
    state['n'] += 1
    h = state['hist']
    h.append(a)
    if len(h) > 5:
        h.pop(0)
    s = state.setdefault('sum', 0.0)
    state['sum'] = s + a
    key = 'pos' if a > 0 else 'neg'
    state['seen'][key] = state['seen'].get(key, 0) + 1
    tot = 0.0
    for k, v in state['seen'].items():
        tot += v * (2 if k == 'pos' else 3)
    keys = list(state['seen'].keys())
    return {'n': state['n'], 'last': h[-1], 'first': h[0], 'sum': state['sum'], 'tot': tot,
            'has': ('pos' in state['seen']) + 2 * (a in h if a == a else 0) + 4 * (1 in [1.0]),
            'nk': len(keys), 'idx': h.index(a) if a == a else -1, 'cnt': h.count(a) if a == a else -1,
            'p': params.get('gain', 1.0) * a}
"#,
    // tables: interp in 1-D and 2-D, keys as ints, floats and strings
    r#"
T1 = {800: 105, 1000: 112.5, 1500: 124, '2000': 132}
T2 = {0: {0: 1.0, 10: 2.0}, 5.5: {0: 3.0, 10: 5.0, 20: 4.0}}
def step(t, dt, inputs, state, params):
    a = inputs['a']; b = inputs['b']
    return {'t1': interp(T1, a * 10.0), 't1i': interp(T1, 1200), 't2': interp(T2, a, b * 10.0),
            't2e': interp(T2, 99.0, -1.0)}
"#,
    // the math module's exact functions and constants
    r#"
import math
from math import floor as fl
def step(t, dt, inputs, state, params):
    a = inputs['a']
    r = {'pi': math.pi * a, 'fabs': math.fabs(a), 'cs': math.copysign(2.0, a),
         'isn': math.isnan(a) + 2 * math.isinf(a) + 4 * math.isfinite(a),
         'rad': math.radians(a), 'deg': math.degrees(a), 'tau': math.tau, 'e': math.e}
    if math.isfinite(a) and abs(a) < 1e15:
        r['floor'] = fl(a); r['ceil'] = math.ceil(a); r['trunc'] = math.trunc(a)
    if a >= 0:
        r['sqrt'] = math.sqrt(a)
    return r
"#,
    // errors Python raises: division by zero, a missing key, a wrong type
    r#"
def step(t, dt, inputs, state, params):
    a = inputs['a']
    k = int(a) % 4 if abs(a) < 1e9 else 0
    if k == 0:
        return {'y': 1.0 / (a - a)}
    if k == 1:
        return {'y': inputs['missing']}
    if k == 2:
        return {'y': 'text' < 1}
    return {'y': a}
"#,
    // integers near 2**53 and 2**63, exact comparisons with floats, ranges
    r#"
BIG = 9007199254740993
M = -9223372036854775807 - 1
def step(t, dt, inputs, state, params):
    a = inputs['a']
    r = {'eq': (BIG == 9007199254740992.0) + 2 * (BIG > 9007199254740992.0)
               + 4 * (M == float(M)) + 8 * (M < -9.2e18),
         'div': 7 // -2 + (-7) % 3 + M // 3, 'tdiv': 9007199254740992 / 3, 'fl': float(BIG)}
    if abs(a) < 1e300:
        r['cmp'] = (BIG < a) + 2 * (M > a) + 4 * (BIG == a)
    r['len'] = len(range(-5, 10 ** 6, 7)) + range(10, -10, -3)[2]
    return r
"#,
    // strings to numbers (Python's ValueError on the bad ones)
    r#"
S = ['1.5', ' -2e3 ', 'inf', '-nan', '1_000', '0x10', '1__0', '', '+_1', '12', ' 7 ', '-0', '1e', 'NaN', '-Infinity']
def step(t, dt, inputs, state, params):
    a = inputs['a']
    k = int(abs(a)) % len(S) if abs(a) < 1e9 else 0
    s = S[k]
    r = {'f': float(s)}
    if t > 0.05:
        r['i'] = int(s)
    return r
"#,
    // mutation while iterating, an iterator object used twice
    r#"
def step(t, dt, inputs, state, params):
    a = inputs['a']
    l = [1.0, 2.0, 3.0]
    s = 0.0
    for x in l:
        s += x
        if len(l) < 6:
            l.append(x * 2)
    e = enumerate([10.0, 20.0, 30.0, 40.0])
    for i, x in e:
        break
    for i, x in e:
        s += x * i
    z = list(zip([1, 2, 3], (a, a)))
    d = {'p': 1.0, 'q': 2.0}
    for k in d:
        d[k] = d[k] * 2
    if a > 0:
        for k in d:
            d['new'] = 1.0
    return {'s': s, 'n': len(l), 'z': len(z), 'p': d['p']}
"#,
    // tables changed between lookups (maps.py keeps a parse while the
    // table still compares equal: -0.0 for 0.0 changes nothing)
    r#"
T = {0: 0.0, 1: 1.0, 2: 4.0}
T2 = {0: {0: 0.0, 1: 1.0}, 1: {0: 2.0, 1: 3.0}}
def step(t, dt, inputs, state, params):
    a = inputs['a']
    r = {'a1': interp(T, a)}
    T[0] = -0.0
    r['a2'] = interp(T, -1.0)
    T[1] = 1.0 + (t > 0.05)
    r['a3'] = interp(T, 0.5)
    r['b1'] = interp(T2, a, 0.5)
    T2[0][1] = 3.0 + t
    r['b2'] = interp(T2, 0.0, 1.0)
    return r
"#,
    // index, count and print
    r#"
def step(t, dt, inputs, state, params):
    a = inputs['a']
    l = [1.0, 2.0, 3.0, 2.0]
    print('x', 1, a)
    r = {'i': l.index(2.0), 'c': l.count(2.0), 'ti': (1, 2, 3).index(3)}
    if a > 0:
        r['j'] = l.index(5.0)
    return r
"#,
];

/// Scripts Python refuses when it reads them: the native runner must too.
const REFUSED: [&str; 15] = [
    "def step(t, dt, inputs, state, params):\n    break\n",
    "def step(t, dt, inputs, state, params):\n    for i in range(2):\n        pass\n    else:\n        continue\n",
    "X = 1_\ndef step(t, dt, inputs, state, params):\n    return {}\n",
    "X = 1__0\ndef step(t, dt, inputs, state, params):\n    return {}\n",
    "X = 1._5\ndef step(t, dt, inputs, state, params):\n    return {}\n",
    "X = 1e5_\ndef step(t, dt, inputs, state, params):\n    return {}\n",
    "G = 1\ndef step(t, dt, inputs, state, params):\n    y = G\n    global G\n    return {'y': y}\n",
    "def step(t, dt, inputs, state, params):\n\tif t:\n        return {}\n",
    "def f(x):\n    return x\ndef step(t, dt, inputs, state, params):\n    return {'y': f(if=1)}\n",
    "import os\ndef step(t, dt, inputs, state, params):\n    return {}\n",
    "def step(t, dt, inputs, state, params):\n    return {'y': open('x')}\n",
    "X = 1\n",
    "def step(t, dt, inputs, state, params):\n    y = 1.0 +\r2.0\n    return {'y': y}\n",
    "def step(t, dt, inputs, state, params):\n    s = 'a\rb'\n    return {'y': len(s)}\n",
    "def step(t, dt, inputs, state, params):\n    return {'y': 1.0}  # \0\n",
];

/// A single statement that does a lot of work (comparing two lists of a
/// million items, again and again) still stops near the time limit: the
/// clock is read by the work done, not only between statements.
#[test]
fn one_heavy_statement_stops_at_the_time_limit() {
    let code = "def step(t, dt, inputs, state, params):\n    a = list(range(999999))\n    b = list(range(999999))\n    b[-1] = 0\n    n = 0\n    while True:\n        if a == b:\n            n += 1\n";
    let mut r = Runner::start(code, "heavy", &serde_json::Map::new()).expect("it starts");
    let started = std::time::Instant::now();
    let got = r.step(0.0, 0.01, &[]);
    let took = started.elapsed().as_secs_f64();
    assert!(matches!(got, Err(ScriptError::Unsupported(_))), "{got:?}");
    assert!(took < lsim_script::TIME_LIMIT_S + 1.0, "stopped after {took:.1} s");
}

/// Scripts at the edges: hostile shapes (very long chains, deep nesting,
/// containers holding themselves or shared 2^80 ways), the reviewers'
/// cases (`tests/cases`), and those Python refuses. On each the native
/// runner gives Python's bits or leaves the script to Python; it neither
/// crashes nor hangs.
#[test]
fn edge_scripts_give_pythons_results_or_go_to_python() {
    let mut cases = vec![];
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    let mut files: Vec<_> =
        std::fs::read_dir(&here).expect("the cases").map(|e| e.unwrap().path()).collect();
    files.sort();
    let inputs = |vals: &[f64]| -> Vec<Call> {
        vals.iter()
            .enumerate()
            .map(|(k, a)| Call { t: k as f64 * 0.01, dt: 0.01, inputs: vec![("a".into(), *a)] })
            .collect()
    };
    for f in files {
        let code = std::fs::read_to_string(&f).expect("a case");
        let params_text = code
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("# params: "))
            .unwrap_or("{}")
            .to_string();
        cases.push(Case {
            label: f.file_name().unwrap().to_string_lossy().into(),
            code,
            params_text,
            calls: inputs(&[0.5, -2.0, f64::NAN]),
            may_leave: true,
        });
    }
    let step = |body: String| format!("def step(t, dt, inputs, state, params):\n{body}");
    let generated = [
        step(format!("    return {{'y': {}}}\n", vec!["1"; 3000].join("+"))),
        step(format!("    return {{'y': {}}}\n", vec!["1"; 30000].join("+"))),
        step(format!("    x = [0]\n    return {{'y': x{}}}\n", "[0]".repeat(20000))),
        step(format!("    return {{'y': {}1{}}}\n", "(".repeat(150), ")".repeat(150))),
        step("    x = []\n    for i in range(300000):\n        x = [x]\n    return {'y': 1.0}\n".into()),
        step("    a = []\n    a.append(a)\n    b = []\n    b.append(b)\n    return {'y': 1.0 if a == b else 0.0}\n".into()),
        step("    a = [0.0]\n    for i in range(80):\n        a = [a, a]\n    return {'y': 1.0 if a in [1] else 0.0}\n".into()),
        step("    d = {}\n    for i in range(300000):\n        d[i] = i * 0.5\n    return {'y': d[299999]}\n".into()),
        step("    e = enumerate([1.0])\n    for i in range(100000):\n        e = enumerate(e)\n    return {'y': 1.0}\n".into()),
        step("    return {'y': interp(params['tbl'], 12345.5)}\n".into()),
    ];
    // a long table among the parameters (a drive cycle's, say)
    let table: Vec<String> = (0..30000).map(|i| format!("\"{i}\": {}", i as f64 * 0.25)).collect();
    let table = format!("{{\"tbl\": {{{}}}}}", table.join(", "));
    let n_generated = generated.len();
    for (k, code) in generated.into_iter().enumerate() {
        cases.push(Case {
            label: format!("hostile {k}"),
            code,
            params_text: if k + 1 == n_generated { table.clone() } else { "{}".into() },
            calls: inputs(&[1.0]),
            may_leave: true,
        });
    }
    let refused_from = cases.len();
    for (k, code) in REFUSED.iter().enumerate() {
        cases.push(Case {
            label: format!("refused {k}"),
            code: code.to_string(),
            params_text: "{}".into(),
            calls: inputs(&[1.0]),
            may_leave: true,
        });
    }
    let started = std::time::Instant::now();
    let py = on_python(&cases);
    for (case, p) in cases[refused_from..].iter().zip(&py[refused_from..]) {
        assert!(p.get("compile_error").is_some(), "Python accepted {}: {p}", case.code);
    }
    let tally = compare(&cases, &py);
    println!("edges: {tally:?} in {:.1} s", started.elapsed().as_secs_f64());
}

/// A random expression over a, b, c.
fn expr(r: &mut Rng, depth: usize) -> String {
    if depth == 0 || r.below(4) == 0 {
        return match r.below(6) {
            0 => "a".into(),
            1 => "b".into(),
            2 => "c".into(),
            3 => format!("{}", r.below(10) as i64 - 3),
            4 => format!("{:?}", SPECIAL[r.below(16)]),
            _ => format!("{:?}", (r.range(-50.0, 50.0) * 8.0).round() / 8.0),
        };
    }
    let x = expr(r, depth - 1);
    let y = expr(r, depth - 1);
    match r.below(22) {
        0 => format!("({x} + {y})"),
        1 => format!("({x} - {y})"),
        2 => format!("({x} * {y})"),
        3 => format!("({x} / {y})"),
        4 => format!("({x} // {y})"),
        5 => format!("({x} % {y})"),
        6 => format!("(-{x})"),
        7 => format!("abs({x})"),
        8 => format!("min({x}, {y})"),
        9 => format!("max({x}, {y}, {})", expr(r, depth - 1)),
        10 => format!("clamp({x}, {y}, {})", expr(r, depth - 1)),
        11 => format!("({x} if {y} < {} else {})", expr(r, depth - 1), expr(r, depth - 1)),
        12 => format!("({x} and {y})"),
        13 => format!("({x} or {y})"),
        14 => format!("({x} < {y} <= {})", expr(r, depth - 1)),
        15 => format!("float(int({x}))"),
        16 => format!("round({x})"),
        17 => format!("math.floor({x})"),
        18 => format!("interp(T, {x})"),
        19 => format!("({x} == {y})"),
        20 => format!("math.copysign({x}, {y})"),
        _ => format!("(not {x})"),
    }
}

#[test]
fn the_native_runner_gives_pythons_bits() {
    let mut cases = vec![];
    let hcu = hcu_code();
    lsim_script::check(&hcu).expect("the Hybrid Control Unit is in the subset");
    for (seed, cumulative) in [(1u64, false), (2, true), (3, false), (4, true)] {
        cases.push(Case {
            label: format!("Hybrid Control Unit (drive {seed})"),
            code: hcu.clone(),
            params_text: "{}".into(),
            calls: hcu_drive(seed, 6000, cumulative),
            may_leave: false,
        });
    }
    let mut r = Rng(0x5eed);
    for (k, code) in FEATURES.iter().enumerate() {
        cases.push(Case {
            label: format!("features {k}"),
            code: code.to_string(),
            params_text: r#"{"gain": 2.5, "a": [1, 2.5], "b": {"x": 1}}"#.into(),
            calls: special_calls(&mut r, &["a", "b", "c"], 400),
            may_leave: false,
        });
    }
    for k in 0..300 {
        let code = format!(
            "T = {{0: 1.0, 2.5: -3.0, 10: 4.0}}\ndef step(t, dt, inputs, state, params):\n    \
             a = inputs['a']; b = inputs['b']; c = inputs['c']\n    return {{'y': {}, 'z': {}}}\n",
            expr(&mut r, 4),
            expr(&mut r, 3)
        );
        cases.push(Case {
            label: format!("random {k}"),
            code,
            params_text: "{}".into(),
            calls: special_calls(&mut r, &["a", "b", "c"], 40),
            may_leave: true,
        });
    }
    let started = std::time::Instant::now();
    let py = on_python(&cases);
    let py_s = started.elapsed().as_secs_f64();
    assert_eq!(py.len(), cases.len());
    let started = std::time::Instant::now();
    let tally = compare(&cases, &py);
    println!(
        "{tally:?}; Python {py_s:.2} s, native and the comparison {:.2} s",
        started.elapsed().as_secs_f64()
    );
    assert!(tally.equal > 30_000, "{tally:?}");
    assert!(tally.both_failed > 100, "the error paths were exercised: {tally:?}");
    assert!(tally.left * 20 < tally.calls, "few random calls leave the subset: {tally:?}");
}
