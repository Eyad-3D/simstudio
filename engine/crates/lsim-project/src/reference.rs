//! The exact-answer problems of `benchmarks/reference/problems/` (Stage 0):
//! their parameters, initial values, run settings and the checkpoints the
//! exact solution gives (the answer at four times, and the event times).
//! Reads the subset of TOML those files use.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One reference problem.
#[derive(Clone, Debug, Default)]
pub struct Problem {
    /// its id (= the file name)
    pub id: String,
    /// `[parameters]`: name → value (in the file's unit)
    pub params: BTreeMap<String, f64>,
    /// `[initial]`: name → value
    pub initial: BTreeMap<String, f64>,
    /// `[run]`: t_end, output_dt
    pub run: BTreeMap<String, f64>,
    /// checkpoint times
    pub times: Vec<f64>,
    /// quantity → its exact value at each checkpoint time
    pub checkpoints: BTreeMap<String, Vec<f64>>,
    /// event → its exact time
    pub events: BTreeMap<String, f64>,
}

impl Problem {
    /// A parameter's value.
    pub fn p(&self, name: &str) -> f64 {
        *self.params.get(name).unwrap_or_else(|| panic!("{}: no parameter {name}", self.id))
    }
    /// An initial value.
    pub fn init(&self, name: &str) -> f64 {
        *self.initial.get(name).unwrap_or_else(|| panic!("{}: no initial {name}", self.id))
    }
    /// The run's end time.
    pub fn t_end(&self) -> f64 {
        self.run["t_end"]
    }
}

/// Where the problems are, from this crate.
pub fn problems_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../benchmarks/reference/problems")
}

fn number(text: &str) -> Option<f64> {
    text.trim().replace('_', "").parse().ok()
}

fn list(text: &str) -> Vec<f64> {
    text.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(number)
        .collect()
}

/// Reads a problem file's parameters, run settings and checkpoints.
pub fn load(id: &str) -> Result<Problem, String> {
    let path = problems_dir().join(format!("{id}.toml"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut pr = Problem { id: id.into(), ..Default::default() };
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        match section.as_str() {
            "parameters" | "initial" => {
                // name = { value = X, unit = "…", note = "…" }
                if let Some(rest) = value.split("value").nth(1)
                    && let Some(v) = rest.trim_start().strip_prefix('=')
                {
                    let num: String = v
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || "+-.eE_".contains(*c))
                        .collect();
                    if let Some(x) = number(&num) {
                        let map =
                            if section == "parameters" { &mut pr.params } else { &mut pr.initial };
                        map.insert(key.into(), x);
                    }
                }
            }
            "run" => {
                if let Some(x) = number(value) {
                    pr.run.insert(key.into(), x);
                }
            }
            "checkpoints" => {
                if key == "times" {
                    pr.times = list(value);
                } else {
                    pr.checkpoints.insert(key.into(), list(value));
                }
            }
            "checkpoints.events" => {
                if let Some(x) = number(value) {
                    pr.events.insert(key.into(), x);
                }
            }
            _ => {}
        }
    }
    if pr.times.is_empty() {
        return Err(format!("{id}: no checkpoints"));
    }
    Ok(pr)
}

/// The problems' ids.
pub fn ids() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(problems_dir())
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str()?.strip_suffix(".toml").map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_a_problem() {
        let p = super::load("mech_gear_change").unwrap();
        assert_eq!(p.p("J2"), 135.0);
        assert_eq!(p.t_end(), 8.0);
        assert_eq!(p.times, vec![2.0, 4.0, 6.0, 8.0]);
        assert!((p.checkpoints["E_shift"][3] - 2797.7719709762023).abs() < 1e-9);
        let q = super::load("veh_coastdown").unwrap();
        assert!((q.events["t_stop"] - 193.18305719109122).abs() < 1e-12);
        assert_eq!(super::ids().len(), 14);
    }
}
