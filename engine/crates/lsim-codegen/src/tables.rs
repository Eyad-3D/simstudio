//! The table runtime of the generated code: the `lsim_tab*` symbols it
//! calls and the store of a compiled model's interpolants (DESIGN.md,
//! *Code generation*). The interpolants themselves are `lsim-ir`'s
//! ([`lsim_ir::interp`]), shared with the interpreter, preparation and the
//! solver.

pub use lsim_ir::interp::{Table, steffen_slopes};
use lsim_ir::table::TableData;

/// The value of 1-D table `t` at `x`.
///
/// # Safety
/// `t` must point to a live [`Table`].
pub(crate) unsafe extern "C" fn lsim_tab1(t: *const Table, x: f64) -> f64 {
    // SAFETY: by the caller's contract (the generated code passes the
    // table store's pointers, which live as long as the compiled model).
    let t = unsafe { &*t };
    t.value([x, 0.0])
}

/// The value of 1-D table `t` at `x`; writes the derivative to `d[0]`.
///
/// # Safety
/// `t` must point to a live [`Table`], `d` to one writable value.
pub(crate) unsafe extern "C" fn lsim_tab1d(t: *const Table, x: f64, d: *mut f64) -> f64 {
    // SAFETY: as in `lsim_tab1`; `d` is the generated code's scratch.
    unsafe {
        let (v, g) = (*t).eval([x, 0.0]);
        *d = g[0];
        v
    }
}

/// The value of 2-D table `t` at `(x, y)`.
///
/// # Safety
/// `t` must point to a live [`Table`].
pub(crate) unsafe extern "C" fn lsim_tab2(t: *const Table, x: f64, y: f64) -> f64 {
    // SAFETY: as in `lsim_tab1`.
    unsafe { (*t).value([x, y]) }
}

/// The value of 2-D table `t` at `(x, y)`; writes the partial derivatives
/// to `d[0]` and `d[1]`.
///
/// # Safety
/// `t` must point to a live [`Table`], `d` to two writable values.
pub(crate) unsafe extern "C" fn lsim_tab2d(t: *const Table, x: f64, y: f64, d: *mut f64) -> f64 {
    // SAFETY: as in `lsim_tab1d`.
    unsafe {
        let (v, g) = (*t).eval([x, y]);
        *d = g[0];
        *d.add(1) = g[1];
        v
    }
}

/// Table `t`'s guard of axis `axis` at argument `a`.
///
/// # Safety
/// `t` must point to a live [`Table`].
pub(crate) unsafe extern "C" fn lsim_tab_guard(t: *const Table, axis: i64, a: f64) -> f64 {
    // SAFETY: as in `lsim_tab1`.
    unsafe { (*t).guard(axis as usize, a) }
}

/// The tables of a compiled model: built interpolants and the pointer
/// array the generated code indexes. After the flat tables come the
/// partial derivatives the model reads (index reduction through tables),
/// each the same interpolant giving that derivative
/// ([`Table::derivative`]).
pub(crate) struct TableStore {
    tables: Vec<Table>,
    ptrs: Vec<*const Table>,
    /// the derivatives' entries: `(table, order)`, in store order after
    /// the flat tables
    derived: Vec<(u32, [u8; 2])>,
}

// SAFETY: the pointers point into `tables`, owned by the store and never
// mutated or resized after construction; sharing them across threads only
// reads immutable data.
unsafe impl Send for TableStore {}
// SAFETY: as above.
unsafe impl Sync for TableStore {}

impl TableStore {
    /// The interpolants of `data`, then the partial derivatives `derived`
    /// reads. A table read differentiated `r` times (in all) must be
    /// continuously differentiable `r - 1` times (index reduction's
    /// condition, checked here again for data given after preparation).
    pub(crate) fn new(
        data: &[&TableData],
        names: &[&str],
        derived: &[(u32, [u8; 2])],
    ) -> Result<TableStore, String> {
        let mut tables = Vec::with_capacity(data.len() + derived.len());
        for (d, name) in data.iter().zip(names) {
            tables.push(Table::new(d).map_err(|e| format!("table '{name}': {e}"))?);
        }
        for &(k, order) in derived {
            let base = tables.get(k as usize).ok_or_else(|| format!("no table {k}"))?;
            let r = u32::from(order[0]) + u32::from(order[1]);
            let c = base.continuity();
            if r > c.saturating_add(1) {
                return Err(format!(
                    "table '{}': the model differentiates it {r} times (index reduction), but \
                     these data are continuously differentiable only {c} times",
                    names.get(k as usize).copied().unwrap_or("?")
                ));
            }
            tables.push(base.derivative(order));
        }
        // (the vector is never resized after this, so the pointers stay valid)
        let ptrs = tables.iter().map(|t| t as *const Table).collect();
        Ok(TableStore { tables, ptrs, derived: derived.to_vec() })
    }

    /// The derivatives' entries after the flat tables.
    pub(crate) fn derived(&self) -> &[(u32, [u8; 2])] {
        &self.derived
    }

    /// The entry of `table`'s partial derivative of `order` (`[0, 0]`: the
    /// table itself).
    pub(crate) fn entry(&self, table: u32, order: [u8; 2]) -> Option<usize> {
        if order == [0, 0] {
            return Some(table as usize);
        }
        let n = self.tables.len() - self.derived.len();
        self.derived.iter().position(|&d| d == (table, order)).map(|i| n + i)
    }

    pub(crate) fn ptrs(&self) -> *const *const Table {
        self.ptrs.as_ptr()
    }

    pub(crate) fn get(&self, k: usize) -> &Table {
        &self.tables[k]
    }

    pub(crate) fn len(&self) -> usize {
        self.tables.len()
    }
}
