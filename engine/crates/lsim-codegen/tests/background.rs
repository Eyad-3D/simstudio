//! A tiered model's background compilation stops when the last holder of
//! the model goes away, so models compiled and dropped in a row (an app
//! recompiling on every edit) do not pile up compiling threads.

#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, MachineCode, background_compiles, compile};
use std::time::{Duration, Instant};

#[test]
fn a_dropped_tiered_model_stops_its_compilation() {
    // large enough that its machine code takes a few hundred
    // milliseconds of one thread
    let m = synth::network(1430, 7);
    let opts = CodegenOptions { threads: 1, ..Default::default() };
    let (running0, cancelled0) = background_compiles();
    assert_eq!(running0, 0);
    let started = Instant::now();
    for _ in 0..4 {
        let j = compile(&m, &opts).expect("compiles");
        assert!(j.report.tiered);
        // a clone keeps it going; the last holder's drop stops it
        let k = j.clone();
        drop(j);
        assert_eq!(k.machine_code(), MachineCode::Compiling);
        drop(k);
    }
    // every background compilation stops, cancelled, well before one of
    // them would have finished
    while background_compiles().0 > 0 {
        assert!(started.elapsed() < Duration::from_secs(20), "{:?}", background_compiles());
        std::thread::sleep(Duration::from_millis(5));
    }
    let (_, cancelled) = background_compiles();
    assert_eq!(cancelled - cancelled0, 4);
    // and one kept compiles to the end
    let j = compile(&m, &opts).expect("compiles");
    let r = j.wait_machine_code().expect("tiered").expect("machine code");
    assert!(r.functions > 0 && j.machine_code() == MachineCode::Ready);
    assert_eq!(background_compiles().1, cancelled);
}
