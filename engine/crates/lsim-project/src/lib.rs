//! # lsim-project: today's project files in the new engine
//!
//! A project's diagram (`systems[].elements`, `connections`,
//! `dataBusConnections`, `docs/spec/project.md`) becomes one top-level
//! [`ComponentDef`]:
//!
//! * each part (`componentDefId`) becomes a sub-component through its
//!   [`BlockMapping`]: the library definition it is, how its port ids map to
//!   the definition's ports, how its `parameterOverrides` (in the app's
//!   display units) become SI modifiers, and how today's channel names
//!   (`el-battery:sig_soc`) map to flat variables (`el_battery.soc`);
//! * a wire becomes a `connect` of two physical ports, a signal link a
//!   `connect` of an output to an input;
//! * a case's `parameterOverrides` are applied as runtime parameter values
//!   (no rebuild), structural ones (`variability: fixed`) as modifiers.
//!
//! The [`ImportReport`]'s channel map lets the app, the Python package and
//! the golden comparisons read new results under today's names, so runs of
//! both engines line up channel by channel.
//!
//! Stage 1 provides the walk over the project, the identifier rules, the
//! unit mapping and the mapping registry; work package 5 writes the 36
//! block mappings (DESIGN.md, *Component library mapping*).

pub mod model;
pub mod reference;

use lsim_ir::Diagnostic;
use lsim_ir::component::{ComponentDef, Connect, Modifier, ParamValue, SubDecl};
use lsim_ir::expr::Expr;
use lsim_ir::units::parse_unit;
use serde_json::Value;
use std::collections::BTreeMap;

/// How one of today's block types maps onto the library.
pub trait BlockMapping: Send + Sync {
    /// The library definition the part becomes.
    fn def_name(&self) -> &str;
    /// The definition's port for one of the app's port ids; `None` when the
    /// app's port has no counterpart (it is then reported if wired).
    fn port(&self, app_port: &str) -> Option<String>;
    /// Modifiers from the part's `parameterOverrides` (app units).
    fn modifiers(
        &self,
        overrides: &serde_json::Map<String, Value>,
    ) -> Result<Vec<Modifier>, String>;
    /// The flat variable (relative to the part) recorded as one of today's
    /// channels (a port id such as `sig_soc`).
    fn channel(&self, app_port: &str) -> Option<String>;
}

/// The mappings by `componentDefId`.
#[derive(Default)]
pub struct Registry {
    map: BTreeMap<String, Box<dyn BlockMapping>>,
}

impl Registry {
    /// Adds a mapping.
    pub fn add(&mut self, component_def_id: &str, m: Box<dyn BlockMapping>) {
        self.map.insert(component_def_id.to_string(), m);
    }
}

/// What the import found besides the model.
#[derive(Debug, Default)]
pub struct ImportReport {
    /// today's channel name (`element:port`) → flat variable name
    pub channel_map: BTreeMap<String, String>,
    /// warnings (errors abort the import)
    pub warnings: Vec<Diagnostic>,
}

/// The app's display unit text as the engine's unit parser reads it. Two
/// of today's units differ in meaning from their SI reading: rotational
/// speed `1/min` is revolutions per minute, and acceleration `g` is
/// standard gravity.
pub fn app_unit(text: &str) -> String {
    match text {
        "1/min" => "rev/min".into(),
        "g" => "gn".into(),
        other => other.into(),
    }
}

/// A part's id as an identifier: letters, digits and `_` (the id itself is
/// kept as the instance's `ui_id`).
pub fn ident(id: &str) -> String {
    let mut s: String =
        id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if s.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}

/// A number in an app unit, converted to SI.
pub fn to_si(value: f64, app_unit_text: &str) -> Result<f64, String> {
    parse_unit(&app_unit(app_unit_text)).map(|u| u.to_si(value)).map_err(|e| e.to_string())
}

/// A real modifier from an app value.
pub fn real(param: &str, value_si: f64) -> Modifier {
    Modifier { param: param.into(), value: ParamValue::Real(Expr::Const(value_si)) }
}

/// Imports the top system of a project (sub-systems: work package 5).
pub fn import(
    project: &Value,
    registry: &Registry,
) -> Result<(ComponentDef, ImportReport), Vec<Diagnostic>> {
    let mut diags = vec![];
    let mut report = ImportReport::default();
    let systems = project["systems"].as_array().cloned().unwrap_or_default();
    let Some(top) = systems.iter().find(|s| s["parentId"].is_null()) else {
        return Err(vec![Diagnostic::error("PROJECT-NO-TOP", "The project has no top system.")]);
    };
    let mut def = ComponentDef {
        name: project["id"].as_str().unwrap_or("project").to_string(),
        doc: project["name"].as_str().unwrap_or("").to_string(),
        ..Default::default()
    };
    let mut kinds: BTreeMap<String, String> = BTreeMap::new();
    for el in top["elements"].as_array().into_iter().flatten() {
        let id = el["id"].as_str().unwrap_or("");
        let kind = el["componentDefId"].as_str().unwrap_or("");
        let label = el["label"].as_str().unwrap_or(id).to_string();
        if el["isSubSystem"].as_bool() == Some(true) {
            diags.push(Diagnostic::error(
                "NOT-YET",
                format!("Sub-system '{label}': sub-systems come with work package 5."),
            ));
            continue;
        }
        let Some(m) = registry.map.get(kind) else {
            diags.push(Diagnostic::error(
                "BLOCK-NOT-MAPPED",
                format!("'{label}' is a {kind}, which the new engine does not model yet."),
            ));
            continue;
        };
        let empty = serde_json::Map::new();
        let overrides = el["parameterOverrides"].as_object().unwrap_or(&empty);
        let modifiers = match m.modifiers(overrides) {
            Ok(x) => x,
            Err(e) => {
                diags.push(Diagnostic::error("PARAM-VALUE", format!("'{label}': {e}")));
                continue;
            }
        };
        let name = ident(id);
        kinds.insert(id.to_string(), kind.to_string());
        def.components.push(SubDecl {
            name: name.clone(),
            def: m.def_name().to_string(),
            modifiers,
            label: Some(label),
            ui_id: Some(id.to_string()),
        });
    }
    let end = |el: &str, port: &str, diags: &mut Vec<Diagnostic>| -> Option<String> {
        let kind = kinds.get(el)?;
        match registry.map[kind].port(port) {
            Some(p) => Some(format!("{}.{p}", ident(el))),
            None => {
                diags.push(Diagnostic::error("PORT-NOT-MAPPED", format!("Port '{port}' of '{el}' is wired, but its block's mapping has no such port.")));
                None
            }
        }
    };
    for w in top["connections"].as_array().into_iter().flatten() {
        let a = end(
            w["sourceElementId"].as_str().unwrap_or(""),
            w["sourcePortId"].as_str().unwrap_or(""),
            &mut diags,
        );
        let b = end(
            w["targetElementId"].as_str().unwrap_or(""),
            w["targetPortId"].as_str().unwrap_or(""),
            &mut diags,
        );
        if let (Some(a), Some(b)) = (a, b) {
            def.connections.push(Connect { a, b });
        }
    }
    for l in project["dataBusConnections"].as_array().into_iter().flatten() {
        let a = end(
            l["element1Id"].as_str().unwrap_or(""),
            l["port1Id"].as_str().unwrap_or(""),
            &mut diags,
        );
        let b = end(
            l["element2Id"].as_str().unwrap_or(""),
            l["port2Id"].as_str().unwrap_or(""),
            &mut diags,
        );
        if let (Some(a), Some(b)) = (a, b) {
            def.connections.push(Connect { a, b });
        }
    }
    for (el, kind) in &kinds {
        let m = &registry.map[kind];
        for port in
            ["sig_soc", "sig_voltage", "sig_current", "sig_power", "sig_speed", "sig_torque"]
        {
            if let Some(v) = m.channel(port) {
                report.channel_map.insert(format!("{el}:{port}"), format!("{}.{v}", ident(el)));
            }
        }
    }
    if diags.is_empty() { Ok((def, report)) } else { Err(diags) }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Source;
    impl BlockMapping for Source {
        fn def_name(&self) -> &str {
            "Electrical.ConstantVoltage"
        }
        fn port(&self, p: &str) -> Option<String> {
            match p {
                "pos" => Some("p".into()),
                "neg" => Some("n".into()),
                _ => None,
            }
        }
        fn modifiers(&self, o: &serde_json::Map<String, Value>) -> Result<Vec<Modifier>, String> {
            let v = o.get("voltage_V").and_then(Value::as_f64).unwrap_or(400.0);
            Ok(vec![real("V", to_si(v, "V")?)])
        }
        fn channel(&self, p: &str) -> Option<String> {
            (p == "sig_current").then(|| "i".into())
        }
    }

    #[test]
    fn imports_parts_wires_and_channel_names() {
        let project = serde_json::json!({
            "id": "demo", "name": "Demo",
            "systems": [{"id": "sys-root", "name": "Demo", "parentId": null,
                "elements": [
                    {"id": "el-src", "componentDefId": "electric.voltage_source", "label": "Bench", "parameterOverrides": {"voltage_V": 48}},
                    {"id": "el-heater", "componentDefId": "electric.heater", "label": "Heater"}
                ],
                "connections": [{"id": "c-1", "sourceElementId": "el-src", "sourcePortId": "pos", "targetElementId": "el-src", "targetPortId": "neg"}]}],
            "dataBusConnections": []
        });
        let mut reg = Registry::default();
        reg.add("electric.voltage_source", Box::new(Source));
        let err = import(&project, &reg).unwrap_err();
        assert_eq!(err[0].code, "BLOCK-NOT-MAPPED");
        assert!(err[0].message.contains("'Heater' is a electric.heater"));
        let mut p2 = project.clone();
        p2["systems"][0]["elements"].as_array_mut().unwrap().pop();
        let (def, rep) = import(&p2, &reg).unwrap();
        assert_eq!(def.components[0].name, "el_src");
        assert_eq!(def.components[0].ui_id.as_deref(), Some("el-src"));
        assert_eq!(def.connections[0].a, "el_src.p");
        assert_eq!(rep.channel_map["el-src:sig_current"], "el_src.i");
        assert_eq!(app_unit("1/min"), "rev/min");
        assert!((to_si(3000.0, "1/min").unwrap() - 314.159_265_358_979_3).abs() < 1e-9);
        assert!((to_si(1.0, "g").unwrap() - 9.80665).abs() < 1e-12);
    }
}
