//! Today's component library (`backend/app/library/components.json`),
//! embedded so that every block's defaults are exactly today's, and the
//! app's display units with their SI conversions.

use serde_json::{Map, Value};
use std::sync::OnceLock;

static JSON: &str = include_str!("../../../../../backend/app/library/components.json");

/// The parsed library.
pub fn catalog() -> &'static Value {
    static CAT: OnceLock<Value> = OnceLock::new();
    CAT.get_or_init(|| serde_json::from_str(JSON).expect("components.json parses"))
}

/// A block's entry by its id (`battery.generic`).
pub fn block(id: &str) -> Option<&'static Value> {
    catalog()["components"].as_array()?.iter().find(|c| c["id"] == id)
}

/// A block's parameter entry.
pub fn param(id: &str, key: &str) -> Option<&'static Value> {
    block(id)?["parameters"].as_array()?.iter().find(|p| p["key"] == key)
}

/// A block's parameters with their defaults, overridden by `overrides`.
pub fn merged(id: &str, overrides: &Map<String, Value>) -> Map<String, Value> {
    let mut m = Map::new();
    if let Some(ps) = block(id).and_then(|b| b["parameters"].as_array()) {
        for p in ps {
            if let Some(k) = p["key"].as_str() {
                m.insert(k.to_string(), p["default"].clone());
            }
        }
    }
    for (k, v) in overrides {
        m.insert(k.clone(), v.clone());
    }
    m
}

/// A number from a parameter value (numbers, numeric text, booleans).
pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(x) => x.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// A parameter as a number, `default` when missing or not a number (as
/// today's `float(p.get(key, default))`, with `or 0` for falsy values).
pub fn get(p: &Map<String, Value>, key: &str, default: f64) -> f64 {
    match p.get(key) {
        None => default,
        Some(Value::Null) => 0.0,
        Some(v) => num(v).unwrap_or(default),
    }
}

/// A parameter as text.
pub fn text<'a>(p: &'a Map<String, Value>, key: &str, default: &'a str) -> &'a str {
    p.get(key).and_then(Value::as_str).unwrap_or(default)
}

/// A parameter as a truth value (today's `bool(p.get(key, default))`).
pub fn flag(p: &Map<String, Value>, key: &str, default: bool) -> bool {
    match p.get(key) {
        None => default,
        Some(Value::Bool(b)) => *b,
        Some(Value::Null) => false,
        Some(Value::Number(x)) => x.as_f64().is_some_and(|v| v != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// An app display unit in SI: `si = value · scale + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AppUnit {
    /// the SI unit text
    pub si: &'static str,
    /// the scale to SI
    pub scale: f64,
    /// the offset to SI (°C only)
    pub offset: f64,
}

impl AppUnit {
    /// value in this unit → SI
    pub fn to_si(&self, v: f64) -> f64 {
        v * self.scale + self.offset
    }
    /// SI → this unit
    pub fn from_si(&self, v: f64) -> f64 {
        (v - self.offset) / self.scale
    }
}

/// An app unit (a parameter's `unit` or a unit group's display unit) in SI.
pub fn app_unit(text: &str) -> AppUnit {
    let u = |si: &'static str, scale: f64| AppUnit { si, scale, offset: 0.0 };
    match text {
        "" | "-" | "kg/kg" => u("1", 1.0),
        "%" => u("1", 0.01),
        "1/(km/h)" => u("s/m", 3.6),
        "1/(km/h·s)" => u("1/m", 3.6),
        "1/kN" => u("1/N", 1e-3),
        "1/m" => u("1/m", 1.0),
        "1/min" => u("rad/s", std::f64::consts::PI / 30.0),
        "1/s" => u("1/s", 1.0),
        "A" => u("A", 1.0),
        "Ah" => u("C", 3600.0),
        "MJ/kg" => u("J/kg", 1e6),
        "N" => u("N", 1.0),
        "N/(km/h)" => u("N.s/m", 3.6),
        "N/(km/h)²" => u("N.s2/m2", 12.96),
        "N·m" => u("N.m", 1.0),
        "V" => u("V", 1.0),
        "g/kWh" => u("kg/J", 1e-3 / 3.6e6),
        "kPa" => u("Pa", 1e3),
        "kW" => u("W", 1e3),
        "kWh" => u("J", 3.6e6),
        "kg" => u("kg", 1.0),
        "kg/h" => u("kg/s", 1.0 / 3600.0),
        "kg/l" => u("kg/m3", 1e3),
        "kg·m²" => u("kg.m2", 1.0),
        "km/h" => u("m/s", 1.0 / 3.6),
        "m" => u("m", 1.0),
        "m²" => u("m2", 1.0),
        "s" => u("s", 1.0),
        "°C" => AppUnit { si: "K", scale: 1.0, offset: 273.15 },
        "Ω" => u("Ohm", 1.0),
        "g" => u("m/s2", 9.80665),
        "N·m/A" => u("N.m/A", 1.0),
        _ => u("1", 1.0),
    }
}

/// A unit group's display unit (`Velocity` → `km/h`).
pub fn group_unit(group: &str) -> &'static str {
    catalog()["unitGroups"][group].as_str().unwrap_or("-")
}

/// A parameter's app unit.
pub fn param_unit(id: &str, key: &str) -> AppUnit {
    app_unit(param(id, key).and_then(|p| p["unit"].as_str()).unwrap_or(""))
}

/// A port's app unit (its unit group's display unit).
pub fn port_unit(id: &str, port: &str) -> AppUnit {
    let g = block(id)
        .and_then(|b| b["ports"].as_array())
        .and_then(|ps| ps.iter().find(|p| p["id"] == port))
        .and_then(|p| p["unitGroup"].as_str())
        .unwrap_or("No Unit");
    app_unit(group_unit(g))
}

/// A parameter's default as SI (0 when it has none).
pub fn default_si(id: &str, key: &str) -> f64 {
    let Some(p) = param(id, key) else { return 0.0 };
    num(&p["default"]).map(|v| param_unit(id, key).to_si(v)).unwrap_or(0.0)
}

/// A table axis' outside-the-data setting, from the library (the block's
/// own `tableOutside` override is applied by the importer).
pub fn axis_outside(id: &str, key: &str, axis: usize) -> crate::table::Outside {
    param(id, key)
        .and_then(|p| p["axes"].as_array())
        .and_then(|a| a.get(axis))
        .and_then(|a| a["outside"].as_str())
        .map(crate::table::outside)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_todays_library() {
        assert_eq!(catalog()["components"].as_array().unwrap().len(), 36);
        assert_eq!(default_si("vehicle.body", "mass_kg"), 1800.0);
        assert!((default_si("battery.generic", "capacity_kWh") - 60.0 * 3.6e6).abs() < 1e-6);
        assert!((default_si("electric.climate", "cabin_setpoint_C") - 294.15).abs() < 1e-12);
        assert_eq!(port_unit("vehicle.body", "sig_speed").scale, 1.0 / 3.6);
        assert_eq!(
            axis_outside("motor.emotor", "full_load_torque", 1),
            crate::table::Outside::Error
        );
    }
}
