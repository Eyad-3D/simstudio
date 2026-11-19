//! What the parity test and the differential fuzzing share: the app's own
//! Script runner on Python (`tests/parity.py`), the cases and calls given
//! to both runners, a deterministic random source and the special values
//! inputs take.
#![allow(dead_code)]

use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

pub fn python() -> String {
    if let Ok(p) = std::env::var("LSIM_PYTHON") {
        return p;
    }
    for p in ["python3", "python"] {
        if Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()) {
            return p.into();
        }
    }
    panic!(
        "the parity test needs Python 3.11 or newer with the backend's requirements: set \
         LSIM_PYTHON or put python3 on the PATH"
    );
}

/// A deterministic random source (xorshift64*).
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

pub struct Call {
    pub t: f64,
    pub dt: f64,
    pub inputs: Vec<(String, f64)>,
}

pub struct Case {
    pub label: String,
    pub code: String,
    /// the parameters as JSON text: Python reads them in their own order,
    /// the native runner as the importer hands them over (sorted)
    pub params_text: String,
    pub calls: Vec<Call>,
    /// whether a call may leave the subset (random scripts may)
    pub may_leave: bool,
}

pub fn bits(x: f64) -> u64 {
    x.to_bits()
}

/// Every case's results on Python.
pub fn on_python(cases: &[Case]) -> Vec<Value> {
    try_on_python(cases, None).unwrap_or_else(|e| panic!("{e}"))
}

/// What Python writes when a call took longer than `call_limit`.
pub const TIMED_OUT: &str = "Timeout (";

/// Every case's results on Python, or what went wrong on its side. With a
/// `call_limit` (seconds), Python stops when a script's start or one call
/// takes longer, and says so ([`TIMED_OUT`]), as the engine stops the
/// app's worker (which has no clock of its own) past its time limit.
pub fn try_on_python(cases: &[Case], call_limit: Option<f64>) -> Result<Vec<Value>, String> {
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
                    "inputs": k.inputs.iter().map(|(n, v)| json!([n, bits(*v)])).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // `-X utf8`: what scripts print is written as UTF-8 on every platform
    let mut child = Command::new(python())
        .args(["-X", "utf8"])
        .arg(here.join("tests/parity.py"))
        .arg(repo().join("backend/app/solver"))
        .env("LSIM_PARITY_CALL_LIMIT", call_limit.map_or(String::new(), |s| s.to_string()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Python starts");
    let text = serde_json::to_string(&doc).expect("JSON");
    let mut stdin = child.stdin.take().expect("stdin");
    let writer = std::thread::spawn(move || stdin.write_all(text.as_bytes()));
    let out = child.wait_with_output().expect("Python finishes");
    let written = writer.join().expect("the writer");
    // Python's own words first: a Python that stops early (without the
    // backend's packages, say) also breaks the pipe the cases go through
    if !out.status.success() {
        return Err(format!(
            "the Python side failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    written.map_err(|e| format!("written: {e}"))?;
    serde_json::from_slice::<Vec<Value>>(&out.stdout).map_err(|e| format!("Python's results: {e}"))
}

pub const SPECIAL: [f64; 22] = [
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

pub fn special(r: &mut Rng) -> f64 {
    if r.below(3) == 0 { r.range(-1000.0, 1000.0) } else { SPECIAL[r.below(SPECIAL.len())] }
}
