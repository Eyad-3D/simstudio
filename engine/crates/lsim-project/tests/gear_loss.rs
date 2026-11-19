//! Where a gear's efficiency acts: on the torque its teeth carry. A motor's
//! rotor and the gear's input side take part of the motor's torque to speed
//! up; that part never reaches the teeth, so the mesh's friction (Coulomb's,
//! proportional to the tooth force) does not act on it. Today's engine
//! lumps the rotor with the rest of the driveline and applies the
//! efficiency to the motor's whole torque; the golden comparison's gear
//! channels differ by that (golden/README.md). This test pins the
//! library's final drive to the exact answer of the mesh law, and to the
//! limit where the gear carries nothing.

mod common;

use common::*;

const RATIO: f64 = 12.8;
const ETA: f64 = 0.9;
const T: f64 = 100.0; // the motor's torque, N·m
const J_ROTOR: f64 = 0.045; // the rotor, kg·m²
const J_IN: f64 = 0.01; // the gear's input side
const J_OUT: f64 = 0.02; // its output side
const T_END: f64 = 2.0;

/// The motor (a constant torque on its rotor) through the library's final
/// drive to a load inertia `j_load`, from rest.
fn final_drive_model(j_load: f64) -> lsim_ir::ComponentDef {
    model(
        "FinalDriveSpinUp",
        vec![
            part("drive", "Rotational.ConstantTorque", &[("tau", T)]),
            part("rotor", "Rotational.Inertia", &[("J", J_ROTOR)]),
            part(
                "fd",
                "Blocks.FinalDrive",
                &[
                    ("ratio", RATIO),
                    ("efficiency_pct", ETA),
                    ("inertia_in_kgm2", J_IN),
                    ("inertia_out_kgm2", J_OUT),
                    // the efficiency's blend through zero power: far below
                    // anything this run passes after its first instant
                    ("p_dir", 1e-9),
                ],
            ),
            part("load", "Rotational.Inertia", &[("J", j_load)]),
        ],
        &[("drive.flange", "rotor.a"), ("rotor.b", "fd.flange_in"), ("fd.flange_out", "load.a")],
    )
}

/// The exact answer with the loss on the mesh's torque: the output's
/// acceleration and the loss at `t`. Newton on the input side: the teeth
/// carry τ = T − (J_rotor + J_in)·i·α; on the output side
/// (J_out + J_load)·α = i·η·τ.
fn mesh_law(j_load: f64, t: f64) -> (f64, f64, f64) {
    let (j1, j2) = (J_ROTOR + J_IN, J_OUT + j_load);
    let alpha = RATIO * ETA * T / (j2 + ETA * RATIO * RATIO * j1);
    let tau = T - j1 * RATIO * alpha;
    (alpha, tau, (1.0 - ETA) * tau * RATIO * alpha * t * t / 2.0)
}

/// Today's rule: the driveline lumped into one inertia, the efficiency on
/// the motor's whole torque.
fn lumped_rule(j_load: f64, t: f64) -> (f64, f64) {
    let (j1, j2) = (J_ROTOR + J_IN, J_OUT + j_load);
    let alpha = RATIO * ETA * T / (j2 + RATIO * RATIO * j1);
    (alpha, (1.0 - ETA) * T * RATIO * alpha * t * t / 2.0)
}

#[test]
fn a_gear_loses_its_efficiency_on_the_torque_its_teeth_carry() {
    let lib = lib();
    // a car-like load (a 1 500 kg car on 0.35 m wheels, at the output) and
    // a gear driving nothing but its own output side
    for j_load in [1500.0 * 0.35 * 0.35, 0.0] {
        let (_b, res) = run(&lib, &final_drive_model(j_load), T_END, 0.5, 1e-10);
        let (alpha, tau, loss) = mesh_law(j_load, T_END);
        let (alpha_l, loss_l) = lumped_rule(j_load, T_END);
        let w = last(&res, "fd.w_out");
        let (_, lost, ..) = books(&res, "fd");
        let lost = *lost.last().unwrap();
        println!(
            "load {j_load:.2} kg·m²: output speed {w:.9} (mesh law {:.9}, lumped {:.9}); loss \
             {lost:.6} J (mesh law {loss:.6} J on {tau:.4} N·m at the teeth, lumped {loss_l:.6} J \
             on {T} N·m)",
            alpha * T_END,
            alpha_l * T_END,
        );
        assert!((w - alpha * T_END).abs() < 1e-8 * alpha * T_END, "speed {w}");
        // the loss's rate at every output time, and the teeth's torque
        for (k, t) in res.times.iter().enumerate().skip(1) {
            let rate = (1.0 - ETA) * tau * RATIO * alpha * t;
            let got = res.channel("fd.sig_losses").unwrap()[k];
            assert!((got - rate).abs() < 1e-8 * rate.max(1e-3), "loss rate at {t}: {got}");
            let got = res.channel("fd.tau_x").unwrap()[k];
            assert!((got - tau).abs() < 1e-8 * T, "the teeth's torque at {t}: {got}");
        }
        // its books: IDA integrates them alongside, at order 2 once the
        // states (here linear in time) settle, so the loss (an integral of
        // t) to round-off; before IDAS's order decrease was decided on the
        // states' and the books' errors together (lsim-sundials-sys), the
        // books' quadratures held it at order 1 and only 0.26·√rtol of the
        // loss came right (golden/README.md: found on the way)
        println!("the loss's book {:.1e} off", (lost - loss).abs() / loss.max(1e-3));
        assert!((lost - loss).abs() < 1e-10 * loss.max(1e-3), "loss {lost} against {loss}");
        // today's rule is a different answer, by the inertia term
        assert!((loss_l - loss).abs() > 1e-3 * loss_l);
        books_close(&res, 1e-9);
    }
    // driving nothing, the teeth carry only what the output side needs:
    // 0.25 % of the motor's torque, and the loss is that small; today's rule
    // loses a tenth of the motor's power in a gear that passes almost none
    let (_, tau, loss) = mesh_law(0.0, T_END);
    let (_, loss_l) = lumped_rule(0.0, T_END);
    assert!(tau < 0.006 * T && loss < 0.006 * loss_l, "{tau} N·m, {loss} J against {loss_l} J");
}
