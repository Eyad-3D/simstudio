//! Parameter sweeps in parallel (DESIGN.md, *Results, quadratures,
//! sweeps*): one compiled model shared by every worker (its functions are
//! pure), one integrator and buffer set per run, rayon's work stealing over
//! the sets. Each run is single-threaded inside (SUNDIALS, faer at
//! `Par::Seq`), so the runs do not contend.

use crate::{OutputGrid, RunInfo, SimResult, SolveError, SolverOptions, simulate};
use lsim_ir::runtime::ModelFunctions;
use rayon::prelude::*;

/// Runs the model once per parameter set (each a complete parameter
/// vector, SI) on `threads` threads (0: one per CPU), results in the order
/// of `sets`.
pub fn sweep(
    model: &(dyn ModelFunctions + Sync),
    info: &RunInfo,
    sets: &[Vec<f64>],
    opts: &SolverOptions,
    grid: OutputGrid,
    threads: usize,
) -> Vec<Result<SimResult, SolveError>> {
    let run = |p: &Vec<f64>| {
        let info = info.with_params(p);
        simulate(model, &info, opts, grid, &mut [])
    };
    if threads == 1 {
        return sets.iter().map(run).collect();
    }
    match rayon::ThreadPoolBuilder::new().num_threads(threads).build() {
        Ok(pool) => pool.install(|| sets.par_iter().map(run).collect()),
        Err(_) => sets.iter().map(run).collect(),
    }
}
