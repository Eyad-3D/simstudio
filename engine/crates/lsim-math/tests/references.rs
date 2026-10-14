//! Every function of the library is correctly rounded on hard cases, in
//! every build this CPU runs, and the builds agree bit for bit.
//!
//! The references (`data/hard_cases.txt`, written by `tools/hard_cases.py`)
//! are the correctly rounded results, computed offline twice and
//! independently (MPFR in an IEEE binary64 context, and mpmath at 4000 bits
//! rounded to nearest), on: an even sample of CORE-MATH's hard-to-round
//! cases for each function (the inputs whose exact value lies closest to a
//! rounding boundary), special values (zeros, infinities, NaN,
//! subnormals, domain edges, overflow and underflow thresholds), random
//! values over the ranges models use, the inputs a CORE-MATH revision got
//! wrong (exp at 37384b5, which the vendored revision precedes), the
//! inputs where glibc 2.39 errs by more than an ulp (up to 1.4e5 ulps), and
//! the 543 references of lsim-solve's former test of the platform's libm.

use lsim_math as m;

/// One reference: function, where it comes from, inputs, result.
struct Case {
    f: String,
    tag: String,
    args: Vec<f64>,
    r: f64,
}

fn cases() -> Vec<Case> {
    let text = include_str!("data/hard_cases.txt");
    let mut out = vec![];
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let t: Vec<&str> = line.split_whitespace().collect();
        let n = t.len();
        assert!(n >= 5 && t[n - 2] == "=", "a malformed line: {line}");
        let bits = |s: &str| f64::from_bits(u64::from_str_radix(s, 16).expect("hex"));
        out.push(Case {
            f: t[0].to_string(),
            tag: t[1].to_string(),
            args: t[2..n - 2].iter().map(|s| bits(s)).collect(),
            r: bits(t[n - 1]),
        });
    }
    out
}

/// The function `f` of a build at `a` (`None`: not one of the library's).
fn eval(b: &m::Build, f: &str, a: &[f64]) -> f64 {
    match f {
        "sin" => b.sin(a[0]),
        "cos" => b.cos(a[0]),
        "tan" => b.tan(a[0]),
        "asin" => b.asin(a[0]),
        "acos" => b.acos(a[0]),
        "atan" => b.atan(a[0]),
        "atan2" => b.atan2(a[0], a[1]),
        "sinh" => b.sinh(a[0]),
        "cosh" => b.cosh(a[0]),
        "tanh" => b.tanh(a[0]),
        "exp" => b.exp(a[0]),
        "log" => b.ln(a[0]),
        "pow" => b.pow(a[0], a[1]),
        _ => panic!("no function {f}"),
    }
}

/// The crate's own entry points (the build chosen for this CPU).
fn eval_api(f: &str, a: &[f64]) -> f64 {
    match f {
        "sin" => m::sin(a[0]),
        "cos" => m::cos(a[0]),
        "tan" => m::tan(a[0]),
        "asin" => m::asin(a[0]),
        "acos" => m::acos(a[0]),
        "atan" => m::atan(a[0]),
        "atan2" => m::atan2(a[0], a[1]),
        "sinh" => m::sinh(a[0]),
        "cosh" => m::cosh(a[0]),
        "tanh" => m::tanh(a[0]),
        "exp" => m::exp(a[0]),
        "log" => m::ln(a[0]),
        "pow" => m::pow(a[0], a[1]),
        "fma" => m::fma(a[0], a[1], a[2]),
        _ => panic!("no function {f}"),
    }
}

/// The C entry points (what SUNDIALS calls).
fn eval_c(f: &str, a: &[f64]) -> f64 {
    match f {
        "sin" => m::lsim_math_sin(a[0]),
        "cos" => m::lsim_math_cos(a[0]),
        "tan" => m::lsim_math_tan(a[0]),
        "asin" => m::lsim_math_asin(a[0]),
        "acos" => m::lsim_math_acos(a[0]),
        "atan" => m::lsim_math_atan(a[0]),
        "atan2" => m::lsim_math_atan2(a[0], a[1]),
        "sinh" => m::lsim_math_sinh(a[0]),
        "cosh" => m::lsim_math_cosh(a[0]),
        "tanh" => m::lsim_math_tanh(a[0]),
        "exp" => m::lsim_math_exp(a[0]),
        "log" => m::lsim_math_log(a[0]),
        "pow" => m::lsim_math_pow(a[0], a[1]),
        "fma" => m::lsim_math_fma(a[0], a[1], a[2]),
        _ => panic!("no function {f}"),
    }
}

/// Bitwise equal, any NaN matching any NaN.
fn same(a: f64, r: f64) -> bool {
    if r.is_nan() { a.is_nan() } else { a.to_bits() == r.to_bits() }
}

const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "exp", "log",
    "pow",
];

#[test]
fn every_function_is_correctly_rounded_on_the_hard_cases() {
    let cases = cases();
    // the file is whole: every function with its hard cases and specials
    for f in FUNCTIONS {
        let n = |tag: &str| cases.iter().filter(|c| c.f == *f && c.tag == tag).count();
        assert!(n("wc") >= 150 && n("special") >= 40 && n("random") >= 50, "{f}");
    }
    assert!(cases.len() > 4600, "{}", cases.len());
    let builds = m::builds();
    let mut wrong = vec![];
    for c in cases.iter().filter(|c| c.f != "fma") {
        for b in &builds {
            let v = eval(b, &c.f, &c.args);
            if !same(v, c.r) {
                wrong.push(format!(
                    "{}({}) [{}], {} build: {:e} ({:#018x}) against {:e} ({:#018x})",
                    c.f,
                    c.args.iter().map(|a| format!("{a:e}")).collect::<Vec<_>>().join(", "),
                    c.tag,
                    b.name,
                    v,
                    v.to_bits(),
                    c.r,
                    c.r.to_bits()
                ));
            }
        }
        for (how, v) in [("the API", eval_api(&c.f, &c.args)), ("C", eval_c(&c.f, &c.args))] {
            if !same(v, c.r) {
                wrong.push(format!("{}({:?}) through {how}: {v:e} against {:e}", c.f, c.args, c.r));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} results are not the correctly rounded ones:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_fused_multiply_add_is_rounded_once() {
    let cases = cases();
    let fma: Vec<&Case> = cases.iter().filter(|c| c.f == "fma").collect();
    assert!(fma.len() > 300);
    for c in fma {
        let (a, b, d) = (c.args[0], c.args[1], c.args[2]);
        assert!(same(m::fma(a, b, d), c.r), "fma({a:e}, {b:e}, {d:e})");
        assert!(same(eval_c("fma", &c.args), c.r), "lsim_math_fma({a:e}, {b:e}, {d:e})");
        #[cfg(target_arch = "x86_64")]
        assert!(same(m::fma_soft(a, b, d), c.r), "fma_soft({a:e}, {b:e}, {d:e})");
    }
    // the exact error of a product, against Dekker's
    let mut rng = Rng(0x853c_49e6_748f_ea9b);
    for _ in 0..200_000 {
        let (x, y) = (rng.wide(), rng.wide());
        let p = x * y;
        if !p.is_finite() || p.abs() < 1e-290 || p.abs() > 1e290 {
            continue;
        }
        let split = |v: f64| {
            let c = 134_217_729.0 * v;
            let h = c - (c - v);
            (h, v - h)
        };
        let ((xh, xl), (yh, yl)) = (split(x), split(y));
        let e = ((xh * yh - p) + xh * yl + xl * yh) + xl * yl;
        assert_eq!(m::fma(x, y, -p).to_bits(), e.to_bits(), "{x:e} × {y:e}");
        #[cfg(target_arch = "x86_64")]
        assert_eq!(m::fma_soft(x, y, -p).to_bits(), e.to_bits(), "{x:e} × {y:e}");
    }
}

/// xorshift64
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// uniform in [lo, hi)
    fn unif(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * ((self.next() >> 11) as f64 / (1u64 << 53) as f64)
    }
    /// ± 10^u, u uniform in [-30, 30)
    fn wide(&mut self) -> f64 {
        let s = if self.next() & 1 == 0 { 1.0 } else { -1.0 };
        s * lsim_math::pow(10.0, self.unif(-30.0, 30.0))
    }
}

#[test]
fn the_builds_agree_bit_for_bit() {
    let builds = m::builds();
    if builds.len() < 2 {
        eprintln!("only the {} build runs on this CPU", builds[0].name);
        return;
    }
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for f in FUNCTIONS {
        for k in 0..20_000 {
            let a = match *f {
                "asin" | "acos" => [rng.unif(-1.0, 1.0), 0.0],
                "atan2" => [rng.wide(), rng.wide()],
                "pow" => [rng.wide().abs(), rng.unif(-20.0, 20.0)],
                "exp" | "sinh" | "cosh" | "tanh" => [rng.unif(-750.0, 750.0), 0.0],
                "log" => [rng.wide().abs(), 0.0],
                _ if k % 2 == 0 => [rng.unif(-100.0, 100.0), 0.0],
                _ => [rng.wide(), 0.0],
            };
            let v: Vec<f64> = builds.iter().map(|b| eval(b, f, &a)).collect();
            assert!(v.iter().all(|x| same(*x, v[0])), "{f}({a:?}): {v:?}");
        }
    }
}

/// `powi` multiplies by repeated squaring, the same products in the same
/// order on every platform (Rust's own `f64::powi` does so on Linux but
/// calls the C runtime's `pow` on Windows: 0.628681636656856^32 differs),
/// within its bound of the exact power.
#[test]
fn powi_multiplies_by_repeated_squaring() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..100_000 {
        let x = rng.unif(-3.0, 3.0);
        let x2 = x * x;
        let x4 = x2 * x2;
        assert!(same(m::powi(x, 1), x));
        assert!(same(m::powi(x, 2), x2));
        assert!(same(m::powi(x, 3), x * x2));
        assert!(same(m::powi(x, 4), x4));
        assert!(same(m::powi(x, 5), x * x4));
        assert!(same(m::powi(x, 6), x2 * x4));
        assert!(same(m::powi(x, 7), x * x2 * x4));
        assert!(same(m::powi(x, 32), (x4 * x4) * (x4 * x4) * ((x4 * x4) * (x4 * x4))));
        assert!(same(m::powi(x, -3), 1.0 / (x * x2)));
        // within (|n| − 1) u of the exact power (double-double), n ≤ 64
        let n = (rng.next() % 64) as i32 + 1;
        let (mut hi, mut lo) = (1.0f64, 0.0f64);
        for _ in 0..n {
            let p = hi * x;
            let e = m::fma(hi, x, -p) + lo * x;
            let s = p + e;
            lo = e - (s - p);
            hi = s;
        }
        // (the error against hi + lo, without rounding that sum)
        let bound = (n - 1) as f64 * f64::EPSILON / 2.0 * hi.abs() * (1.0 + 1e-12);
        assert!(((m::powi(x, n) - hi) - lo).abs() <= bound + 1e-300, "{x}^{n}");
    }
    assert_eq!(m::powi(2.0, -1022), f64::MIN_POSITIVE);
    // (the reciprocal of a power that overflows)
    assert_eq!(m::powi(2.0, -1074), 0.0);
    assert_eq!(m::powi(0.0, -1), f64::INFINITY);
    assert_eq!(m::powi(-1.5, 0), 1.0);
}
