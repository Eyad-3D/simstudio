/* -----------------------------------------------------------------
 * LightSim addition to the in-tree SUNDIALS build (not part of SUNDIALS).
 *
 * The polynomial CVODES' dense output is over the last step, for
 * selected components, as CVODES holds it (no arithmetic here): the run
 * loop encloses it rigorously over any part of the step.
 *
 * CVODES (CVodeGetDky with k = 0): y(t) = sum over j <= q of zn[j] s^j,
 * s = (t - tn) / h, valid for t in [tn - hu, tn].
 * -----------------------------------------------------------------*/

#include <nvector/nvector_serial.h>

#include "cvodes_impl.h"

/* coef: per component m, its q + 1 coefficients (coef[m * (q + 1) + j] =
 * zn[j][idx[m]]), cap values at most; info: tn, h, hu; *q: the order.
 * CV_ILL_INPUT when n_idx (q + 1) > cap. */
int lsim_cvode_dense_select(void* cvode_mem, int n_idx, const sunindextype* idx,
                            int cap, sunrealtype* coef, sunrealtype* info, int* q)
{
  CVodeMem cv_mem;
  int j, m, l;
  if (cvode_mem == NULL) { return CV_MEM_NULL; }
  cv_mem = (CVodeMem)cvode_mem;
  *q     = cv_mem->cv_q;
  l      = cv_mem->cv_q + 1;
  if (n_idx * l > cap) { return CV_ILL_INPUT; }
  info[0] = cv_mem->cv_tn;
  info[1] = cv_mem->cv_h;
  info[2] = cv_mem->cv_hu;
  for (j = 0; j < l; j++)
  {
    sunrealtype* zd = NV_DATA_S(cv_mem->cv_zn[j]);
    for (m = 0; m < n_idx; m++) { coef[m * l + j] = zd[idx[m]]; }
  }
  return CV_SUCCESS;
}
