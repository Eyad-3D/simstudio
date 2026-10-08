//! The fixed-step inverse-model stepper.
//!
//! The model is `x' = f(t, x, z, p, d, u)`, `0 = g(t, x, z, p, d, u)`. The
//! stepper treats it in *state-space form*: wherever it evaluates the model
//! it first solves `g = 0` for the iteration variables `z` by Newton's
//! method (with the LU of `∂g/∂z`, refreshed only when convergence slows),
//! so every evaluated point is consistent and every recorded channel and
//! flag comes from a point that satisfies all the equations exactly (to
//! Newton's tolerance). The remaining states `x` are stepped by the
//! Rosenbrock-W method with the Jacobian of the state-space form,
//! `S = f_x - f_z g_z⁻¹ g_x`, kept over many steps: a W-method keeps its
//! order with any matrix there, so a stale Jacobian costs nothing in order
//! and only matters for the stability of stiff components; the Jacobian is
//! refreshed when the embedded error estimate says the old one has gone
//! bad.
//!
//! Each step `[t_n, t_{n+1}]` sees the traces' values on one segment (the
//! step grid is split at every trace sample inside a step), so the
//! prescribed speed is linear and its derivative constant within it. The
//! recorded value at `t_{n+1}` is the left limit (the end of the step, with
//! the step's own acceleration); each output interval's minimum, maximum
//! and mean come from the start, the middle (cubic Hermite interpolation of
//! the states, made consistent) and the end of every step inside it, the
//! mean by Simpson's rule.
//!
//! `when` clauses: a zero crossing between the start and the end of a step
//! is located on the Hermite interpolant (Illinois method, every point made
//! consistent), the step is redone up to the event, the clause's actions
//! run, and stepping goes on from there. Sampled blocks
//! ([`lsim_ir::DiscreteBlock`]) tick at the start of the first step at or
//! after each of their tick times.

#![allow(clippy::needless_range_loop)] // index loops read like the formulas

use crate::limits::FlagTracker;
use crate::lu::Lu;
use crate::ros::Tableau;
use crate::{BoundBlock, FastError, FastEvent, FastProblem, FastReport, FastResult, FixedMethod};
use lsim_ir::prepared::Direction;
use lsim_ir::runtime::{EvalInput, ModelFunctions};
use std::time::Instant;

const MAX_NEWTON: usize = 30;
const MAX_EVENTS_PER_STEP: usize = 64;

struct Stepper<'a> {
    m: &'a dyn ModelFunctions,
    p: &'a [f64],
    tab: Tableau,
    nx: usize,
    nz: usize,
    ny: usize,
    nom: Vec<f64>,
    d: Vec<f64>,
    u: Vec<f64>,
    work: Vec<f64>,
    out: Vec<f64>,
    y: Vec<f64>,
    seg: Vec<usize>,
    jac: Vec<f64>,
    schur: Vec<f64>,
    wbuf: Vec<f64>,
    gz: Lu,
    gz_valid: bool,
    wlu: Vec<(f64, Lu)>,
    jac_valid: bool,
    jac_age: usize,
    err_ref: f64,
    stage: Vec<Vec<f64>>,
    rhs: Vec<f64>,
    tmp: Vec<f64>,
    jvp_v: Vec<f64>,
    jvp_out: Vec<f64>,
    newton_tol: f64,
    rep: FastReport,
}

impl<'a> Stepper<'a> {
    fn set_inputs(&mut self, traces: &[crate::Trace], t: f64) {
        for (i, tr) in traces.iter().enumerate() {
            let (v, s) = tr.on_segment(self.seg[i], t);
            self.u[2 * i] = v;
            self.u[2 * i + 1] = s;
        }
    }

    fn residual(&mut self, t: f64) {
        let inp = EvalInput { t, y: &self.y, p: self.p, d: &self.d, u: &self.u };
        self.m.residual(&inp, &mut self.work, &mut self.out);
        self.rep.residual_evals += 1;
    }

    fn vars_into(&mut self, t: f64, out: &mut [f64]) {
        let inp = EvalInput { t, y: &self.y, p: self.p, d: &self.d, u: &self.u };
        self.m.vars(&inp, &mut self.work, out);
        self.rep.vars_evals += 1;
    }

    fn roots_into(&mut self, t: f64, out: &mut [f64]) {
        let inp = EvalInput { t, y: &self.y, p: self.p, d: &self.d, u: &self.u };
        self.m.roots(&inp, &mut self.work, out);
    }

    /// Refreshes the LU of `∂g/∂z` at the current point.
    fn refresh_gz(&mut self, t: f64) -> Result<(), FastError> {
        let (nx, nz, ny) = (self.nx, self.nz, self.ny);
        let mut block = vec![0.0; nz * nz];
        for k in 0..nz {
            self.jvp_v.iter_mut().for_each(|v| *v = 0.0);
            self.jvp_v[nx + k] = 1.0;
            let inp = EvalInput { t, y: &self.y, p: self.p, d: &self.d, u: &self.u };
            self.m.jvp(&inp, &self.jvp_v, &mut self.work, &mut self.jvp_out);
            block[k * nz..(k + 1) * nz].copy_from_slice(&self.jvp_out[nx..ny]);
        }
        self.rep.gz_refreshes += 1;
        self.gz.factor(&block).map_err(|_| FastError::Singular {
            t,
            what: "the algebraic equations' Jacobian with respect to the iteration variables"
                .into(),
        })?;
        self.gz_valid = true;
        Ok(())
    }

    /// Solves `g(t, x, z) = 0` for `z` (x = `y[..nx]`, z starts from
    /// `y[nx..]`); on return `out` holds the residual `[x'; g]` at the
    /// accepted point.
    fn solve_z(&mut self, t: f64) -> Result<(), FastError> {
        self.residual(t);
        if self.nz == 0 {
            return Ok(());
        }
        let (nx, ny) = (self.nx, self.ny);
        if !self.gz_valid {
            self.refresh_gz(t)?;
        }
        let mut prev = f64::INFINITY;
        let mut refreshes = 0;
        for it in 0..MAX_NEWTON {
            if self.out[nx..ny].iter().any(|v| !v.is_finite()) {
                return Err(FastError::NoConvergence {
                    t,
                    what: "the algebraic equations give a value that is not a number".into(),
                });
            }
            for k in 0..self.nz {
                self.tmp[k] = -self.out[nx + k];
            }
            self.gz.solve(&mut self.tmp[..self.nz]);
            let mut conv = 0.0f64;
            for k in 0..self.nz {
                let scale = self.y[nx + k].abs().max(self.nom[nx + k]);
                conv = conv.max(self.tmp[k].abs() / scale);
            }
            if conv <= self.newton_tol {
                return Ok(());
            }
            // slow (or no) convergence with the kept LU: a fresh one (full
            // Newton from here on while it stays slow)
            if it > 0 && conv > 0.25 * prev && refreshes < 8 {
                self.refresh_gz(t)?;
                refreshes += 1;
                prev = f64::INFINITY;
                continue;
            }
            prev = conv;
            self.rep.newton_iterations += 1;
            // apply, halving the step while the residual is not finite
            let mut lambda = 1.0;
            for _ in 0..40 {
                for k in 0..self.nz {
                    self.y[nx + k] += lambda * self.tmp[k];
                }
                self.residual(t);
                if self.out.iter().all(|v| v.is_finite()) {
                    break;
                }
                for k in 0..self.nz {
                    self.y[nx + k] -= lambda * self.tmp[k];
                }
                lambda *= 0.5;
            }
        }
        Err(FastError::NoConvergence {
            t,
            what: format!(
                "the algebraic equations did not converge in {MAX_NEWTON} Newton iterations"
            ),
        })
    }

    /// Refreshes the Jacobian (and the Schur complement) at the current,
    /// consistent point.
    fn refresh_jacobian(&mut self, t: f64) -> Result<(), FastError> {
        let (nx, nz, ny) = (self.nx, self.nz, self.ny);
        {
            let inp = EvalInput { t, y: &self.y, p: self.p, d: &self.d, u: &self.u };
            self.m.jacobian_dense(&inp, &mut self.work, &mut self.jac);
        }
        self.rep.jacobian_evals += 1;
        let jac = &self.jac;
        let at = |r: usize, c: usize| jac[c * ny + r];
        if nz > 0 {
            let mut block = vec![0.0; nz * nz];
            for c in 0..nz {
                for r in 0..nz {
                    block[c * nz + r] = at(nx + r, nx + c);
                }
            }
            self.gz.factor(&block).map_err(|_| FastError::Singular {
                t,
                what: "the algebraic equations' Jacobian with respect to the iteration variables"
                    .into(),
            })?;
            self.gz_valid = true;
        }
        // S = f_x - f_z (g_z⁻¹ g_x)
        let mut col = vec![0.0; nz];
        for c in 0..nx {
            for r in 0..nx {
                self.schur[c * nx + r] = at(r, c);
            }
            if nz > 0 {
                for r in 0..nz {
                    col[r] = at(nx + r, c);
                }
                self.gz.solve(&mut col);
                for r in 0..nx {
                    let mut s = 0.0;
                    for (k, g) in col.iter().enumerate() {
                        s += at(r, nx + k) * g;
                    }
                    self.schur[c * nx + r] -= s;
                }
            }
        }
        self.wlu.clear();
        self.jac_valid = true;
        self.jac_age = 0;
        Ok(())
    }

    /// The LU of `W = I/(γh) - S` for step `h`.
    fn w_index(&mut self, h: f64, t: f64) -> Result<usize, FastError> {
        if let Some(i) = self.wlu.iter().position(|(hh, _)| (hh - h).abs() <= 1e-12 * h) {
            return Ok(i);
        }
        let nx = self.nx;
        let g = 1.0 / (self.tab.gamma * h);
        for c in 0..nx {
            for r in 0..nx {
                self.wbuf[c * nx + r] = if r == c { g } else { 0.0 } - self.schur[c * nx + r];
            }
        }
        let mut lu = Lu::new(nx);
        lu.factor(&self.wbuf).map_err(|_| FastError::Singular {
            t,
            what: "the step matrix (I/(γh) - J) of the states".into(),
        })?;
        self.rep.lu_factorisations += 1;
        if self.wlu.len() >= 4 {
            self.wlu.remove(0);
        }
        self.wlu.push((h, lu));
        Ok(self.wlu.len() - 1)
    }

    fn scaled_error(&self, x0: &[f64], x1: &[f64], err: &[f64], o: &crate::FastOptions) -> f64 {
        if err.is_empty() {
            return 0.0;
        }
        let mut s = 0.0;
        for i in 0..err.len() {
            let w = o.atol * self.nom[i] + o.rtol * x0[i].abs().max(x1[i].abs());
            s += (err[i] / w).powi(2);
        }
        (s / err.len() as f64).sqrt()
    }
}

/// The state after a (sub)step.
struct StepEnd {
    x: Vec<f64>,
    z: Vec<f64>,
    f: Vec<f64>,
}

/// Runs fast mode.
pub fn run(prob: &FastProblem<'_>, blocks: &mut [BoundBlock]) -> Result<FastResult, FastError> {
    let started = Instant::now();
    let o = prob.opts;
    let m = prob.model;
    let l = *m.layout();
    let (nx, nz, ny) = (l.n_x, l.n_z, l.n_y());
    if l.n_u != 2 * prob.traces.len() {
        return Err(FastError::Inputs(format!(
            "the inverse model takes {} inputs (a value and its derivative per prescribed \
             variable) but {} traces were given",
            l.n_u,
            prob.traces.len()
        )));
    }
    if prob.params.len() != l.n_p {
        return Err(FastError::Inputs(format!(
            "the model has {} parameters but {} values were given",
            l.n_p,
            prob.params.len()
        )));
    }
    for tr in prob.traces {
        tr.check()?;
    }
    if !(o.step > 0.0 && o.step.is_finite()) {
        return Err(FastError::Inputs(format!("the step must be positive, not {}", o.step)));
    }
    let t_start = o
        .t_start
        .unwrap_or_else(|| prob.traces.iter().map(|t| t.start()).fold(f64::NEG_INFINITY, f64::max));
    let t_end = o
        .t_end
        .unwrap_or_else(|| prob.traces.iter().map(|t| t.end()).fold(f64::INFINITY, f64::min));
    if prob.traces.is_empty() && (o.t_start.is_none() || o.t_end.is_none()) {
        return Err(FastError::Inputs("without traces, give the start and end times".into()));
    }
    if t_end.partial_cmp(&t_start) != Some(std::cmp::Ordering::Greater) {
        return Err(FastError::Inputs(format!(
            "nothing to run: the traces span [{t_start}, {t_end}] s"
        )));
    }
    let tab = match o.method {
        FixedMethod::RosenbrockW => Tableau::ros34pw2(),
        FixedMethod::LinearImplicitEuler => Tableau::linear_implicit_euler(),
    };

    // the step grid: uniform, split at every trace sample inside a step
    let n_out = ((t_end - t_start) / o.step - 1e-9).ceil().max(1.0) as usize;
    let grid: Vec<f64> = (0..=n_out).map(|k| (t_start + k as f64 * o.step).min(t_end)).collect();
    let mut bounds: Vec<(f64, bool)> = grid.iter().map(|t| (*t, true)).collect();
    let eps = 1e-9 * o.step;
    for tr in prob.traces {
        for &tk in &tr.t {
            if tk > t_start + eps && tk < t_end - eps {
                let k = ((tk - t_start) / o.step).floor() as usize;
                let near = grid.get(k).is_some_and(|g| (g - tk).abs() <= eps)
                    || grid.get(k + 1).is_some_and(|g| (g - tk).abs() <= eps);
                if !near {
                    bounds.push((tk, false));
                }
            }
        }
    }
    bounds.sort_by(|a, b| a.0.total_cmp(&b.0));
    bounds.dedup_by(|b, a| {
        (b.0 - a.0).abs() <= eps && {
            a.1 |= b.1;
            true
        }
    });

    let nom: Vec<f64> =
        (0..ny).map(|i| prob.y_nominal.get(i).copied().unwrap_or(1.0).abs().max(1e-300)).collect();
    let mut y0 = vec![0.0; ny];
    let mut d0 = vec![0.0; l.n_d];
    m.start(prob.params, &mut y0, &mut d0);
    let s = tab.s;
    let mut st = Stepper {
        m,
        p: prob.params,
        tab,
        nx,
        nz,
        ny,
        nom,
        d: d0,
        u: vec![0.0; l.n_u],
        work: vec![0.0; l.n_work],
        out: vec![0.0; ny],
        y: y0,
        seg: prob.traces.iter().map(|_| 0).collect(),
        jac: vec![0.0; ny * ny],
        schur: vec![0.0; nx * nx],
        wbuf: vec![0.0; nx * nx],
        gz: Lu::new(nz),
        gz_valid: false,
        wlu: vec![],
        jac_valid: false,
        jac_age: 0,
        err_ref: f64::INFINITY,
        stage: vec![vec![0.0; nx]; s],
        rhs: vec![0.0; nx],
        tmp: vec![0.0; nz.max(nx)],
        jvp_v: vec![0.0; ny],
        jvp_out: vec![0.0; ny],
        newton_tol: o.newton_tol,
        rep: FastReport { method: "", n_x: nx, n_z: nz, ..Default::default() },
    };
    st.rep.method = st.tab.name;

    // what to record
    let all: Vec<usize>;
    let rec: &[usize] = match prob.record {
        Some(r) => r,
        None => {
            all = (0..l.n_vars).collect();
            &all
        }
    };
    let nrec = rec.len();
    let names: Vec<String> = rec
        .iter()
        .map(|&i| prob.names.get(i).cloned().unwrap_or_else(|| format!("v[{i}]")))
        .collect();
    let need_vars = nrec > 0 || !prob.limits.is_empty() || !blocks.is_empty();
    let stats = o.interval_stats;
    let mut times = Vec::with_capacity(n_out + 1);
    let mut values = Vec::with_capacity((n_out + 1) * nrec);
    let mut vmin = Vec::with_capacity((n_out + 1) * nrec);
    let mut vmax = Vec::with_capacity((n_out + 1) * nrec);
    let mut vmean = Vec::with_capacity((n_out + 1) * nrec);
    let mut acc_lo = vec![f64::INFINITY; nrec];
    let mut acc_hi = vec![f64::NEG_INFINITY; nrec];
    let mut acc_int = vec![0.0; nrec];
    let mut acc_t0 = t_start;
    let mut tracker = FlagTracker::new(prob.limits, o.flag_rtol);
    let mut events: Vec<FastEvent> = vec![];
    let nv = l.n_vars;
    let mut v_start = vec![0.0; nv];
    let mut v_mid = vec![0.0; nv];
    let mut v_end = vec![0.0; nv];
    let nr = l.n_roots;
    let mut r_start = vec![0.0; nr];
    let mut r_end = vec![0.0; nr];
    let mut x_n = vec![0.0; nx];
    let mut f_n = vec![0.0; nx];
    let mut next_tick: Vec<f64> = blocks.iter().map(|b| t_start + b.block.offset()).collect();

    // the first point: consistent at the start, then the blocks' initial outputs
    let first_seg = |tr: &crate::Trace, a: f64, b: f64| tr.segment(0.5 * (a + b));
    let (b0, b1) = (bounds[0].0, bounds.get(1).map(|b| b.0).unwrap_or(t_end));
    for (i, tr) in prob.traces.iter().enumerate() {
        st.seg[i] = first_seg(tr, b0, b1);
    }
    st.set_inputs(prob.traces, t_start);
    st.solve_z(t_start)?;
    if !blocks.is_empty() {
        st.vars_into(t_start, &mut v_start);
        let mut changed = false;
        for b in blocks.iter_mut() {
            let ins: Vec<f64> = b.inputs.iter().map(|&i| v_start[i]).collect();
            let mut outs: Vec<f64> = b.outputs.iter().map(|&k| st.d[k]).collect();
            b.block
                .init(t_start, &ins, &mut outs)
                .map_err(|e| FastError::Block { name: b.block.name().to_string(), message: e })?;
            for (k, v) in b.outputs.iter().zip(outs) {
                if st.d[*k] != v {
                    st.d[*k] = v;
                    changed = true;
                }
            }
        }
        if changed {
            st.solve_z(t_start)?;
        }
    }
    st.refresh_jacobian(t_start)?;
    x_n.copy_from_slice(&st.y[..nx]);
    f_n.copy_from_slice(&st.out[..nx]);
    let mut z_n: Vec<f64> = st.y[nx..].to_vec();
    if need_vars {
        st.vars_into(t_start, &mut v_start);
    }
    if nr > 0 {
        st.roots_into(t_start, &mut r_start);
    }
    tracker.point(t_start, &v_start);
    times.push(t_start);
    for &i in rec {
        values.push(v_start[i]);
        vmin.push(v_start[i]);
        vmax.push(v_start[i]);
        vmean.push(v_start[i]);
    }

    for w in 0..bounds.len() - 1 {
        let (ta, tb) = (bounds[w].0, bounds[w + 1].0);
        let is_output = bounds[w + 1].1;
        // the traces' segments for this step; a new slope makes a new start point
        let mut new_seg = false;
        for (i, tr) in prob.traces.iter().enumerate() {
            let k = first_seg(tr, ta, tb);
            if k != st.seg[i] {
                st.seg[i] = k;
                new_seg = true;
            }
        }
        // sampled blocks due now
        let due: Vec<usize> =
            (0..blocks.len()).filter(|&b| ta >= next_tick[b] - 1e-9 * o.step.max(1.0)).collect();
        if new_seg || !due.is_empty() {
            st.y[..nx].copy_from_slice(&x_n);
            st.y[nx..].copy_from_slice(&z_n);
            st.set_inputs(prob.traces, ta);
            st.solve_z(ta)?;
            if !due.is_empty() {
                st.vars_into(ta, &mut v_start);
                let mut changed = false;
                for &bi in &due {
                    let b = &mut blocks[bi];
                    let ins: Vec<f64> = b.inputs.iter().map(|&i| v_start[i]).collect();
                    let mut outs: Vec<f64> = b.outputs.iter().map(|&k| st.d[k]).collect();
                    b.block.tick(ta, &ins, &mut outs).map_err(|e| FastError::Block {
                        name: b.block.name().to_string(),
                        message: e,
                    })?;
                    let period = b.block.period();
                    next_tick[bi] = if period > 0.0 {
                        let mut n = next_tick[bi];
                        while n <= ta + 1e-9 * o.step.max(1.0) {
                            n += period;
                        }
                        n
                    } else {
                        ta + 0.5 * (tb - ta)
                    };
                    for (k, v) in b.outputs.iter().zip(outs) {
                        if st.d[*k] != v {
                            st.d[*k] = v;
                            changed = true;
                        }
                    }
                }
                if changed {
                    st.rep.block_changes += 1;
                    st.solve_z(ta)?;
                }
            }
            z_n.copy_from_slice(&st.y[nx..]);
            f_n.copy_from_slice(&st.out[..nx]);
            if need_vars {
                st.vars_into(ta, &mut v_start);
                tracker.point(ta, &v_start);
            }
            if nr > 0 {
                st.roots_into(ta, &mut r_start);
            }
        }

        // the step, split at events
        let mut t0 = ta;
        let mut n_events = 0;
        loop {
            let end = advance(&mut st, prob, o, t0, tb, &x_n, &z_n, &f_n)?;
            // events between t0 and tb?
            let mut fired_at: Option<(f64, Vec<f64>)> = None;
            if nr > 0 && !prob.whens.is_empty() {
                st.roots_into(tb, &mut r_end);
                let crosses = |r0: f64, r1: f64, dir: Direction| match dir {
                    Direction::Rising => r0 < 0.0 && r1 >= 0.0,
                    Direction::Falling => r0 > 0.0 && r1 <= 0.0,
                    Direction::Both => (r0 < 0.0 && r1 >= 0.0) || (r0 > 0.0 && r1 <= 0.0),
                };
                let mut best: Option<f64> = None;
                for w in prob.whens.iter() {
                    let (r0, r1) = (r_start[w.crossing], r_end[w.crossing]);
                    if crosses(r0, r1, w.direction) {
                        let th =
                            locate(&mut st, prob, t0, tb, &x_n, &f_n, &end, w.crossing, r0, r1)?;
                        best = Some(best.map_or(th, |b: f64| b.min(th)));
                    }
                }
                if let Some(th) = best {
                    let te = t0 + th * (tb - t0);
                    // the clauses that fire at te: their crossing has changed sign by then
                    let mut r_te = vec![0.0; nr];
                    hermite_point(&mut st, prob, t0, tb, &x_n, &f_n, &end, th)?;
                    st.roots_into(te, &mut r_te);
                    let mut fired = vec![0.0; l.n_whens];
                    for (k, w) in prob.whens.iter().enumerate() {
                        if crosses(r_start[w.crossing], r_te[w.crossing], w.direction) {
                            fired[k] = 1.0;
                        }
                    }
                    if fired.iter().any(|f| *f != 0.0) {
                        fired_at = Some((te, fired));
                    }
                }
            }
            match fired_at {
                None => {
                    record_step(
                        &mut st,
                        prob,
                        t0,
                        tb,
                        &x_n,
                        &f_n,
                        &end,
                        need_vars,
                        stats,
                        &mut v_start,
                        &mut v_mid,
                        &mut v_end,
                        rec,
                        &mut acc_lo,
                        &mut acc_hi,
                        &mut acc_int,
                        &mut tracker,
                    )?;
                    x_n.copy_from_slice(&end.x);
                    z_n.copy_from_slice(&end.z);
                    f_n.copy_from_slice(&end.f);
                    if nr > 0 {
                        r_start.copy_from_slice(&r_end);
                    }
                    break;
                }
                Some((te, fired)) => {
                    n_events += 1;
                    if n_events > MAX_EVENTS_PER_STEP {
                        return Err(FastError::NoConvergence {
                            t: te,
                            what: "events keep firing (more than 64 in one step)".into(),
                        });
                    }
                    // redo the step up to the event, record it, apply the clauses
                    let end_e = advance(&mut st, prob, o, t0, te, &x_n, &z_n, &f_n)?;
                    record_step(
                        &mut st,
                        prob,
                        t0,
                        te,
                        &x_n,
                        &f_n,
                        &end_e,
                        need_vars,
                        stats,
                        &mut v_start,
                        &mut v_mid,
                        &mut v_end,
                        rec,
                        &mut acc_lo,
                        &mut acc_hi,
                        &mut acc_int,
                        &mut tracker,
                    )?;
                    st.y[..nx].copy_from_slice(&end_e.x);
                    st.y[nx..].copy_from_slice(&end_e.z);
                    st.set_inputs(prob.traces, te);
                    let mut d_new = st.d.clone();
                    {
                        let inp = EvalInput { t: te, y: &st.y, p: st.p, d: &st.d, u: &st.u };
                        st.m.when(&inp, &fired, &mut st.work, &mut d_new);
                    }
                    st.d.copy_from_slice(&d_new);
                    for (k, f) in fired.iter().enumerate() {
                        if *f != 0.0 {
                            events.push(FastEvent {
                                t: te,
                                when: k,
                                label: prob.whens[k].label.clone(),
                            });
                        }
                    }
                    st.rep.events += 1;
                    st.solve_z(te)?;
                    x_n.copy_from_slice(&st.y[..nx]);
                    z_n.copy_from_slice(&st.y[nx..]);
                    f_n.copy_from_slice(&st.out[..nx]);
                    if need_vars {
                        st.vars_into(te, &mut v_start);
                        tracker.point(te, &v_start);
                        for (j, &i) in rec.iter().enumerate() {
                            acc_lo[j] = acc_lo[j].min(v_start[i]);
                            acc_hi[j] = acc_hi[j].max(v_start[i]);
                        }
                    }
                    st.roots_into(te, &mut r_start);
                    t0 = te;
                }
            }
        }
        // the end of this step is the start of the next (same slope unless
        // the segment changes, which the next step handles)
        if need_vars {
            std::mem::swap(&mut v_start, &mut v_end);
        }
        st.rep.steps += 1;
        if is_output {
            times.push(tb);
            let len = tb - acc_t0;
            for (j, &i) in rec.iter().enumerate() {
                values.push(v_start[i]);
                vmin.push(acc_lo[j]);
                vmax.push(acc_hi[j]);
                vmean.push(if len > 0.0 { acc_int[j] / len } else { v_start[i] });
            }
            acc_lo.iter_mut().for_each(|v| *v = f64::INFINITY);
            acc_hi.iter_mut().for_each(|v| *v = f64::NEG_INFINITY);
            acc_int.iter_mut().for_each(|v| *v = 0.0);
            acc_t0 = tb;
        }
    }
    let flags = tracker.finish(t_end);
    let mut rep = st.rep;
    rep.max_error = rep.max_error.max(0.0);
    Ok(FastResult {
        times,
        names,
        record: rec.to_vec(),
        values,
        min: vmin,
        max: vmax,
        mean: vmean,
        flags,
        events,
        report: rep,
        wall_seconds: started.elapsed().as_secs_f64(),
    })
}

/// One Rosenbrock-W step from `(t0, x0)` (consistent, with `f0 = x'` there)
/// to `t1`; on return the stepper's `y` and `out` hold the consistent end
/// point (left limit at `t1`).
#[allow(clippy::too_many_arguments)]
fn advance(
    st: &mut Stepper<'_>,
    prob: &FastProblem<'_>,
    o: &crate::FastOptions,
    t0: f64,
    t1: f64,
    x0: &[f64],
    z0: &[f64],
    f0: &[f64],
) -> Result<StepEnd, FastError> {
    let (nx, s) = (st.nx, st.tab.s);
    let h = t1 - t0;
    let mut x1 = vec![0.0; nx];
    let mut err = vec![0.0; nx];
    let mut redone = false;
    loop {
        if o.max_jacobian_age > 0 && st.jac_age >= o.max_jacobian_age {
            st.y[..nx].copy_from_slice(x0);
            st.y[st.nx..].copy_from_slice(z0);
            st.set_inputs(prob.traces, t0);
            st.refresh_jacobian(t0)?;
            st.err_ref = f64::INFINITY;
        }
        if nx > 0 {
            let wi = st.w_index(h, t0)?;
            // stage 1 at the start point
            st.stage[0].copy_from_slice(f0);
            st.wlu[wi].1.solve(&mut st.stage[0]);
            for i in 1..s {
                for k in 0..nx {
                    let mut v = x0[k];
                    for j in 0..i {
                        v += st.tab.a[i][j] * st.stage[j][k];
                    }
                    st.y[k] = v;
                }
                let ti = t0 + st.tab.alpha[i] * h;
                st.set_inputs(prob.traces, ti);
                st.solve_z(ti)?;
                for k in 0..nx {
                    let mut v = st.out[k];
                    for j in 0..i {
                        v += st.tab.c[i][j] / h * st.stage[j][k];
                    }
                    st.rhs[k] = v;
                }
                let wi = st.w_index(h, t0)?;
                st.wlu[wi].1.solve(&mut st.rhs);
                st.stage[i].copy_from_slice(&st.rhs);
            }
            for k in 0..nx {
                let mut v = x0[k];
                let mut e = 0.0;
                for i in 0..s {
                    v += st.tab.m[i] * st.stage[i][k];
                    e += st.tab.e[i] * st.stage[i][k];
                }
                x1[k] = v;
                err[k] = e;
            }
        }
        if x1.iter().any(|v| !v.is_finite()) {
            return Err(FastError::NoConvergence {
                t: t1,
                what: "the step produced a state that is not a number".into(),
            });
        }
        let e = if st.tab.s > 1 { st.scaled_error(x0, &x1, &err, o) } else { 0.0 };
        // a stale Jacobian that has gone bad: refresh it and redo the step once
        if !redone && st.jac_age > 0 && e > 1.0 && e > 4.0 * st.err_ref {
            st.y[..nx].copy_from_slice(x0);
            st.y[st.nx..].copy_from_slice(z0);
            st.set_inputs(prob.traces, t0);
            st.solve_z(t0)?;
            st.refresh_jacobian(t0)?;
            st.rep.step_redos += 1;
            redone = true;
            continue;
        }
        if st.jac_age == 0 {
            st.err_ref = e;
        }
        st.jac_age += 1;
        if e > st.rep.max_error {
            st.rep.max_error = e;
            st.rep.max_error_at = t1;
        }
        break;
    }
    // the end point, consistent (z from the last stage as the guess)
    st.y[..nx].copy_from_slice(&x1);
    if s == 1 || nx == 0 {
        st.y[nx..].copy_from_slice(z0);
    }
    st.set_inputs(prob.traces, t1);
    st.solve_z(t1)?;
    Ok(StepEnd { x: x1, z: st.y[nx..].to_vec(), f: st.out[..nx].to_vec() })
}

/// Makes the stepper's point the consistent Hermite interpolant at
/// `θ ∈ [0, 1]` of the step `[t0, t1]`.
#[allow(clippy::too_many_arguments)]
fn hermite_point(
    st: &mut Stepper<'_>,
    prob: &FastProblem<'_>,
    t0: f64,
    t1: f64,
    x0: &[f64],
    f0: &[f64],
    end: &StepEnd,
    th: f64,
) -> Result<(), FastError> {
    let nx = st.nx;
    let h = t1 - t0;
    let (t2, t3) = (th * th, th * th * th);
    let (h00, h10, h01, h11) =
        (2.0 * t3 - 3.0 * t2 + 1.0, t3 - 2.0 * t2 + th, -2.0 * t3 + 3.0 * t2, t3 - t2);
    for k in 0..nx {
        st.y[k] = h00 * x0[k] + h10 * h * f0[k] + h01 * end.x[k] + h11 * h * end.f[k];
    }
    st.y[nx..].copy_from_slice(&end.z);
    let t = t0 + th * h;
    st.set_inputs(prob.traces, t);
    st.solve_z(t)
}

/// Locates a zero crossing of root `c` inside the step (Illinois method on
/// the consistent Hermite interpolant); returns θ.
#[allow(clippy::too_many_arguments)]
fn locate(
    st: &mut Stepper<'_>,
    prob: &FastProblem<'_>,
    t0: f64,
    t1: f64,
    x0: &[f64],
    f0: &[f64],
    end: &StepEnd,
    c: usize,
    r0: f64,
    r1: f64,
) -> Result<f64, FastError> {
    let nr = st.m.layout().n_roots;
    let mut r = vec![0.0; nr];
    let (mut a, mut fa, mut b, mut fb) = (0.0f64, r0, 1.0f64, r1);
    let mut side = 0i32;
    let h = t1 - t0;
    for _ in 0..100 {
        if (b - a) * h <= 1e-12 * t1.abs().max(1.0) {
            break;
        }
        let th = (a * fb - b * fa) / (fb - fa);
        let th = if th.is_finite() && th > a && th < b { th } else { 0.5 * (a + b) };
        hermite_point(st, prob, t0, t1, x0, f0, end, th)?;
        st.roots_into(t0 + th * h, &mut r);
        let ft = r[c];
        if (ft < 0.0) == (fb < 0.0) || ft == 0.0 && fb == 0.0 {
            b = th;
            fb = ft;
            if side == 1 {
                fa *= 0.5;
            }
            side = 1;
        } else {
            a = th;
            fa = ft;
            if side == -1 {
                fb *= 0.5;
            }
            side = -1;
        }
    }
    Ok(b)
}

/// Evaluates and records the start, middle and end of a (sub)step: limit
/// flags, and each recorded channel's running min, max and integral.
#[allow(clippy::too_many_arguments)]
fn record_step(
    st: &mut Stepper<'_>,
    prob: &FastProblem<'_>,
    t0: f64,
    t1: f64,
    x0: &[f64],
    f0: &[f64],
    end: &StepEnd,
    need_vars: bool,
    stats: bool,
    v_start: &mut [f64],
    v_mid: &mut [f64],
    v_end: &mut [f64],
    rec: &[usize],
    lo: &mut [f64],
    hi: &mut [f64],
    int: &mut [f64],
    tracker: &mut FlagTracker<'_>,
) -> Result<(), FastError> {
    let nx = st.nx;
    let h = t1 - t0;
    if !need_vars {
        st.y[..nx].copy_from_slice(&end.x);
        st.y[nx..].copy_from_slice(&end.z);
        return Ok(());
    }
    if stats {
        hermite_point(st, prob, t0, t1, x0, f0, end, 0.5)?;
        st.vars_into(t0 + 0.5 * h, v_mid);
        tracker.point(t0 + 0.5 * h, v_mid);
    }
    // the end point (left limit at t1), consistent
    st.y[..nx].copy_from_slice(&end.x);
    st.y[nx..].copy_from_slice(&end.z);
    st.set_inputs(prob.traces, t1);
    st.vars_into(t1, v_end);
    tracker.point(t1, v_end);
    for (j, &i) in rec.iter().enumerate() {
        let (a, b) = (v_start[i], v_end[i]);
        let mut l = a.min(b);
        let mut u = a.max(b);
        let area = if stats {
            let c = v_mid[i];
            l = l.min(c);
            u = u.max(c);
            h * (a + 4.0 * c + b) / 6.0
        } else {
            0.5 * h * (a + b)
        };
        lo[j] = lo[j].min(l);
        hi[j] = hi[j].max(u);
        int[j] += area;
    }
    Ok(())
}
