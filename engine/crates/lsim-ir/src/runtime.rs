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

use serde::{Deserialize, Serialize};

/// The sparsity of `∂[x'; g]/∂y`, column-compressed: the rows of column
/// `j` are `row_idx[col_ptr[j]..col_ptr[j + 1]]`, increasing. Produced by
/// lsim-prep (work package 2), used by lsim-codegen to colour and fill
/// the Jacobian and by lsim-solve's sparse LU (work packages 3 and 4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SparsityPattern {
    /// n_y
    pub n: usize,
    /// column starts, n + 1 values
    pub col_ptr: Vec<usize>,
    /// row indices
    pub row_idx: Vec<usize>,
}

impl SparsityPattern {
    /// The number of structural non-zeros.
    pub fn nnz(&self) -> usize {
        self.row_idx.len()
    }

    /// The rows of column `j`.
    pub fn col(&self, j: usize) -> &[usize] {
        &self.row_idx[self.col_ptr[j]..self.col_ptr[j + 1]]
    }

    /// The pattern of a dense `n × n` matrix.
    pub fn dense(n: usize) -> Self {
        SparsityPattern {
            n,
            col_ptr: (0..=n).map(|j| j * n).collect(),
            row_idx: (0..n * n).map(|k| k % n).collect(),
        }
    }
}

/// One axis of one table read in the model, which the run loop watches
/// against the table's data range (today's "outside the data" handling:
/// an `Error` axis stops the run where it leaves its data, the others are
/// booked as time spent outside). Its guard function
/// ([`ModelFunctions::table_guards`]) is positive inside the data and
/// negative outside: `min(a - lo, hi - a)` of the axis argument `a`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableGuard {
    /// the table's index in [`crate::FlatSystem::tables`]
    pub table: u32,
    /// 0: the first axis, 1: the second
    pub axis: u8,
    /// the axis's setting
    pub outside: crate::table::Outside,
}

/// The compiled initialisation problem ([`crate::InitSystem`]): Newton
/// iterates on `w` (passed as [`EvalInput::y`]) until the residuals
/// vanish, then [`InitFunctions::finish`] gives the model's start vector.
pub trait InitFunctions: Send + Sync {
    /// The number of unknowns w (and residuals).
    fn n_w(&self) -> usize;
    /// The unknowns' start guesses.
    fn guess(&self, p: &[f64], w0: &mut [f64]);
    /// The residuals at `w`.
    fn residual(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]);
    /// The exact Jacobian of the residuals times a vector.
    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], work: &mut [f64], out: &mut [f64]);
    /// The structure [`InitFunctions::jacobian_sparse`] fills.
    fn sparsity(&self) -> &SparsityPattern;
    /// The Jacobian's values (column-compressed, in [`InitFunctions::sparsity`]'s order).
    fn jacobian_sparse(&self, inp: &EvalInput<'_>, work: &mut [f64], values: &mut [f64]);
    /// The model's `y0 = [x; z]` from the solved `w` (every entry the
    /// initialisation does not compute keeps its start value).
    fn finish(&self, inp: &EvalInput<'_>, work: &mut [f64], y0: &mut [f64]);
}

/// A block with its own sample clock, run outside the equations: a Script
/// block (sandboxed Python), an FMU for co-simulation, a digital
/// controller. Between its ticks its outputs hold (they are discrete
/// variables of the model); at a tick the run loop stops the integrator
/// exactly there, reads the block's inputs, calls [`DiscreteBlock::tick`]
/// and restarts the integrator only if an output changed. The host (the
/// Python layer, work package 6) implements it; the run loop (work
/// package 4) drives it.
pub trait DiscreteBlock: Send {
    /// The block's name, for messages.
    fn name(&self) -> &str;
    /// Its sample period, s.
    fn period(&self) -> f64;
    /// The first tick's time, s.
    fn offset(&self) -> f64 {
        0.0
    }
    /// Called once at the start with the inputs' initial values; sets the
    /// outputs' initial values.
    fn init(&mut self, t0: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String>;
    /// One tick: reads the inputs at `t`, writes the held outputs.
    fn tick(&mut self, t: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String>;
}

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

    /// The structure of `∂[x'; g]/∂y` that [`Self::jacobian_sparse`]
    /// fills, if the model knows it.
    fn sparsity(&self) -> Option<&SparsityPattern> {
        None
    }

    /// The Jacobian's values, column-compressed in [`Self::sparsity`]'s
    /// order. The default gathers them from [`Self::jacobian_dense`]; code
    /// generators override it with a coloured forward-mode evaluation.
    fn jacobian_sparse(&self, inp: &EvalInput<'_>, work: &mut [f64], values: &mut [f64]) {
        let Some(pat) = self.sparsity() else {
            return;
        };
        let n = pat.n;
        let mut dense = vec![0.0; n * n];
        self.jacobian_dense(inp, work, &mut dense);
        for j in 0..n {
            for k in pat.col_ptr[j]..pat.col_ptr[j + 1] {
                values[k] = dense[j * n + pat.row_idx[k]];
            }
        }
    }

    /// Sets each mode's Boolean ([`crate::Mode`]) from its relation at the
    /// input point: on entry `d_out` holds the discrete values, on return
    /// the modes' entries are 1 or 0. Models without modes leave it alone.
    fn modes(&self, inp: &EvalInput<'_>, work: &mut [f64], d_out: &mut [f64]) {
        let _ = (inp, work, d_out);
    }

    /// The compiled initialisation problem, if the model has one.
    fn init(&self) -> Option<&dyn InitFunctions> {
        None
    }

    /// The table axes the run loop watches, in the order
    /// [`Self::table_guards`] writes them.
    fn table_guard_list(&self) -> &[TableGuard] {
        &[]
    }

    /// Each watched table axis's guard (positive inside the data, negative
    /// outside), one per entry of [`Self::table_guard_list`].
    fn table_guards(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        let _ = (inp, work, out);
    }

    /// Table `k` of the flat system ([`crate::expr::Expr::Table`]) at
    /// `args` (the second ignored by a 1-D table) as the compiled code
    /// interpolates it: its value and its partial derivatives, so that
    /// expressions evaluated outside the compiled code (the energy books'
    /// declared stored energies and losses) read the same tables. `None`
    /// when the model does not give its tables (the default) or has no
    /// table `k`.
    fn eval_table(&self, k: u32, args: [f64; 2]) -> Option<(f64, [f64; 2])> {
        let _ = (k, args);
        None
    }

    /// Table `k`'s breakpoints as the compiled code interpolates it: its
    /// first axis's, and its second's (empty for a 1-D table), so that the
    /// run loop's enclosures of the tables follow the data the model was
    /// given (a model's tables may be swapped after preparation). `None`
    /// when the model does not give them (the default) or has no table
    /// `k`.
    fn table_axes(&self, k: u32) -> Option<[Vec<f64>; 2]> {
        let _ = k;
        None
    }
}
