//! Structural parameters, enumerations, Boolean parameters, tables and
//! asserts, from the text format through flattening and preparation.

use lsim_ir::Expr;
use lsim_lang::{Report, parse_library};
use lsim_prep::PrepOptions;

const TEXT: &str = r#"
type DriveMode = enumeration(Eco "gentle", Normal, Sport "eager");

model Test.Cell "A cell with a table, a mode and a check."
  connector p: Pin;
  connector n: Pin;
  parameter Real ocv(unit = "V") = table(x = {0, 0.5, 1}, y = {3.0, 3.6, 4.2}, xUnit = "1");
  parameter Real r(unit = "Ohm") = 0.01;
  parameter DriveMode mode = DriveMode.Normal;
  parameter Boolean derate = false;
  structural parameter Real n_series(unit = "1") = 2;
  Real v(unit = "V");
  Real i(unit = "A");
  Real soc(unit = "1", start = 0.5, fixed = true);
equation
  v = p.v - n.v;
  0 = p.i + n.i;
  i = p.i;
  v = n_series * ocv(soc) - r * i;
  der(soc) = 0 "held for the test";
  assert(soc >= 0, "the cell is empty", AssertionLevel.warning) "not empty";
end Test.Cell;

model Test.Pack "Two cells, one with its own table and mode."
  parameter Real ocv_pack(unit = "V") = table(x1 = {0, 1}, x2 = {273.15, 313.15}, values = [3.0, 3.1; 4.1, 4.2], x1Unit = "1", x2Unit = "K", interpolation = linear, outside = {clamp, linear});
  parameter Real ocv_a(unit = "V") = table(x = {0, 1}, y = {3.1, 4.1}, xUnit = "1");
  Test.Cell a(ocv = ocv_a, mode = DriveMode.Sport, derate = true, n_series = 3) "Cell A";
  Test.Cell b(r = 0.02);
  Electrical.Ground gnd;
  Electrical.Resistor load(R = 10);
equation
  connect(a.n, gnd.p);
  connect(a.p, b.n);
  connect(b.p, load.p);
  connect(load.n, gnd.p);
end Test.Pack;
"#;

#[test]
fn every_kind_of_parameter_reaches_the_flat_system() {
    let mut lib = lsim_lib::library();
    let parsed = parse_library(TEXT, Some(&lib)).unwrap_or_else(|e| panic!("{}", Report(&e)));
    for (k, t) in parsed.types {
        lib.types.insert(k, t);
    }
    for c in parsed.components.into_values() {
        lib.add(c);
    }
    let top = lib.components["Test.Pack"].clone();
    let flat = lsim_prep::flatten::flatten(&lib, &top).unwrap_or_else(|d| panic!("{d:#?}"));
    let param =
        |n: &str| &flat.params[flat.find_param(n).unwrap_or_else(|| panic!("{n}")).0 as usize];

    // tables: the pack's 2-D table with its rules, cell a's handed-down
    // table (shared with the pack's ocv_a), cell b's own default
    let names: Vec<&str> = flat.tables.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["ocv_pack", "ocv_a", "b.ocv"]);
    assert_eq!(flat.tables[0].data.dims(), 2);
    assert_eq!(flat.tables[0].data.outside[1], lsim_ir::Outside::Linear);
    assert_eq!(flat.tables[0].data.axis_units, ["1".to_string(), "K".to_string()]);
    assert_eq!(param("a.ocv").value, 1.0, "a.ocv is the pack's table 1");
    // cell a reads table 1 (the pack's ocv_a), cell b its own table 2
    let reads = |var: &str| {
        let v = flat.find_var(var).unwrap();
        let mut tables = vec![];
        for e in flat.equations.iter().filter(|e| e.lhs == Expr::Var(v)) {
            e.rhs.walk(&mut |x| {
                if let Expr::Table { table, args } = x {
                    tables.push((*table, args.len()));
                }
            });
        }
        tables
    };
    assert_eq!(reads("a.v"), [(1, 1)]);
    assert_eq!(reads("b.v"), [(2, 1)]);

    // enumerations are numbers, Booleans 1 or 0, both structural
    assert_eq!((param("a.mode").value, param("b.mode").value), (3.0, 2.0));
    assert_eq!((param("a.derate").value, param("b.derate").value), (1.0, 0.0));
    assert!(param("a.mode").structural && param("a.derate").structural);
    // a structural parameter stays structural when a part is given a value
    assert!(param("a.n_series").structural && param("b.n_series").structural);
    assert!(!param("a.r").structural);
    assert_eq!(param("a.n_series").value, 3.0);

    // asserts, with their origin's label
    assert_eq!(flat.asserts.len(), 2);
    assert!(flat.asserts.iter().all(|a| !a.error && a.message == "the cell is empty"));
    assert_eq!(flat.asserts[0].origin.label.as_deref(), Some("not empty"));
}

const STRUCTURAL: &str = r#"
model Test.Divider "A divider whose number of cells is structural."
  structural parameter Real n(unit = "1") = 2;
  parameter Real v_cell(unit = "V") = 3.7;
  Electrical.ConstantVoltage src(V = n * v_cell);
  Electrical.Resistor r1(R = 1);
  Electrical.Resistor r2(R = 3);
  Electrical.Ground gnd;
equation
  connect(src.p, r1.p);
  connect(r1.n, r2.p);
  connect(r2.n, src.n);
  connect(src.n, gnd.p);
end Test.Divider;
"#;

#[test]
fn a_structural_parameter_is_part_of_the_structure_key() {
    let lib = lsim_lib::library();
    let key = |text: &str| {
        let def = lsim_lang::parse_with(text, &lib)
            .unwrap_or_else(|e| panic!("{}", Report(&e)))
            .remove(0);
        lsim_prep::prepare(&lib, &def, &PrepOptions::default())
            .unwrap_or_else(|d| panic!("{d:#?}"))
            .structure_key
    };
    let base = key(STRUCTURAL);
    // a runtime parameter's value does not change the compiled code
    assert_eq!(
        base,
        key(&STRUCTURAL.replace("v_cell(unit = \"V\") = 3.7", "v_cell(unit = \"V\") = 4.1"))
    );
    // a structural one does
    assert_ne!(base, key(&STRUCTURAL.replace("n(unit = \"1\") = 2", "n(unit = \"1\") = 3")));
}
