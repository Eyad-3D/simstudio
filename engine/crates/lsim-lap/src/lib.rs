//! # lsim-lap: a lap's speed profile from the engine's own car
//!
//! A lap case (the Race Track) is quasi-steady: the speed the car can drive
//! at each point of the track, from today's method (backend
//! `solver/lapsim.py`, re-derived from Heilmeier et al., EVER 2019, and the
//! point-mass lap simulators such as OpenLAP): each point's cornering speed,
//! a forward pass at full acceleration and a backward pass at full braking,
//! the speed the lower of the two. What the car can do at a point comes
//! from the engine's own model of it, not from a second copy: the case's
//! lap model (fast mode's inverse model with the lap variants of the Race
//! Track, the Vehicle and its wheels: the curvature and grade, the lateral
//! load transfer, the friction ellipse) solved at one instant at a time
//! ([`lsim_fast::PointSolver`]), its states held at the lap's start. At a
//! speed v and an acceleration a, on curvature κ and slope θ, every limit of
//! the model (each wheel's grip, the Driver's pedal from full brake to full
//! throttle, each E-Motor's supply) is inside its band or not, so:
//!
//! * the **cornering speed** is the highest v at which, at a = 0, every
//!   wheel's force is inside its grip (the corner's share of it);
//! * the **most acceleration** at v is the highest a at which no wheel
//!   pushes past its grip, the pedal is at most full throttle and no
//!   E-Motor draws more than its supply gives;
//! * the **most deceleration** is the lowest a at which no wheel brakes
//!   past its grip and the pedal is at least full brake: the brakes act as
//!   the car's are set (their Max Torque, front and rear), so the first
//!   axle to reach its grip sets it, and the profile says which;
//! * a point the model has no answer at (a solve that fails) is a point
//!   the car cannot reach, never a value: the lap is held below the highest
//!   speed at which the model solves on the level (the motors' top speed in
//!   their gear), and that limit is named so.
//!
//! What held the car back on each stretch is today's code ([`Code`]) with
//! the cause in finer words ([`Cause`]): which axle's grip or the brakes'
//! torque when braking, which wheel when driving, and for an E-Motor held by
//! its supply, the battery's power cap (its Output Power Limit less its
//! margin) or which part of its window (its maximum-power point, its
//! current or voltage limits, its minimum SOC: [`battery_cause`]).

use lsim_engine::Model;
use lsim_fast::PointSolver;
use std::collections::HashMap;

/// a speed this close below a point's cornering speed counts as limited by
/// it (today's AT_APEX)
const AT_APEX: f64 = 0.005;
/// the highest speed looked at, m/s (today's envelope's)
const V_MAX: f64 = 150.0;
/// accelerations searched, m/s²
const A_MAX: f64 = 60.0;
/// how close a root is found: speed, m/s; acceleration, m/s²
const V_TOL: f64 = 1e-6;
const A_TOL: f64 = 1e-6;
const MAX_ROOT: usize = 60;

/// What held the car back on a stretch: today's codes (lapsim.LIMITS
/// position + 1) and the motors' top speed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Code {
    /// the tyres' lateral grip in a corner
    Corner = 1,
    /// the driven tyres' grip
    Traction = 2,
    /// the motors at full load
    Motor = 3,
    /// a battery's window
    Battery = 4,
    /// a battery's Output Power Limit
    Cap = 5,
    /// braking for what comes
    Braking = 6,
    /// the driver lifts and coasts
    Coast = 7,
    /// the motors' top speed in their gear (the model has no answer above it)
    TopSpeed = 8,
}

/// The cause in finer words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Cause {
    /// nothing more to say
    None = 0,
    /// a front wheel's grip
    FrontAxle = 1,
    /// a rear wheel's grip
    RearAxle = 2,
    /// the brakes at full command (their Max Torque)
    Brakes = 3,
    /// a battery's maximum-power point
    MaxPower = 4,
    /// a battery's current or voltage limits (its cells', or its pack's,
    /// with the derating band)
    CurrentLimit = 5,
    /// a battery near its minimum SOC
    SocFloor = 6,
    /// a battery's Output Power Limit less its margin
    PowerCap = 7,
}

/// The parts of the lap model the solver reads, by instance name.
#[derive(Clone, Debug, Default)]
pub struct Parts {
    /// the Race Track (its `kappa_now`, `grade_now`)
    pub track: String,
    /// the Driver (its pedal `cmd`)
    pub driver: String,
    /// each wheel on the Vehicle, and whether it is on the front axle
    pub wheels: Vec<(String, bool)>,
    /// the E-Motors
    pub motors: Vec<String>,
    /// the batteries
    pub batteries: Vec<String>,
}

/// The track, a point every `ds` m from 0 to its length.
#[derive(Clone, Debug, Default)]
pub struct Track {
    /// curvature at each point, 1/m (+ left)
    pub kappa: Vec<f64>,
    /// sine of the slope angle at each point (+ uphill)
    pub sin_t: Vec<f64>,
    /// point spacing, m
    pub ds: f64,
    /// a closed circuit (the lap ends where the next begins)
    pub closed: bool,
}

/// One lap's settings.
#[derive(Clone, Debug, Default)]
pub struct LapOptions {
    /// the speed the lap starts from, m/s (no faster than its first point allows)
    pub v_start: f64,
    /// the share of each stretch of acceleration before braking driven as
    /// lift-and-coast (0-1)
    pub coast: f64,
    /// a lap follows this one, driven on from its end
    pub more: bool,
    /// the lap ends at a standstill (the endurance's driver change)
    pub stop: bool,
}

/// One lap's speed profile.
#[derive(Clone, Debug, Default)]
pub struct Profile {
    /// speed at each point, m/s
    pub v: Vec<f64>,
    /// time from the lap's start at each point, s
    pub t: Vec<f64>,
    /// what held the car back on each stretch (point i to i + 1)
    pub code: Vec<Code>,
    /// in finer words
    pub cause: Vec<Cause>,
    /// each point's cornering speed, m/s (inf on a straight)
    pub apex: Vec<f64>,
    /// the highest speed the model solves at on the level, m/s
    pub v_cap: f64,
    /// the point solver's work
    pub stats: lsim_fast::PointStats,
}

/// Why a lap could not be solved.
#[derive(Clone, Debug, PartialEq)]
pub enum LapError {
    /// the model could not be started or read
    Model(String),
    /// the car stops on the track, at this distance, m
    Stops(f64),
}

impl std::fmt::Display for LapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LapError::Model(m) => write!(f, "{m}"),
            LapError::Stops(s) => write!(
                f,
                "the car cannot drive the lap: it stops {s:.0} m into it (its motors do not \
                 overcome the road load there)"
            ),
        }
    }
}

/// A limit site's role.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Site {
    Pedal,
    Supply,
    Wheel(bool),
    Other,
}

/// A battery's window, by index into the variables and parameters.
struct Window {
    p_deliver: usize,
    i_mpp: usize,
    i_dis_lim: usize,
    i_floor: usize,
    p_cap: usize,
}

/// Which part of a battery's window holds it: its Output Power Limit (the
/// window at its cap, less its margin), else whichever of its currents is
/// the least (the maximum-power point's, its current or voltage limits', the
/// one its minimum SOC allows). `vars` the model's variables, `params` its
/// parameters.
fn battery_cause(w: &Window, vars: &[f64], params: &[f64]) -> Cause {
    let cap = params[w.p_cap];
    if cap.is_finite() && cap < 1e29 && vars[w.p_deliver] >= cap * (1.0 - 1e-9) {
        return Cause::PowerCap;
    }
    let (mpp, lim, floor) = (vars[w.i_mpp], vars[w.i_dis_lim], vars[w.i_floor]);
    if floor < mpp && floor < lim {
        Cause::SocFloor
    } else if lim < mpp {
        Cause::CurrentLimit
    } else {
        Cause::MaxPower
    }
}

/// The car at one instant at a time.
struct Car<'a> {
    s: PointSolver<'a>,
    kappa: usize,
    grade: usize,
    pedal: usize,
    sites: Vec<Site>,
    windows: Vec<Window>,
    apex: HashMap<(u64, u64), f64>,
    v_cap: f64,
}

/// The margin of a value inside its upper (`up`) or lower bound, as a share
/// of the band's or the value's size: ≥ 0 inside.
fn margin(x: [f64; 3], up: bool) -> f64 {
    let [v, lo, hi] = x;
    let scale = v.abs().max(lo.abs()).max(hi.abs()).max(1e-9);
    if up { (hi - v) / scale } else { (v - lo) / scale }
}

impl<'a> Car<'a> {
    fn new(model: &'a Model, params: &[f64], parts: &Parts, v0: f64) -> Result<Car<'a>, LapError> {
        let flat = &model.prepared.flat;
        let param = |name: String| {
            flat.find_param(&name)
                .map(|p| p.0 as usize)
                .ok_or_else(|| LapError::Model(format!("the lap model has no parameter '{name}'")))
        };
        let names = &model.info.var_names;
        let var = |name: String| {
            names
                .iter()
                .position(|n| *n == name)
                .ok_or_else(|| LapError::Model(format!("the lap model has no variable '{name}'")))
        };
        let kappa = param(format!("{}.kappa_now", parts.track))?;
        let grade = param(format!("{}.grade_now", parts.track))?;
        let pedal = var(format!("{}.cmd", parts.driver))?;
        let sites = model
            .limits
            .iter()
            .map(|l| {
                if l.part == parts.driver {
                    Site::Pedal
                } else if parts.motors.contains(&l.part) {
                    Site::Supply
                } else if let Some((_, front)) = parts.wheels.iter().find(|(w, _)| *w == l.part) {
                    Site::Wheel(*front)
                } else {
                    Site::Other
                }
            })
            .collect();
        let mut windows = vec![];
        for b in &parts.batteries {
            windows.push(Window {
                p_deliver: var(format!("{b}.p_deliver"))?,
                i_mpp: var(format!("{b}.i_mpp"))?,
                i_dis_lim: var(format!("{b}.i_dis_lim"))?,
                i_floor: var(format!("{b}.i_floor"))?,
                p_cap: param(format!("{b}.p_cap"))?,
            });
        }
        let s = model
            .point_solver(params, &[(v0, 0.0)])
            .map_err(|e| LapError::Model(format!("the lap model could not be started: {e}")))?;
        let mut car = Car { s, kappa, grade, pedal, sites, windows, apex: HashMap::new(), v_cap: V_MAX };
        car.v_cap = car.top_speed();
        Ok(car)
    }

    /// The limits at speed `v` and acceleration `a` on curvature `kappa` and
    /// slope `sin_t`: each site's `[value, lo, hi]`; None where the model
    /// has no answer.
    fn at(&mut self, v: f64, a: f64, kappa: f64, sin_t: f64) -> Option<Vec<[f64; 3]>> {
        let p = self.s.params_mut();
        p[self.kappa] = kappa;
        p[self.grade] = sin_t / (1.0 - sin_t * sin_t).max(1e-12).sqrt();
        self.s.solve(&[(v, a)]).ok()?;
        Some(self.s.sites())
    }

    /// The smallest driving margin (no wheel pushing past its grip, the
    /// pedal at most full throttle, no motor past its supply) and its
    /// site; -1 where the model has no answer.
    fn drive(&mut self, v: f64, a: f64, kappa: f64, sin_t: f64) -> (f64, Option<usize>) {
        let Some(x) = self.at(v, a, kappa, sin_t) else { return (-1.0, None) };
        let mut worst = (f64::INFINITY, None);
        for (k, site) in self.sites.iter().enumerate() {
            if matches!(site, Site::Pedal | Site::Supply | Site::Wheel(_)) {
                let m = margin(x[k], true);
                if m < worst.0 {
                    worst = (m, Some(k));
                }
            }
        }
        worst
    }

    /// The smallest braking margin (no wheel braking past its grip, the
    /// pedal at least full brake) and its site; -1 where the model has no
    /// answer.
    fn brake(&mut self, v: f64, a: f64, kappa: f64, sin_t: f64) -> (f64, Option<usize>) {
        let Some(x) = self.at(v, a, kappa, sin_t) else { return (-1.0, None) };
        let mut worst = (f64::INFINITY, None);
        for (k, site) in self.sites.iter().enumerate() {
            if matches!(site, Site::Pedal | Site::Wheel(_)) {
                let m = margin(x[k], false);
                if m < worst.0 {
                    worst = (m, Some(k));
                }
            }
        }
        worst
    }

    /// The highest speed at which the model solves on the level, at a = 0.
    fn top_speed(&mut self) -> f64 {
        if self.at(V_MAX, 0.0, 0.0, 0.0).is_some() {
            return V_MAX;
        }
        let (mut lo, mut hi) = (0.0, V_MAX);
        while hi - lo > V_TOL {
            let mid = 0.5 * (lo + hi);
            if self.at(mid, 0.0, 0.0, 0.0).is_some() {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// The highest steady speed on curvature `kappa`: every wheel's force
    /// inside its grip at a = 0 (inf on a straight).
    fn apex(&mut self, kappa: f64, sin_t: f64) -> f64 {
        if kappa.abs() < 1e-9 {
            return f64::INFINITY;
        }
        let key = (kappa.to_bits(), sin_t.to_bits());
        if let Some(v) = self.apex.get(&key) {
            return *v;
        }
        let holds = |car: &mut Self, v: f64| -> bool {
            let Some(x) = car.at(v, 0.0, kappa, sin_t) else { return false };
            car.sites.iter().enumerate().all(|(k, s)| {
                !matches!(s, Site::Wheel(_)) || (margin(x[k], true) >= 0.0 && margin(x[k], false) >= 0.0)
            })
        };
        let cap = self.v_cap;
        let v = if holds(self, cap) {
            f64::INFINITY
        } else {
            let (mut lo, mut hi) = (0.0, cap);
            while hi - lo > V_TOL {
                let mid = 0.5 * (lo + hi);
                if holds(self, mid) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            lo
        };
        self.apex.insert(key, v);
        v
    }

    /// The root of a margin `f` in `a` between `lo` (inside) and `hi`
    /// (outside), by regula falsi with Illinois' step, bisecting where the
    /// model has no answer; returns the last `a` inside and its site.
    fn root(
        &mut self,
        f: &mut dyn FnMut(&mut Self, f64) -> (f64, Option<usize>),
        mut lo: f64,
        mut hi: f64,
    ) -> (f64, Option<usize>) {
        let (mut flo, mut site) = f(self, lo);
        let (mut fhi, _) = f(self, hi);
        let mut side = 0i8;
        for _ in 0..MAX_ROOT {
            if (hi - lo).abs() <= A_TOL {
                break;
            }
            let x = if fhi > -1.0 && flo.is_finite() && fhi.is_finite() && flo != fhi {
                let x = lo - flo * (hi - lo) / (fhi - flo);
                if (x - lo) * (x - hi) < 0.0 { x } else { 0.5 * (lo + hi) }
            } else {
                0.5 * (lo + hi)
            };
            let (fx, sx) = f(self, x);
            if fx >= 0.0 {
                lo = x;
                flo = fx;
                site = sx;
                if side == 1 {
                    fhi *= 0.5;
                }
                side = 1;
            } else {
                hi = x;
                fhi = fx;
                if side == -1 {
                    flo *= 0.5;
                }
                side = -1;
            }
        }
        // (the site that held it, read just inside)
        (lo, site)
    }

    /// The most acceleration at speed `v` and what limits it.
    fn accel(&mut self, kappa: f64, sin_t: f64, v: f64) -> (f64, Code, Cause) {
        let mut f = |car: &mut Self, a: f64| car.drive(v, a, kappa, sin_t);
        let lo = if f(self, 0.0).0 >= 0.0 { 0.0 } else { -A_MAX };
        let (a, site) = self.root(&mut f, lo, A_MAX);
        let _ = self.drive(v, a, kappa, sin_t); // (the point itself, for the cause)
        let (code, cause) = match site.map(|k| self.sites[k]) {
            Some(Site::Pedal) => (Code::Motor, Cause::None),
            Some(Site::Supply) => {
                let vars = self.s.last_vars().to_vec();
                let params = self.s.params().to_vec();
                let cause = self
                    .windows
                    .iter()
                    .map(|w| battery_cause(w, &vars, &params))
                    .find(|c| *c == Cause::PowerCap)
                    .or_else(|| self.windows.first().map(|w| battery_cause(w, &vars, &params)))
                    .unwrap_or(Cause::None);
                (if cause == Cause::PowerCap { Code::Cap } else { Code::Battery }, cause)
            }
            Some(Site::Wheel(front)) => {
                (Code::Traction, if front { Cause::FrontAxle } else { Cause::RearAxle })
            }
            _ => (Code::Motor, Cause::None),
        };
        (a, code, cause)
    }

    /// The most deceleration at speed `v` (positive) and what limits it.
    fn decel(&mut self, kappa: f64, sin_t: f64, v: f64) -> (f64, Cause) {
        let mut f = |car: &mut Self, d: f64| car.brake(v, -d, kappa, sin_t);
        let (d, site) = self.root(&mut f, 0.0, A_MAX);
        let cause = match site.map(|k| self.sites[k]) {
            Some(Site::Wheel(true)) => Cause::FrontAxle,
            Some(Site::Wheel(false)) => Cause::RearAxle,
            Some(Site::Pedal) => Cause::Brakes,
            _ => Cause::None,
        };
        (d, cause)
    }

    /// The acceleration with the pedal at 0 (lift-and-coast): no drive, no
    /// brakes.
    fn coast(&mut self, kappa: f64, sin_t: f64, v: f64) -> f64 {
        let pedal = self.pedal;
        let mut f = |car: &mut Self, a: f64| -> (f64, Option<usize>) {
            match car.at(v, a, kappa, sin_t) {
                Some(_) => (-car.s.last_vars()[pedal], None),
                None => (-1.0, None),
            }
        };
        self.root(&mut f, -A_MAX, A_MAX).0
    }
}

/// One lap's speed profile on `track` (today's LapRun.solve: the cornering
/// speeds, the forward and backward passes, and with `opt.coast` a second
/// forward pass that lifts before the braking points), the lap model
/// `model` with parameters `params`, its parts `parts`.
pub fn solve(
    model: &Model,
    params: &[f64],
    parts: &Parts,
    track: &Track,
    opt: &LapOptions,
) -> Result<Profile, LapError> {
    let n = track.kappa.len().saturating_sub(1);
    if n == 0 || track.sin_t.len() != n + 1 || !(track.ds > 0.0) {
        return Err(LapError::Model("the track needs two points or more, a spacing above 0".into()));
    }
    let mut car = Car::new(model, params, parts, opt.v_start.max(0.0).min(V_MAX))?;
    let apex: Vec<f64> =
        track.kappa.iter().zip(&track.sin_t).map(|(&k, &s)| car.apex(k, s)).collect();
    let (vf, why) = forward(&mut car, track, &apex, opt.v_start, None);
    let mut prof = braking(&mut car, track, &apex, &vf, &why, opt)?;
    if opt.coast > 0.0 {
        let mask = coast_mask(&prof, opt.coast);
        if mask.iter().any(|m| *m) {
            let (vf, why) = forward(&mut car, track, &apex, opt.v_start, Some(&mask));
            prof = braking(&mut car, track, &apex, &vf, &why, opt)?;
        }
    }
    prof.apex = apex;
    prof.v_cap = car.v_cap;
    prof.stats = car.s.stats;
    Ok(prof)
}

/// Full acceleration from `v_start` (Heun's method over each spacing),
/// capped at each point's cornering speed and the top speed; on the
/// stretches `coast` marks, the pedal at 0. Returns the speeds and what
/// limited each stretch.
fn forward(
    car: &mut Car,
    tr: &Track,
    apex: &[f64],
    v_start: f64,
    coast: Option<&[bool]>,
) -> (Vec<f64>, Vec<(Code, Cause)>) {
    let n = tr.kappa.len() - 1;
    let ds = tr.ds;
    let mut vf = vec![0.0; n + 1];
    let mut why = vec![(Code::Motor, Cause::None); n];
    vf[0] = v_start.min(apex[0]).min(car.v_cap);
    for i in 0..n {
        let v = vf[i];
        let a = if coast.is_some_and(|c| c[i]) {
            let a1 = car.coast(tr.kappa[i], tr.sin_t[i], v);
            let v2 = (v * v + 2.0 * a1 * ds).max(0.0).sqrt().min(car.v_cap);
            let a2 = car.coast(tr.kappa[i + 1], tr.sin_t[i + 1], v2);
            why[i] = (Code::Coast, Cause::None);
            0.5 * (a1 + a2)
        } else {
            let (a1, code, cause) = car.accel(tr.kappa[i], tr.sin_t[i], v);
            let v2 = (v * v + 2.0 * a1 * ds).max(0.0).sqrt().min(car.v_cap);
            let (a2, _, _) = car.accel(tr.kappa[i + 1], tr.sin_t[i + 1], v2);
            why[i] = (code, cause);
            0.5 * (a1 + a2)
        };
        let next = (v * v + 2.0 * a * ds).max(0.0).sqrt();
        vf[i + 1] = next.min(apex[i + 1]);
        if next >= car.v_cap && car.v_cap <= apex[i + 1] {
            vf[i + 1] = car.v_cap;
            why[i] = (Code::TopSpeed, Cause::None);
        }
    }
    (vf, why)
}

/// Full braking back from the lap's end onto the forward speeds `vf`
/// (today's LapRun._braking): on a closed track, or one with a lap to come,
/// over the next lap's corners too; into a standstill for a driver change.
fn braking(
    car: &mut Car,
    tr: &Track,
    apex: &[f64],
    vf: &[f64],
    why: &[(Code, Cause)],
    opt: &LapOptions,
) -> Result<Profile, LapError> {
    let n = tr.kappa.len() - 1;
    let ds = tr.ds;
    let wrap = (tr.closed || opt.more) && !opt.stop;
    let (mut vb, kappa, sin_t): (Vec<f64>, Vec<f64>, Vec<f64>) = if wrap {
        (
            vf.iter().chain(&apex[1..]).copied().collect(),
            tr.kappa.iter().chain(&tr.kappa[1..]).copied().collect(),
            tr.sin_t.iter().chain(&tr.sin_t[1..]).copied().collect(),
        )
    } else {
        (vf.to_vec(), tr.kappa.clone(), tr.sin_t.clone())
    };
    for v in vb.iter_mut() {
        *v = v.min(car.v_cap);
    }
    if opt.stop {
        vb[n] = 0.0;
    }
    if wrap {
        vb[n] = vb[n].min(apex[0]);
    }
    let mut held = vec![Cause::None; vb.len()];
    for j in (1..vb.len()).rev() {
        let v = vb[j];
        if v.is_infinite() {
            continue;
        }
        let (d1, cause) = car.decel(kappa[j], sin_t[j], v);
        let v1 = (v * v + 2.0 * d1 * ds).max(0.0).sqrt().min(car.v_cap);
        let (d2, _) = car.decel(kappa[j - 1], sin_t[j - 1], v1);
        let d = 0.5 * (d1 + d2);
        let v1 = (v * v + 2.0 * d * ds).max(0.0).sqrt();
        if v1 < vb[j - 1] {
            vb[j - 1] = v1;
            held[j - 1] = cause;
        }
    }
    let v: Vec<f64> = vb[..=n].to_vec();
    let mut t = vec![0.0; n + 1];
    let mut code = vec![Code::Motor; n];
    let mut cause = vec![Cause::None; n];
    for i in 0..n {
        if v[i] + v[i + 1] <= 0.0 {
            return Err(LapError::Stops(i as f64 * ds));
        }
        t[i + 1] = t[i] + 2.0 * ds / (v[i] + v[i + 1]);
        (code[i], cause[i]) = if v[i] < vf[i] || v[i + 1] < vf[i + 1] {
            (Code::Braking, held[i])
        } else if why[i].0 == Code::Coast {
            why[i]
        } else if v[i + 1] >= apex[i + 1] * (1.0 - AT_APEX) {
            (Code::Corner, Cause::None)
        } else {
            why[i]
        };
    }
    Ok(Profile { v, t, code, cause, ..Default::default() })
}

/// The stretches where the driver lifts (today's LapRun._coast_mask): the
/// last `share` of each stretch of acceleration that ends where braking
/// begins, from no slower than half the speed braking begins at.
fn coast_mask(prof: &Profile, share: f64) -> Vec<bool> {
    let (code, v) = (&prof.code, &prof.v);
    let n = code.len();
    let mut mask = vec![false; n];
    let driving = |c: Code| matches!(c, Code::Traction | Code::Motor | Code::Battery | Code::Cap);
    for i in 1..n {
        if code[i] == Code::Braking && code[i - 1] != Code::Braking {
            let mut j = i;
            while j > 0 && driving(code[j - 1]) {
                j -= 1;
            }
            let mut w = i - ((share * (i - j) as f64).round() as usize).min(i - j);
            while w < i && v[w] < 0.5 * v[i] {
                w += 1;
            }
            for m in mask.iter_mut().take(i).skip(w) {
                *m = true;
            }
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Window {
        Window { p_deliver: 0, i_mpp: 1, i_dis_lim: 2, i_floor: 3, p_cap: 0 }
    }

    #[test]
    fn a_window_at_its_cap_is_the_power_cap() {
        assert_eq!(battery_cause(&window(), &[80e3, 500.0, 300.0, 400.0], &[80e3]), Cause::PowerCap);
    }

    #[test]
    fn below_its_cap_the_least_current_names_the_cause() {
        let p = [1e30];
        assert_eq!(battery_cause(&window(), &[50e3, 200.0, 300.0, 400.0], &p), Cause::MaxPower);
        assert_eq!(battery_cause(&window(), &[50e3, 500.0, 300.0, 400.0], &p), Cause::CurrentLimit);
        assert_eq!(battery_cause(&window(), &[50e3, 500.0, 300.0, 100.0], &p), Cause::SocFloor);
        // a cap far above the window is no cause
        assert_eq!(battery_cause(&window(), &[50e3, 200.0, 300.0, 400.0], &[80e3]), Cause::MaxPower);
    }

    #[test]
    fn a_margin_is_a_share_of_the_band() {
        assert!((margin([0.5, -1.0, 1.0], true) - 0.5).abs() < 1e-12);
        assert!((margin([-1.5, -1.0, 1.0], false) + 1.0 / 3.0).abs() < 1e-12);
        assert!(margin([0.0, 0.0, 0.0], true) >= 0.0);
    }

    #[test]
    fn lift_and_coast_marks_the_end_of_each_run_into_braking() {
        use Code::*;
        let code = vec![Motor, Motor, Motor, Motor, Braking, Braking, Corner];
        let prof = Profile {
            v: vec![10.0, 12.0, 14.0, 16.0, 18.0, 14.0, 10.0, 10.0],
            code,
            ..Default::default()
        };
        assert_eq!(coast_mask(&prof, 0.5), vec![false, false, true, true, false, false, false]);
        assert_eq!(coast_mask(&prof, 0.0), vec![false; 7]);
    }
}
