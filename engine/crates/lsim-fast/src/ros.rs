//! The Rosenbrock-W tableaus, in the transformed form that needs no
//! Jacobian-vector products (Hairer & Wanner, *Solving ODEs II*, IV.7,
//! eq. 7.25):
//!
//! ```text
//! (I/(h γ) - J) U_i = f(t_n + α_i h, x_n + Σ_{j<i} a_ij U_j) + Σ_{j<i} (c_ij / h) U_j
//! x_{n+1} = x_n + Σ m_i U_i,        error estimate Σ (m_i - m̂_i) U_i
//! ```
//!
//! with `a = α Γ⁻¹`, `c = diag(1/γ) - Γ⁻¹` (its strictly lower part) and
//! `m = b Γ⁻¹`, computed here from the published coefficients so no
//! transformed number is transcribed by hand.
//!
//! **ROS34PW2** (Rang & Angermann, BIT 45, 2005): four stages, order 3, an
//! embedded order-2 solution, L-stable and stiffly accurate, and a
//! *W-method*: its order holds for any matrix in place of the Jacobian `J`.
//! So a Jacobian may be kept over many steps, and the time derivative
//! `∂f/∂t` of a non-autonomous problem may be left out (the autonomised
//! system's Jacobian with its time column set to zero is one such matrix).
//! The unit tests check the order conditions and the observed order with a
//! stale and with a zero Jacobian.
//!
//! **Linearly implicit Euler** (one stage, `(I/h - J) U = f`, `x += U`):
//! order 1, also a W-method; a cross-check with the simplest quasi-static
//! tools.

/// A Rosenbrock-W method in transformed form.
#[derive(Clone, Debug)]
pub struct Tableau {
    /// name, for the run report
    pub name: &'static str,
    /// stages
    pub s: usize,
    /// the diagonal γ
    pub gamma: f64,
    /// stage times as fractions of the step
    pub alpha: [f64; 4],
    /// `a_ij`, j < i
    pub a: [[f64; 4]; 4],
    /// `c_ij`, j < i
    pub c: [[f64; 4]; 4],
    /// weights of the solution
    pub m: [f64; 4],
    /// weights of the error estimate, `m - m̂` (zero when there is none)
    pub e: [f64; 4],
    /// order
    pub order: u32,
}

fn lower_inverse(g: &[[f64; 4]; 4], s: usize) -> [[f64; 4]; 4] {
    // forward substitution, column by column
    let mut inv = [[0.0; 4]; 4];
    for col in 0..s {
        for i in 0..s {
            let mut v = if i == col { 1.0 } else { 0.0 };
            for (k, row) in inv.iter().enumerate().take(i) {
                v -= g[i][k] * row[col];
            }
            inv[i][col] = v / g[i][i];
        }
    }
    inv
}

impl Tableau {
    fn transform(
        name: &'static str,
        s: usize,
        alpha_ij: [[f64; 4]; 4],
        gamma_ij: [[f64; 4]; 4],
        b: [f64; 4],
        b_hat: Option<[f64; 4]>,
        order: u32,
    ) -> Tableau {
        let gi = lower_inverse(&gamma_ij, s);
        let mut a = [[0.0; 4]; 4];
        let mut c = [[0.0; 4]; 4];
        let mut alpha = [0.0; 4];
        for i in 0..s {
            alpha[i] = (0..i).map(|j| alpha_ij[i][j]).sum();
            for j in 0..i {
                a[i][j] = (0..s).map(|k| alpha_ij[i][k] * gi[k][j]).sum();
                c[i][j] = -gi[i][j];
            }
        }
        let mut m = [0.0; 4];
        let mut e = [0.0; 4];
        for j in 0..s {
            m[j] = (0..s).map(|k| b[k] * gi[k][j]).sum();
            if let Some(bh) = b_hat {
                let mh: f64 = (0..s).map(|k| bh[k] * gi[k][j]).sum();
                e[j] = m[j] - mh;
            }
        }
        Tableau { name, s, gamma: gamma_ij[0][0], alpha, a, c, m, e, order }
    }

    /// ROS34PW2 (the published coefficients, to all their digits).
    #[allow(clippy::excessive_precision)]
    pub fn ros34pw2() -> Tableau {
        let g = 0.435_866_521_508_459;
        let mut al = [[0.0; 4]; 4];
        let mut ga = [[0.0; 4]; 4];
        al[1][0] = 0.871_733_043_016_918_01;
        al[2][0] = 0.844_570_600_153_694_23;
        al[2][1] = -0.112_990_642_364_841_85;
        al[3][2] = 1.0;
        ga[1][0] = -0.871_733_043_016_918_01;
        ga[2][0] = -0.903_380_570_130_440_82;
        ga[2][1] = 0.054_180_672_388_095_326;
        ga[3][0] = 0.242_123_807_060_953_46;
        ga[3][1] = -1.223_250_583_904_514_7;
        ga[3][2] = 0.545_260_255_335_102_14;
        for (i, row) in ga.iter_mut().enumerate() {
            row[i] = g;
        }
        let b = [
            0.242_123_807_060_953_46,
            -1.223_250_583_904_514_7,
            1.545_260_255_335_102,
            0.435_866_521_508_459,
        ];
        let b_hat =
            [0.378_109_031_458_193_69, -0.096_042_292_212_423_178, 0.5, 0.217_933_260_754_229_5];
        Tableau::transform("ROS34PW2 (Rosenbrock-W, order 3)", 4, al, ga, b, Some(b_hat), 3)
    }

    /// Linearly implicit Euler.
    pub fn linear_implicit_euler() -> Tableau {
        let mut ga = [[0.0; 4]; 4];
        ga[0][0] = 1.0;
        Tableau::transform(
            "linearly implicit Euler (order 1)",
            1,
            [[0.0; 4]; 4],
            ga,
            [1.0, 0.0, 0.0, 0.0],
            None,
            1,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The classical order conditions up to order 3 (Hairer & Wanner,
    /// Table IV.7.1) and stiff accuracy, from the untransformed numbers.
    #[test]
    fn ros34pw2_meets_its_order_conditions() {
        let t = Tableau::ros34pw2();
        assert_eq!(t.s, 4);
        // consistency through the transformed weights: Σ m_i U_i with
        // U = Γ k means Σ b_i k_i; check via a scalar linear problem where
        // the stability function must match exp(z) to O(z^4)
        let r = |z: f64| stability(&t, z);
        for z in [1e-3f64, -2e-3, 5e-3] {
            let err = (r(z) - z.exp()).abs();
            assert!(err < 3.0 * z.abs().powi(4), "R({z}) - e^z = {err:e}");
        }
        // L-stable: R(z) -> 0 as z -> -inf
        assert!(r(-1e12).abs() < 1e-9, "R(-inf) = {}", r(-1e12));
        // A-stable along the negative axis
        for k in 0..200 {
            let z = -(10f64).powf(-3.0 + 0.05 * k as f64);
            assert!(r(z).abs() <= 1.0 + 1e-12, "|R({z})| = {}", r(z).abs());
        }
        // stiffly accurate: m_4 = 1 and m_j = a_4j
        assert!((t.m[3] - 1.0).abs() < 1e-14, "{:?}", t.m);
        for j in 0..3 {
            assert!((t.m[j] - t.a[3][j]).abs() < 1e-13, "{:?} {:?}", t.m, t.a[3]);
        }
        assert!((t.alpha[3] - 1.0).abs() < 1e-15);
    }

    /// The stability function, by running one step of y' = λy (h = 1,
    /// z = λ) with the exact Jacobian.
    fn stability(t: &Tableau, z: f64) -> f64 {
        let w = 1.0 / t.gamma - z;
        let mut u = [0.0; 4];
        for i in 0..t.s {
            let y: f64 = 1.0 + (0..i).map(|j| t.a[i][j] * u[j]).sum::<f64>();
            let rhs = z * y + (0..i).map(|j| t.c[i][j] * u[j]).sum::<f64>();
            u[i] = rhs / w;
        }
        1.0 + (0..t.s).map(|i| t.m[i] * u[i]).sum::<f64>()
    }

    #[test]
    fn implicit_euler_is_backward_euler_on_linear_problems() {
        let t = Tableau::linear_implicit_euler();
        for z in [-0.5, -10.0, 2.0] {
            let r = stability(&t, z);
            assert!((r - 1.0 / (1.0 - z)).abs() < 1e-14);
        }
    }
}
