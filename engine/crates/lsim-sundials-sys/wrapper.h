/* The SUNDIALS headers the Rust bindings (src/bindings.rs) cover. */
#include <sundials/sundials_core.h>
#include <cvodes/cvodes.h>
#include <cvodes/cvodes_ls.h>
#include <idas/idas.h>
#include <idas/idas_ls.h>
#include <kinsol/kinsol.h>
#include <kinsol/kinsol_ls.h>
#include <nvector/nvector_serial.h>
#include <sunmatrix/sunmatrix_dense.h>
#include <sunmatrix/sunmatrix_band.h>
#include <sunmatrix/sunmatrix_sparse.h>
#include <sunlinsol/sunlinsol_dense.h>
#include <sunlinsol/sunlinsol_band.h>
#include <sunnonlinsol/sunnonlinsol_newton.h>
#include <sunnonlinsol/sunnonlinsol_fixedpoint.h>
