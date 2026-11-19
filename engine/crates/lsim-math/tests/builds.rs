//! The FMA build and the software-fma build give the same bits on many
//! random inputs over every magnitude, not only on the hard cases of
//! `references.rs`: sign, exponent and mantissa drawn independently, plus
//! inputs in the ranges where each function does its work. (From the
//! review of 627dd01..80c8ab0, which ran it first.)
//!
//! The default run checks 6.4 million inputs (about 1.5 s); `cargo test -p
//! lsim-math --test builds -- --ignored` checks ten times as many.

use lsim_math as m;

/// xorshift64
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// uniform in [0, 1)
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// any double whose biased exponent is in [lo, hi], with a random sign
    /// and mantissa
    fn wide(&mut self, lo: u64, hi: u64) -> f64 {
        let e = lo + self.next() % (hi - lo + 1);
        let s = self.next() & (1 << 63);
        f64::from_bits(s | (e << 52) | (self.next() >> 12))
    }
}

/// A function of one argument of a build, and of two.
type F1 = fn(&m::Build, f64) -> f64;
type F2 = fn(&m::Build, f64, f64) -> f64;

/// Checks `n` inputs per function of one argument and `2 n` per function
/// of two; returns how many.
fn the_builds_agree(n: usize, seed: u64) -> u64 {
    let builds = m::builds();
    if builds.len() < 2 {
        println!("this CPU runs one build only: nothing to compare");
        return 0;
    }
    let (fma, soft) = (builds[0], builds[1]);
    assert_eq!((fma.name, soft.name), ("fma", "soft"));
    let same = |a: f64, b: f64| a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan());
    let one: [(&str, F1); 11] = [
        ("sin", |b, x| b.sin(x)),
        ("cos", |b, x| b.cos(x)),
        ("tan", |b, x| b.tan(x)),
        ("asin", |b, x| b.asin(x)),
        ("acos", |b, x| b.acos(x)),
        ("atan", |b, x| b.atan(x)),
        ("sinh", |b, x| b.sinh(x)),
        ("cosh", |b, x| b.cosh(x)),
        ("tanh", |b, x| b.tanh(x)),
        ("exp", |b, x| b.exp(x)),
        ("log", |b, x| b.ln(x)),
    ];
    let mut r = Rng(seed);
    let mut checked = 0u64;
    for (name, f) in one {
        // (exp twice as often: it has the widest range of results)
        let count = if name == "exp" { 2 * n } else { n };
        for k in 0..count {
            let x = match k % 4 {
                0 => r.wide(0, 2046),                // anything finite
                1 => r.wide(1013, 1033),             // |x| in [2^-10, 2^11)
                2 => (r.unit() * 2.0 - 1.0) * 1.5,   // the unit range
                _ => (r.unit() * 2.0 - 1.0) * 745.0, // exp's, sinh's, cosh's range
            };
            let (a, b) = (f(fma, x), f(soft, x));
            assert!(same(a, b), "{name}({x:e} = {:#x}): fma {a:e}, soft {b:e}", x.to_bits());
            checked += 1;
        }
    }
    let two: [(&str, F2); 2] = [("atan2", |b, y, x| b.atan2(y, x)), ("pow", |b, x, y| b.pow(x, y))];
    for (name, f) in two {
        for k in 0..2 * n {
            let (x, y) = match k % 4 {
                0 => (r.wide(0, 2046), r.wide(0, 2046)),
                1 => (r.wide(1013, 1033), r.wide(1013, 1033)),
                2 => (r.unit() * 4.0, (r.unit() * 2.0 - 1.0) * 30.0),
                _ => (r.unit() * 1e3, (r.next() % 33) as f64 - 16.0),
            };
            let (a, b) = (f(fma, x, y), f(soft, x, y));
            assert!(
                same(a, b),
                "{name}({x:e} = {:#x}, {y:e} = {:#x}): fma {a:e}, soft {b:e}",
                x.to_bits(),
                y.to_bits()
            );
            checked += 1;
        }
    }
    checked
}

#[test]
fn the_two_builds_agree_on_random_inputs() {
    let checked = the_builds_agree(400_000, 0x9e37_79b9_7f4a_7c15);
    println!("{checked} inputs, the two builds bit for bit the same");
}

#[test]
#[ignore = "64 million inputs, about 15 s: run with --ignored"]
fn the_two_builds_agree_on_many_more_random_inputs() {
    let checked = the_builds_agree(4_000_000, 0x2545_f491_4f6c_dd1d);
    println!("{checked} inputs, the two builds bit for bit the same");
}
