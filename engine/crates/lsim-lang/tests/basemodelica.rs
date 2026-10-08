//! Hand-written Base Modelica models of the reference problems
//! (`benchmarks/reference/problems/*.toml`) import, prepare, compile,
//! simulate, and match the problems' exact answers at their checkpoints.
//!
//! Between them the models use what the import reads: a package with
//! types (aliases with units, enumerations), records, functions,
//! constants; parameters bound to parameters, Boolean variables, discrete
//! variables of an enumeration type, signal outputs; when-clauses setting
//! discrete values (and recording event times), if-expressions, an
//! if-equation, a nonlinear algebraic loop (constant power), unit
//! inference for undeclared units.

use lsim_codegen::CodegenOptions;
use lsim_ir::component::Library;
use lsim_lang::{Report, basemodelica};
use lsim_prep::PrepOptions;
use lsim_solve::{OutputGrid, RunInfo, SimResult, SolverOptions};

/// Imports, prepares, compiles and runs a model at a tight tolerance.
fn run(text: &str, t_end: f64, dt: f64) -> SimResult {
    let def = basemodelica::import(text).unwrap_or_else(|e| panic!("import:\n{}", Report(&e)));
    let lib = Library::default();
    let prepared = lsim_prep::prepare(&lib, &def, &PrepOptions::default())
        .unwrap_or_else(|d| panic!("prepare {}: {d:#?}", def.name));
    let jit = lsim_codegen::compile(&prepared, &CodegenOptions::default())
        .unwrap_or_else(|e| panic!("compile {}: {e}", def.name));
    let info = RunInfo::from_prepared(&prepared);
    let opts = SolverOptions { rtol: 1e-10, atol: 1e-10, ..Default::default() };
    lsim_solve::simulate(&jit, &info, &opts, OutputGrid { t0: 0.0, t_end, dt })
        .unwrap_or_else(|e| panic!("simulate {}: {e}", def.name))
}

/// The channel's values at the checkpoint times against the exact ones,
/// relative to the largest exact magnitude; returns the worst error.
fn check(run: &SimResult, name: &str, times: &[f64], exact: &[f64], tol: f64) -> f64 {
    let ch = run.channel(name).unwrap_or_else(|| panic!("no channel '{name}' in {:?}", run.names));
    let scale = exact.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-300);
    let mut worst = 0.0f64;
    for (t, want) in times.iter().zip(exact) {
        let k = run
            .times
            .iter()
            .position(|x| (x - t).abs() < 1e-9 * t.max(1.0))
            .unwrap_or_else(|| panic!("{t} is not on the output grid"));
        let err = (ch[k] - want).abs() / scale;
        worst = worst.max(err);
        assert!(
            err < tol,
            "{name} at t = {t}: {} against the exact {want} (relative error {err:.2e})",
            ch[k]
        );
    }
    worst
}

/// The time an event was recorded at, in a discrete channel.
fn recorded(run: &SimResult, name: &str) -> f64 {
    *run.channel(name).unwrap_or_else(|| panic!("no channel '{name}'")).last().expect("values")
}

fn event(run: &SimResult, name: &str, exact: f64, tol: f64) -> f64 {
    let t = recorded(run, name);
    let err = (t - exact).abs();
    assert!(err < tol, "{name}: {t} against the exact {exact} ({err:.1e} s)");
    err
}

const RC_STEP: &str = r#"//! base 0.1.0
package 'RCStep'
  type 'Voltage' = Real(final quantity = "ElectricPotential", final unit = "V");
  type 'Current' = Real(unit = "A");
  type 'Energy' = Real(unit = "J", start = 0, fixed = true);
  record 'Circuit' "the resistor and the capacitor"
    Real 'R'(unit = "Ohm") "precharge resistance";
    Real 'C'(unit = "F") "DC-link capacitance";
  end 'Circuit';
  model 'RCStep' "elec_rc_step: a 400 V source charges a 1 mF DC link through 50 ohm"
    parameter 'Voltage' 'V' = 400.0 "source voltage";
    parameter 'Circuit' 'rc'('R' = 50.0, 'C' = 1.0e-3);
    parameter Real 'fraction'(unit = "1") = 0.95 "precharge done at this share of V";
    'Voltage' 'v_C'(start = 0.0, fixed = true) "capacitor voltage";
    'Current' 'i' "charging current";
    'Energy' 'E_source';
    'Energy' 'E_R';
    Real 'E_C'(unit = "J");
    discrete Real 't_event'(unit = "s", start = -1, fixed = true);
  equation
    'rc'.'C' * der('v_C') = 'i';
    'i' = ('V' - 'v_C') / 'rc'.'R';
    der('E_source') = 'V' * 'i';
    der('E_R') = 'rc'.'R' * 'i' ^ 2;
    'E_C' = 'rc'.'C' / 2 * 'v_C' ^ 2;
    when 'v_C' >= 'fraction' * 'V' then
      't_event' = time;
    end when;
  end 'RCStep';
end 'RCStep';
"#;

#[test]
fn rc_step() {
    let r = run(RC_STEP, 0.25, 1e-3);
    let t = [0.062, 0.125, 0.188, 0.25];
    check(
        &r,
        "v_C",
        &t,
        &[284.24631282437974, 367.16600055044046, 390.6865038500412, 397.30482120036584],
        1e-7,
    );
    check(
        &r,
        "i",
        &t,
        &[2.315073743512405, 0.6566799889911904, 0.18626992299917608, 0.053903575992683736],
        1e-7,
    );
    check(
        &r,
        "E_source",
        &t,
        &[113.6985251297519, 146.8664002201762, 156.27460154001648, 158.92192848014633],
        1e-7,
    );
    check(
        &r,
        "E_R",
        &t,
        &[73.30054195262433, 79.46096424007317, 79.95662939473235, 79.996368005619],
        1e-7,
    );
    check(
        &r,
        "E_C",
        &t,
        &[40.39798317712758, 67.40543598010302, 76.31797214528413, 78.92556047452732],
        1e-7,
    );
    event(&r, "t_event", 0.14978661367769955, 1e-8);
}

const RL_STEP: &str = r#"//! base 0.1.0
package 'Coil'
  function 'drop' "the voltage across a resistance"
    input Real 'R';
    input Real 'i';
    output Real 'v';
  algorithm
    'v' := 'R' * 'i';
  end 'drop';
  model 'Coil' "elec_rl_step: a 12 V supply energises a 24 ohm, 0.6 H contactor coil"
    parameter Real 'V'(unit = "V") = 12;
    parameter Real 'R'(unit = "Ohm") = 24;
    parameter Real 'L'(unit = "H") = 0.6;
    parameter Real 'i_event'(unit = "A") = 0.3 "pull-in current";
    Real 'i'(unit = "A", start = 0, fixed = true);
    Real 'v_L'(unit = "V");
    Real 'E_source'(start = 0, fixed = true);
    Real 'E_R'(start = 0, fixed = true);
    Real 'E_L';
    discrete Real 't_event'(unit = "s", start = -1, fixed = true);
    Boolean 'pulled_in'(start = false, fixed = true);
  equation
    'v_L' = 'V' - 'drop'('R', 'i');
    'L' * der('i') = 'v_L';
    der('E_source') = 'V' * 'i';
    der('E_R') = 'drop'(i = 'i', R = 'R') * 'i';
    'E_L' = 0.5 * 'L' * 'i' ^ 2;
    when 'i' >= 'i_event' then
      't_event' = time;
      'pulled_in' = true;
    end when;
  end 'Coil';
end 'Coil';
"#;

#[test]
fn rl_step_with_a_function_and_inferred_units() {
    let def = basemodelica::import(RL_STEP).expect("imports");
    let unit = |n: &str| def.vars.iter().find(|v| v.name == n).map(|v| v.unit.clone()).unwrap();
    // nothing declared these units; their equations imply them
    assert_eq!(
        (unit("E_source"), unit("E_R"), unit("E_L")),
        ("N.m".into(), "N.m".into(), "N.m".into())
    );
    let r = run(RL_STEP, 0.15, 5e-4);
    let t = [0.0375, 0.075, 0.1125, 0.15];
    check(
        &r,
        "i",
        &t,
        &[0.3884349199257851, 0.475106465816068, 0.49444550173087887, 0.4987606239116668],
        1e-7,
    );
    check(
        &r,
        "v_L",
        &t,
        &[2.677561921781158, 0.5974448204143673, 0.13330795845890767, 0.029745026119996302],
        1e-7,
    );
    check(
        &r,
        "E_source",
        &t,
        &[0.10846952402226442, 0.3074680602551796, 0.5266663494807363, 0.7503718128264999],
        1e-7,
    );
    check(
        &r,
        "E_R",
        &t,
        &[0.06320501791693917, 0.23975021409710917, 0.45332344322616624, 0.6757431648370733],
        1e-7,
    );
    check(
        &r,
        "E_L",
        &t,
        &[0.04526450610532533, 0.06771784615807039, 0.07334290625457014, 0.07462864798942655],
        1e-7,
    );
    event(&r, "t_event", 0.022907268296853876, 1e-8);
    assert_eq!(recorded(&r, "pulled_in"), 1.0);
}

const DC_MOTOR: &str = r#"//! base 0.1.0
package 'DcMotor'
  type 'AngularVelocity' = Real(unit = "rad/s", displayUnit = "rev/min");
  type 'Torque' = Real(unit = "N.m");
  model 'DcMotor' "motor_dc_spinup: a 48 V DC motor with a 0.5 mH winding spins up an inertia"
    parameter Real 'V'(unit = "V") = 48;
    parameter Real 'R'(unit = "Ohm") = 0.1;
    parameter Real 'L'(unit = "H") = 5.0e-4;
    parameter Real 'k'(unit = "N.m/A") = 0.1;
    parameter Real 'J'(unit = "kg.m2") = 0.02;
    parameter Real 'b'(unit = "N.m.s/rad") = 1.0e-4;
    parameter 'Torque' 'T_load' = 0;
    parameter 'AngularVelocity' 'w_final' = ('k' * 'V' - 'R' * 'T_load') / ('k' ^ 2 + 'R' * 'b');
    Real 'i'(unit = "A", start = 0, fixed = true);
    'AngularVelocity' 'omega'(start = 0, fixed = true);
    'Torque' 'T_e';
    Real 'E_in'(unit = "J", start = 0, fixed = true);
    Real 'E_R'(unit = "J", start = 0, fixed = true);
    Real 'E_L'(unit = "J");
    Real 'E_kin'(unit = "J");
    Real 'E_friction'(unit = "J", start = 0, fixed = true);
    discrete Real 't_event'(unit = "s", start = -1, fixed = true);
  equation
    'L' * der('i') = 'V' - 'R' * 'i' - 'k' * 'omega';
    'J' * der('omega') = 'T_e' - 'b' * 'omega' - 'T_load';
    'T_e' = 'k' * 'i';
    der('E_in') = 'V' * 'i';
    der('E_R') = 'R' * 'i' ^ 2;
    'E_L' = 'L' / 2 * 'i' ^ 2;
    'E_kin' = 'J' / 2 * 'omega' ^ 2;
    der('E_friction') = 'b' * 'omega' ^ 2;
    when 'omega' >= 0.9 * 'w_final' then
      't_event' = time;
    end when;
  end 'DcMotor';
end 'DcMotor';
"#;

#[test]
fn dc_motor_spin_up() {
    let r = run(DC_MOTOR, 1.5, 1e-3);
    let t = [0.375, 0.75, 1.125, 1.5];
    check(
        &r,
        "omega",
        &t,
        &[407.7697055557759, 469.06740021589764, 477.9976127845523, 479.2986192410519],
        1e-7,
    );
    check(
        &r,
        "i",
        &t,
        &[74.12172276716282, 11.208154306599036, 2.0425316364376247, 0.7072292368537243],
        1e-7,
    );
    check(
        &r,
        "E_in",
        &t,
        &[3919.2920920299553, 4515.808543572012, 4610.086506018092, 4631.195379599482],
        1e-7,
    );
    check(
        &r,
        "E_R",
        &t,
        &[2252.079029720158, 2304.9292855201975, 2306.205631920458, 2306.2624594509493],
        1e-7,
    );
    check(
        &r,
        "E_L",
        &t,
        &[1.3735074464930361, 0.03140568074013363, 0.0010429838714621402, 0.00012504329836517531],
        1e-6,
    );
    check(
        &r,
        "E_kin",
        &t,
        &[1662.7613276904422, 2200.2422594530112, 2284.8171782773084, 2297.2716640637886],
        1e-7,
    );
    check(
        &r,
        "E_friction",
        &t,
        &[3.078227172862727, 10.605592918064117, 19.062652836454504, 27.661131041446474],
        1e-7,
    );
    event(&r, "t_event", 0.4534528704588024, 1e-8);
}

const DC_MOTOR_L0: &str = r#"//! base 0.1.0
package 'DcMotorL0'
  constant Real 'fraction' = 0.9 "the event: this share of the final speed";
  model 'DcMotorL0' "motor_dc_spinup_l0: the motor with its inductance neglected"
    parameter Real 'V'(unit = "V") = 48;
    parameter Real 'R'(unit = "Ohm") = 0.1;
    parameter Real 'k'(unit = "N.m/A") = 0.1;
    parameter Real 'J'(unit = "kg.m2") = 0.02;
    parameter Real 'b'(unit = "N.m.s/rad") = 1.0e-4;
    parameter Real 'T_load'(unit = "N.m") = 0;
    Real 'i'(unit = "A");
    Real 'omega'(unit = "rad/s", start = 0, fixed = true);
    output Real 'tau_e'(unit = "N.m") "the motor's torque, as a signal";
    Real 'E_in'(unit = "J", start = 0, fixed = true);
    Real 'E_R'(unit = "J", start = 0, fixed = true);
    Real 'E_kin'(unit = "J");
    Real 'E_friction'(unit = "J", start = 0, fixed = true);
    discrete Real 't_event'(unit = "s", start = -1, fixed = true);
  equation
    'V' = 'R' * 'i' + 'k' * 'omega' "the winding: no inductance";
    'J' * der('omega') = 'tau_e' - 'b' * 'omega' - 'T_load';
    'tau_e' = 'k' * 'i';
    der('E_in') = 'V' * 'i';
    der('E_R') = 'R' * 'i' ^ 2;
    'E_kin' = 'J' / 2 * 'omega' ^ 2;
    der('E_friction') = 'b' * 'omega' ^ 2;
    when 'omega' >= 'fraction' * ('k' * 'V' - 'R' * 'T_load') / ('k' ^ 2 + 'R' * 'b') then
      't_event' = time;
    end when;
  end 'DcMotorL0';
end 'DcMotorL0';
"#;

#[test]
fn dc_motor_spin_up_without_inductance() {
    let r = run(DC_MOTOR_L0, 1.5, 0.01);
    let t = [0.38, 0.75, 1.12, 1.5];
    check(
        &r,
        "omega",
        &t,
        &[407.9354044452017, 468.28544920917756, 477.7571803137226, 479.2572459121421],
        1e-7,
    );
    check(
        &r,
        "i",
        &t,
        &[72.0645955547983, 11.714550790822411, 2.2428196862773913, 0.7427540878578827],
        1e-7,
    );
    check(
        &r,
        "E_in",
        &t,
        &[3921.01406860533, 4508.312000407695, 4607.6660649467885, 4630.798761994567],
        1e-7,
    );
    check(
        &r,
        "E_R",
        &t,
        &[2253.7342205233153, 2304.8311738242755, 2306.251112550944, 2306.3189629626813],
        1e-7,
    );
    check(
        &r,
        "E_kin",
        &t,
        &[1664.1129419987026, 2192.912619410412, 2282.519233413188, 2296.8750775929143],
        1e-7,
    );
    check(
        &r,
        "E_friction",
        &t,
        &[3.1669060833126554, 10.568207173008883, 18.895718982657364, 27.60472143897355],
        1e-7,
    );
    event(&r, "t_event", 0.4600569616371719, 1e-8);
}

const THERMAL_MASS: &str = r#"//! base 0.1.0
package 'Stator'
  type 'Temperature' = Real(unit = "K", displayUnit = "degC", nominal = 300);
  type 'Phase' = enumeration('Heating' "copper losses on", 'Cooling' "losses off");
  model 'Stator' "therm_lumped_mass: 1.5 kW into a 15 kJ/K stator for 20 minutes, then cooling"
    parameter Real 'C'(unit = "J/K") = 15000;
    parameter Real 'G'(unit = "W/K") = 20;
    parameter Real 'P'(unit = "W") = 1500;
    parameter Real 't_off'(unit = "s") = 1200;
    parameter 'Temperature' 'T_amb' = 298.15;
    parameter 'Temperature' 'T_hot' = 353.15;
    parameter 'Temperature' 'T_cool' = 323.15;
    'Temperature' 'T'(start = 298.15, fixed = true);
    'Phase' 'phase'(start = 'Phase'.'Heating', fixed = true);
    Real 'Q_gen'(unit = "W");
    Real 'E_heat'(unit = "J", start = 0, fixed = true);
    Real 'E_ambient'(unit = "J", start = 0, fixed = true);
    Real 'E_stored'(unit = "J");
    discrete Real 't_hot'(unit = "s", start = -1, fixed = true);
    discrete Real 't_cool'(unit = "s", start = -1, fixed = true);
  equation
    'Q_gen' = if noEvent('phase' == 'Phase'.'Heating') then 'P' else 0;
    'C' * der('T') = 'Q_gen' - 'G' * ('T' - 'T_amb');
    der('E_heat') = 'Q_gen';
    der('E_ambient') = 'G' * ('T' - 'T_amb');
    'E_stored' = 'C' * ('T' - 298.15);
    when time >= 't_off' then
      'phase' = 'Phase'.'Cooling';
    end when;
    when 'T' >= 'T_hot' then
      't_hot' = time;
    end when;
    when 'T' <= 'T_cool' then
      't_cool' = time;
    end when;
  end 'Stator';
end 'Stator';
"#;

#[test]
fn thermal_mass_heated_then_cooling() {
    let def = basemodelica::import(THERMAL_MASS).expect("imports");
    assert_eq!(def.types[0].literals.len(), 2);
    let r = run(THERMAL_MASS, 3000.0, 1.0);
    let t = [750.0, 1500.0, 2250.0, 3000.0];
    let c = |v: [f64; 4]| v.map(|x| x + 273.15);
    check(
        &r,
        "T",
        &t,
        &c([72.40904191214182, 65.123857209927, 39.76074216803069, 30.430173580050877]),
        1e-9,
    );
    check(&r, "E_heat", &t, &[1125000.0, 1800000.0, 1800000.0, 1800000.0], 1e-7);
    check(
        &r,
        "E_ambient",
        &t,
        &[413864.37131787254, 1198142.141851095, 1578588.86747954, 1718547.3962992372],
        1e-7,
    );
    check(
        &r,
        "E_stored",
        &t,
        &[711135.6286821272, 601857.858148905, 221411.13252046032, 81452.60370076315],
        1e-7,
    );
    event(&r, "t_hot", 991.3168799867397, 1e-5);
    event(&r, "t_cool", 1854.821456570184, 1e-5);
    assert_eq!(recorded(&r, "phase"), 2.0, "Cooling is the second option");
}

const TWO_MASSES: &str = r#"//! base 0.1.0
package 'CellsOnPlate'
  record 'Path' "a conductance between two temperatures"
    Real 'G'(unit = "W/K");
  end 'Path';
  function 'flow' "heat through a conductance"
    input Real 'G';
    input Real 'Ta';
    input Real 'Tb';
    output Real 'Q';
  protected
    Real 'dT';
  algorithm
    'dT' := 'Ta' - 'Tb';
    'Q' := 'G' * 'dT';
  end 'flow';
  model 'CellsOnPlate' "therm_two_masses: battery cells on a cooled plate"
    parameter Real 'C1'(unit = "J/K") = 40000 "cells";
    parameter Real 'C2'(unit = "J/K") = 10000 "plate and coolant";
    parameter 'Path' 'cells_plate'('G' = 50);
    parameter 'Path' 'plate_air' = 'Path'(40);
    parameter Real 'P'(unit = "W") = 1000;
    parameter Real 'T_amb'(unit = "K") = 298.15;
    parameter Real 'T1_event'(unit = "K") = 323.15;
    Real 'T1'(unit = "K", start = 298.15, fixed = true);
    Real 'T2'(unit = "K", start = 298.15, fixed = true);
    Real 'Q12'(unit = "W");
    Real 'Q2a'(unit = "W");
    Real 'E_ambient'(unit = "J", start = 0, fixed = true);
    Real 'E_12'(unit = "J", start = 0, fixed = true);
    Real 'E_stored1'(unit = "J");
    Real 'E_stored2'(unit = "J");
    discrete Real 't_event'(unit = "s", start = -1, fixed = true);
  equation
    'Q12' = 'flow'('cells_plate'.'G', 'T1', 'T2');
    'Q2a' = 'flow'('plate_air'.'G', 'T2', 'T_amb');
    'C1' * der('T1') = 'P' - 'Q12';
    'C2' * der('T2') = 'Q12' - 'Q2a';
    der('E_ambient') = 'Q2a';
    der('E_12') = 'Q12';
    'E_stored1' = 'C1' * ('T1' - 298.15);
    'E_stored2' = 'C2' * ('T2' - 298.15);
    when 'T1' >= 'T1_event' then
      't_event' = time;
    end when;
  end 'CellsOnPlate';
end 'CellsOnPlate';
"#;

#[test]
fn two_thermal_masses_with_records_and_a_function() {
    let r = run(TWO_MASSES, 7200.0, 5.0);
    let t = [1800.0, 3600.0, 5400.0, 7200.0];
    let k = |v: [f64; 4]| v.map(|x| x + 273.15);
    check(
        &r,
        "T1",
        &t,
        &k([52.22601561392032, 62.94754437254444, 67.20168931676204, 68.88967147138949]),
        1e-9,
    );
    check(
        &r,
        "T2",
        &t,
        &k([39.52804212872786, 45.844881098315405, 48.35130992282429, 49.34582402215359]),
        1e-9,
    );
    check(
        &r,
        "E_ambient",
        &t,
        &[565678.9541559094, 1873649.41411507, 3478419.3281012783, 5200954.900922888],
        1e-7,
    );
    check(
        &r,
        "E_stored1",
        &t,
        &[1089040.6245568127, 1517901.7749017777, 1688067.5726704816, 1755586.8588555793],
        1e-7,
    );
    check(
        &r,
        "E_stored2",
        &t,
        &[145280.42128727864, 208448.81098315405, 233513.09922824294, 243458.2402215359],
        1e-7,
    );
    check(
        &r,
        "E_12",
        &t,
        &[710959.3754431873, 2082098.2250982223, 3711932.427329519, 5444413.141144421],
        1e-7,
    );
    event(&r, "t_event", 1570.2263290022897, 1e-5);
}

const BATTERY_CC: &str = r#"//! base 0.1.0
package 'BatteryCC'
  function 'ocv' "open-circuit voltage, linear in the state of charge"
    input Real 'soc';
    input Real 'a'(unit = "V");
    input Real 'b'(unit = "V");
    output Real 'v'(unit = "V");
  algorithm
    'v' := 'a' + 'b' * 'soc';
  end 'ocv';
  model 'BatteryCC' "batt_cc_rc: a 60 Ah pack discharged at 120 A for 10 minutes"
    parameter Real 'I'(unit = "A") = 120 "discharge current";
    parameter Real 'Q'(unit = "C") = 60 * 3600 "capacity (60 A h)";
    parameter Real 'R0'(unit = "Ohm") = 0.08;
    parameter Real 'R1'(unit = "Ohm") = 0.04;
    parameter Real 'tau1'(unit = "s") = 30;
    parameter Real 'C1'(unit = "F") = 'tau1' / 'R1';
    parameter Real 'ocv_a'(unit = "V") = 340 "open-circuit voltage when empty";
    parameter Real 'ocv_b'(unit = "V") = 60 "its rise from empty to full";
    Real 'SOC'(unit = "1", start = 0.9, fixed = true);
    Real 'v1'(unit = "V", start = 0, fixed = true);
    Real 'V'(unit = "V");
    Real 'E_chem'(unit = "J", start = 0, fixed = true);
    Real 'E_terminal'(unit = "J", start = 0, fixed = true);
    Real 'E_R0'(unit = "J", start = 0, fixed = true);
    Real 'E_R1'(unit = "J", start = 0, fixed = true);
    Real 'E_C1'(unit = "J");
  equation
    'Q' * der('SOC') = -'I';
    'C1' * der('v1') = 'I' - 'v1' / 'R1';
    'V' = 'ocv'('SOC', 'ocv_a', 'ocv_b') - 'R0' * 'I' - 'v1';
    der('E_chem') = 'ocv'(b = 'ocv_b', a = 'ocv_a', soc = 'SOC') * 'I';
    der('E_terminal') = 'V' * 'I';
    der('E_R0') = 'R0' * 'I' ^ 2;
    der('E_R1') = 'v1' ^ 2 / 'R1';
    'E_C1' = 'C1' / 2 * 'v1' ^ 2;
  end 'BatteryCC';
end 'BatteryCC';
"#;

#[test]
fn battery_at_constant_current() {
    let r = run(BATTERY_CC, 600.0, 0.5);
    let t = [150.0, 300.0, 450.0, 600.0];
    check(
        &r,
        "V",
        &t,
        &[374.6323421455956, 369.60021791966284, 364.60000146833113, 359.6000000098935],
        1e-9,
    );
    check(&r, "SOC", &t, &[0.8166666666666667, 0.7333333333333334, 0.65, 0.5666666666666667], 1e-9);
    check(
        &r,
        "v1",
        &t,
        &[4.76765785440439, 4.79978208033714, 4.799998531668861, 4.799999990106462],
        1e-7,
    );
    check(&r, "E_chem", &t, &[7047000.0, 14004000.0, 20871000.0, 27648000.0], 1e-8);
    check(
        &r,
        "E_terminal",
        &t,
        &[6804963.568275855, 13502879.215489212, 20110679.994714003, 26628479.99996438],
        1e-8,
    );
    check(&r, "E_R0", &t, &[172800.0, 345600.0, 518400.0, 691200.0], 1e-8);
    check(
        &r,
        "E_R1",
        &t,
        &[60712.471192895246, 146881.56900376422, 233280.01057198338, 319680.0000712335],
        1e-7,
    );
    check(
        &r,
        "E_C1",
        &t,
        &[8523.960531248951, 8639.21550702207, 8639.99471400871, 8639.999964383265],
        1e-7,
    );
}

const BATTERY_CP: &str = r#"//! base 0.1.0
package 'BatteryCP'
  model 'BatteryCP' "batt_cp_rc: a pack with strong polarisation delivers 60 kW"
    parameter Real 'P'(unit = "W") = 60000 "terminal power";
    parameter Real 'E'(unit = "V") = 380 "open-circuit voltage";
    parameter Real 'R0'(unit = "Ohm") = 0.05;
    parameter Real 'R1'(unit = "Ohm") = 0.15;
    parameter Real 'tau1'(unit = "s") = 20;
    parameter Real 'C1'(unit = "F") = 'tau1' / 'R1';
    parameter Real 'Q'(unit = "C") = 60 * 3600;
    Real 'I'(unit = "A", start = 160) "current: the guess picks the high-voltage root";
    Real 'V'(unit = "V", start = 375);
    Real 'v1'(unit = "V", start = 0, fixed = true);
    Real 'SOC'(unit = "1", start = 0.9, fixed = true);
    Real 'E_chem'(unit = "J", start = 0, fixed = true);
    Real 'E_R0'(unit = "J", start = 0, fixed = true);
    Real 'E_R1'(unit = "J", start = 0, fixed = true);
    Real 'E_C1'(unit = "J");
  equation
    'V' * 'I' = 'P' "the load takes constant power";
    'V' = 'E' - 'R0' * 'I' - 'v1';
    'C1' * der('v1') = 'I' - 'v1' / 'R1';
    'Q' * der('SOC') = -'I';
    der('E_chem') = 'E' * 'I';
    der('E_R0') = 'R0' * 'I' ^ 2;
    der('E_R1') = 'v1' ^ 2 / 'R1';
    'E_C1' = 'C1' / 2 * 'v1' ^ 2;
  end 'BatteryCP';
end 'BatteryCP';
"#;

#[test]
fn battery_at_constant_power_an_algebraic_loop() {
    let r = run(BATTERY_CP, 120.0, 0.1);
    let t = [30.0, 60.0, 90.0, 120.0];
    check(
        &r,
        "I",
        &t,
        &[170.49872363190053, 172.95492160869566, 173.58094699464164, 173.73851004222797],
        1e-7,
    );
    check(
        &r,
        "V",
        &t,
        &[351.90879275751905, 346.91120346229786, 345.6600568140245, 345.34657851858356],
        1e-7,
    );
    check(
        &r,
        "SOC",
        &t,
        &[0.8768249949452183, 0.8529366280931481, 0.8288619629288967, 0.8047401125662852],
        1e-8,
    );
    check(
        &r,
        "E_chem",
        &t,
        &[1902204.4148964882, 3862961.5661144047, 5839010.082796165, 7818931.5605593175],
        1e-7,
    );
    check(
        &r,
        "E_R0",
        &t,
        &[41773.71127067807, 86148.50464928983, 131217.43190641547, 176463.1534992922],
        1e-7,
    );
    check(
        &r,
        "E_R1",
        &t,
        &[34908.106077272656, 136988.7316348023, 263893.8792151801, 397517.81283047434],
        1e-7,
    );
    check(
        &r,
        "E_C1",
        &t,
        &[25522.597548537466, 39824.32983031258, 43898.771674569165, 44950.59422955094],
        1e-7,
    );
}

const VOLTAGE_LIMIT: &str = r#"//! base 0.1.0
package 'VoltageLimit'
  model 'VoltageLimit' "batt_voltage_limit: constant current until the minimum voltage, then held there"
    parameter Real 'I_cc'(unit = "A") = 120;
    parameter Real 'V_min'(unit = "V") = 352;
    parameter Real 'Q'(unit = "C") = 60 * 3600;
    parameter Real 'ocv_a'(unit = "V") = 340;
    parameter Real 'ocv_b'(unit = "V") = 60;
    parameter Real 'R0'(unit = "Ohm") = 0.08;
    parameter Real 'R1'(unit = "Ohm") = 0.04;
    parameter Real 'tau1'(unit = "s") = 30;
    parameter Real 'C1'(unit = "F") = 'tau1' / 'R1';
    Real 'SOC'(unit = "1", start = 0.5, fixed = true);
    Real 'v1'(unit = "V", start = 0, fixed = true);
    Real 'V'(unit = "V", start = 370);
    Real 'I'(unit = "A", start = 120);
    Real 'V_cc'(unit = "V") "the voltage the pack would have at I_cc";
    Boolean 'limited'(start = false, fixed = true) "the BMS holds the minimum voltage";
    Real 'E_terminal'(unit = "J", start = 0, fixed = true);
    Real 'E_R0'(unit = "J", start = 0, fixed = true);
    discrete Real 't_vmin'(unit = "s", start = -1, fixed = true);
  equation
    'Q' * der('SOC') = -'I';
    'C1' * der('v1') = 'I' - 'v1' / 'R1';
    'V' = 'ocv_a' + 'ocv_b' * 'SOC' - 'R0' * 'I' - 'v1';
    if 'limited' then
      'V' = 'V_min';
    else
      'I' = 'I_cc';
    end if;
    der('E_terminal') = 'V' * 'I';
    der('E_R0') = 'R0' * 'I' ^ 2;
    // the event watches the voltage at constant current, which keeps
    // falling after the limit; the held voltage itself would sit on the
    // threshold
    'V_cc' = 'ocv_a' + 'ocv_b' * 'SOC' - 'R0' * 'I_cc' - 'v1';
    when 'V_cc' <= 'V_min' then
      'limited' = true;
      't_vmin' = time;
    end when;
  end 'VoltageLimit';
end 'VoltageLimit';
"#;

#[test]
fn battery_held_at_its_minimum_voltage_an_if_equation() {
    let r = run(VOLTAGE_LIMIT, 400.0, 0.5);
    let t = [100.0, 200.0, 300.0, 400.0];
    check(&r, "V", &t, &[352.43790183473345, 352.0, 352.0, 352.0], 1e-8);
    check(&r, "I", &t, &[120.0, 95.04324979294417, 75.7837796304539, 60.45375313934508], 1e-7);
    check(
        &r,
        "SOC",
        &t,
        &[0.4444444444444444, 0.3946114421308645, 0.35524166280488345, 0.32383878336959854],
        1e-8,
    );
    check(
        &r,
        "E_terminal",
        &t,
        &[4263863.553394958, 8053066.170739364, 11046429.232452363, 13434052.961675942],
        1e-7,
    );
    check(
        &r,
        "E_R0",
        &t,
        &[115200.0, 208392.17646782496, 266491.6444552465, 303455.69254921755],
        1e-7,
    );
    event(&r, "t_vmin", 111.5012027152658, 1e-6);
}

const CONSTANT_POWER: &str = r#"//! base 0.1.0
package 'Launch'
  type 'Velocity' = Real(unit = "m/s", displayUnit = "km/h");
  model 'Launch' "veh_constant_power: 60 kW at the wheels from 18 km/h"
    parameter Real 'm'(unit = "kg") = 1500;
    parameter Real 'P'(unit = "W") = 60000;
    parameter 'Velocity' 'v_event' = 100 / 3.6;
    'Velocity' 'v'(start = 5, fixed = true);
    Real 'x'(unit = "m", start = 0, fixed = true);
    Real 'F'(unit = "N") "tractive force";
    Real 'E_supplied'(unit = "J", start = 0, fixed = true);
    Real 'E_kin'(unit = "J");
    discrete Real 't_event'(unit = "s", start = -1, fixed = true);
  equation
    'F' * 'v' = 'P';
    'm' * der('v') = 'F';
    der('x') = 'v';
    der('E_supplied') = 'P';
    'E_kin' = 'm' / 2 * ('v' ^ 2 - 25);
    when 'v' >= 'v_event' then
      't_event' = time;
    end when;
  end 'Launch';
end 'Launch';
"#;

#[test]
fn car_at_constant_power() {
    let r = run(CONSTANT_POWER, 15.0, 0.01);
    let t = [3.75, 7.5, 11.25, 15.0];
    check(&r, "v", &t, &[18.027756377319946, 25.0, 30.4138126514911, 35.0], 1e-8);
    check(&r, "x", &t, &[47.78350685524152, 129.16666666666666, 233.39813918857723, 356.25], 1e-8);
    check(&r, "E_supplied", &t, &[225000.0, 450000.0, 675000.0, 900000.0], 1e-9);
    check(&r, "E_kin", &t, &[225000.0, 450000.0, 675000.0, 900000.0], 1e-8);
    event(&r, "t_event", 9.332561728395062, 1e-7);
}

const COASTDOWN: &str = r#"//! base 0.1.0
package 'Coastdown'
  model 'Coastdown' "veh_coastdown: a 1500 kg car let go at 108 km/h, to a stop"
    parameter Real 'm'(unit = "kg") = 1500;
    parameter Real 'A'(unit = "N") = 150 "rolling resistance";
    parameter Real 'C'(unit = "N.s2/m2") = 0.40 "air drag coefficient";
    parameter Real 'v_event'(unit = "m/s") = 15;
    Real 'v'(unit = "m/s", start = 30, fixed = true);
    Real 'x'(unit = "m", start = 0, fixed = true);
    Real 'E_aero'(unit = "J", start = 0, fixed = true);
    Real 'E_roll'(unit = "J");
    Real 'E_kin'(unit = "J");
    Boolean 'stopped'(start = false, fixed = true);
    discrete Real 't_event'(unit = "s", start = -1, fixed = true);
    discrete Real 't_stop'(unit = "s", start = -1, fixed = true);
  equation
    'm' * der('v') = if 'stopped' then 0 else -('A' + 'C' * 'v' ^ 2);
    der('x') = 'v';
    der('E_aero') = if 'stopped' then 0 else 'C' * 'v' ^ 3;
    'E_roll' = 'A' * 'x';
    'E_kin' = 'm' / 2 * ('v' ^ 2 - 900);
    when 'v' <= 'v_event' then
      't_event' = time;
    end when;
    when 'v' <= 0 then
      'stopped' = true;
      't_stop' = time;
    end when;
  end 'Coastdown';
end 'Coastdown';
"#;

#[test]
fn car_coasting_to_a_stop() {
    let r = run(COASTDOWN, 220.0, 0.1);
    let t = [55.0, 110.0, 165.0, 220.0];
    check(&r, "v", &t, &[16.765422458577994, 8.87074093023122, 2.8383738690553995, 0.0], 1e-8);
    check(
        &r,
        "x",
        &t,
        &[1245.7868518169707, 1937.4135257695032, 2254.723704284546, 2294.578934291467],
        1e-8,
    );
    check(
        &r,
        "E_aero",
        &t,
        &[277322.42956651084, 325370.43764611497, 330749.16969191574, 330813.15985627996],
        1e-7,
    );
    check(
        &r,
        "E_roll",
        &t,
        &[186868.0277725456, 290612.0288654255, 338208.5556426819, 344186.84014372004],
        1e-8,
    );
    check(
        &r,
        "E_kin",
        &t,
        &[-464190.4573390564, -615982.4665115405, -668957.7253345976, -675000.0],
        1e-8,
    );
    event(&r, "t_event", 65.55701734409855, 1e-6);
    event(&r, "t_stop", 193.18305719109122, 1e-6);
}

const SPIN_DOWN: &str = r#"//! base 0.1.0
package 'SpinDown'
  type 'Torque' = Real(unit = "N.m");
  model 'SpinDown' "mech_inertia_coastdown: a rotor slowed by viscous and Coulomb friction"
    parameter Real 'J'(unit = "kg.m2") = 0.5;
    parameter Real 'c'(unit = "N.m.s/rad") = 0.01;
    parameter 'Torque' 'T_c' = 2;
    Real 'omega'(unit = "rad/s", start = 300, fixed = true);
    Real 'theta'(unit = "rad", start = 0, fixed = true);
    'Torque' 'T_friction';
    Real 'E_viscous'(unit = "J", start = 0, fixed = true);
    Real 'E_coulomb'(unit = "J");
    Real 'E_kin'(unit = "J");
    Boolean 'stuck'(start = false, fixed = true);
    discrete Real 't_stop'(unit = "s", start = -1, fixed = true);
  equation
    'T_friction' = if 'stuck' then 0 else 'c' * 'omega' + 'T_c';
    'J' * der('omega') = -'T_friction';
    der('theta') = 'omega';
    der('E_viscous') = if 'stuck' then 0 else 'c' * 'omega' ^ 2;
    'E_coulomb' = 'T_c' * 'theta';
    'E_kin' = 'J' / 2 * ('omega' ^ 2 - 300 ^ 2);
    when 'omega' <= 0 then
      'stuck' = true;
      't_stop' = time;
    end when;
  end 'SpinDown';
end 'SpinDown';
"#;

#[test]
fn rotor_coasting_down_against_friction() {
    let r = run(SPIN_DOWN, 60.0, 0.05);
    let t = [15.0, 30.0, 45.0, 60.0];
    check(&r, "omega", &t, &[170.40911034085894, 74.40581804701321, 3.2848298702995464, 0.0], 1e-8);
    check(
        &r,
        "theta",
        &t,
        &[3479.5444829570515, 5279.709097649342, 5835.758506485021, 5837.092681258451],
        1e-8,
    );
    check(
        &r,
        "E_viscous",
        &t,
        &[8281.094812295145, 10556.525364889996, 10825.785460210755, 10825.814637483098],
        1e-7,
    );
    check(
        &r,
        "E_coulomb",
        &t,
        &[6959.088965914103, 10559.418195298684, 11671.517012970042, 11674.185362516902],
        1e-8,
    );
    check(
        &r,
        "E_kin",
        &t,
        &[-15240.183778209248, -21115.943560188683, -22497.3024731808, -22500.0],
        1e-8,
    );
    event(&r, "t_stop", 45.81453659370776, 1e-6);
}

const CLUTCH: &str = r#"//! base 0.1.0
package 'Clutch'
  model 'Clutch' "mech_clutch_lockup: an engine-side inertia clutched onto a load at rest"
    parameter Real 'J1'(unit = "kg.m2") = 0.25;
    parameter Real 'J2'(unit = "kg.m2") = 1.0;
    parameter Real 'T_c'(unit = "N.m") = 100 "friction torque while slipping";
    parameter Real 'T_drive'(unit = "N.m") = 20;
    parameter Real 'T_load'(unit = "N.m") = 0;
    parameter Real 'eps_lock'(unit = "rad/s") = 0.1;
    Real 'omega1'(unit = "rad/s", start = 250, fixed = true);
    Real 'omega2'(unit = "rad/s", start = 0, fixed = true);
    Real 'T_clutch'(unit = "N.m");
    Real 'E_drive'(unit = "J", start = 0, fixed = true);
    Real 'E_clutch'(unit = "J", start = 0, fixed = true);
    Real 'E_kin'(unit = "J");
    Boolean 'locked'(start = false, fixed = true);
    discrete Real 't_lock'(unit = "s", start = -1, fixed = true);
  equation
    'T_clutch' = if 'locked' then ('J2' * 'T_drive' + 'J1' * 'T_load') / ('J1' + 'J2') else 'T_c';
    'J1' * der('omega1') = 'T_drive' - 'T_clutch';
    'J2' * der('omega2') = 'T_clutch' - 'T_load';
    der('E_drive') = 'T_drive' * 'omega1';
    der('E_clutch') = 'T_clutch' * ('omega1' - 'omega2');
    'E_kin' = 'J1' / 2 * ('omega1' ^ 2 - 250 ^ 2) + 'J2' / 2 * 'omega2' ^ 2;
    when 'omega1' - 'omega2' <= 'eps_lock' then
      't_lock' = time;
    end when;
    when 'omega1' - 'omega2' <= 0 then
      'locked' = true;
    end when;
  end 'Clutch';
end 'Clutch';
"#;

#[test]
fn clutch_slipping_then_locked() {
    let r = run(CLUTCH, 1.5, 0.01);
    let t = [0.38, 0.75, 1.12, 1.5];
    check(&r, "omega1", &t, &[128.4, 61.99999999999999, 67.92, 74.0], 1e-8);
    check(&r, "omega2", &t, &[38.0, 61.99999999999999, 67.92, 74.0], 1e-8);
    check(&r, "T_clutch", &t, &[100.0, 16.0, 16.0, 16.0], 1e-9);
    check(
        &r,
        "E_drive",
        &t,
        &[1437.92, 2030.4761904761904, 2511.1801904761905, 3050.4761904761904],
        1e-8,
    );
    check(
        &r,
        "E_clutch",
        &t,
        &[6467.6, 7440.476190476192, 7440.476190476192, 7440.476190476192],
        1e-8,
    );
    check(&r, "E_kin", &t, &[-5029.68, -5410.0, -4929.296, -4390.0], 1e-8);
    event(&r, "t_lock", 0.595, 1e-8);
}

#[test]
fn import_errors_have_a_place_and_quote_the_equation() {
    // a unit slip in an imported model is caught when it is read
    let bad = RC_STEP.replace("'i' = ('V' - 'v_C') / 'rc'.'R';", "'i' = ('V' - 'v_C') * 'rc'.'R';");
    let errs = basemodelica::import(&bad).expect_err("does not balance");
    assert_eq!(errs.len(), 1, "{}", Report(&errs));
    assert_eq!(errs[0].code, "UNIT-MISMATCH");
    assert_eq!(
        errs[0].message,
        "In 'RCStep', the equation “'i' = ('V' - 'v_C') * 'rc'.'R'” does not balance its units: \
         the left side is in A, the right side in m4.kg2.s-6.A-3."
    );
    assert_eq!((errs[0].span.line, errs[0].span.col), (22, 5));
    for (text, code, line, start) in [
        ("package 'P'\nend 'P';", "NO-MODEL", 1, "the text defines no model"),
        ("model 'M'\n  Real 'x'[2];\nend 'M';", "ARRAY", 2, "'x' has an array subscript"),
        (
            "model 'M'\n  Real 'x';\nequation\n  'x' = 'f'(1);\nend 'M';",
            "UNKNOWN-FUNCTION",
            4,
            "'f()' cannot be used",
        ),
        (
            "package 'P'\n  function 'f'\n    input Real 'a';\n    output Real 'b';\n  algorithm\n    \
             'b' := 'f'('a');\n  end 'f';\n  model 'M'\n    Real 'x';\n  equation\n    'x' = 'f'(1);\n  \
             end 'M';\nend 'P';",
            "FUNCTION",
            6,
            "the function 'f' calls itself",
        ),
        (
            "package 'P'\n  record 'R'\n    Real 'a';\n  end 'R';\n  model 'M'\n    parameter 'R' 'r'('z' = 1);\n  \
             end 'M';\nend 'P';",
            "RECORD",
            6,
            "the record 'R' has no field 'z'",
        ),
        (
            "model 'M'\n  Real 'x';\nequation\n  'x' = 'y';\nend 'M';",
            "UNKNOWN-NAME",
            4,
            "'y' is not a variable",
        ),
        (
            "model 'M'\n  Real 'x';\nequation\n  for 'k' in 1:2 loop\n  end for;\nend 'M';",
            "UNSUPPORTED",
            4,
            "'for' loops are not supported",
        ),
    ] {
        let errs = basemodelica::import(text).expect_err(text);
        assert!(
            errs.iter()
                .any(|e| e.code == code && e.span.line == line && e.message.starts_with(start)),
            "no [{code}] on line {line} starting «{start}»:\n{}\n{text}",
            Report(&errs)
        );
    }
}

#[test]
fn an_imported_model_prints_and_parses_back() {
    for text in [RC_STEP, RL_STEP, THERMAL_MASS, TWO_MASSES, BATTERY_CC, VOLTAGE_LIMIT, CLUTCH] {
        let def = basemodelica::import(text).expect("imports");
        let printed = lsim_lang::to_text(&def);
        let back =
            lsim_lang::parse(&printed).unwrap_or_else(|e| panic!("{}\n{printed}", Report(&e)));
        assert_eq!(back, vec![def], "{printed}");
    }
}
