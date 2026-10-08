//! The structure key: a digest of everything that shapes the generated
//! code, and nothing that does not (runtime parameter values, labels,
//! origins). Two models with the same key share compiled code.

use lsim_ir::prepared::PreparedModel;
use sha2::{Digest, Sha256};

/// SHA-256 (hex) of the model's structure.
pub fn structure_key(m: &PreparedModel) -> String {
    let assignments: Vec<_> = m.assignments.iter().map(|a| (&a.target, &a.expr)).collect();
    let residuals: Vec<_> = m.residuals.iter().map(|r| &r.expr).collect();
    let crossings: Vec<_> = m.zero_crossings.iter().map(|z| &z.expr).collect();
    let whens: Vec<_> = m.whens.iter().map(|w| (w.crossing, w.direction, &w.assign)).collect();
    let structural: Vec<_> = m
        .flat
        .params
        .iter()
        .filter(|p| p.structural)
        .map(|p| (&p.name, p.value.to_bits()))
        .collect();
    let modes: Vec<_> = m.modes.iter().map(|x| (x.var, &x.relation, x.crossing)).collect();
    let init = (
        &m.init.unknowns,
        &m.init.guesses,
        m.init.assignments.iter().map(|a| (&a.target, &a.expr)).collect::<Vec<_>>(),
        m.init.residuals.iter().map(|r| &r.expr).collect::<Vec<_>>(),
        &m.init.discrete_starts,
    );
    let limits: Vec<_> = m.limits.iter().map(|l| (&l.value, &l.lo, &l.hi)).collect();
    let sizes = (m.flat.vars.len(), m.flat.params.len(), &m.aliases);
    let text = serde_json::to_string(&(
        (&m.states, &m.algebraics, &m.discretes, &m.inputs),
        assignments,
        residuals,
        crossings,
        whens,
        structural,
        sizes,
        (modes, init, limits),
    ))
    .expect("the IR serialises");
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
