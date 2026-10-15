//! One fast-mode run of the test car on the WLTC, for a profiler:
//! `valgrind --tool=callgrind target/release/examples/fast_profile`.

use lsim_engine::testcar::{self, car, inverse_spec, library, speed_trace};
use lsim_engine::{BuildOptions, Engine};
use lsim_fast::{FastOptions, Trace};

fn main() {
    let (t, v) = testcar::wltc();
    let engine = Engine::new(library(speed_trace("TestCar.Trace", &t, &v)));
    let inv = engine
        .build_inverse(&car("TestCar.Trace"), &inverse_spec(), &BuildOptions::default())
        .unwrap();
    let _ = inv.jit.wait_machine_code();
    let traces = [Trace::new(t, v).unwrap()];
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    for _ in 0..n {
        let r = inv.fast(&traces, &FastOptions::default()).unwrap();
        println!("{:.3} ms", r.wall_seconds * 1e3);
    }
}
