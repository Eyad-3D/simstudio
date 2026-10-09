/* -----------------------------------------------------------------
 * LightSim addition to the in-tree SUNDIALS build (not part of SUNDIALS).
 *
 * CVODES' dense output for selected components only: the same formula and
 * summation order as CVodeGetDky with k = 0 (sum over j of s^j zn[j], the
 * highest order first, as N_VLinearCombination_Serial adds), but O(q) per
 * component instead of O(q n). The run loop uses it to read a sampled
 * block's inputs at every tick without interpolating the whole state.
 * -----------------------------------------------------------------*/

#include "cvodes_impl.h"

#define LSIM_FUZZ SUN_RCONST(100.0)
#define LSIM_ZERO SUN_RCONST(0.0)
#define LSIM_ONE SUN_RCONST(1.0)

int lsim_cvode_dky_select(void* cvode_mem, sunrealtype t, int n_idx,
                          const sunindextype* idx, sunrealtype* out)
{
  CVodeMem cv_mem;
  sunrealtype s, tfuzz, tp, tn1, c[L_MAX];
  sunrealtype* zd[L_MAX];
  int i, j, m, nvec;

  if (cvode_mem == NULL) { return CV_MEM_NULL; }
  cv_mem = (CVodeMem)cvode_mem;

  tfuzz = LSIM_FUZZ * cv_mem->cv_uround *
          (SUNRabs(cv_mem->cv_tn) + SUNRabs(cv_mem->cv_hu));
  if (cv_mem->cv_hu < LSIM_ZERO) { tfuzz = -tfuzz; }
  tp  = cv_mem->cv_tn - cv_mem->cv_hu - tfuzz;
  tn1 = cv_mem->cv_tn + tfuzz;
  if ((t - tp) * (t - tn1) > LSIM_ZERO) { return CV_BAD_T; }

  s    = (t - cv_mem->cv_tn) / cv_mem->cv_h;
  nvec = 0;
  for (j = cv_mem->cv_q; j >= 0; j--)
  {
    c[nvec] = LSIM_ONE;
    for (i = 0; i < j; i++) { c[nvec] *= s; }
    zd[nvec] = N_VGetArrayPointer(cv_mem->cv_zn[j]);
    nvec += 1;
  }
  for (m = 0; m < n_idx; m++)
  {
    sunindextype k = idx[m];
    sunrealtype acc;
    if (nvec == 1) { acc = c[0] * zd[0][k]; }
    else if (nvec == 2) { acc = c[0] * zd[0][k] + c[1] * zd[1][k]; }
    else
    {
      acc = c[0] * zd[0][k];
      for (i = 1; i < nvec; i++) { acc += c[i] * zd[i][k]; }
    }
    out[m] = acc;
  }
  return CV_SUCCESS;
}
