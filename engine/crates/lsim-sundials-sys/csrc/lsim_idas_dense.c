/* -----------------------------------------------------------------
 * LightSim addition to the in-tree SUNDIALS build (not part of SUNDIALS).
 *
 * The polynomial IDAS' dense output is over the last step, for selected
 * components, as IDAS holds it (no arithmetic here): the run loop
 * encloses it rigorously over any part of the step.
 *
 * IDAS (IDAGetDky with k = 0): y(t) = sum over j <= kused of phi[j] c_j,
 * c_0 = 1, c_j = c_(j-1) (t - tn + psi[j-2]) / psi[j-1] (psi[-1] = 0),
 * valid for t in [tn - hused, tn].
 * -----------------------------------------------------------------*/

#include <nvector/nvector_serial.h>

#include "idas_impl.h"

/* coef: per component m, its kused + 1 coefficients (coef[m * (kused + 1) +
 * j] = phi[j][idx[m]]), cap values at most; psi: the first kused values of
 * psi (MXORD at most); info: tn, hused; *kused: the order. IDA_ILL_INPUT
 * when n_idx (kused + 1) > cap. */
int lsim_ida_dense_select(void* ida_mem, int n_idx, const sunindextype* idx, int cap,
                          sunrealtype* coef, sunrealtype* psi, sunrealtype* info,
                          int* kused)
{
  IDAMem IDA_mem;
  int j, m, l;
  if (ida_mem == NULL) { return IDA_MEM_NULL; }
  IDA_mem = (IDAMem)ida_mem;
  *kused  = IDA_mem->ida_kused;
  l       = IDA_mem->ida_kused + 1;
  if (n_idx * l > cap) { return IDA_ILL_INPUT; }
  info[0] = IDA_mem->ida_tn;
  info[1] = IDA_mem->ida_hused;
  for (j = 0; j < IDA_mem->ida_kused; j++) { psi[j] = IDA_mem->ida_psi[j]; }
  for (j = 0; j < l; j++)
  {
    sunrealtype* pd = NV_DATA_S(IDA_mem->ida_phi[j]);
    for (m = 0; m < n_idx; m++) { coef[m * l + j] = pd[idx[m]]; }
  }
  return IDA_SUCCESS;
}
