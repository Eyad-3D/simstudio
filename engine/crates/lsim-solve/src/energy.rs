//! The energy books (DESIGN.md, *Energy books*).
//!
//! For every primitive part with physical ports, integrated as quadratures
//! next to the states ([`Integrand`]): the energy that entered it through
//! its ports (∫ Σ across × through dt), the energy it turned into heat
//! (∫ loss dt) and the change of the energy it stores (∫ dE/dt dt, the rate
//! taken along the solution by a fourth-order central difference of the
//! declared stored energy in the direction (1, y')). Jumps of the stored
//! energy at events are booked separately, from the states just before and
//! just after.
//!
//! The books of the whole model: what the parts without declared storage
//! or losses supplied (ideal sources and other boundaries: the energy that
//! entered the model) must equal what was lost to heat, plus what vanished
//! at events, plus the change of stored energy. The **closure** is the
//! difference, as a share of the energy throughput (half the sum over all
//! parts of ∫ |power in| dt: each joule leaves one part and enters
//! another). Connections conserve power exactly at every instant, so a
//! closure above round-off means a part whose declared loss or stored
//! energy disagrees with its equations: the run names the parts.
//!
//! The **drift** is separate: the stored energy computed from the states at
//! the end minus the books' (integrated) change. It is the integration
//! error of the energies (the solution satisfies its equations only to the
//! tolerance), of the order of rtol; the one-click tighter run shrinks it.

use crate::info::{ChannelEnv, EnergyInfo};
use lsim_ir::eval::eval;
use lsim_ir::runtime::{EvalInput, Layout, ModelFunctions};
use std::sync::Arc;

/// Where one part's integrals sit.
#[derive(Clone, Copy, Debug)]
struct Slot {
    /// ∫ power in
    power: usize,
    /// ∫ |power in|
    abs: usize,
    /// ∫ loss
    loss: Option<usize>,
    /// ∫ d(stored)/dt
    stored: Option<usize>,
}

fn slots(info: &EnergyInfo) -> (Vec<Slot>, usize) {
    let mut n = 0;
    let s = info
        .parts
        .iter()
        .map(|p| {
            let mut take = || {
                n += 1;
                n - 1
            };
            Slot {
                power: take(),
                abs: take(),
                loss: p.loss.as_ref().map(|_| take()),
                stored: p.stored.as_ref().map(|_| take()),
            }
        })
        .collect();
    (s, n)
}

/// The quadrature integrand: for each part ∫ power in, ∫ |power in| and,
/// when it declares them, ∫ loss and ∫ d(stored)/dt.
pub struct Integrand {
    info: Arc<EnergyInfo>,
    vars: Vec<f64>,
    shifted: Vec<f64>,
    stored: Vec<[f64; 4]>,
    slots: Vec<Slot>,
    any_stored: bool,
    n: usize,
}

impl Integrand {
    /// The integrand of `info`'s books for a model of layout `l`.
    pub fn new(info: &Arc<EnergyInfo>, l: &Layout) -> Self {
        let (slots, n) = slots(info);
        Integrand {
            any_stored: info.parts.iter().any(|p| p.stored.is_some()),
            stored: vec![[0.0; 4]; info.parts.len()],
            info: info.clone(),
            vars: vec![0.0; l.n_vars],
            shifted: vec![0.0; l.n_y()],
            slots,
            n,
        }
    }

    /// Number of integrals.
    pub fn len(&self) -> usize {
        self.n
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The integrands at `inp`, where y changes at the rate `ydot`.
    pub fn eval(
        &mut self,
        m: &dyn ModelFunctions,
        inp: &EvalInput<'_>,
        ydot: &[f64],
        work: &mut [f64],
        out: &mut [f64],
    ) {
        m.vars(inp, work, &mut self.vars);
        let env = ChannelEnv { t: inp.t, vars: &self.vars, params: inp.p };
        for (part, s) in self.info.parts.iter().zip(&self.slots) {
            let p = eval(&part.power, &env);
            out[s.power] = p;
            out[s.abs] = p.abs();
            if let (Some(il), Some(loss)) = (s.loss, &part.loss) {
                out[il] = eval(loss, &env);
            }
        }
        if !self.any_stored {
            return;
        }
        // dE/dt along (1, y'): a fourth-order central difference with a step
        // that moves every entry of y by at most 1e-3 of its size (exact for
        // the quadratic stored energies of capacitors, inductors, masses)
        let mut h = f64::INFINITY;
        for (y, yd) in inp.y.iter().zip(ydot) {
            if *yd != 0.0 {
                h = h.min(1e-3 * (y.abs() + 1e-9) / yd.abs());
            }
        }
        if !h.is_finite() {
            h = 1e-3 * inp.t.abs().max(1e-3);
        }
        for (k, (sign, mult)) in
            [(1.0, 2.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, 2.0)].iter().enumerate()
        {
            let step = sign * mult * h;
            for ((s, y), yd) in self.shifted.iter_mut().zip(inp.y).zip(ydot) {
                *s = y + step * yd;
            }
            let at = EvalInput { t: inp.t + step, y: &self.shifted, ..*inp };
            m.vars(&at, work, &mut self.vars);
            let env = ChannelEnv { t: at.t, vars: &self.vars, params: inp.p };
            for (j, part) in self.info.parts.iter().enumerate() {
                if let Some(e) = &part.stored {
                    self.stored[j][k] = eval(e, &env);
                }
            }
        }
        for (j, s) in self.slots.iter().enumerate() {
            if let Some(is) = s.stored {
                let e = &self.stored[j];
                out[is] = (-e[0] + 8.0 * e[1] - 8.0 * e[2] + e[3]) / (12.0 * h);
            }
        }
    }
}

impl Clone for Integrand {
    fn clone(&self) -> Self {
        Integrand {
            info: self.info.clone(),
            vars: self.vars.clone(),
            shifted: self.shifted.clone(),
            stored: self.stored.clone(),
            slots: self.slots.clone(),
            any_stored: self.any_stored,
            n: self.n,
        }
    }
}

/// One part's books, J.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartBooks {
    /// the part's path
    pub path: String,
    /// how a person names it
    pub name: String,
    /// whether it declares stored energy or losses (otherwise it is a
    /// boundary: an ideal source, a ground, a lossless converter)
    pub declared: bool,
    /// energy that entered through its ports (negative: it delivered)
    pub energy_in: f64,
    /// ∫ |power in| dt
    pub throughput: f64,
    /// energy turned into heat
    pub lost: f64,
    /// change of stored energy over the run, from the states
    pub stored_change: f64,
    /// change of stored energy at events, from the states before and after
    pub event_change: f64,
    /// change of stored energy between events, integrated (∫ dE/dt dt)
    pub stored_integral: f64,
    /// the kinetic energy rigid engagements took at events (part of what
    /// the model lost at events): an engaging part (a gearbox at its
    /// shifts) books what its rigid coupling lost as the inertias it ties
    /// together met; a coupling declared to pass the impulse on, when it
    /// alone did, what its relaxation to its relative velocity before the
    /// event lost
    pub impulse_lost: f64,
    /// energy_in - lost - stored_integral: zero when the part's declared
    /// books agree with its equations (declared parts only)
    pub closure: f64,
    /// stored_change - event_change - stored_integral: the integration
    /// error of its energy
    pub drift: f64,
    /// on the output grid: energy in so far
    pub energy_in_t: Vec<f64>,
    /// on the output grid: energy lost so far
    pub lost_t: Vec<f64>,
    /// on the output grid: stored energy from the states (0 when it
    /// declares none)
    pub stored_t: Vec<f64>,
}

/// The whole model's books, J.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnergyBooks {
    /// every primitive part with physical ports
    pub parts: Vec<PartBooks>,
    /// net energy the boundary parts (sources) delivered into the model
    pub supplied: f64,
    /// energy lost to heat
    pub lost: f64,
    /// energy that vanished at events (stored energy before minus after)
    pub event_loss: f64,
    /// of it, what rigid engagements took (gear shifts: the kinetic energy
    /// an impulse that keeps the momentum loses)
    pub impulse_loss: f64,
    /// of that, what the couplings declared to pass the impulse on lost as
    /// they relaxed (booked to the coupling when one did, to the event when
    /// several did); the rest is the engaging parts' (the gearboxes')
    pub impulse_link_loss: f64,
    /// change of the stored energy between events, integrated
    pub stored_integral: f64,
    /// change of the stored energy over the run, from the states
    pub stored_change: f64,
    /// half the sum of every part's ∫ |power in| dt
    pub throughput: f64,
    /// supplied - lost - stored_integral: zero up to round-off when every
    /// part's books agree with its equations
    pub closure: f64,
    /// |closure| / throughput
    pub relative_closure: f64,
    /// stored_change - event change - stored_integral: the integration
    /// error of the energies
    pub drift: f64,
    /// |drift| / throughput
    pub relative_drift: f64,
}

impl EnergyBooks {
    /// The parts whose books close worst, worst first.
    pub fn worst_parts(&self, n: usize) -> Vec<&PartBooks> {
        let mut p: Vec<&PartBooks> = self.parts.iter().filter(|p| p.declared).collect();
        p.sort_by(|a, b| b.closure.abs().total_cmp(&a.closure.abs()));
        p.truncate(n);
        p
    }

    /// A sentence for the report.
    pub fn summary(&self) -> String {
        format!(
            "energy books: supplied {:.6e} J = lost {:.6e} J + lost at events {:.6e} J (at gear shifts and other engagements {:.6e} J, {:.6e} J of it in the couplings that passed the impulse on) + stored {:+.6e} J; \
             closure {:.1e} of the throughput {:.6e} J; integration drift {:.1e}",
            self.supplied,
            self.lost,
            self.event_loss,
            self.impulse_loss,
            self.impulse_link_loss,
            self.stored_integral,
            self.relative_closure,
            self.throughput,
            self.relative_drift
        )
    }
}

/// The run loop's side of the books: stored energies at the start, the
/// jumps at events, and the series on the output grid.
pub(crate) struct Ledger {
    info: Arc<EnergyInfo>,
    slots: Vec<Slot>,
    stored0: Vec<f64>,
    jumps: Vec<f64>,
    scratch: Vec<f64>,
    pub q: Vec<f64>,
    books: Vec<PartBooks>,
    /// what impulses lost with no part to book it to
    impulse_unbooked: f64,
    /// what the couplings that passed impulses on took
    impulse_links: f64,
}

/// What the rigid engagements at an event lost, and where
/// ([`Ledger::after_event`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Engagement {
    /// (energy part, J lost there, whether it is a link that passed the
    /// impulse on): the engaging parts' stage, the links' relaxation; the
    /// coupled parts' stored energies before and after each, nothing else
    pub losses: Vec<(Option<usize>, f64, bool)>,
    /// how many projections moved states
    pub count: u64,
}

impl Engagement {
    /// Another projection at the same event.
    pub fn merge(&mut self, o: Engagement) {
        self.losses.extend(o.losses);
        self.count += o.count;
    }
}

impl Ledger {
    pub fn new(info: &Arc<EnergyInfo>, _l: &Layout) -> Self {
        let info = info.clone();
        let (slots, n_q) = slots(&info);
        let n = info.parts.len();
        Ledger {
            books: info
                .parts
                .iter()
                .map(|p| PartBooks {
                    path: p.path.clone(),
                    name: p.name.clone(),
                    declared: p.loss.is_some() || p.stored.is_some(),
                    ..Default::default()
                })
                .collect(),
            info,
            slots,
            stored0: vec![0.0; n],
            jumps: vec![0.0; n],
            scratch: vec![0.0; n],
            q: vec![0.0; n_q],
            impulse_unbooked: 0.0,
            impulse_links: 0.0,
        }
    }

    fn stored(&mut self, t: f64, vars: &[f64], params: &[f64]) {
        let env = ChannelEnv { t, vars, params };
        for (k, p) in self.info.parts.iter().enumerate() {
            self.scratch[k] = p.stored.as_ref().map(|e| eval(e, &env)).unwrap_or(0.0);
        }
    }

    /// At the start (the first grid point).
    pub fn start(&mut self, t: f64, vars: &[f64], params: &[f64]) {
        self.stored(t, vars, params);
        self.stored0.copy_from_slice(&self.scratch);
        self.q.iter_mut().for_each(|x| *x = 0.0);
        self.grid_point(t, vars, params);
    }

    /// Stored energy just before an event; [`Self::after_event`] takes the
    /// difference.
    pub fn before_event(&mut self, t: f64, vars: &[f64], params: &[f64]) -> Vec<f64> {
        self.stored(t, vars, params);
        self.scratch.clone()
    }

    /// Stored energy just after an event: the jump is booked. When rigid
    /// engagements moved the states (`impulse`), what they lost is booked
    /// to the engaging parts and the links, as the projection measured it
    /// on the coupled parts' stored energies (other jumps at the same
    /// instant are not theirs).
    pub fn after_event(
        &mut self,
        t: f64,
        vars: &[f64],
        params: &[f64],
        before: &[f64],
        impulse: Option<&Engagement>,
    ) {
        self.stored(t, vars, params);
        for ((j, now), was) in self.jumps.iter_mut().zip(&self.scratch).zip(before) {
            *j += now - was;
        }
        if let Some(e) = impulse {
            for &(part, lost, link) in &e.losses {
                match part {
                    Some(k) => self.books[k].impulse_lost += lost,
                    None => self.impulse_unbooked += lost,
                }
                if link {
                    self.impulse_links += lost;
                }
            }
        }
    }

    /// A grid point: `self.q` must hold the integrals at `t`.
    pub fn grid_point(&mut self, t: f64, vars: &[f64], params: &[f64]) {
        self.stored(t, vars, params);
        for (k, b) in self.books.iter_mut().enumerate() {
            let s = self.slots[k];
            b.energy_in_t.push(self.q[s.power]);
            b.lost_t.push(s.loss.map(|i| self.q[i]).unwrap_or(0.0));
            b.stored_t.push(self.scratch[k]);
        }
    }

    /// The books at the end: `self.q` must hold the integrals at `t`.
    pub fn finish(mut self, t: f64, vars: &[f64], params: &[f64]) -> EnergyBooks {
        self.stored(t, vars, params);
        let mut e = EnergyBooks {
            impulse_loss: self.impulse_unbooked,
            impulse_link_loss: self.impulse_links,
            ..Default::default()
        };
        let mut twice_throughput = 0.0;
        for (k, mut b) in self.books.into_iter().enumerate() {
            let s = self.slots[k];
            b.energy_in = self.q[s.power];
            b.throughput = self.q[s.abs];
            b.lost = s.loss.map(|i| self.q[i]).unwrap_or(0.0);
            b.stored_integral = s.stored.map(|i| self.q[i]).unwrap_or(0.0);
            b.stored_change = self.scratch[k] - self.stored0[k];
            b.event_change = self.jumps[k];
            b.drift = b.stored_change - b.event_change - b.stored_integral;
            twice_throughput += b.throughput;
            e.impulse_loss += b.impulse_lost;
            if b.declared {
                b.closure = b.energy_in - b.lost - b.stored_integral;
                e.lost += b.lost;
                e.stored_integral += b.stored_integral;
                e.stored_change += b.stored_change;
                e.event_loss -= b.event_change;
                e.drift += b.drift;
            } else {
                e.supplied -= b.energy_in;
            }
            e.parts.push(b);
        }
        e.throughput = 0.5 * twice_throughput;
        e.closure = e.supplied - e.lost - e.stored_integral;
        let scale = e.throughput.max(e.supplied.abs()).max(e.lost).max(e.stored_change.abs());
        e.relative_closure = if scale > 0.0 { e.closure.abs() / scale } else { 0.0 };
        e.relative_drift = if scale > 0.0 { e.drift.abs() / scale } else { 0.0 };
        e
    }
}
