/* -----------------------------------------------------------------
 * LightSim addition to the in-tree SUNDIALS build (not part of SUNDIALS).
 *
 * IDAS' dense output for selected components only: the same coefficients
 * and summation order as IDAGetDky with k = 0, but O(kused) per component
 * instead of O(kused n). See lsim_cvodes_dky.c.
 * -----------------------------------------------------------------*/

#include <nvector/nvector_serial.h>

#include "idas_impl.h"

#define LSIM_HUNDRED SUN_RCONST(100.0)
#define LSIM_ZERO SUN_RCONST(0.0)

int lsim_ida_dky_select(void* ida_mem, sunrealtype t, int n_idx,
                        const sunindextype* idx, sunrealtype* out)
{
  IDAMem IDA_mem;
  sunrealtype tfuzz, tp, delt, psij_1;
  sunrealtype cjk[MXORDP1];
  sunrealtype* pd[MXORDP1];
  int i, j, m, nvec;

  if (ida_mem == NULL) { return IDA_MEM_NULL; }
  IDA_mem = (IDAMem)ida_mem;

  tfuzz = LSIM_HUNDRED * IDA_mem->ida_uround *
          (SUNRabs(IDA_mem->ida_tn) + SUNRabs(IDA_mem->ida_hh));
  if (IDA_mem->ida_hh < LSIM_ZERO) { tfuzz = -tfuzz; }
  tp = IDA_mem->ida_tn - IDA_mem->ida_hused - tfuzz;
  if ((t - tp) * IDA_mem->ida_hh < LSIM_ZERO) { return IDA_BAD_T; }

  for (i = 0; i < MXORDP1; i++) { cjk[i] = 0; }
  delt   = t - IDA_mem->ida_tn;
  cjk[0] = 1;
  psij_1 = 0;
  for (j = 1; j <= IDA_mem->ida_kused; j++)
  {
    cjk[j] = (cjk[j - 1] * (delt + psij_1)) / IDA_mem->ida_psi[j - 1];
    psij_1 = IDA_mem->ida_psi[j - 1];
  }
  nvec = IDA_mem->ida_kused + 1;
  for (j = 0; j < nvec; j++) { pd[j] = NV_DATA_S(IDA_mem->ida_phi[j]); }
  for (m = 0; m < n_idx; m++)
  {
    sunindextype k = idx[m];
    sunrealtype acc;
    if (nvec == 1) { acc = cjk[0] * pd[0][k]; }
    else if (nvec == 2) { acc = cjk[0] * pd[0][k] + cjk[1] * pd[1][k]; }
    else
    {
      acc = cjk[0] * pd[0][k];
      for (j = 1; j < nvec; j++) { acc += cjk[j] * pd[j][k]; }
    }
    out[m] = acc;
  }
  return IDA_SUCCESS;
}
