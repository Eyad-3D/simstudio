//! The evidence for the engine's mathematical library (DESIGN.md §7, "The
//! mathematical library"): accuracy against correctly rounded references
//! and time per call, for glibc (the platform's, through `f64`), pxfm
//! 0.1.30, the libm crate 0.2.16 and CORE-MATH built three ways (`build.rs`).
//!
//! ```text
//! python -m venv v && v/bin/pip install gmpy2 mpmath
//! git clone https://gitlab.inria.fr/core-math/core-math       # its .wc files
//! v/bin/python -I gen_refs.py core-math/src/binary64 refs     # MPFR references
//! cargo run --release -- check refs/wc      # mismatches per function and library
//! cargo run --release -- check refs/rand
//! cargo run --release -- check refs/special
//! v/bin/python -I ulps.py refs/wc           # the worst error of each, in ulps
//! cargo run --release -- bench              # ns per call
//! ```
//!
//! `check DIR` reads `DIR/<f>.bin` (x, y, correctly rounded result: u64
//! triples), counts the results that are not the correctly rounded one and
//! writes them to `DIR/<f>.<library>.mis` for `ulps.py`. `bench` times
//! each function over 65 536 inputs in the ranges models use (the best of
//! 15 passes).
use std::hint::black_box;
use std::io::Write;
use std::time::Instant;

#[allow(non_snake_case)]
unsafe extern "C" {
    fn cmA_sin(x: f64) -> f64;
    fn cmA_cos(x: f64) -> f64;
    fn cmA_tan(x: f64) -> f64;
    fn cmA_asin(x: f64) -> f64;
    fn cmA_acos(x: f64) -> f64;
    fn cmA_atan(x: f64) -> f64;
    fn cmA_atan2(y: f64, x: f64) -> f64;
    fn cmA_sinh(x: f64) -> f64;
    fn cmA_cosh(x: f64) -> f64;
    fn cmA_tanh(x: f64) -> f64;
    fn cmA_exp(x: f64) -> f64;
    fn cmA_log(x: f64) -> f64;
    fn cmA_pow(x: f64, y: f64) -> f64;
    fn cmB_sin(x: f64) -> f64;
    fn cmB_cos(x: f64) -> f64;
    fn cmB_tan(x: f64) -> f64;
    fn cmB_asin(x: f64) -> f64;
    fn cmB_acos(x: f64) -> f64;
    fn cmB_atan(x: f64) -> f64;
    fn cmB_atan2(y: f64, x: f64) -> f64;
    fn cmB_sinh(x: f64) -> f64;
    fn cmB_cosh(x: f64) -> f64;
    fn cmB_tanh(x: f64) -> f64;
    fn cmB_exp(x: f64) -> f64;
    fn cmB_log(x: f64) -> f64;
    fn cmB_pow(x: f64, y: f64) -> f64;
    fn cmC_sin(x: f64) -> f64;
    fn cmC_cos(x: f64) -> f64;
    fn cmC_tan(x: f64) -> f64;
    fn cmC_asin(x: f64) -> f64;
    fn cmC_acos(x: f64) -> f64;
    fn cmC_atan(x: f64) -> f64;
    fn cmC_atan2(x: f64, y: f64) -> f64;
    fn cmC_sinh(x: f64) -> f64;
    fn cmC_cosh(x: f64) -> f64;
    fn cmC_tanh(x: f64) -> f64;
    fn cmC_exp(x: f64) -> f64;
    fn cmC_log(x: f64) -> f64;
    fn cmC_pow(x: f64, y: f64) -> f64;
}

const CANDS: &[&str] = &["glibc", "pxfm", "libm", "coremath", "coremath_fma", "coremath_soft"];
const FUNCS: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "exp", "log",
    "pow",
];

#[inline(never)]
fn eval(c: usize, f: &str) -> fn(f64, f64) -> f64 {
    macro_rules! u {
        ($e:expr) => {{ |x: f64, _y: f64| -> f64 { $e(x) } }};
    }
    macro_rules! b {
        ($e:expr) => {{ |x: f64, y: f64| -> f64 { $e(x, y) } }};
    }
    match (CANDS[c], f) {
        ("glibc", "sin") => u!(f64::sin),
        ("glibc", "cos") => u!(f64::cos),
        ("glibc", "tan") => u!(f64::tan),
        ("glibc", "asin") => u!(f64::asin),
        ("glibc", "acos") => u!(f64::acos),
        ("glibc", "atan") => u!(f64::atan),
        ("glibc", "atan2") => b!(f64::atan2),
        ("glibc", "sinh") => u!(f64::sinh),
        ("glibc", "cosh") => u!(f64::cosh),
        ("glibc", "tanh") => u!(f64::tanh),
        ("glibc", "exp") => u!(f64::exp),
        ("glibc", "log") => u!(f64::ln),
        ("glibc", "pow") => b!(f64::powf),
        ("pxfm", "sin") => u!(pxfm::f_sin),
        ("pxfm", "cos") => u!(pxfm::f_cos),
        ("pxfm", "tan") => u!(pxfm::f_tan),
        ("pxfm", "asin") => u!(pxfm::f_asin),
        ("pxfm", "acos") => u!(pxfm::f_acos),
        ("pxfm", "atan") => u!(pxfm::f_atan),
        ("pxfm", "atan2") => b!(pxfm::f_atan2),
        ("pxfm", "sinh") => u!(pxfm::f_sinh),
        ("pxfm", "cosh") => u!(pxfm::f_cosh),
        ("pxfm", "tanh") => u!(pxfm::f_tanh),
        ("pxfm", "exp") => u!(pxfm::f_exp),
        ("pxfm", "log") => u!(pxfm::f_log),
        ("pxfm", "pow") => b!(pxfm::f_pow),
        ("libm", "sin") => u!(libm::sin),
        ("libm", "cos") => u!(libm::cos),
        ("libm", "tan") => u!(libm::tan),
        ("libm", "asin") => u!(libm::asin),
        ("libm", "acos") => u!(libm::acos),
        ("libm", "atan") => u!(libm::atan),
        ("libm", "atan2") => b!(libm::atan2),
        ("libm", "sinh") => u!(libm::sinh),
        ("libm", "cosh") => u!(libm::cosh),
        ("libm", "tanh") => u!(libm::tanh),
        ("libm", "exp") => u!(libm::exp),
        ("libm", "log") => u!(libm::log),
        ("libm", "pow") => b!(libm::pow),
        ("coremath", "sin") => u!(|x| unsafe { cmA_sin(x) }),
        ("coremath", "cos") => u!(|x| unsafe { cmA_cos(x) }),
        ("coremath", "tan") => u!(|x| unsafe { cmA_tan(x) }),
        ("coremath", "asin") => u!(|x| unsafe { cmA_asin(x) }),
        ("coremath", "acos") => u!(|x| unsafe { cmA_acos(x) }),
        ("coremath", "atan") => u!(|x| unsafe { cmA_atan(x) }),
        ("coremath", "atan2") => b!(|x, y| unsafe { cmA_atan2(x, y) }),
        ("coremath", "sinh") => u!(|x| unsafe { cmA_sinh(x) }),
        ("coremath", "cosh") => u!(|x| unsafe { cmA_cosh(x) }),
        ("coremath", "tanh") => u!(|x| unsafe { cmA_tanh(x) }),
        ("coremath", "exp") => u!(|x| unsafe { cmA_exp(x) }),
        ("coremath", "log") => u!(|x| unsafe { cmA_log(x) }),
        ("coremath", "pow") => b!(|x, y| unsafe { cmA_pow(x, y) }),
        ("coremath_fma", "sin") => u!(|x| unsafe { cmB_sin(x) }),
        ("coremath_fma", "cos") => u!(|x| unsafe { cmB_cos(x) }),
        ("coremath_fma", "tan") => u!(|x| unsafe { cmB_tan(x) }),
        ("coremath_fma", "asin") => u!(|x| unsafe { cmB_asin(x) }),
        ("coremath_fma", "acos") => u!(|x| unsafe { cmB_acos(x) }),
        ("coremath_fma", "atan") => u!(|x| unsafe { cmB_atan(x) }),
        ("coremath_fma", "atan2") => b!(|x, y| unsafe { cmB_atan2(x, y) }),
        ("coremath_fma", "sinh") => u!(|x| unsafe { cmB_sinh(x) }),
        ("coremath_fma", "cosh") => u!(|x| unsafe { cmB_cosh(x) }),
        ("coremath_fma", "tanh") => u!(|x| unsafe { cmB_tanh(x) }),
        ("coremath_fma", "exp") => u!(|x| unsafe { cmB_exp(x) }),
        ("coremath_fma", "log") => u!(|x| unsafe { cmB_log(x) }),
        ("coremath_fma", "pow") => b!(|x, y| unsafe { cmB_pow(x, y) }),
        ("coremath_soft", "sin") => u!(|x| unsafe { cmC_sin(x) }),
        ("coremath_soft", "cos") => u!(|x| unsafe { cmC_cos(x) }),
        ("coremath_soft", "tan") => u!(|x| unsafe { cmC_tan(x) }),
        ("coremath_soft", "asin") => u!(|x| unsafe { cmC_asin(x) }),
        ("coremath_soft", "acos") => u!(|x| unsafe { cmC_acos(x) }),
        ("coremath_soft", "atan") => u!(|x| unsafe { cmC_atan(x) }),
        ("coremath_soft", "atan2") => b!(|x, y| unsafe { cmC_atan2(x, y) }),
        ("coremath_soft", "sinh") => u!(|x| unsafe { cmC_sinh(x) }),
        ("coremath_soft", "cosh") => u!(|x| unsafe { cmC_cosh(x) }),
        ("coremath_soft", "tanh") => u!(|x| unsafe { cmC_tanh(x) }),
        ("coremath_soft", "exp") => u!(|x| unsafe { cmC_exp(x) }),
        ("coremath_soft", "log") => u!(|x| unsafe { cmC_log(x) }),
        ("coremath_soft", "pow") => b!(|x, y| unsafe { cmC_pow(x, y) }),
        _ => unreachable!(),
    }
}

fn read_u64s(path: &str) -> Vec<u64> {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    b.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect()
}

fn same(a: f64, r: f64) -> bool {
    if r.is_nan() { a.is_nan() } else { a.to_bits() == r.to_bits() }
}

fn check(dir: &str, only: &[String]) {
    for f in FUNCS {
        if !only.is_empty() && !only.iter().any(|o| o == f) {
            continue;
        }
        let path = format!("{dir}/{f}.bin");
        if !std::path::Path::new(&path).exists() {
            continue;
        }
        let v = read_u64s(&path);
        let n = v.len() / 3;
        let mut line = format!("{f:6} {n:8}");
        for c in 0..CANDS.len() {
            let g = eval(c, f);
            let mut out = std::io::BufWriter::new(
                std::fs::File::create(format!("{dir}/{f}.{}.mis", CANDS[c])).unwrap(),
            );
            let mut bad = 0usize;
            for k in 0..n {
                let (x, y, r) = (
                    f64::from_bits(v[3 * k]),
                    f64::from_bits(v[3 * k + 1]),
                    f64::from_bits(v[3 * k + 2]),
                );
                let a = g(x, y);
                if !same(a, r) {
                    bad += 1;
                    out.write_all(&v[3 * k].to_le_bytes()).unwrap();
                    out.write_all(&v[3 * k + 1].to_le_bytes()).unwrap();
                    out.write_all(&a.to_bits().to_le_bytes()).unwrap();
                }
            }
            line += &format!("  {}={bad}", CANDS[c]);
        }
        println!("{line}");
    }
}

/// xorshift
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unif(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * ((self.next() >> 11) as f64 / (1u64 << 53) as f64)
    }
}

fn inputs(f: &str, n: usize, rng: &mut Rng) -> Vec<(f64, f64)> {
    (0..n)
        .map(|_| match f {
            "sin" | "cos" | "tan" => (rng.unif(-10.0, 10.0), 0.0),
            "asin" | "acos" => (rng.unif(-1.0, 1.0), 0.0),
            "atan" => (rng.unif(-10.0, 10.0), 0.0),
            "atan2" => (rng.unif(-10.0, 10.0), rng.unif(-10.0, 10.0)),
            "sinh" | "cosh" | "tanh" => (rng.unif(-5.0, 5.0), 0.0),
            "exp" => (rng.unif(-20.0, 20.0), 0.0),
            "log" => (10f64.powf(rng.unif(-3.0, 4.0)), 0.0),
            "pow" => (rng.unif(0.1, 10.0), rng.unif(-3.0, 3.0)),
            _ => unreachable!(),
        })
        .collect()
}

#[inline(never)]
fn run(g: fn(f64, f64) -> f64, xs: &[(f64, f64)]) -> f64 {
    let mut s = 0.0;
    for &(x, y) in xs {
        s += g(black_box(x), black_box(y));
    }
    s
}

fn bench(only: &[String]) {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let n = 1 << 16;
    print!("{:6}", "ns");
    for c in CANDS {
        print!(" {c:>13}");
    }
    println!();
    for f in FUNCS {
        if !only.is_empty() && !only.iter().any(|o| o == f) {
            continue;
        }
        let xs = inputs(f, n, &mut rng);
        print!("{f:6}");
        for c in 0..CANDS.len() {
            let g = eval(c, f);
            black_box(run(g, &xs));
            let mut best = f64::INFINITY;
            for _ in 0..15 {
                let t = Instant::now();
                black_box(run(g, &xs));
                best = best.min(t.elapsed().as_secs_f64() / n as f64 * 1e9);
            }
            print!(" {best:13.2}");
        }
        println!();
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(|s| s.as_str()) {
        Some("check") => check(&a[1], &a[2..]),
        Some("bench") => bench(&a[1..]),
        _ => eprintln!("usage: libm-eval check DIR [f..] | bench [f..]"),
    }
}
