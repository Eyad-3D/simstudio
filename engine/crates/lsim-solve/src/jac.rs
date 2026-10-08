//! The Jacobian `∂[x'; g]/∂y` from Jacobian-vector products: its structure
//! (with the diagonal, which every Newton matrix needs), a column colouring
//! (columns that share no row are evaluated by one product), the band
//! widths, and the coloured evaluation into column-compressed values.

use lsim_ir::runtime::{EvalInput, ModelFunctions, SparsityPattern};

/// The Jacobian's structure and colouring.
#[derive(Clone, Debug)]
pub struct JacStructure {
    /// n_y
    pub n: usize,
    /// column starts (n + 1)
    pub col_ptr: Vec<usize>,
    /// row indices, increasing within each column; every diagonal entry is
    /// present
    pub row_idx: Vec<usize>,
    /// the columns of each colour
    pub colours: Vec<Vec<usize>>,
    /// lower band width (largest i - j)
    pub ml: usize,
    /// upper band width (largest j - i)
    pub mu: usize,
}

impl JacStructure {
    /// From a pattern (or dense when there is none), with the diagonal
    /// added.
    pub fn new(pattern: Option<&SparsityPattern>, n: usize) -> Self {
        let mut cols: Vec<Vec<usize>> = match pattern {
            Some(p) if p.n == n => {
                (0..n).map(|j| p.row_idx[p.col_ptr[j]..p.col_ptr[j + 1]].to_vec()).collect()
            }
            _ => (0..n).map(|_| (0..n).collect()).collect(),
        };
        for (j, c) in cols.iter_mut().enumerate() {
            c.push(j);
            c.sort_unstable();
            c.dedup();
        }
        Self::from_columns(n, cols)
    }

    fn from_columns(n: usize, cols: Vec<Vec<usize>>) -> Self {
        let mut col_ptr = vec![0];
        let mut row_idx = vec![];
        let (mut ml, mut mu) = (0, 0);
        for (j, c) in cols.iter().enumerate() {
            for &i in c {
                if i > j {
                    ml = ml.max(i - j);
                } else {
                    mu = mu.max(j - i);
                }
            }
            row_idx.extend_from_slice(c);
            col_ptr.push(row_idx.len());
        }
        let colours = colour(n, &col_ptr, &row_idx);
        JacStructure { n, col_ptr, row_idx, colours, ml, mu }
    }

    /// Number of stored entries.
    pub fn nnz(&self) -> usize {
        self.row_idx.len()
    }

    /// Adds entries (row, column) and recolours.
    pub fn extend(&mut self, extra: &[(usize, usize)]) {
        let mut cols: Vec<Vec<usize>> = (0..self.n)
            .map(|j| self.row_idx[self.col_ptr[j]..self.col_ptr[j + 1]].to_vec())
            .collect();
        for &(i, j) in extra {
            cols[j].push(i);
        }
        for c in &mut cols {
            c.sort_unstable();
            c.dedup();
        }
        *self = Self::from_columns(self.n, cols);
    }

    /// The Jacobian's values at `inp`, in the order of `row_idx`, from one
    /// Jacobian-vector product per colour. `seed` and `out` are scratch of
    /// n values.
    pub fn eval(
        &self,
        m: &dyn ModelFunctions,
        inp: &EvalInput<'_>,
        work: &mut [f64],
        seed: &mut [f64],
        out: &mut [f64],
        values: &mut [f64],
    ) {
        for cols in &self.colours {
            for &j in cols {
                seed[j] = 1.0;
            }
            m.jvp(inp, seed, work, out);
            for &j in cols {
                seed[j] = 0.0;
                for k in self.col_ptr[j]..self.col_ptr[j + 1] {
                    values[k] = out[self.row_idx[k]];
                }
            }
        }
    }

    /// The entries the structure misses at `inp` (non-zero in the full
    /// Jacobian, computed one column at a time): a check of a pattern that
    /// came from elsewhere.
    pub fn missing(
        &self,
        m: &dyn ModelFunctions,
        inp: &EvalInput<'_>,
        work: &mut [f64],
    ) -> Vec<(usize, usize)> {
        let n = self.n;
        let mut seed = vec![0.0; n];
        let mut out = vec![0.0; n];
        let mut miss = vec![];
        for j in 0..n {
            seed[j] = 1.0;
            m.jvp(inp, &seed, work, &mut out);
            seed[j] = 0.0;
            let rows = &self.row_idx[self.col_ptr[j]..self.col_ptr[j + 1]];
            for (i, v) in out.iter().enumerate() {
                if *v != 0.0 && rows.binary_search(&i).is_err() {
                    miss.push((i, j));
                }
            }
        }
        miss
    }
}

/// Greedy column colouring (columns in order, smallest free colour): two
/// columns sharing a row never share a colour.
fn colour(n: usize, col_ptr: &[usize], row_idx: &[usize]) -> Vec<Vec<usize>> {
    // rows → columns
    let mut row_cols: Vec<Vec<usize>> = vec![vec![]; n];
    for j in 0..n {
        for &i in &row_idx[col_ptr[j]..col_ptr[j + 1]] {
            row_cols[i].push(j);
        }
    }
    let mut colour_of = vec![usize::MAX; n];
    let mut forbidden: Vec<usize> = vec![usize::MAX; n + 1];
    let mut n_colours = 0;
    for j in 0..n {
        for &i in &row_idx[col_ptr[j]..col_ptr[j + 1]] {
            for &k in &row_cols[i] {
                if colour_of[k] != usize::MAX {
                    forbidden[colour_of[k]] = j;
                }
            }
        }
        let c = (0..).find(|&c| forbidden[c] != j).expect("a free colour");
        colour_of[j] = c;
        n_colours = n_colours.max(c + 1);
    }
    let mut colours = vec![vec![]; n_colours];
    for (j, c) in colour_of.into_iter().enumerate() {
        colours[c].push(j);
    }
    colours
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tridiagonal_pattern_needs_three_colours() {
        let n: usize = 10;
        let mut col_ptr = vec![0];
        let mut row_idx = vec![];
        for j in 0..n {
            for i in j.saturating_sub(1)..(j + 2).min(n) {
                row_idx.push(i);
            }
            col_ptr.push(row_idx.len());
        }
        let s = JacStructure::new(Some(&SparsityPattern { n, col_ptr, row_idx }), n);
        assert_eq!(s.colours.len(), 3);
        assert_eq!((s.ml, s.mu), (1, 1));
        // no two columns of one colour share a row
        for cols in &s.colours {
            let mut seen = vec![false; n];
            for &j in cols {
                for &i in &s.row_idx[s.col_ptr[j]..s.col_ptr[j + 1]] {
                    assert!(!seen[i]);
                    seen[i] = true;
                }
            }
        }
    }

    #[test]
    fn no_pattern_means_dense() {
        let s = JacStructure::new(None, 4);
        assert_eq!(s.nnz(), 16);
        assert_eq!(s.colours.len(), 4);
    }
}
