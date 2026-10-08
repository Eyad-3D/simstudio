//! The Stage 1 entry points of structural analysis, kept for callers that
//! used them; the analysis itself lives in [`crate::graph`],
//! [`crate::index`], [`crate::causal`] and [`crate::pipeline`].

pub use crate::graph::scc;
use crate::graph::{Bipartite, NONE, hopcroft_karp};

/// Options for causalisation.
#[derive(Clone, Debug, Default)]
pub struct CausalOptions {
    /// keep every block implicit (iteration variables and residuals), even
    /// when it could be solved explicitly: exercises the DAE path
    pub force_implicit: bool,
}

/// A maximum matching of equations (`inc[e]`: the unknowns of equation
/// `e`) to `n_unk` unknowns, by Hopcroft–Karp: each equation's unknown and
/// each unknown's equation.
pub fn max_matching(n_unk: usize, inc: &[Vec<usize>]) -> (Vec<Option<usize>>, Vec<Option<usize>>) {
    let g = Bipartite::from_rows(n_unk, inc);
    let m = hopcroft_karp(&g);
    let opt = |x: usize| (x != NONE).then_some(x);
    (m.row.into_iter().map(opt).collect(), m.col.into_iter().map(opt).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_and_blocks() {
        // e0: u0 ; e1: u0,u1 ; e2: u1,u2 ; e3: u2,u3 ; e4: u3,u2 (a 2-block)
        let inc = vec![vec![0], vec![0, 1], vec![1, 2], vec![2, 3], vec![2, 3]];
        let (me, mu) = max_matching(4, &inc);
        assert_eq!(me[0], Some(0));
        assert!(me.iter().filter(|m| m.is_none()).count() == 1);
        assert!(mu.iter().all(|m| m.is_some()));
        // a 3-cycle and a tail
        let adj = vec![vec![1], vec![2], vec![0], vec![0]];
        let comps = scc(&adj);
        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0].len(), 3);
        assert_eq!(comps[1], vec![3]);
    }

    #[test]
    fn augmenting_paths_find_a_perfect_matching() {
        // the greedy start matches e0-u0, leaving e1 (only u0) to an augmenting path
        let inc = vec![vec![0, 1], vec![0], vec![1, 2]];
        let (me, _) = max_matching(3, &inc);
        assert_eq!(me, vec![Some(1), Some(0), Some(2)]);
    }
}
