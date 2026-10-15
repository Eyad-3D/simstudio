//! The cost of one call of the Hybrid Control Unit on the native runner
//! (release build): `cargo run --release -p lsim-script --example hcu_speed`.

use lsim_script::Runner;

fn main() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let text = std::fs::read_to_string(root.join("backend/projects/hybrid-car.json")).unwrap();
    let p: serde_json::Value = serde_json::from_str(&text).unwrap();
    let code = p["systems"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["elements"].as_array().unwrap().iter())
        .find(|e| e["id"] == "el-hcu")
        .unwrap()["parameterOverrides"]["code"]
        .as_str()
        .unwrap()
        .to_string();
    let mut r = Runner::start(&code, "Hybrid Control Unit", &Default::default()).unwrap();
    let n = 200_000;
    let t0 = std::time::Instant::now();
    let mut acc = 0.0;
    for k in 0..n {
        let t = k as f64 * 0.01;
        let v = 50.0 + 40.0 * lsim_ir::math::sin(t * 0.05);
        let ins = [
            ("soc", 55.0 + lsim_ir::math::sin(t * 0.001)),
            ("speed", v),
            ("traction", lsim_ir::math::sin(t * 0.3)),
            ("motor_rpm", v * 60.0),
            ("engine_rpm", v * 58.0),
        ];
        let out = r.step(t, 0.01, &ins).unwrap();
        acc += out[0].1;
    }
    let per = t0.elapsed().as_secs_f64() / n as f64;
    println!("{:.2} µs per call ({acc})", per * 1e6);
}
