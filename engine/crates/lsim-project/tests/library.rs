//! Every library component flattens and balances its units, standing alone.

#[test]
fn every_component_flattens_and_balances_its_units() {
    let lib = lsim_lib::library();
    let mut faults = vec![];
    for def in lib.components.values() {
        match lsim_prep::flatten::flatten(&lib, def) {
            Err(e) => faults.extend(e.iter().map(|d| format!("{}: {d}", def.name))),
            Ok(flat) => {
                for d in lsim_prep::units_check::check(&flat, &lib, def) {
                    faults.push(format!("{}: {d}", def.name));
                }
            }
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}
