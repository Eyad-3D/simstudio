//! Risk R1 (DESIGN.md §17): what a 10 ms sampled block that changes
//! nothing costs. Runs the 20-stage ladder drive for 1800 s (a WLTC's
//! length) with no block, with a block reading a state (the rotor speed),
//! and with a block reading a computed channel (the motor current), and
//! prints the best of several runs and the machine's load.
//!
//! `cargo run --release -p lsim-solve --example r1_blocks`

mod models;

use lsim_ir::runtime::DiscreteBlock;
use lsim_solve::{BlockInfo, OutputGrid, RunInfo, SolverOptions, simulate};
use std::time::Instant;

struct Idle;

impl DiscreteBlock for Idle {
    fn name(&self) -> &str {
        "'Script'"
    }
    fn period(&self) -> f64 {
        0.01
    }
    fn init(&mut self, _: f64, _: &[f64], _: &mut [f64]) -> Result<(), String> {
        Ok(())
    }
    fn tick(&mut self, _: f64, i: &[f64], o: &mut [f64]) -> Result<(), String> {
        // reads its input, holds its output: changes nothing
        std::hint::black_box(i);
        o[0] = 0.0;
        Ok(())
    }
}

fn load() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .unwrap_or_default()
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

fn main() {
    let lib = models::library();
    let mut top = models::ladder_drive(20);
    // the block's output: a discrete variable the model holds (unused by
    // the equations, as a Script block's output that changes nothing)
    top.vars.push(lsim_ir::component::build::discrete("script_out", "1", 0.0, ""));
    let prepared = lsim_prep::prepare(&lib, &top, &Default::default()).expect("prepares");
    let jit = lsim_codegen::compile(&prepared, &Default::default()).expect("compiles");
    let info = RunInfo::from_prepared(&prepared);
    let out =
        prepared.discretes.iter().position(|v| prepared.flat.var(*v).name == "script_out").unwrap();
    let ch = |name: &str| prepared.flat.find_var(name).unwrap().0 as usize;
    let grid = OutputGrid { t0: 0.0, t_end: 1800.0, dt: 1.0 };
    let opts = SolverOptions::default();
    let reps: usize = std::env::var("REPS").ok().and_then(|s| s.parse().ok()).unwrap_or(7);
    println!(
        "{} states, {} channels; load before: {}",
        prepared.states.len(),
        prepared.flat.vars.len(),
        load()
    );
    let mut results = vec![];
    for (label, input) in [
        ("no block", None),
        ("block reads a state", Some(ch("rotor.w"))),
        ("block reads a computed channel", Some(ch("motor.i"))),
    ] {
        let mut info = info.clone();
        if let Some(i) = input {
            info.blocks = vec![BlockInfo {
                name: "'Script'".into(),
                inputs: vec![i],
                outputs: vec![out],
                period: 0.01,
            }];
        }
        let mut best = f64::MAX;
        let mut last = None;
        for _ in 0..reps {
            let mut blocks: Vec<Box<dyn DiscreteBlock>> =
                if input.is_some() { vec![Box::new(Idle)] } else { vec![] };
            let t0 = Instant::now();
            let r = simulate(&jit, &info, &opts, grid, &mut blocks).expect("runs");
            best = best.min(t0.elapsed().as_secs_f64());
            last = Some(r);
        }
        let r = last.unwrap();
        println!(
            "{label:<32} best {:.2} ms ({:.0}x real time), {} steps, {} ticks, {} changed",
            best * 1e3,
            1800.0 / best,
            r.stats.steps,
            r.report.block_ticks,
            r.report.block_changes
        );
        results.push(best);
    }
    println!(
        "overhead of an idle 10 ms block: {:+.1} % (state input), {:+.1} % (computed input); load after: {}",
        100.0 * (results[1] / results[0] - 1.0),
        100.0 * (results[2] / results[0] - 1.0),
        load()
    );
}
