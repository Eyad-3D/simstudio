//! Units of measure.
//!
//! Every quantity is held in SI inside the engine. A [`Unit`] is a dimension
//! (exponents of the seven SI base units) with a scale and, for
//! temperatures only, an offset: `value_si = value * scale + offset`.
//!
//! Unit text follows Modelica's unit syntax (`N.m`, `kg.m2`, `m/s2`, `s-1`),
//! which Base Modelica keeps, and also accepts the forms people type
//! (`N·m`, `kg·m²`, `m/s^2`, `km/h`, `°C`). Angles are dimensionless
//! (`rad` = 1), as in SI, so `rad/s` and `1/s` have the same dimension;
//! revolutions are explicit (`rev/min`, `rpm` = 2π/60 rad/s). Equations
//! are checked for dimensional consistency when a model is prepared
//! ([`Dim`] arithmetic), never by scale.
//!
//! Two of today's display units are ambiguous and the project importer
//! maps them before parsing: the app's rotational speed `1/min` means
//! revolutions per minute (`rev/min`), and its acceleration `g` means
//! standard gravity (`gn` here, since `g` is the gram).

use serde::{Deserialize, Serialize};
use std::fmt;

/// The SI base units, in the order of [`Dim`]'s exponents.
pub const BASE_SYMBOLS: [&str; 7] = ["m", "kg", "s", "A", "K", "mol", "cd"];

/// A physical dimension: the exponents of m, kg, s, A, K, mol and cd.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Dim(pub [i8; 7]);

impl Dim {
    /// Dimensionless.
    pub const NONE: Dim = Dim([0; 7]);
    /// Length, m.
    pub const LENGTH: Dim = Dim([1, 0, 0, 0, 0, 0, 0]);
    /// Mass, kg.
    pub const MASS: Dim = Dim([0, 1, 0, 0, 0, 0, 0]);
    /// Time, s.
    pub const TIME: Dim = Dim([0, 0, 1, 0, 0, 0, 0]);
    /// Electric current, A.
    pub const CURRENT: Dim = Dim([0, 0, 0, 1, 0, 0, 0]);
    /// Temperature, K.
    pub const TEMPERATURE: Dim = Dim([0, 0, 0, 0, 1, 0, 0]);

    /// This dimension to a whole power.
    pub fn powi(self, n: i8) -> Dim {
        Dim(self.0.map(|a| a * n))
    }

    /// The n-th root, when every exponent divides by n.
    pub fn root(self, n: i8) -> Option<Dim> {
        if n == 0 || self.0.iter().any(|a| a % n != 0) {
            return None;
        }
        Some(Dim(self.0.map(|a| a / n)))
    }

    /// Whether this is dimensionless.
    pub fn is_none(self) -> bool {
        self == Dim::NONE
    }
}

// exponents add when quantities multiply
#[allow(clippy::suspicious_arithmetic_impl)]
impl std::ops::Mul for Dim {
    type Output = Dim;
    /// The product of two dimensions.
    fn mul(self, o: Dim) -> Dim {
        let mut d = self.0;
        for (a, b) in d.iter_mut().zip(o.0) {
            *a += b;
        }
        Dim(d)
    }
}

// exponents subtract when quantities divide
#[allow(clippy::suspicious_arithmetic_impl)]
impl std::ops::Div for Dim {
    type Output = Dim;
    /// The quotient of two dimensions.
    fn div(self, o: Dim) -> Dim {
        let mut d = self.0;
        for (a, b) in d.iter_mut().zip(o.0) {
            *a -= b;
        }
        Dim(d)
    }
}

impl fmt::Display for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return write!(f, "1");
        }
        let mut first = true;
        for (sym, e) in BASE_SYMBOLS.iter().zip(self.0) {
            if e == 0 {
                continue;
            }
            if !first {
                write!(f, ".")?;
            }
            first = false;
            if e == 1 {
                write!(f, "{sym}")?;
            } else {
                write!(f, "{sym}{e}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Dim({self})")
    }
}

/// The dimension in words people read: a named SI unit when there is one
/// (`V`, `N.m`, `W`), else its base units (`m2.kg.s-3.A-1`).
pub fn describe(dim: Dim) -> String {
    const NAMED: [&str; 18] = [
        "1", "m", "kg", "s", "A", "K", "N", "N.m", "W", "V", "Ohm", "F", "H", "Pa", "m/s", "rad/s",
        "m/s2", "kg.m2",
    ];
    for n in NAMED {
        if let Ok(u) = parse_unit(n)
            && u.dim == dim
        {
            return if n == "rad/s" { "1/s (rad/s)".into() } else { n.into() };
        }
    }
    dim.to_string()
}

/// A unit: dimension, scale to SI and (temperatures only) offset.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Unit {
    /// Its dimension.
    pub dim: Dim,
    /// value_si = value * scale + offset.
    pub scale: f64,
    /// Non-zero only for °C-like units.
    pub offset: f64,
}

/// The dimensionless unit 1.
impl Default for Unit {
    fn default() -> Self {
        Unit::ONE
    }
}

impl Unit {
    /// The dimensionless unit 1.
    pub const ONE: Unit = Unit { dim: Dim::NONE, scale: 1.0, offset: 0.0 };

    /// A unit with no offset.
    pub const fn new(dim: Dim, scale: f64) -> Unit {
        Unit { dim, scale, offset: 0.0 }
    }

    /// Converts a value in this unit to SI.
    pub fn to_si(&self, v: f64) -> f64 {
        v * self.scale + self.offset
    }

    /// Converts an SI value to this unit.
    pub fn from_si(&self, v: f64) -> f64 {
        (v - self.offset) / self.scale
    }

    fn mul(self, o: Unit) -> Unit {
        Unit::new(self.dim * o.dim, self.scale * o.scale)
    }

    fn div(self, o: Unit) -> Unit {
        Unit::new(self.dim / o.dim, self.scale / o.scale)
    }

    fn powi(self, n: i8) -> Unit {
        Unit::new(self.dim.powi(n), self.scale.powi(n as i32))
    }
}

/// Why a unit text was not understood.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum UnitError {
    /// A symbol that is not a known unit.
    #[error("'{symbol}' in the unit '{text}' is not a unit LightSim knows")]
    UnknownSymbol {
        /// the whole unit text
        text: String,
        /// the symbol not understood
        symbol: String,
    },
    /// Text that does not follow the unit syntax.
    #[error("the unit '{text}' is not written as units are ({why})")]
    Syntax {
        /// the whole unit text
        text: String,
        /// what is wrong
        why: String,
    },
    /// °C (or another offset unit) inside a product or quotient.
    #[error(
        "'{text}': a temperature with an offset (°C) cannot be multiplied or divided; use K for temperature differences"
    )]
    OffsetInCompound {
        /// the whole unit text
        text: String,
    },
}

const PI: f64 = std::f64::consts::PI;

fn d(m: i8, kg: i8, s: i8, a: i8, k: i8) -> Dim {
    Dim([m, kg, s, a, k, 0, 0])
}

/// Symbols that are units on their own (not prefixed).
fn plain_symbol(s: &str) -> Option<Unit> {
    let u = |dim, scale| Some(Unit::new(dim, scale));
    match s {
        "1" | "-" => Some(Unit::ONE),
        "%" => u(Dim::NONE, 0.01),
        "rad" => Some(Unit::ONE),
        "deg" | "°" => u(Dim::NONE, PI / 180.0),
        "rev" => u(Dim::NONE, 2.0 * PI),
        "rpm" => u(Dim::TIME.powi(-1), 2.0 * PI / 60.0),
        "m" => u(Dim::LENGTH, 1.0),
        "kg" => u(Dim::MASS, 1.0),
        "g" => u(Dim::MASS, 1e-3),
        "t" => u(Dim::MASS, 1e3),
        "s" => u(Dim::TIME, 1.0),
        "min" => u(Dim::TIME, 60.0),
        "h" => u(Dim::TIME, 3600.0),
        "d" => u(Dim::TIME, 86400.0),
        "A" => u(Dim::CURRENT, 1.0),
        "K" => u(Dim::TEMPERATURE, 1.0),
        "degC" | "°C" => Some(Unit { dim: Dim::TEMPERATURE, scale: 1.0, offset: 273.15 }),
        "mol" => u(Dim([0, 0, 0, 0, 0, 1, 0]), 1.0),
        "cd" => u(Dim([0, 0, 0, 0, 0, 0, 1]), 1.0),
        "N" => u(d(1, 1, -2, 0, 0), 1.0),
        "Nm" => u(d(2, 1, -2, 0, 0), 1.0),
        "J" => u(d(2, 1, -2, 0, 0), 1.0),
        "W" => u(d(2, 1, -3, 0, 0), 1.0),
        "Wh" => u(d(2, 1, -2, 0, 0), 3600.0),
        "V" => u(d(2, 1, -3, -1, 0), 1.0),
        "Ohm" | "Ω" | "ohm" => u(d(2, 1, -3, -2, 0), 1.0),
        "S" => u(d(-2, -1, 3, 2, 0), 1.0),
        "F" => u(d(-2, -1, 4, 2, 0), 1.0),
        "H" => u(d(2, 1, -2, -2, 0), 1.0),
        "C" => u(d(0, 0, 1, 1, 0), 1.0),
        "Ah" => u(d(0, 0, 1, 1, 0), 3600.0),
        "Wb" => u(d(2, 1, -2, -1, 0), 1.0),
        "T" => u(d(0, 1, -2, -1, 0), 1.0),
        "Pa" => u(d(-1, 1, -2, 0, 0), 1.0),
        "bar" => u(d(-1, 1, -2, 0, 0), 1e5),
        "Hz" => u(Dim::TIME.powi(-1), 1.0),
        "L" | "l" => u(Dim::LENGTH.powi(3), 1e-3),
        "gn" => u(d(1, 0, -2, 0, 0), 9.80665),
        _ => None,
    }
}

/// Symbols that take an SI prefix.
fn prefixable(s: &str) -> bool {
    matches!(
        s,
        "m" | "g"
            | "s"
            | "A"
            | "K"
            | "mol"
            | "N"
            | "J"
            | "W"
            | "Wh"
            | "V"
            | "Ohm"
            | "Ω"
            | "S"
            | "F"
            | "H"
            | "C"
            | "Ah"
            | "Pa"
            | "Hz"
            | "L"
            | "l"
            | "bar"
            | "T"
            | "Wb"
    )
}

fn prefix(p: &str) -> Option<f64> {
    Some(match p {
        "p" => 1e-12,
        "n" => 1e-9,
        "u" | "µ" | "μ" => 1e-6,
        "m" => 1e-3,
        "c" => 1e-2,
        "d" => 1e-1,
        "da" => 1e1,
        "h" => 1e2,
        "k" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        "T" => 1e12,
        _ => return None,
    })
}

fn symbol(text: &str, s: &str) -> Result<Unit, UnitError> {
    if let Some(u) = plain_symbol(s) {
        return Ok(u);
    }
    for split in [2, 1] {
        if s.chars().count() > split {
            let cut = s.char_indices().nth(split).map(|(i, _)| i).unwrap_or(s.len());
            let (p, rest) = s.split_at(cut);
            if let (Some(f), true) = (prefix(p), prefixable(rest)) {
                let base = plain_symbol(rest).expect("prefixable symbols are plain");
                return Ok(Unit::new(base.dim, base.scale * f));
            }
        }
    }
    Err(UnitError::UnknownSymbol { text: text.to_string(), symbol: s.to_string() })
}

struct Parser<'a> {
    text: &'a str,
    chars: Vec<char>,
    pos: usize,
    factors: usize,
    offset_seen: bool,
}

impl Parser<'_> {
    fn err(&self, why: &str) -> UnitError {
        UnitError::Syntax { text: self.text.to_string(), why: why.to_string() }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while self.peek() == Some(' ') {
            self.pos += 1;
        }
    }

    /// expr := term ('/' (factor | '(' expr ')'))*
    fn expr(&mut self) -> Result<Unit, UnitError> {
        let mut u = self.term()?;
        loop {
            self.skip_space();
            if self.peek() == Some('/') {
                self.pos += 1;
                self.skip_space();
                let den = if self.peek() == Some('(') {
                    self.pos += 1;
                    let inner = self.expr()?;
                    if self.peek() != Some(')') {
                        return Err(self.err("a '(' is not closed"));
                    }
                    self.pos += 1;
                    inner
                } else {
                    self.factor()?
                };
                u = u.div(den);
            } else {
                return Ok(u);
            }
        }
    }

    /// term := factor (('.' | '·' | '*' | ' ') factor)*
    fn term(&mut self) -> Result<Unit, UnitError> {
        let mut u = self.factor()?;
        loop {
            let save = self.pos;
            self.skip_space();
            match self.peek() {
                Some('.' | '·' | '*' | '⋅') => {
                    self.pos += 1;
                    self.skip_space();
                    u = u.mul(self.factor()?);
                }
                Some(c) if self.pos > save && (c.is_alphabetic() || c == '°') => {
                    u = u.mul(self.factor()?);
                }
                _ => {
                    self.pos = save;
                    return Ok(u);
                }
            }
        }
    }

    /// factor := '(' expr ')' | symbol [exponent]
    fn factor(&mut self) -> Result<Unit, UnitError> {
        self.factors += 1;
        if self.peek() == Some('(') {
            self.pos += 1;
            let u = self.expr()?;
            if self.peek() != Some(')') {
                return Err(self.err("a '(' is not closed"));
            }
            self.pos += 1;
            return Ok(u);
        }
        let start = self.pos;
        while let Some(c) = self.peek() {
            let first = self.pos == start;
            if c.is_alphabetic()
                || c == '°'
                || c == '%'
                || c == 'Ω'
                || (first && (c == '1' || c == '-'))
            {
                self.pos += 1;
                if first && (c == '1' || c == '-' || c == '%') {
                    break;
                }
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(self.err("a unit symbol is missing"));
        }
        let sym: String = self.chars[start..self.pos].iter().collect();
        let u = symbol(self.text, &sym)?;
        self.offset_seen |= u.offset != 0.0;
        let e = self.exponent()?;
        if e == 1 {
            return Ok(u);
        }
        if u.offset != 0.0 {
            return Err(UnitError::OffsetInCompound { text: self.text.to_string() });
        }
        Ok(u.powi(e))
    }

    /// exponent := ['^'] ['-'] digits | superscripts
    fn exponent(&mut self) -> Result<i8, UnitError> {
        let sup = |c: char| match c {
            '⁰' => Some(0),
            '¹' => Some(1),
            '²' => Some(2),
            '³' => Some(3),
            '⁴' => Some(4),
            '⁵' => Some(5),
            '⁶' => Some(6),
            '⁷' => Some(7),
            '⁸' => Some(8),
            '⁹' => Some(9),
            _ => None,
        };
        if self.peek() == Some('^') {
            self.pos += 1;
        }
        let mut neg = false;
        if matches!(self.peek(), Some('-' | '⁻')) {
            neg = true;
            self.pos += 1;
        }
        let mut val: i32 = 0;
        let mut any = false;
        while let Some(c) = self.peek() {
            let digit = c.to_digit(10).map(|v| v as i32).or(sup(c));
            match digit {
                Some(v) => {
                    val = val * 10 + v;
                    any = true;
                    self.pos += 1;
                }
                None => break,
            }
        }
        if !any {
            if neg {
                return Err(self.err("a '-' is not followed by an exponent"));
            }
            return Ok(1);
        }
        let v = if neg { -val } else { val };
        i8::try_from(v).map_err(|_| self.err("an exponent is too large"))
    }
}

/// Parses unit text, such as `N.m`, `kg·m²`, `km/h` or `°C`.
pub fn parse_unit(text: &str) -> Result<Unit, UnitError> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(Unit::ONE);
    }
    let mut p = Parser { text, chars: t.chars().collect(), pos: 0, factors: 0, offset_seen: false };
    let u = p.expr()?;
    p.skip_space();
    if p.pos != p.chars.len() {
        return Err(p.err("there is text after the unit"));
    }
    if p.factors > 1 && p.offset_seen {
        return Err(UnitError::OffsetInCompound { text: text.to_string() });
    }
    Ok(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12 * b.abs().max(1.0)
    }

    #[test]
    fn parses_common_units() {
        let nm = parse_unit("N.m").unwrap();
        assert_eq!(nm.dim, d(2, 1, -2, 0, 0));
        assert_eq!(parse_unit("N·m").unwrap(), nm);
        assert_eq!(parse_unit("J").unwrap().dim, nm.dim);
        let kgm2 = parse_unit("kg.m2").unwrap();
        assert_eq!(kgm2, parse_unit("kg·m²").unwrap());
        assert_eq!(kgm2.dim, d(2, 1, 0, 0, 0));
        let kmh = parse_unit("km/h").unwrap();
        assert!(close(kmh.to_si(36.0), 10.0));
        assert_eq!(parse_unit("m/s2").unwrap().dim, d(1, 0, -2, 0, 0));
        assert_eq!(parse_unit("m/s^2").unwrap().dim, d(1, 0, -2, 0, 0));
        assert_eq!(parse_unit("s-1").unwrap().dim, Dim::TIME.powi(-1));
        assert!(close(parse_unit("kW").unwrap().to_si(1.5), 1500.0));
        assert!(close(parse_unit("kWh").unwrap().to_si(1.0), 3.6e6));
        assert!(close(parse_unit("rev/min").unwrap().to_si(60.0), 2.0 * PI));
        assert!(close(parse_unit("rpm").unwrap().to_si(60.0), 2.0 * PI));
        assert!(close(parse_unit("%").unwrap().to_si(50.0), 0.5));
        assert!(close(parse_unit("°C").unwrap().to_si(20.0), 293.15));
        assert!(close(parse_unit("degC").unwrap().from_si(273.15), 0.0));
        assert_eq!(parse_unit("W/(m.K)").unwrap().dim, d(1, 1, -3, 0, -1));
        assert_eq!(parse_unit("Ohm").unwrap(), parse_unit("V/A").unwrap());
        assert!(close(parse_unit("mAh").unwrap().to_si(1.0), 3.6));
        assert!(close(parse_unit("gn").unwrap().to_si(1.0), 9.80665));
        assert_eq!(parse_unit("-").unwrap(), Unit::ONE);
        assert_eq!(parse_unit("").unwrap(), Unit::ONE);
        assert_eq!(parse_unit("rad/s").unwrap().dim, Dim::TIME.powi(-1));
        assert!(close(parse_unit("1/min").unwrap().to_si(60.0), 1.0));
    }

    #[test]
    fn rejects_bad_units() {
        assert!(matches!(parse_unit("furlong"), Err(UnitError::UnknownSymbol { .. })));
        assert!(matches!(parse_unit("W/(m.K"), Err(UnitError::Syntax { .. })));
        assert!(matches!(parse_unit("°C/s"), Err(UnitError::OffsetInCompound { .. })));
    }

    #[test]
    fn dimension_arithmetic() {
        let v = parse_unit("V").unwrap().dim;
        let a = parse_unit("A").unwrap().dim;
        assert_eq!(v * a, parse_unit("W").unwrap().dim);
        assert_eq!(parse_unit("m2").unwrap().dim.root(2), Some(Dim::LENGTH));
        assert_eq!(Dim::LENGTH.root(2), None);
        assert_eq!(format!("{}", parse_unit("N.m").unwrap().dim), "m2.kg.s-2");
        assert_eq!(describe(parse_unit("V/A").unwrap().dim), "Ohm");
        assert_eq!(describe(parse_unit("J").unwrap().dim), "N.m");
    }
}
