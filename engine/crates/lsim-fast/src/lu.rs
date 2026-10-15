//! A small dense LU factorisation with partial pivoting.
//!
//! Fast mode's matrices are small (the inverse model's remaining states and
//! its iteration variables: a handful to a few dozen), so a plain
//! column-major Doolittle factorisation is the fastest choice and needs no
//! dependency.

/// The matrix is singular (to working precision) at this column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Singular(pub usize);

/// `P A = L U`, stored in place.
#[derive(Clone, Debug, Default)]
pub struct Lu {
    n: usize,
    /// column-major, L below the diagonal (unit diagonal implied), U on and above
    a: Vec<f64>,
    piv: Vec<usize>,
}

impl Lu {
    /// An empty factorisation for `n × n` matrices.
    pub fn new(n: usize) -> Lu {
        Lu { n, a: vec![0.0; n * n], piv: vec![0; n] }
    }

    /// The size.
    pub fn n(&self) -> usize {
        self.n
    }

    /// Factorises the column-major `n × n` matrix `m` (a matrix holding a
    /// value that is not a number counts as singular).
    pub fn factor(&mut self, m: &[f64]) -> Result<(), Singular> {
        let n = self.n;
        debug_assert_eq!(m.len(), n * n);
        if let Some(i) = m.iter().position(|v| !v.is_finite()) {
            return Err(Singular(i / n.max(1)));
        }
        self.a.copy_from_slice(m);
        let a = &mut self.a;
        for k in 0..n {
            // pivot: the largest entry of column k at or below the diagonal
            let mut p = k;
            let mut best = a[k * n + k].abs();
            for i in k + 1..n {
                let v = a[k * n + i].abs();
                if v > best {
                    best = v;
                    p = i;
                }
            }
            if best == 0.0 || !best.is_finite() {
                return Err(Singular(k));
            }
            self.piv[k] = p;
            if p != k {
                for j in 0..n {
                    a.swap(j * n + k, j * n + p);
                }
            }
            let d = a[k * n + k];
            for i in k + 1..n {
                a[k * n + i] /= d;
            }
            for j in k + 1..n {
                let f = a[j * n + k];
                if f != 0.0 {
                    for i in k + 1..n {
                        a[j * n + i] -= a[k * n + i] * f;
                    }
                }
            }
        }
        Ok(())
    }

    /// Solves `A x = b` in place.
    pub fn solve(&self, b: &mut [f64]) {
        let n = self.n;
        let a = &self.a;
        for k in 0..n {
            let p = self.piv[k];
            if p != k {
                b.swap(k, p);
            }
        }
        for j in 0..n {
            let bj = b[j];
            if bj != 0.0 {
                for i in j + 1..n {
                    b[i] -= a[j * n + i] * bj;
                }
            }
        }
        for j in (0..n).rev() {
            b[j] /= a[j * n + j];
            let bj = b[j];
            if bj != 0.0 {
                for i in 0..j {
                    b[i] -= a[j * n + i] * bj;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_a_system_that_needs_pivoting() {
        // column-major [[0, 2, 1], [1, 1, 0], [3, 0, 4]] as rows
        let rows = [[0.0, 2.0, 1.0], [1.0, 1.0, 0.0], [3.0, 0.0, 4.0]];
        let mut m = vec![0.0; 9];
        for (i, r) in rows.iter().enumerate() {
            for (j, v) in r.iter().enumerate() {
                m[j * 3 + i] = *v;
            }
        }
        let x = [1.5, -2.0, 0.25];
        let mut b: Vec<f64> =
            rows.iter().map(|r| r.iter().zip(&x).map(|(a, b)| a * b).sum()).collect();
        let mut lu = Lu::new(3);
        lu.factor(&m).unwrap();
        lu.solve(&mut b);
        for k in 0..3 {
            assert!((b[k] - x[k]).abs() < 1e-14, "{b:?}");
        }
    }

    #[test]
    fn reports_a_singular_matrix() {
        let mut lu = Lu::new(2);
        assert_eq!(lu.factor(&[1.0, 2.0, 2.0, 4.0]), Err(Singular(1)));
        assert!(lu.factor(&[1.0, f64::NAN, 0.0, 1.0]).is_err());
    }
}
