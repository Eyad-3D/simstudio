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
/// the share below their top speed over which the E-Motors' torque fades
/// out (MOD-18): a car the motors hold at full load there is at its top
/// speed in its gear
const TOP_FADE: f64 = 0.02;
/// accelerations searched, m/s²
const A_MAX: f64 = 60.0;
/// how close a root is found: speed, m/s; acceleration, m/s²
const V_TOL: f64 = 1e-6;
const A_TOL: f64 = 1e-6;
const MAX_ROOT: usize = 60;
/// the slowest speed the car is solved at, m/s: at rest the model has no
/// answer (the stage 2 spike solved every point at 0.5 m/s, and the solve
/// at 0 m/s and 0 m/s² fails), and below it the forces barely change (the
/// recuperation's fade below 3 m/s the most)
const V_MIN: f64 = 0.5;
/// the energy pass's longest step, s: the books are integrated by
/// Simpson's rule over each step, and across the kinks inside a step (a
/// clamp, a magnitude) that costs accuracy (the FS autocross closed to
/// 5.2e-6 with the track's points as steps, 4.6e-7 at 0.05 s, 6.4e-10 at
/// 0.01 s, 5.0e-9 at 0.002 s, where round-off adds up); the states do not
/// move by it (the battery's energy the same to the digits shown)
pub const BOOKS_STEP: f64 = 0.01;
/// the times a lap's profile is solved again where its energy pass could not
/// follow it (drive_resolving)
pub const RESOLVES: usize = 3;
/// the share of a battery's charge each time again holds it lower for the
/// rest of the lap (drive_resolving): 0.5 % moves the FS car's 1.2 ohm test
/// pack's maximum-power point down by about 0.2 %
const RESOLVE_SOC: f64 = 5e-3;
/// a battery that starts a lap full is held this share of its charge below
/// full for the lap's profile, where it is once the lap's first acceleration
/// has drawn on it (80 kW for 1 s draws 0.3 % of the FS car's 14 A·h): at
/// exactly full it takes no charge back, so the profile would brake the whole
/// lap without regeneration while its energy pass regenerates from the first
/// braking on, on the driven axle alone, past that axle's grip (the FS car's
/// rear tyres by 2.6 %); 0.1 % moves its open-circuit voltage by 0.2 V
const FULL_SHARE: f64 = 1e-3;
/// a stretch of full acceleration takes the mean of the limits at its two
/// ends (Heun's method), but where the motors at full load or the battery's
/// window hold the car at either end, the lesser of the two: the energy pass drives the
/// stretch at that constant acceleration, which with the mean runs past a
/// limit falling along the stretch near its end, and past those two the
/// model has no answer (a motor past its full-load torque reads its loss
/// table past an axis set to Error; a battery past its maximum-power point
/// has no current), where past a tyre's grip or the power cap it is flagged.
/// (The lesser of the two everywhere made the FS autocross 1.1 % slower.)
///
/// a stretch the motors at full load or the battery's window hold is driven
/// in this many steps, each at the lesser of its ends' limits, and its energy
/// pass follows them (their error a quarter of one step's)
const SUB_STEPS: usize = 4;
/// a speed this close below a point's cornering speed is held there by the
/// forward pass (a = 0): the grip left to speed up is about √(2 × this) of
/// the tyres', 0.3 m/s² on the FS car, for one stretch at most
const APEX_HELD: f64 = 1e-4;
/// the first step a search's bracket widens by from its guess, m/s²
const A_STEP: f64 = 0.5;

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
    /// the batteries are held this share of their charge lower for the
    /// profile (0: as they are; drive_resolving's, where the car could not
    /// follow the profile at their limit)
    pub soc_drop: f64,
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
    /// the speed for the energy pass: each point's and, on a stretch the
    /// motors at full load or the battery's window hold, its sub-steps'
    /// (SUB_STEPS), as (time, speed); empty: the points'
    pub fine: Vec<(f64, f64)>,
}

/// Why a lap could not be solved.
#[derive(Clone, Debug, PartialEq)]
pub enum LapError {
    /// the model could not be started or read
    Model(String),
    /// the car stops on the track, at this distance, m
    Stops(f64),
    /// the energy pass stopped at this time from the lap's start, s, and why
    PassStopped(f64, String),
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
            LapError::PassStopped(_, m) => write!(f, "the lap's energy pass: {m}"),
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
    pedal: usize,
    /// each wheel's share of its grip its corner leaves (its `left`)
    lefts: Vec<usize>,
    sites: Vec<Site>,
    windows: Vec<Window>,
    apex: HashMap<(u64, u64), f64>,
    v_cap: f64,
    /// the last answers of the searches: where the next one starts
    /// (acceleration, deceleration, coasting), m/s²
    guess: [f64; 3],
}

/// The lap model's prescribed inputs at a point: the speed and its slope
/// (the acceleration), the Race Track's curvature and its grade (from the
/// slope's sine), each with a slope of 0.
pub fn prescribed(v: f64, a: f64, kappa: f64, sin_t: f64) -> [(f64, f64); 3] {
    [(v, a), (kappa, 0.0), (sin_t / (1.0 - sin_t * sin_t).max(1e-12).sqrt(), 0.0)]
}

/// The margin of a value inside its upper (`up`) or lower bound, as a share
/// of the band's or the value's size: ≥ 0 inside.
fn margin(x: [f64; 3], up: bool) -> f64 {
    let [v, lo, hi] = x;
    let scale = v.abs().max(lo.abs()).max(hi.abs()).max(1e-9);
    if up { (hi - v) / scale } else { (v - lo) / scale }
}

impl<'a> Car<'a> {
    fn new(
        model: &'a Model,
        params: &[f64],
        parts: &Parts,
        v0: f64,
        state: Option<&State>,
        soc_drop: f64,
    ) -> Result<Car<'a>, LapError> {
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
        let pedal = var(format!("{}.cmd", parts.driver))?;
        let lefts =
            parts.wheels.iter().map(|(w, _)| var(format!("{w}.left"))).collect::<Result<_, _>>()?;
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
        let mut s = match state {
            Some(st) => model.point_solver_at(params, st.y.clone(), st.d.clone()),
            None => model
                .point_solver(params, &prescribed(v0, 0.0, 0.0, 0.0))
                .map_err(|e| LapError::Model(format!("the lap model could not be started: {e}")))?,
        };
        // a battery that starts the lap full is held just below full
        // (FULL_SHARE), and each one `soc_drop` lower where asked
        for b in &parts.batteries {
            let soc = format!("{b}.soc");
            if let Some(i) = model.info.y_names.iter().position(|n| *n == soc) {
                let x = s.y()[i];
                let held = (x - soc_drop.max(0.0)).clamp(0.0, 1.0 - FULL_SHARE);
                if held < x {
                    s.hold_state(i, held);
                }
            }
        }
        let mut car = Car {
            s,
            pedal,
            lefts,
            sites,
            windows,
            apex: HashMap::new(),
            v_cap: V_MAX,
            guess: [0.0; 3],
        };
        car.v_cap = car.top_speed();
        Ok(car)
    }

    /// The limits at speed `v` and acceleration `a` on curvature `kappa` and
    /// slope `sin_t`: each site's `[value, lo, hi]`; None where the model
    /// has no answer.
    fn at(&mut self, v: f64, a: f64, kappa: f64, sin_t: f64) -> Option<Vec<[f64; 3]>> {
        self.s.solve(&prescribed(v.max(V_MIN), a, kappa, sin_t)).ok()?;
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
        // every wheel's force inside its grip, and grip left in each one's
        // friction ellipse: a car with no drag and no rolling resistance
        // needs no force at a = 0, which a band of no width (no grip left,
        // the corner past its lateral grip) would hold
        let holds = |car: &mut Self, v: f64| -> bool {
            let Some(x) = car.at(v, 0.0, kappa, sin_t) else { return false };
            let vars = car.s.last_vars();
            car.lefts.iter().all(|&i| vars[i] > 0.0)
                && car.sites.iter().enumerate().all(|(k, s)| {
                    !matches!(s, Site::Wheel(_))
                        || (margin(x[k], true) >= 0.0 && margin(x[k], false) >= 0.0)
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

    /// The root of a margin `f` in `x` (≥ 0 inside, falling as `x` grows):
    /// bracketed from `guess`, widening by A_STEP and doubling within
    /// ±A_MAX, then regula falsi with Illinois' step, bisecting where the
    /// model has no answer; returns the last `x` inside and its site, or
    /// None where no `x` down to -A_MAX is inside.
    #[allow(clippy::type_complexity)] // (a margin of the car at one value)
    fn root(
        &mut self,
        f: &mut dyn FnMut(&mut Self, f64) -> (f64, Option<usize>),
        guess: f64,
    ) -> Option<(f64, Option<usize>)> {
        let g = guess.clamp(-A_MAX, A_MAX);
        let (fg, sg) = f(self, g);
        let (mut lo, mut flo, mut site, mut hi, mut fhi);
        let mut step = A_STEP;
        if fg >= 0.0 {
            (lo, flo, site) = (g, fg, sg);
            loop {
                let x = (lo + step).min(A_MAX);
                let (fx, sx) = f(self, x);
                if fx < 0.0 {
                    (hi, fhi) = (x, fx);
                    break;
                }
                (lo, flo, site) = (x, fx, sx);
                if x >= A_MAX {
                    return Some((lo, site));
                }
                step *= 2.0;
            }
        } else {
            (hi, fhi) = (g, fg);
            loop {
                let x = (hi - step).max(-A_MAX);
                let (fx, sx) = f(self, x);
                if fx >= 0.0 {
                    (lo, flo, site) = (x, fx, sx);
                    break;
                }
                (hi, fhi) = (x, fx);
                if x <= -A_MAX {
                    return None;
                }
                step *= 2.0;
            }
        }
        let mut side = 0i8;
        for _ in 0..MAX_ROOT {
            if (hi - lo).abs() <= A_TOL {
                break;
            }
            let x = if fhi > -1.0 && flo != fhi {
                let x = lo - flo * (hi - lo) / (fhi - flo);
                if (x - lo) * (x - hi) < 0.0 { x } else { 0.5 * (lo + hi) }
            } else {
                0.5 * (lo + hi)
            };
            let (fx, sx) = f(self, x);
            if fx >= 0.0 {
                (lo, flo, site) = (x, fx, sx);
                if side == 1 {
                    fhi *= 0.5;
                }
                side = 1;
            } else {
                (hi, fhi) = (x, fx);
                if side == -1 {
                    flo *= 0.5;
                }
                side = -1;
            }
        }
        Some((lo, site))
    }

    /// The most acceleration at speed `v` and what limits it.
    fn accel(&mut self, kappa: f64, sin_t: f64, v: f64) -> (f64, Code, Cause) {
        let mut f = |car: &mut Self, a: f64| car.drive(v, a, kappa, sin_t);
        let Some((a, site)) = self.root(&mut f, self.guess[0]) else {
            // no drive inside the grip the corner leaves: it coasts
            return (self.coast(kappa, sin_t, v), Code::Corner, Cause::None);
        };
        self.guess[0] = a;
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
        let Some((d, site)) = self.root(&mut f, self.guess[1]) else {
            // no braking inside the grip the corner leaves: the road load alone
            return (-self.coast(kappa, sin_t, v), Cause::None);
        };
        self.guess[1] = d;
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
        let a = self.root(&mut f, self.guess[2]).map_or(-A_MAX, |r| r.0);
        self.guess[2] = a;
        a
    }
}

/// The car's state between laps: the lap model's `y = [x; z]` and discrete
/// values where the last lap's energy pass ended (its battery's charge,
/// its energies).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// the states and iteration variables
    pub y: Vec<f64>,
    /// the discrete values
    pub d: Vec<f64>,
}

/// One lap's speed profile on `track` (today's LapRun.solve: the cornering
/// speeds, the forward and backward passes, and with `opt.coast` a second
/// forward pass that lifts before the braking points), the lap model
/// `model` with parameters `params`, its parts `parts`, the car's state at
/// the lap's start `state` (None: the model's own start).
pub fn solve(
    model: &Model,
    params: &[f64],
    parts: &Parts,
    track: &Track,
    opt: &LapOptions,
    state: Option<&State>,
) -> Result<Profile, LapError> {
    let n = track.kappa.len().saturating_sub(1);
    if n == 0 || track.sin_t.len() != n + 1 || track.ds.is_nan() || track.ds <= 0.0 {
        return Err(LapError::Model(
            "the track needs two points or more, a spacing above 0".into(),
        ));
    }
    let mut car =
        Car::new(model, params, parts, opt.v_start.clamp(0.0, V_MAX), state, opt.soc_drop)?;
    let apex: Vec<f64> =
        track.kappa.iter().zip(&track.sin_t).map(|(&k, &s)| car.apex(k, s)).collect();
    let (vf, why, subs) = forward(&mut car, track, &apex, opt.v_start, None);
    let mut prof = braking(&mut car, track, &apex, &vf, &why, &subs, opt)?;
    if opt.coast > 0.0 {
        let mask = coast_mask(&prof, opt.coast);
        if mask.iter().any(|m| *m) {
            let (vf, why, subs) = forward(&mut car, track, &apex, opt.v_start, Some(&mask));
            prof = braking(&mut car, track, &apex, &vf, &why, &subs, opt)?;
        }
    }
    prof.apex = apex;
    prof.v_cap = car.v_cap;
    prof.stats = car.s.stats;
    Ok(prof)
}

/// One lap's energy pass (today's LapSlave and ElectricalSlave): fast mode
/// along the profile `prof` on `track` (the speed, the curvature and the
/// grade as traces in time, the speed linear between the track's points as
/// the profile's constant acceleration makes it), from the car's state
/// `state` (None: the model's own start), an output point at every track
/// point, the quantities `integrate` integrated over it (the energy books:
/// `Model::books_quantities`). The run ends in the state the next lap starts
/// from ([`State`] from its `end_y` and `end_d`).
pub fn drive(
    model: &Model,
    params: &[f64],
    track: &Track,
    prof: &Profile,
    state: Option<&State>,
    integrate: &[lsim_engine::lsim_ir::expr::Expr],
) -> Result<lsim_fast::FastResult, LapError> {
    use lsim_fast::{FastOptions, Trace};
    let err = |e: &dyn std::fmt::Display| LapError::Model(format!("the lap's energy pass: {e}"));
    let t = prof.t.clone();
    let grade: Vec<f64> = track.sin_t.iter().map(|s| s / (1.0 - s * s).max(1e-12).sqrt()).collect();
    let (ft, fv): (Vec<f64>, Vec<f64>) = if prof.fine.is_empty() {
        (t.clone(), prof.v.clone())
    } else {
        prof.fine.iter().copied().unzip()
    };
    let traces = [
        Trace::new(ft, fv).map_err(|e| err(&e))?,
        Trace::new(t.clone(), track.kappa.clone()).map_err(|e| err(&e))?,
        Trace::new(t.clone(), grade).map_err(|e| err(&e))?,
    ];
    let t_end = *t.last().unwrap_or(&0.0);
    let o = FastOptions {
        step: BOOKS_STEP.min(t_end.max(1e-9)),
        output_grid: false,
        t_start: Some(0.0),
        t_end: Some(t_end),
        output_at_samples: true,
        output_trace: 1,
        ..Default::default()
    };
    let start = state.map(|s| (s.y.as_slice(), s.d.as_slice()));
    model.fast_from(params, start, &traces, &o, None, &mut [], integrate).map_err(|e| match &e {
        lsim_engine::RunError::Fast(
            lsim_fast::FastError::NoConvergence { t, .. }
            | lsim_fast::FastError::Singular { t, .. },
        ) => LapError::PassStopped(*t, e.to_string()),
        _ => err(&e),
    })
}

/// One lap's energy pass, the profile solved again where the car could not
/// follow it: where the pass stops (a battery asked past its maximum-power
/// point has no answer), or where the motors ask their supply for more than
/// it gives over a stretch or more, the rest of the lap is solved again from
/// the car's state a stretch before (its battery's charge then, not the
/// lap's start), and the lap is driven again; at most RESOLVES times, then
/// the last stop or the flagged pass is the answer. The profile driven, the
/// pass, and how many times it was solved again.
#[allow(clippy::too_many_arguments)]
pub fn drive_resolving(
    model: &Model,
    params: &[f64],
    parts: &Parts,
    track: &Track,
    prof: &Profile,
    opt: &LapOptions,
    state: Option<&State>,
    integrate: &[lsim_engine::lsim_ir::expr::Expr],
) -> Result<(Profile, lsim_fast::FastResult, usize), LapError> {
    let mut prof = prof.clone();
    // the last pass that ran (flagged where the car could not follow): the
    // answer when solving again does not help (an empty battery)
    let mut ran: Option<(Profile, lsim_fast::FastResult, usize)> = None;
    for k in 0..=RESOLVES {
        let t_bad = match drive(model, params, track, &prof, state, integrate) {
            Ok(r) => match supply_short(parts, &prof, &r) {
                Some(t) if k < RESOLVES => {
                    ran = Some((prof.clone(), r, k));
                    t
                }
                _ => return Ok((prof, r, k)),
            },
            Err(LapError::PassStopped(t, _)) if k < RESOLVES => t,
            Err(e) => return ran.ok_or(e),
        };
        match resolve_from(model, params, parts, track, &prof, opt, state, integrate, t_bad, k) {
            Ok(p) => prof = p,
            Err(e) => return ran.ok_or(e),
        }
    }
    unreachable!("the loop returns by its last round")
}

/// The profile solved again from a stretch before `t_bad` on (drive_resolving).
#[allow(clippy::too_many_arguments)]
fn resolve_from(
    model: &Model,
    params: &[f64],
    parts: &Parts,
    track: &Track,
    prof: &Profile,
    opt: &LapOptions,
    state: Option<&State>,
    integrate: &[lsim_engine::lsim_ir::expr::Expr],
    t_bad: f64,
    k: usize,
) -> Result<Profile, LapError> {
    // a stretch before the trouble: the pass up to there gives the state
    let i0 = prof.t.iter().rposition(|t| *t < t_bad).unwrap_or(0).saturating_sub(1);
    if i0 == 0 {
        return Err(LapError::PassStopped(t_bad, "at the lap's start".into()));
    }
    let head = cut(track, prof, i0);
    let r0 = drive(model, params, &head.0, &head.1, state, integrate)?;
    let at = State { y: r0.end_y, d: r0.end_d };
    let rest = Track {
        kappa: track.kappa[i0..].to_vec(),
        sin_t: track.sin_t[i0..].to_vec(),
        ds: track.ds,
        closed: false,
    };
    // (each time again with the batteries held a further RESOLVE_SOC lower:
    // a battery driven at its limit asked a hair more than it then gives)
    let o = LapOptions {
        v_start: prof.v[i0],
        soc_drop: opt.soc_drop + RESOLVE_SOC * (k + 1) as f64,
        ..opt.clone()
    };
    let tail = solve(model, params, parts, &rest, &o, Some(&at))?;
    Ok(splice(prof, i0, &tail))
}

/// The time the motors first ask their supply for more than it gives over
/// a whole stretch or more (two track points or more inside the flag).
fn supply_short(parts: &Parts, prof: &Profile, r: &lsim_fast::FastResult) -> Option<f64> {
    r.flags
        .iter()
        .filter(|f| f.upper && parts.motors.contains(&f.part))
        .filter(|f| prof.t.iter().filter(|t| **t > f.t_start && **t < f.t_end).count() >= 2)
        .map(|f| f.t_start)
        .next()
}

/// The track and the profile up to point `i` (inclusive).
fn cut(track: &Track, prof: &Profile, i: usize) -> (Track, Profile) {
    (
        Track {
            kappa: track.kappa[..=i].to_vec(),
            sin_t: track.sin_t[..=i].to_vec(),
            ds: track.ds,
            closed: false,
        },
        cut_profile(prof, i),
    )
}

/// The profile up to point `i` (inclusive).
fn cut_profile(prof: &Profile, i: usize) -> Profile {
    let fine = prof.fine.iter().copied().filter(|(t, _)| *t <= prof.t[i]).collect();
    Profile {
        v: prof.v[..=i].to_vec(),
        t: prof.t[..=i].to_vec(),
        code: prof.code[..i].to_vec(),
        cause: prof.cause[..i].to_vec(),
        apex: prof.apex.get(..=i).map(|a| a.to_vec()).unwrap_or_default(),
        fine,
        ..prof.clone()
    }
}

/// The profile `prof` up to point `i`, then `tail` (solved from there) on.
fn splice(prof: &Profile, i: usize, tail: &Profile) -> Profile {
    let t0 = prof.t[i];
    let mut p = cut_profile(prof, i);
    p.v[i] = tail.v[0].min(p.v[i]);
    p.v.extend_from_slice(&tail.v[1..]);
    p.t.extend(tail.t[1..].iter().map(|t| t + t0));
    p.code.extend_from_slice(&tail.code);
    p.cause.extend_from_slice(&tail.cause);
    if !p.apex.is_empty() {
        p.apex.extend(tail.apex.iter().skip(1));
    }
    if !p.fine.is_empty() || !tail.fine.is_empty() {
        if p.fine.is_empty() {
            p.fine = p.t.iter().copied().zip(p.v.iter().copied()).take(i + 1).collect();
        }
        let tf: Vec<(f64, f64)> = if tail.fine.is_empty() {
            tail.t.iter().copied().zip(tail.v.iter().copied()).collect()
        } else {
            tail.fine.clone()
        };
        p.fine.extend(tf.into_iter().skip(1).map(|(t, v)| (t + t0, v)));
    }
    p.stats = prof.stats;
    p
}

/// Joins a lap's energy pass `lap` onto the laps before it `run` (None:
/// it is the first) as one run: its times after theirs (its first point,
/// the last one's end again, dropped), its flags and events moved with
/// them, the work and the integrals summed, the books' start values the
/// first lap's and their end values and the end state this one's.
pub fn join(
    run: Option<lsim_fast::FastResult>,
    mut lap: lsim_fast::FastResult,
) -> lsim_fast::FastResult {
    let Some(mut run) = run else { return lap };
    let t0 = *run.times.last().unwrap_or(&0.0);
    let n = run.names.len();
    debug_assert_eq!(n, lap.names.len());
    let skip = usize::from(!lap.times.is_empty());
    run.times.extend(lap.times.iter().skip(skip).map(|t| t + t0));
    for (to, from) in [
        (&mut run.values, &lap.values),
        (&mut run.min, &lap.min),
        (&mut run.max, &lap.max),
        (&mut run.mean, &lap.mean),
    ] {
        to.extend_from_slice(&from[(skip * n).min(from.len())..]);
    }
    run.flags.extend(lap.flags.drain(..).map(|mut f| {
        f.t_start += t0;
        f.t_end += t0;
        f
    }));
    run.events.extend(lap.events.drain(..).map(|mut e| {
        e.t += t0;
        e
    }));
    let (a, b) = (&mut run.report, &lap.report);
    for (x, y) in [
        (&mut a.steps, b.steps),
        (&mut a.residual_evals, b.residual_evals),
        (&mut a.vars_evals, b.vars_evals),
        (&mut a.jacobian_evals, b.jacobian_evals),
        (&mut a.lu_factorisations, b.lu_factorisations),
        (&mut a.newton_iterations, b.newton_iterations),
        (&mut a.gz_refreshes, b.gz_refreshes),
        (&mut a.step_redos, b.step_redos),
        (&mut a.newton_restarts, b.newton_restarts),
        (&mut a.events, b.events),
        (&mut a.block_changes, b.block_changes),
    ] {
        *x += y;
    }
    if b.max_error > a.max_error {
        a.max_error = b.max_error;
        a.max_error_at = b.max_error_at + t0;
    }
    run.wall_seconds += lap.wall_seconds;
    for (x, y) in run.integrals.iter_mut().zip(&lap.integrals) {
        *x += y;
    }
    for (x, y) in run.integrals_abs.iter_mut().zip(&lap.integrals_abs) {
        *x += y;
    }
    run.q_end = lap.q_end;
    run.end_y = lap.end_y;
    run.end_d = lap.end_d;
    run
}

/// Full acceleration from `v_start` (Heun's method over each spacing),
/// capped at each point's cornering speed and the top speed; on the
/// stretches `coast` marks, the pedal at 0. Returns the speeds and what
/// limited each stretch.
#[allow(clippy::type_complexity)] // (the speeds, what held each stretch, its sub-steps)
fn forward(
    car: &mut Car,
    tr: &Track,
    apex: &[f64],
    v_start: f64,
    coast: Option<&[bool]>,
) -> (Vec<f64>, Vec<(Code, Cause)>, Vec<Vec<f64>>) {
    let n = tr.kappa.len() - 1;
    let ds = tr.ds;
    // (each step's predictor is taken no faster than the next point allows:
    // past its cornering speed the car has no steady answer, and the step is
    // held there anyway)
    let mut vf = vec![0.0; n + 1];
    let mut why = vec![(Code::Motor, Cause::None); n];
    let mut subs = vec![vec![]; n];
    vf[0] = v_start.min(apex[0]).min(car.v_cap);
    for i in 0..n {
        let v = vf[i];
        let mut fine = vec![];
        let a = if coast.is_some_and(|c| c[i]) {
            let a1 = car.coast(tr.kappa[i], tr.sin_t[i], v);
            let v2 = (v * v + 2.0 * a1 * ds).max(0.0).sqrt().min(car.v_cap).min(apex[i + 1]);
            let a2 = car.coast(tr.kappa[i + 1], tr.sin_t[i + 1], v2);
            why[i] = (Code::Coast, Cause::None);
            0.5 * (a1 + a2)
        } else {
            // at a point's cornering speed the car holds it, no grip left to
            // speed up (the model, at the edge of its friction ellipse, may
            // have no answer there)
            let held = |v: f64, apex: f64| v >= apex * (1.0 - APEX_HELD);
            let (a1, code, cause) = if held(v, apex[i]) {
                (0.0, Code::Corner, Cause::None)
            } else {
                car.accel(tr.kappa[i], tr.sin_t[i], v)
            };
            let v2 = (v * v + 2.0 * a1 * ds).max(0.0).sqrt().min(car.v_cap).min(apex[i + 1]);
            why[i] = (code, cause);
            if held(v2, apex[i + 1]) {
                a1 // (into a cornering speed: held there)
            } else {
                let (a2, code2, _) = car.accel(tr.kappa[i + 1], tr.sin_t[i + 1], v2);
                let hard = |c: Code| matches!(c, Code::Motor | Code::Battery);
                if hard(code) || hard(code2) {
                    let (vk, speeds) = sub_steps(car, tr, i, v, a1);
                    fine = speeds;
                    (vk * vk - v * v) / (2.0 * ds)
                } else {
                    0.5 * (a1 + a2)
                }
            }
        };
        let next = (v * v + 2.0 * a * ds).max(0.0).sqrt();
        vf[i + 1] = next.min(apex[i + 1]).min(car.v_cap);
        if vf[i + 1] == next {
            subs[i] = fine;
        }
        // at the top speed: held there, or held by the motors at full load
        // in their last 2 %, where their torque fades out (the car settles
        // where it meets the road load, short of the cap)
        let near = vf[i + 1] >= car.v_cap * (1.0 - TOP_FADE) && car.v_cap <= apex[i + 1];
        if near && (vf[i + 1] >= car.v_cap * (1.0 - AT_APEX) || why[i].0 == Code::Motor) {
            why[i] = (Code::TopSpeed, Cause::None);
        }
    }
    (vf, why, subs)
}

/// A stretch the motors at full load or the battery's window hold, from
/// speed `v` at point `i` (where the most acceleration is `a1`), in
/// SUB_STEPS steps, each at the lesser of the limits at its two ends (the
/// curvature and slope between the points' linearly): the speed at its end
/// and at each step between.
fn sub_steps(car: &mut Car, tr: &Track, i: usize, v: f64, a1: f64) -> (f64, Vec<f64>) {
    let h = tr.ds / SUB_STEPS as f64;
    let at = |k: usize| {
        let f = k as f64 / SUB_STEPS as f64;
        (
            tr.kappa[i] + f * (tr.kappa[i + 1] - tr.kappa[i]),
            tr.sin_t[i] + f * (tr.sin_t[i + 1] - tr.sin_t[i]),
        )
    };
    let (mut vk, mut b1) = (v, a1);
    let mut speeds = vec![];
    for k in 0..SUB_STEPS {
        let vp = (vk * vk + 2.0 * b1 * h).max(0.0).sqrt().min(car.v_cap);
        let (kappa, sin_t) = at(k + 1);
        let b2 = car.accel(kappa, sin_t, vp).0;
        vk = (vk * vk + 2.0 * b1.min(b2) * h).max(0.0).sqrt().min(car.v_cap);
        if k + 1 < SUB_STEPS {
            speeds.push(vk);
            b1 = car.accel(kappa, sin_t, vk).0; // (the next step's start)
        }
    }
    (vk, speeds)
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
    subs: &[Vec<f64>],
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
    let mut fine = vec![(0.0, v[0])];
    let mut any_sub = false;
    for i in 0..n {
        if v[i] + v[i + 1] <= 0.0 {
            return Err(LapError::Stops(i as f64 * ds));
        }
        // a stretch driven in sub-steps, where braking did not take it over
        let sub = !subs[i].is_empty() && v[i] == vf[i] && v[i + 1] == vf[i + 1];
        if sub {
            let h = ds / (subs[i].len() + 1) as f64;
            let mut tk = t[i];
            let mut vk = v[i];
            for &vs in subs[i].iter().chain(std::iter::once(&v[i + 1])) {
                tk += 2.0 * h / (vk + vs);
                vk = vs;
                fine.push((tk, vs));
            }
            t[i + 1] = tk;
            any_sub = true;
        } else {
            t[i + 1] = t[i] + 2.0 * ds / (v[i] + v[i + 1]);
            fine.push((t[i + 1], v[i + 1]));
        }
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
    if !any_sub {
        fine.clear();
    }
    Ok(Profile { v, t, code, cause, fine, ..Default::default() })
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
    use lsim_engine::{BuildOptions, Engine};
    use lsim_project::{ImportOptions, import_case, standard_registry};

    /// The FS example's car as its autocross case's lap model, and its parts.
    fn fs_car() -> (Model, Parts) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../backend/projects/fs-electric.json");
        let pj: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("the FS example")).unwrap();
        let opts = ImportOptions { inverse: true, lap: true, ..Default::default() };
        let (top, rep) = import_case(&pj, Some("case-autocross"), &standard_registry(), &opts)
            .expect("its lap model imports");
        let pr = rep.inverse.clone().unwrap();
        let lp = rep.lap.clone().unwrap();
        let model = Engine::new(rep.library())
            .build_inverse(&top, &pr.spec, &BuildOptions::default())
            .expect("its lap model builds");
        let parts = Parts {
            track: lp.track,
            driver: lp.driver,
            wheels: lp.wheels,
            motors: lp.motors,
            batteries: lp.batteries,
        };
        (model, parts)
    }

    /// A track a point a metre: `straight` m, a corner of `radius` m (+
    /// left) and `length` m, then `straight` m again.
    fn track(straight: usize, radius: f64, length: usize) -> Track {
        let n = 2 * straight + length;
        let kappa = (0..=n)
            .map(|i| if i > straight && i <= straight + length { 1.0 / radius } else { 0.0 })
            .collect();
        Track { kappa, sin_t: vec![0.0; n + 1], ds: 1.0, closed: false }
    }

    /// The top speed is the motor's in its gear: 6 500 1/min through the
    /// chain's 4.4 on 0.228 m wheels, 35.272 m/s, the highest speed the model
    /// solves at; the car settles within the motor's last 2 %, where its
    /// torque fades out, and the limit is named so.
    #[test]
    fn the_lap_is_held_at_the_motors_top_speed() {
        let (model, parts) = fs_car();
        let by_hand = 6500.0 * std::f64::consts::PI / 30.0 / 4.4 * 0.228;
        let tr = track(2000, 1e9, 1);
        let p =
            solve(&model, &model.info.params, &parts, &tr, &LapOptions::default(), None).unwrap();
        assert!((p.v_cap - by_hand).abs() < 1e-3 * by_hand, "{} against {by_hand}", p.v_cap);
        let top = p.v.iter().cloned().fold(0.0, f64::max);
        // (where the fading torque meets the road load: 35.005 m/s)
        assert!(top <= p.v_cap * (1.0 + 1e-12) && top > (1.0 - TOP_FADE) * p.v_cap, "{top}");
        assert!(p.code.contains(&Code::TopSpeed));
    }

    /// Every braking probe at or below the top speed solves (a failed one
    /// would read as out of reach): the deceleration search on the level and
    /// in corners, from 0.5 m/s to the cap.
    #[test]
    fn braking_probes_at_or_below_the_cap_all_solve() {
        let (model, parts) = fs_car();
        let mut car = Car::new(&model, &model.info.params, &parts, 0.0, None, 0.0).unwrap();
        let cap = car.v_cap;
        for kappa in [0.0, 0.02, 0.1] {
            for k in 0..=40 {
                let v = cap * k as f64 / 40.0;
                for a in [-1.0, -5.0, -10.0, -20.0] {
                    assert!(
                        car.at(v, a, kappa, 0.0).is_some(),
                        "braking at {v} m/s, {a} m/s², κ {kappa}"
                    );
                }
            }
        }
    }

    /// From rest on the level the driven rear tyres hold the car; at speed
    /// the battery's 80 kW Output Power Limit does, named as its cap; and
    /// braking into the corner is held by an axle's grip, named.
    #[test]
    fn the_limits_are_named_as_the_car_meets_them() {
        let (model, parts) = fs_car();
        let tr = track(200, 15.0, 40);
        let p =
            solve(&model, &model.info.params, &parts, &tr, &LapOptions::default(), None).unwrap();
        assert_eq!((p.code[0], p.cause[0]), (Code::Traction, Cause::RearAxle));
        assert!(p.code.iter().zip(&p.cause).any(|(c, w)| *c == Code::Cap && *w == Cause::PowerCap));
        assert!(
            p.code.iter().zip(&p.cause).any(
                |(c, w)| *c == Code::Braking && matches!(w, Cause::RearAxle | Cause::FrontAxle)
            )
        );
        assert!(p.code.contains(&Code::Corner));
        // in the corner, at its cornering speed
        let mid = 200 + 20;
        assert!(
            (p.v[mid] - p.apex[mid]).abs() < 1e-3 * p.apex[mid],
            "{} against {}",
            p.v[mid],
            p.apex[mid]
        );
        assert!(p.t.windows(2).all(|w| w[1] > w[0]));
    }

    /// A lap that ends at a standstill (the endurance's driver change) ends
    /// at 0, braking into it.
    #[test]
    fn a_lap_into_the_driver_change_ends_at_rest() {
        let (model, parts) = fs_car();
        let tr = track(100, 1e9, 1);
        let opt = LapOptions { v_start: 10.0, stop: true, ..Default::default() };
        let p = solve(&model, &model.info.params, &parts, &tr, &opt, None).unwrap();
        assert_eq!(*p.v.last().unwrap(), 0.0);
        assert_eq!(*p.code.last().unwrap(), Code::Braking);
    }

    /// Two laps' energy passes, the second from the first one's end state,
    /// join into one run: its time goes on, no point twice, the battery's
    /// energy carried over, and the books close over both.
    #[test]
    fn laps_driven_on_join_into_one_run_whose_books_close() {
        let (model, parts) = fs_car();
        let tr = track(150, 20.0, 60);
        let (q, _) = model.books_quantities();
        let p = &model.info.params;
        let (mut run, mut state, mut v0) = (None, None::<State>, 0.0);
        let mut ends = vec![];
        for k in 0..2 {
            let opt = LapOptions { v_start: v0, more: k == 0, ..Default::default() };
            let prof = solve(&model, p, &parts, &tr, &opt, state.as_ref()).unwrap();
            let r = drive(&model, p, &tr, &prof, state.as_ref(), &q).unwrap();
            let e = r.channel("el_battery.e_delivered").unwrap();
            ends.push((*e.first().unwrap(), *e.last().unwrap(), r.times.len()));
            v0 = *prof.v.last().unwrap();
            state = Some(State { y: r.end_y.clone(), d: r.end_d.clone() });
            run = Some(join(run, r));
        }
        let run = run.unwrap();
        assert_eq!(ends[1].0, ends[0].1, "the second lap starts with the first one's energy");
        assert_eq!(run.times.len(), ends[0].2 + ends[1].2 - 1);
        assert!(run.times.windows(2).all(|w| w[1] > w[0]));
        assert_eq!(*run.channel("el_battery.e_delivered").unwrap().last().unwrap(), ends[1].1);
        let b = model.books(&run);
        assert!(b.closure.abs() < 1e-6 * b.throughput, "{b:?}");
    }

    fn window() -> Window {
        Window { p_deliver: 0, i_mpp: 1, i_dis_lim: 2, i_floor: 3, p_cap: 0 }
    }

    #[test]
    fn a_window_at_its_cap_is_the_power_cap() {
        assert_eq!(
            battery_cause(&window(), &[80e3, 500.0, 300.0, 400.0], &[80e3]),
            Cause::PowerCap
        );
    }

    #[test]
    fn below_its_cap_the_least_current_names_the_cause() {
        let p = [1e30];
        assert_eq!(battery_cause(&window(), &[50e3, 200.0, 300.0, 400.0], &p), Cause::MaxPower);
        assert_eq!(battery_cause(&window(), &[50e3, 500.0, 300.0, 400.0], &p), Cause::CurrentLimit);
        assert_eq!(battery_cause(&window(), &[50e3, 500.0, 300.0, 100.0], &p), Cause::SocFloor);
        // a cap far above the window is no cause
        assert_eq!(
            battery_cause(&window(), &[50e3, 200.0, 300.0, 400.0], &[80e3]),
            Cause::MaxPower
        );
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
