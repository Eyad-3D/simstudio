//! The compiled model as the solver sees it.
//!
//! lsim-codegen implements [`ModelFunctions`] with JIT-compiled machine
//! code; tests and the solver's own unit tests may implement it by hand.
//! Every function is pure: it reads its inputs, writes its outputs and uses
//! the caller's `work` buffer (of [`Layout::n_work`] values) as scratch, so
//! one compiled model serves any number of simulations at once (parallel
//! sweeps give each its own buffers).
//!
//! Vector layouts:
//!
//! * `y = [x; z]`: states then iteration variables, `n_x + n_z` values;
//! * `p`: parameters, `n_p` values, SI, in [`crate::FlatSystem::params`] order;
//! * `d`: discrete variables, in [`crate::PreparedModel::discretes`] order;
//! * `u`: inputs, in [`crate::PreparedModel::inputs`] order;
//! * residual output `[x'; g]`: the state derivatives, then the residuals.

/// Sizes of the compiled model's vectors.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Layout {
    /// states
    pub n_x: usize,
    /// iteration variables
    pub n_z: usize,
    /// parameters
    pub n_p: usize,
    /// discrete variables
    pub n_d: usize,
    /// inputs
    pub n_u: usize,
    /// zero-crossing functions
    pub n_roots: usize,
    /// `when` clauses
    pub n_whens: usize,
    /// all flat variables (the recorded channels)
    pub n_vars: usize,
    /// scratch values each call needs
    pub n_work: usize,
}

impl Layout {
    /// n_x + n_z
    pub fn n_y(&self) -> usize {
        self.n_x + self.n_z
    }
}

/// What every model function reads.
#[derive(Clone, Copy, Debug)]
pub struct EvalInput<'a> {
    /// time, s
    pub t: f64,
    /// `[x; z]`
    pub y: &'a [f64],
    /// parameters
    pub p: &'a [f64],
    /// discrete variables
    pub d: &'a [f64],
    /// inputs
    pub u: &'a [f64],
}

/// A compiled model.
pub trait ModelFunctions: Send + Sync {
    /// The vector sizes.
    fn layout(&self) -> &Layout;

    /// `out = [x'; g]` at the input point.
    fn residual(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]);

    /// `out = (∂[x'; g]/∂y) · v`: the exact Jacobian times a vector
    /// (forward-mode differentiation of the generated code).
    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], work: &mut [f64], out: &mut [f64]);

    /// The zero-crossing functions.
    fn roots(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]);

    /// Every flat variable's value (aliases included), for the results.
    fn vars(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]);

    /// Applies the `when` clauses whose `fired` entry is non-zero: on entry
    /// `d_out` holds the discrete values before the event, on return the
    /// values after it.
    fn when(&self, inp: &EvalInput<'_>, fired: &[f64], work: &mut [f64], d_out: &mut [f64]);

    /// Start values: `y0` (states' initial values, iteration variables'
    /// guesses) and `d0`, from the parameters.
    fn start(&self, p: &[f64], y0: &mut [f64], d0: &mut [f64]);

    /// The dense Jacobian `∂[x'; g]/∂y`, column-major (`n_y × n_y`), from
    /// one [`Self::jvp`] per column. Code generators may override it with a
    /// sparse, coloured evaluation.
    fn jacobian_dense(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        let n = self.layout().n_y();
        let mut v = vec![0.0; n];
        for j in 0..n {
            v[j] = 1.0;
            self.jvp(inp, &v, work, &mut out[j * n..(j + 1) * n]);
            v[j] = 0.0;
        }
    }
}
