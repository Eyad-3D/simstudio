//! What the importer learns about a project before it builds anything:
//! its elements with their merged parameters, its wires and signal links,
//! its electrical buses, its vehicle and wheels, and its drivelines'
//! speed ratios (for the driver's recuperation blending, the brakes'
//! capacity and the initial speeds). Today's `network.py` reduces the
//! project to the same facts.

use lsim_lib::blocks::catalog::{self, AppUnit, app_unit};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// An element of the project, with its parameters merged: the library's
/// defaults, the element's overrides, the case's overrides.
#[derive(Clone, Debug)]
pub struct Element {
    /// its id
    pub id: String,
    /// its component id (`battery.generic`)
    pub kind: String,
    /// its label
    pub label: String,
    /// merged parameters (today's keys and display units)
    pub params: Map<String, Value>,
    /// per-table outside-the-data settings (`tableOutside`)
    pub table_outside: Map<String, Value>,
    /// its dynamic ports (Script, FMU, Monitor)
    pub dynamic_ports: Vec<Value>,
}

impl Element {
    /// A table axis's outside-the-data setting: the element's own when it
    /// names one per axis, else the library's.
    pub fn outside(&self, key: &str, axis: usize) -> lsim_lib::table::Outside {
        let n_axes = catalog::param(&self.kind, key)
            .and_then(|p| p["axes"].as_array().map(|a| a.len()))
            .unwrap_or(1);
        if let Some(own) = self.table_outside.get(key).and_then(Value::as_array)
            && own.len() == n_axes
            && let Some(s) = own.get(axis).and_then(Value::as_str)
        {
            return lsim_lib::table::Outside::parse(s);
        }
        catalog::axis_outside(&self.kind, key, axis)
    }

    /// A number parameter (today's `float(p.get(key, default))`).
    pub fn num(&self, key: &str, default: f64) -> f64 {
        catalog::get(&self.params, key, default)
    }

    /// A port's unit (its unit group's display unit), from the library or
    /// its dynamic ports.
    pub fn port_unit(&self, port: &str) -> AppUnit {
        if let Some(p) = self.dynamic_ports.iter().find(|p| p["id"] == port) {
            let g = p["unitGroup"].as_str().unwrap_or("No Unit");
            return app_unit(catalog::group_unit(g));
        }
        catalog::port_unit(&self.kind, port)
    }

    /// The port's kind (`electrical`, `mechanical`, `signal`, `thermal`)
    /// and direction.
    pub fn port_kind(&self, port: &str) -> Option<(String, String)> {
        let from = |p: &Value| {
            (p["kind"].as_str().unwrap_or("signal").to_string(), p["direction"].as_str().unwrap_or("").to_string())
        };
        if let Some(p) = self.dynamic_ports.iter().find(|p| p["id"] == port) {
            return Some(from(p));
        }
        catalog::block(&self.kind)?["ports"].as_array()?.iter().find(|p| p["id"] == port).map(from)
    }
}

/// An end of a wire or link: (element id, port id).
pub type End = (String, String);

/// An electrical bus: the positive rail its parts share.
#[derive(Clone, Debug, Default)]
pub struct Bus {
    /// its source: (element id, component id)
    pub source: Option<(String, String)>,
    /// E-Motors on it
    pub motors: Vec<String>,
    /// power consumers and climate controls on it
    pub consumers: Vec<String>,
    /// electric nodes on it, with the terminal wired to its source
    pub nodes: Vec<(String, Option<usize>)>,
}

/// A wheel's share of the weight.
#[derive(Clone, Debug)]
pub struct WheelShare {
    /// its share, scaled so the wheels' add up to 1
    pub share: f64,
    /// on the front axle
    pub front: bool,
    /// its share over its axle's
    pub axle_part: f64,
}

/// A mechanical port's kinematics relative to the wheels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kin {
    /// its speed when every wheel turns at 1 rad/s (gearboxes in their
    /// default gear)
    pub speed: f64,
    /// the efficiency of the way from it to the wheels
    pub eff: f64,
    /// the gearboxes between it and the wheels
    pub gearboxes: Vec<String>,
    /// its speed at the start (the vehicle's initial speed)
    pub w0: f64,
}

/// The facts.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// the elements in project order
    pub elements: Vec<Element>,
    /// element index by id
    pub index: HashMap<String, usize>,
    /// physical wires
    pub wires: Vec<(End, End)>,
    /// signal links: (output, input)
    pub links: Vec<(End, End)>,
    /// the ports that are wired or linked
    pub wired: BTreeSet<End>,
    /// the Vehicle, Driver, first Ambient, Fuel Tank, Hydrogen Tank
    pub vehicle: Option<String>,
    /// the Driver
    pub driver: Option<String>,
    /// the first Ambient
    pub ambient: Option<String>,
    /// the Fuel Tank
    pub fuel_tank: Option<String>,
    /// the Hydrogen Tank
    pub h2_tank: Option<String>,
    /// the wheels' shares
    pub wheels: BTreeMap<String, WheelShare>,
    /// the electrical buses
    pub buses: Vec<Bus>,
    /// each mechanical port's kinematics relative to the wheels
    pub kin: HashMap<End, Kin>,
    /// the axle gears run lossless (coefficients that include their drag)
    pub lossless_axle: bool,
    /// warnings
    pub warnings: Vec<String>,
}

impl Facts {
    /// An element by id.
    pub fn el(&self, id: &str) -> &Element {
        &self.elements[self.index[id]]
    }

    /// Whether a port is wired or linked.
    pub fn is_wired(&self, el: &str, port: &str) -> bool {
        self.wired.contains(&(el.to_string(), port.to_string()))
    }

    /// The elements of a component id.
    pub fn of_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.elements.iter().filter(move |e| e.kind == kind)
    }

    /// A mechanical port's kinematics, if it reaches a wheel.
    pub fn kin_of(&self, el: &str, port: &str) -> Option<&Kin> {
        self.kin.get(&(el.to_string(), port.to_string()))
    }

    /// The output linked to an input, if any.
    pub fn source_of(&self, el: &str, port: &str) -> Option<&End> {
        self.links.iter().find(|(_, to)| to.0 == el && to.1 == port).map(|(from, _)| from)
    }
}

const RATIO_TYPES: [&str; 2] = ["mech.final_drive", "mech.gearbox"];

/// Reads the project into facts, with the case's overrides applied.
pub fn read(project: &Value, case: Option<&Value>, cycles: &dyn Fn(&str) -> Option<String>) -> Result<Facts, Vec<String>> {
    let mut f = Facts::default();
    let mut errors = vec![];
    let case_over = case.and_then(|c| c["parameterOverrides"].as_object()).cloned().unwrap_or_default();
    for sys in project["systems"].as_array().into_iter().flatten() {
        for el in sys["elements"].as_array().into_iter().flatten() {
            let id = el["id"].as_str().unwrap_or("").to_string();
            let kind = el["componentDefId"].as_str().unwrap_or("").to_string();
            if el["isSubSystem"].as_bool() == Some(true) || kind == "container.system" {
                continue; // a container: its children are elements of their own system
            }
            if catalog::block(&kind).is_none() {
                errors.push(format!("'{}' is a {kind}, which is not in LightSim's library.", el["label"].as_str().unwrap_or(&id)));
                continue;
            }
            let empty = Map::new();
            let mut over = el["parameterOverrides"].as_object().unwrap_or(&empty).clone();
            if let Some(case_el) = case_over.get(&id).and_then(Value::as_object) {
                for (k, v) in case_el {
                    over.insert(k.clone(), v.clone());
                }
                if case_el.contains_key("profile") && !case_el.contains_key("cycle") {
                    over.insert("cycle".into(), Value::String(String::new()));
                }
            }
            let mut params = catalog::merged(&kind, &over);
            // a drive cycle's trace becomes the profile (today's build_model)
            if matches!(kind.as_str(), "signal.driving_task" | "signal.road_profile")
                && let Some(cy) = params.get("cycle").and_then(Value::as_str).filter(|s| !s.is_empty())
            {
                let cy = cy.to_string();
                match cycles(&cy) {
                    Some(text) if kind == "signal.driving_task" => {
                        params.insert("profile".into(), Value::String(text));
                    }
                    Some(_) => errors.push(format!(
                        "Road Profile '{}': grades from drive cycles are not imported yet.",
                        el["label"].as_str().unwrap_or(&id)
                    )),
                    None => errors.push(format!("the drive cycle '{cy}' is not available to the importer.")),
                }
            }
            f.index.insert(id.clone(), f.elements.len());
            f.elements.push(Element {
                id: id.clone(),
                kind,
                label: el["label"].as_str().unwrap_or(&id).to_string(),
                params,
                table_outside: el["tableOutside"].as_object().cloned().unwrap_or_default(),
                dynamic_ports: el["dynamicPorts"].as_array().cloned().unwrap_or_default(),
            });
        }
    }
    let end = |e: &str, p: &str| (e.to_string(), p.to_string());
    let mut add_signal = |f: &mut Facts, a: End, b: End| {
        let (ka, kb) = (
            f.index.get(&a.0).and_then(|&i| f.elements[i].port_kind(&a.1)),
            f.index.get(&b.0).and_then(|&i| f.elements[i].port_kind(&b.1)),
        );
        let (Some((_, da)), Some((_, db))) = (ka, kb) else { return };
        let (from, to) = if da == "output" && db != "output" {
            (a, b)
        } else if db == "output" && da != "output" {
            (b, a)
        } else {
            return;
        };
        // today a second link into an input replaces the first
        f.links.retain(|(_, t)| *t != to);
        f.wired.insert(from.clone());
        f.wired.insert(to.clone());
        f.links.push((from, to));
    };
    for sys in project["systems"].as_array().into_iter().flatten() {
        for w in sys["connections"].as_array().into_iter().flatten() {
            let a = end(w["sourceElementId"].as_str().unwrap_or(""), w["sourcePortId"].as_str().unwrap_or(""));
            let b = end(w["targetElementId"].as_str().unwrap_or(""), w["targetPortId"].as_str().unwrap_or(""));
            let (Some(ia), Some(ib)) = (f.index.get(&a.0), f.index.get(&b.0)) else { continue };
            let (ka, kb) = (f.elements[*ia].port_kind(&a.1), f.elements[*ib].port_kind(&b.1));
            let (Some((ka, _)), Some((kb, _))) = (ka, kb) else { continue };
            if ka == "signal" || kb == "signal" {
                add_signal(&mut f, a, b);
            } else if ka == kb {
                f.wired.insert(a.clone());
                f.wired.insert(b.clone());
                f.wires.push((a, b));
            }
        }
    }
    for l in project["dataBusConnections"].as_array().into_iter().flatten() {
        let a = end(l["element1Id"].as_str().unwrap_or(""), l["port1Id"].as_str().unwrap_or(""));
        let b = end(l["element2Id"].as_str().unwrap_or(""), l["port2Id"].as_str().unwrap_or(""));
        add_signal(&mut f, a, b);
    }
    let single = |f: &Facts, kind: &str, errors: &mut Vec<String>| -> Option<String> {
        let found: Vec<&Element> = f.of_kind(kind).collect();
        if found.len() > 1 {
            errors.push(format!("Only one {kind} element per model is supported."));
        }
        found.first().map(|e| e.id.clone())
    };
    f.vehicle = single(&f, "vehicle.body", &mut errors);
    f.driver = single(&f, "driver.driver", &mut errors);
    f.fuel_tank = single(&f, "fuel.tank", &mut errors);
    f.h2_tank = single(&f, "fuel.h2_tank", &mut errors);
    f.ambient = f.of_kind("boundary.ambient").next().map(|e| e.id.clone());
    if let Some(v) = &f.vehicle {
        let vp = &f.el(v).params;
        f.lossless_axle = catalog::text(vp, "road_load_mode", "") == "Coefficients A/B/C"
            && catalog::flag(vp, "abc_include_driveline_losses", true);
    }
    wheel_shares(&mut f);
    buses(&mut f);
    kinematics(&mut f);
    if errors.is_empty() { Ok(f) } else { Err(errors) }
}

/// Each wheel's share of the weight, scaled so they add up to 1, and its
/// part of its axle (today's `normalize_wheel_loads`).
fn wheel_shares(f: &mut Facts) {
    let wheels: Vec<(String, f64, bool)> = f
        .of_kind("propulsion.wheel")
        .filter(|e| f.is_wired(&e.id, "shaft"))
        .map(|e| {
            let share = (e.num("vehicle_load_share_pct", 25.0) / 100.0).max(0.0);
            let front = catalog::text(&e.params, "axle", "Front") != "Rear";
            (e.id.clone(), share, front)
        })
        .collect();
    let total: f64 = wheels.iter().map(|w| w.1).sum();
    let scale = total > 0.0 && (total - 1.0).abs() > 1e-9;
    let norm = |s: f64| if scale { s / total } else { s };
    let axle_sum = |front: bool| -> f64 { wheels.iter().filter(|w| w.2 == front).map(|w| norm(w.1)).sum() };
    let (sf, sr) = (axle_sum(true), axle_sum(false));
    let count = |front: bool| wheels.iter().filter(|w| w.2 == front).count() as f64;
    for (id, s, front) in &wheels {
        let s_axle = if *front { sf } else { sr };
        let part = if s_axle > 0.0 { norm(*s) / s_axle } else { 1.0 / count(*front).max(1.0) };
        f.wheels.insert(id.clone(), WheelShare { share: norm(*s), front: *front, axle_part: part });
    }
}

/// Union-find over electrical ports: the positive rails and their parts.
fn buses(f: &mut Facts) {
    let mut parent: HashMap<End, End> = HashMap::new();
    fn find(p: &mut HashMap<End, End>, x: &End) -> End {
        let mut r = x.clone();
        while let Some(q) = p.get(&r).cloned() {
            if q == r {
                break;
            }
            r = q;
        }
        r
    }
    let mut union = |p: &mut HashMap<End, End>, a: &End, b: &End| {
        let (ra, rb) = (find(p, a), find(p, b));
        if ra != rb {
            p.insert(ra, rb);
        }
    };
    let elec = |f: &Facts, e: &End| {
        f.index.get(&e.0).and_then(|&i| f.elements[i].port_kind(&e.1)).is_some_and(|k| k.0 == "electrical")
    };
    for (a, b) in f.wires.clone() {
        if elec(f, &a) {
            parent.entry(a.clone()).or_insert(a.clone());
            parent.entry(b.clone()).or_insert(b.clone());
            union(&mut parent, &a, &b);
        }
    }
    for e in &f.elements {
        let ts: &[&str] = match e.kind.as_str() {
            "electric.node" => &["t1", "t2", "t3", "t4", "t5"],
            "boundary.ground" => &["t1", "t2", "t3"],
            _ => &[],
        };
        for w in ts.windows(2) {
            let (a, b) = ((e.id.clone(), w[0].to_string()), (e.id.clone(), w[1].to_string()));
            parent.entry(a.clone()).or_insert(a.clone());
            parent.entry(b.clone()).or_insert(b.clone());
            union(&mut parent, &a, &b);
        }
    }
    let mut groups: BTreeMap<End, Vec<End>> = BTreeMap::new();
    let keys: Vec<End> = parent.keys().cloned().collect();
    for k in keys {
        let r = find(&mut parent, &k);
        groups.entry(r).or_default().push(k);
    }
    for members in groups.values() {
        let grounded = members.iter().any(|(e, _)| f.el(e).kind == "boundary.ground");
        if grounded {
            continue;
        }
        let mut bus = Bus::default();
        for (e, p) in members {
            let k = f.el(e).kind.as_str();
            match (k, p.as_str()) {
                ("battery.generic" | "electric.voltage_source" | "fuelcell.stack", "pos") | ("controller.dcdc", "b_pos") => {
                    if bus.source.is_none() {
                        bus.source = Some((e.clone(), k.to_string()));
                    }
                }
                ("motor.emotor", "pos") => bus.motors.push(e.clone()),
                ("electric.constant_drive" | "electric.climate", "pos") => bus.consumers.push(e.clone()),
                _ => {}
            }
        }
        for (e, _) in members {
            if f.el(e).kind == "electric.node" && !bus.nodes.iter().any(|(n, _)| n == e) {
                // the terminal wired straight to the source's positive port
                let src = bus.source.clone();
                let term = src.and_then(|(s, k)| {
                    let sp = if k == "controller.dcdc" { "b_pos" } else { "pos" };
                    (0..5).find(|t| {
                        let tp = format!("t{}", t + 1);
                        f.wires.iter().any(|(a, b)| {
                            (a.0 == *e && a.1 == tp && b.0 == s && b.1 == sp)
                                || (b.0 == *e && b.1 == tp && a.0 == s && a.1 == sp)
                        })
                    })
                });
                bus.nodes.push((e.clone(), term));
            }
        }
        bus.motors.sort();
        bus.motors.dedup();
        bus.consumers.sort();
        bus.consumers.dedup();
        f.buses.push(bus);
    }
}

/// The mechanical ports of an element and how their speeds relate:
/// (port, factor): all ports' speed = factor × the element's own speed
/// for rigid parts; gears and splits are handled apart.
fn rigid_ports(kind: &str) -> &'static [&'static str] {
    match kind {
        "mech.node" => &["f1", "f2", "f3", "f4"],
        "mech.shaft" => &["flange_a", "flange_b"],
        "mech.brake" => &["flange"],
        "motor.emotor" | "engine.combustion" | "propulsion.wheel" | "propulsion.propeller" => &["shaft"],
        _ => &[],
    }
}

fn efficiency(e: &Element, lossless_axle: bool) -> f64 {
    let axle = matches!(e.kind.as_str(), "mech.final_drive" | "mech.differential" | "mech.transfer_case");
    if axle && lossless_axle {
        return 1.0;
    }
    (e.num("efficiency_pct", 100.0) / 100.0).max(1e-3)
}

/// The gearbox's ratio in its default gear (the nearest defined gear).
pub fn default_ratio(e: &Element) -> f64 {
    let g = e.num("default_gear", 1.0);
    let t = e.params.get("ratios").and_then(|v| lsim_lib::table::Table1::from_json(v, Default::default()).ok());
    let Some(t) = t else { return 1.0 };
    let mut best = (f64::INFINITY, 1.0);
    for (x, y) in t.x.iter().zip(&t.y) {
        if (x - g).abs() < best.0 {
            best = ((x - g).abs(), if *y != 0.0 { *y } else { 1.0 });
        }
    }
    best.1
}

/// Speeds when every wheel turns at 1 rad/s (and at the start), from the
/// wheels up through nodes, shafts, brakes, splits and gears, with the
/// efficiency of the way down; clutches are taken closed.
fn kinematics(f: &mut Facts) {
    let v0 = f.vehicle.as_ref().map(|v| f.el(v).num("initial_speed_kmh", 0.0).max(0.0) / 3.6).unwrap_or(0.0);
    // port → peers
    let mut peers: HashMap<End, Vec<End>> = HashMap::new();
    for (a, b) in &f.wires {
        peers.entry(a.clone()).or_default().push(b.clone());
        peers.entry(b.clone()).or_default().push(a.clone());
    }
    // (speed per unit wheel speed, efficiency to the wheels, gearboxes below, start speed)
    let mut port_val: HashMap<End, Kin> = HashMap::new();
    let mut todo: Vec<End> = vec![];
    for e in f.of_kind("propulsion.wheel") {
        let r = e.num("radius_m", 0.33).max(1e-3);
        let k = (e.id.clone(), "shaft".to_string());
        port_val.insert(k.clone(), Kin { speed: 1.0, eff: 1.0, gearboxes: vec![], w0: v0 / r });
        todo.push(k);
    }
    let mut guard = 0;
    while let Some(p) = todo.pop() {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        let val = port_val[&p].clone();
        // across the wire: same speed
        for q in peers.get(&p).cloned().unwrap_or_default() {
            if !port_val.contains_key(&q) {
                port_val.insert(q.clone(), val.clone());
                todo.push(q);
            }
        }
        // through the element
        let Some(&i) = f.index.get(&p.0) else { continue };
        let e = f.elements[i].clone();
        let mut set = |port: &str, v: Kin, todo: &mut Vec<End>| {
            let k = (e.id.clone(), port.to_string());
            if !port_val.contains_key(&k) {
                port_val.insert(k.clone(), v);
                todo.push(k);
            }
        };
        let kind = e.kind.as_str();
        if rigid_ports(kind).contains(&p.1.as_str()) || kind == "mech.clutch" {
            let ports: &[&str] = if kind == "mech.clutch" { &["flange_a", "flange_b"] } else { rigid_ports(kind) };
            let eta = if kind == "mech.shaft" { efficiency(&e, f.lossless_axle) } else { 1.0 };
            for q in ports {
                if *q != p.1 {
                    set(q, Kin { eff: val.eff * eta, ..val.clone() }, &mut todo);
                }
            }
        } else if RATIO_TYPES.contains(&kind) && p.1 == "flange_out" {
            let ratio = if kind == "mech.gearbox" { default_ratio(&e) } else { e.num("ratio", 1.0) };
            let ratio = if ratio != 0.0 { ratio } else { 1.0 };
            let mut gbs = val.gearboxes.clone();
            if kind == "mech.gearbox" {
                gbs.push(e.id.clone());
            }
            let k = Kin {
                speed: val.speed * ratio,
                eff: val.eff * efficiency(&e, f.lossless_axle),
                gearboxes: gbs,
                w0: val.w0 * ratio,
            };
            set("flange_in", k, &mut todo);
        } else if matches!(kind, "mech.differential" | "mech.transfer_case") && p.1 != "flange_in" {
            let other = if p.1 == "flange_out_a" { "flange_out_b" } else { "flange_out_a" };
            let ko = (e.id.clone(), other.to_string());
            if let Some(vo) = port_val.get(&ko).cloned() {
                let fb = if kind == "mech.transfer_case" {
                    1.0 - (e.num("torque_split_a_pct", 50.0) / 100.0).clamp(0.0, 1.0)
                } else {
                    0.5
                };
                let (va, vb) = if p.1 == "flange_out_a" { (val.clone(), vo) } else { (vo, val.clone()) };
                let r = e.num("ratio", 1.0);
                let r = if r != 0.0 { r } else { 1.0 };
                let mut gbs = va.gearboxes.clone();
                gbs.extend(vb.gearboxes.clone());
                let k = Kin {
                    speed: r * ((1.0 - fb) * va.speed + fb * vb.speed),
                    eff: efficiency(&e, f.lossless_axle) * ((1.0 - fb) * va.eff + fb * vb.eff),
                    gearboxes: gbs,
                    w0: r * ((1.0 - fb) * va.w0 + fb * vb.w0),
                };
                set("flange_in", k, &mut todo);
            }
        }
    }
    f.kin = port_val;
}
