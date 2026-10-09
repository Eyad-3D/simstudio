//! # lsim-py: the Python module `lightsim_engine`
//!
//! Built into a wheel with maturin (`maturin build --release -m
//! engine/crates/lsim-py/Cargo.toml --features extension-module`); the
//! `lightsim` Python package and the app server wrap it. The API it grows
//! into (DESIGN.md, *Python API*):
//!
//! ```python
//! import lightsim_engine as le
//! model = le.build(project_json, case_id, cache_dir=…)   # prepare + JIT
//! model.set_params({"el-battery.capacity_Ah": 210.0})   # no rebuild
//! run = model.simulate(mode="full", rtol=1e-6, output_step=1.0)
//! run.channels["el-battery:sig_soc"]      # NumPy arrays, today's names
//! run.min / run.max / run.mean / run.events / run.report / run.energy
//! runs = model.sweep([{…}, {…}], threads=4)  # parallel, GIL released
//! ```
//!
//! Stage 1 exposes the engine version and the spike, which proves the
//! PyO3 build and the GIL-free call into the engine.

#![allow(missing_docs)] // the #[pymodule] expansion has undocumented items

/// The Python module.
#[pyo3::pymodule]
mod lightsim_engine {
    use lsim_engine::spike::{CHANNELS, Exact, Params, battery_drive};
    use lsim_engine::{BuildOptions, Engine};
    use lsim_solve::{OutputGrid, SolverOptions};
    use pyo3::exceptions::PyRuntimeError;
    use pyo3::prelude::*;

    /// The engine's version.
    #[pyfunction]
    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Builds and runs the Stage 1 spike at `rtol` (the GIL is released
    /// while it runs). Returns (solve seconds, worst relative error against
    /// the exact answer, event time error in s).
    #[pyfunction]
    fn spike(py: Python<'_>, rtol: f64) -> PyResult<(f64, f64, f64)> {
        py.detach(|| {
            let p = Params::default();
            let model = Engine::standard()
                .build(&battery_drive(&p), &BuildOptions::default())
                .map_err(|d| PyRuntimeError::new_err(format!("{d:?}")))?;
            let grid = OutputGrid { t0: 0.0, t_end: 4.0, dt: 0.01 };
            let run = model
                .simulate(&SolverOptions { rtol, atol: rtol, ..Default::default() }, grid)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            let exact = Exact::new(&p);
            let mut worst = 0.0f64;
            for (j, name) in CHANNELS.iter().enumerate() {
                let ch = run.channel(name).expect("channel");
                let scale = run.times.iter().map(|t| exact.at(*t)[j].abs()).fold(0.0, f64::max);
                for (k, t) in run.times.iter().enumerate() {
                    worst = worst.max((ch[k] - exact.at(*t)[j]).abs() / scale);
                }
            }
            let event = run.events.first().map(|e| (e.t - exact.t_event).abs()).unwrap_or(f64::NAN);
            Ok((run.wall_seconds, worst, event))
        })
    }
}
