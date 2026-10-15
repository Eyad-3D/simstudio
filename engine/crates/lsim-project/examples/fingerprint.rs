//! A fingerprint of whole runs: one hash over every number each case
//! produces (output times, every channel's values, interval minima, maxima
//! and means, left limits, events, counts and energy books), to show two
//! builds or two switches give bitwise the same runs.
//!
//! ```text
//! cargo run --release -p lsim-project --example fingerprint -- [project/case ...]
//! ```
//!
//! Without arguments: every time-domain case of the example projects but
//! the hybrid's (whose Script runs on `LSIM_SCRIPTS`' runner; add them by
//! name). `LSIM_CONDITION_KERNELS=0` interprets the conditions instead of
//! using their compiled forms.

use lsim_project::golden::{self, PythonHost};
use lsim_solve::SolverOptions;
use serde_json::Value;
use std::path::PathBuf;

struct Fnv(u64);

impl Fnv {
    fn bytes(&mut self, b: &[u8]) {
        for x in b {
            self.0 ^= *x as u64;
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
    }
    fn f(&mut self, x: f64) {
        self.bytes(&x.to_bits().to_le_bytes());
    }
}

fn main() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut cases: Vec<String> = std::env::args().skip(1).collect();
    if cases.is_empty() {
        cases = [
            "aero-bev/case-hwfet",
            "aero-bev/case-udds",
            "aero-bev/case-wltc",
            "bev-car/case-city",
            "bev-car/case-city-live",
            "bev-car/case-wltc",
            "bev-car/case-wltc-hvac",
            "bev-car/case-wltc-summer",
            "bev-car/case-wltc-winter",
            "fs-electric/case-accel-75m",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
    }
    let python = std::env::var("LSIM_PYTHON")
        .ok()
        .map(|p| PythonHost { python: PathBuf::from(p), backend: repo.join("backend") });
    let solver = SolverOptions {
        rtol: 1e-6,
        atol: 1e-8,
        condition_kernels: std::env::var("LSIM_CONDITION_KERNELS").map_or(true, |v| v != "0"),
        ..Default::default()
    };
    let mut all = Fnv(0xcbf2_9ce4_8422_2325);
    for tag in cases {
        let (project, case) = tag.split_once('/').expect("project/case");
        let pj: Value = serde_json::from_str(
            &std::fs::read_to_string(repo.join(format!("backend/projects/{project}.json")))
                .expect("project"),
        )
        .expect("JSON");
        let run = golden::run_case(&pj, case, &solver, python.as_ref()).expect("the case runs");
        let r = &run.result;
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        r.times.iter().for_each(|x| h.f(*x));
        for table in [&r.values, &r.min, &r.max, &r.mean] {
            for c in table.iter() {
                c.iter().for_each(|x| h.f(*x));
            }
        }
        h.bytes(format!("{:?}", r.left_limits).as_bytes());
        for e in &r.events {
            h.f(e.t);
            h.bytes(format!("{e:?}").as_bytes());
        }
        h.bytes(format!("{:?}{:?}", r.stats, r.energy).as_bytes());
        all.bytes(&h.0.to_le_bytes());
        println!("{tag}: {:016x} ({:.2} s, {} steps)", h.0, run.run_seconds, r.stats.steps);
    }
    println!("all: {:016x}", all.0);
}
