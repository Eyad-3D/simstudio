//! The one-click accuracy check (DESIGN.md, *Accuracy reports*): the same
//! run with tolerances 10× tighter, and how far every output moved. The
//! movement estimates the global error of the original run (the tighter
//! run's own error being about ten times smaller), channel by channel, in
//! terms the user can judge.

use crate::{OutputGrid, RunInfo, SimResult, SolveError, SolverOptions, simulate};
use lsim_ir::runtime::{DiscreteBlock, ModelFunctions};

/// How far one output moved.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelChange {
    /// the channel (or `energy: <part>` for a part's energy books)
    pub name: String,
    /// the largest difference over the output grid, SI
    pub max_abs: f64,
    /// when it was largest, s
    pub at: f64,
    /// the channel's largest magnitude in either run
    pub scale: f64,
    /// `max_abs / scale` (0 for a channel that is zero throughout)
    pub relative: f64,
}

/// The original run, the tighter one, and the differences.
#[derive(Clone, Debug)]
pub struct AccuracyReport {
    /// the run at the given tolerances
    pub base: SimResult,
    /// the same run 10× tighter
    pub tight: SimResult,
    /// every channel's movement, the largest relative first
    pub channels: Vec<ChannelChange>,
    /// how far each event moved, s (by order of occurrence)
    pub event_shifts: Vec<(String, f64)>,
    /// whether both runs had the same events
    pub same_events: bool,
}

impl AccuracyReport {
    /// A sentence for the user.
    pub fn summary(&self) -> String {
        let worst = self.channels.first();
        let ev = self.event_shifts.iter().map(|e| e.1.abs()).fold(0.0, f64::max);
        match worst {
            Some(c) => format!(
                "10× tighter tolerances moved the outputs by at most {:.1e} of their range ({} by {:.3e} at t = {:.4} s){}{}",
                c.relative,
                c.name,
                c.max_abs,
                c.at,
                if self.event_shifts.is_empty() {
                    String::new()
                } else {
                    format!("; events by at most {ev:.1e} s")
                },
                if self.same_events { "" } else { "; the event sequence changed" }
            ),
            None => "no outputs to compare".into(),
        }
    }
}

/// Every channel's largest difference between two runs on the same grid
/// (and every part's energy books at the end), the largest relative first.
pub fn compare_runs(base: &SimResult, tight: &SimResult) -> Vec<ChannelChange> {
    let mut out = vec![];
    if base.times == tight.times {
        for (i, name) in base.names.iter().enumerate() {
            let (a, b) = (&base.values[i], &tight.values[i]);
            let mut c = ChannelChange {
                name: name.clone(),
                max_abs: 0.0,
                at: base.times.first().copied().unwrap_or(0.0),
                scale: 0.0,
                relative: 0.0,
            };
            for k in 0..a.len().min(b.len()) {
                c.scale = c.scale.max(a[k].abs()).max(b[k].abs());
                let diff = (a[k] - b[k]).abs();
                if diff > c.max_abs || diff.is_nan() {
                    c.max_abs = diff;
                    c.at = base.times[k];
                }
            }
            c.relative = if c.scale > 0.0 { c.max_abs / c.scale } else { 0.0 };
            out.push(c);
        }
    }
    if let (Some(ea), Some(eb)) = (&base.energy, &tight.energy) {
        let scale = ea.throughput.max(eb.throughput);
        for (pa, pb) in ea.parts.iter().zip(&eb.parts) {
            for (what, x, y) in [
                ("energy in", pa.energy_in, pb.energy_in),
                ("lost", pa.lost, pb.lost),
                ("stored change", pa.stored_change, pb.stored_change),
            ] {
                let diff = (x - y).abs();
                out.push(ChannelChange {
                    name: format!("energy: {} {what}", pa.name),
                    max_abs: diff,
                    at: base.times.last().copied().unwrap_or(0.0),
                    scale,
                    relative: if scale > 0.0 { diff / scale } else { 0.0 },
                });
            }
        }
    }
    out.sort_by(|a, b| b.relative.total_cmp(&a.relative));
    out
}

/// Runs the model at `opts` and 10× tighter, and compares. Sampled blocks
/// are initialised afresh for each run ([`DiscreteBlock::init`]).
pub fn accuracy_check(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    opts: &SolverOptions,
    grid: OutputGrid,
    blocks: &mut [Box<dyn DiscreteBlock>],
) -> Result<AccuracyReport, SolveError> {
    let base = simulate(model, info, opts, grid, blocks)?;
    let tight = simulate(model, info, &opts.tighter(), grid, blocks)?;
    Ok(report(base, tight))
}

/// The comparison of a run with its tighter twin.
pub fn report(base: SimResult, tight: SimResult) -> AccuracyReport {
    let channels = compare_runs(&base, &tight);
    let same_events = base.events.len() == tight.events.len()
        && base.events.iter().zip(&tight.events).all(|(a, b)| a.kind == b.kind);
    let event_shifts = base
        .events
        .iter()
        .zip(&tight.events)
        .filter(|(a, b)| a.kind == b.kind)
        .map(|(a, b)| (a.label.clone(), a.t - b.t))
        .collect();
    AccuracyReport { base, tight, channels, event_shifts, same_events }
}
