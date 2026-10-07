/*
 * LightSimTest: a tiny FMI 2.0 Co-Simulation FMU written for LightSim's tests
 * (STD-01). LightSim's own code, under the repository's licence; it uses no
 * FMI headers (the few types it needs are declared below, as the FMI 2.0
 * standard defines them).
 *
 * The model is a first-order lag with a gain:
 *     tau * dy/dt = k * u - y,   solved exactly over each step,
 * so its answer is known in closed form. Its "mode" parameter makes it
 * misbehave on purpose once its time reaches 0.5 s, to prove that a bad FMU
 * cannot hurt the engine:
 *     0 behave, 1 crash (null pointer write), 2 hang (endless loop),
 *     3 try to read and write files and report what worked in "escaped",
 *     4 eat memory, 5 report an error from fmi2DoStep.
 */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
#define EXPORT __declspec(dllexport)
#else
#define EXPORT __attribute__((visibility("default")))
#endif

typedef void *fmi2Component;
typedef void *fmi2ComponentEnvironment;
typedef unsigned int fmi2ValueReference;
typedef double fmi2Real;
typedef int fmi2Integer;
typedef int fmi2Boolean;
typedef const char *fmi2String;
typedef enum { fmi2OK, fmi2Warning, fmi2Discard, fmi2Error, fmi2Fatal, fmi2Pending } fmi2Status;
typedef enum { fmi2ModelExchange, fmi2CoSimulation } fmi2Type;
typedef enum { fmi2DoStepStatus, fmi2PendingStatus, fmi2LastSuccessfulTime, fmi2Terminated } fmi2StatusKind;

enum { VR_U = 0, VR_K = 1, VR_TAU = 2, VR_Y = 3, VR_T = 4, VR_ESCAPED = 5, VR_MODE = 6 };

typedef struct {
    double u, k, tau, y, t, escaped;
    int mode;
} Model;

static void misbehave(Model *m) {
    if (m->t < 0.5) return;
    switch (m->mode) {
    case 1: {
        volatile int *p = NULL;
        *p = 1; /* crash */
        break;
    }
    case 2:
        for (volatile unsigned long i = 0;; i++) {
        } /* hang */
    case 3: {
        double found = 0;
        FILE *f = fopen("/etc/hostname", "r");
        if (f) { found += 1; fclose(f); }
        f = fopen("lightsim-fmu-escape.txt", "w");
        if (f) { found += 2; fputs("escaped", f); fclose(f); }
        m->escaped = found;
        break;
    }
    case 4:
        for (;;) {
            char *block = malloc(64u << 20);
            if (!block) break;
            memset(block, 1, 64u << 20);
        }
        break;
    default:
        break;
    }
}

EXPORT const char *fmi2GetTypesPlatform(void) { return "default"; }
EXPORT const char *fmi2GetVersion(void) { return "2.0"; }

EXPORT fmi2Status fmi2SetDebugLogging(fmi2Component c, fmi2Boolean on, size_t n, const fmi2String cats[]) {
    (void)c; (void)on; (void)n; (void)cats;
    return fmi2OK;
}

EXPORT fmi2Component fmi2Instantiate(fmi2String name, fmi2Type type, fmi2String guid, fmi2String resources,
                                     const void *callbacks, fmi2Boolean visible, fmi2Boolean logging) {
    (void)name; (void)resources; (void)callbacks; (void)visible; (void)logging;
    if (type != fmi2CoSimulation || !guid || strcmp(guid, "{7a1c6a0e-5f0b-4b8e-9d3e-1c2b3a4d5e6f}") != 0)
        return NULL;
    Model *m = calloc(1, sizeof(Model));
    if (!m) return NULL;
    m->k = 2.0;
    m->tau = 0.5;
    return m;
}

EXPORT void fmi2FreeInstance(fmi2Component c) { free(c); }

EXPORT fmi2Status fmi2SetupExperiment(fmi2Component c, fmi2Boolean tolDefined, fmi2Real tol, fmi2Real start,
                                      fmi2Boolean stopDefined, fmi2Real stop) {
    (void)tolDefined; (void)tol; (void)stopDefined; (void)stop;
    ((Model *)c)->t = start;
    return fmi2OK;
}

EXPORT fmi2Status fmi2EnterInitializationMode(fmi2Component c) { (void)c; return fmi2OK; }
EXPORT fmi2Status fmi2ExitInitializationMode(fmi2Component c) { (void)c; return fmi2OK; }
EXPORT fmi2Status fmi2Terminate(fmi2Component c) { (void)c; return fmi2OK; }

EXPORT fmi2Status fmi2Reset(fmi2Component c) {
    Model *m = c;
    m->u = m->y = m->t = m->escaped = 0;
    return fmi2OK;
}

EXPORT fmi2Status fmi2GetReal(fmi2Component c, const fmi2ValueReference vr[], size_t n, fmi2Real v[]) {
    Model *m = c;
    for (size_t i = 0; i < n; i++) {
        switch (vr[i]) {
        case VR_U: v[i] = m->u; break;
        case VR_K: v[i] = m->k; break;
        case VR_TAU: v[i] = m->tau; break;
        case VR_Y: v[i] = m->y; break;
        case VR_T: v[i] = m->t; break;
        case VR_ESCAPED: v[i] = m->escaped; break;
        default: return fmi2Error;
        }
    }
    return fmi2OK;
}

EXPORT fmi2Status fmi2SetReal(fmi2Component c, const fmi2ValueReference vr[], size_t n, const fmi2Real v[]) {
    Model *m = c;
    for (size_t i = 0; i < n; i++) {
        switch (vr[i]) {
        case VR_U: m->u = v[i]; break;
        case VR_K: m->k = v[i]; break;
        case VR_TAU: m->tau = v[i]; break;
        default: return fmi2Error;
        }
    }
    return fmi2OK;
}

EXPORT fmi2Status fmi2GetInteger(fmi2Component c, const fmi2ValueReference vr[], size_t n, fmi2Integer v[]) {
    for (size_t i = 0; i < n; i++) {
        if (vr[i] != VR_MODE) return fmi2Error;
        v[i] = ((Model *)c)->mode;
    }
    return fmi2OK;
}

EXPORT fmi2Status fmi2SetInteger(fmi2Component c, const fmi2ValueReference vr[], size_t n, const fmi2Integer v[]) {
    for (size_t i = 0; i < n; i++) {
        if (vr[i] != VR_MODE) return fmi2Error;
        ((Model *)c)->mode = v[i];
    }
    return fmi2OK;
}

EXPORT fmi2Status fmi2GetBoolean(fmi2Component c, const fmi2ValueReference vr[], size_t n, fmi2Boolean v[]) {
    (void)c; (void)vr; (void)v;
    return n ? fmi2Error : fmi2OK;
}

EXPORT fmi2Status fmi2SetBoolean(fmi2Component c, const fmi2ValueReference vr[], size_t n, const fmi2Boolean v[]) {
    (void)c; (void)vr; (void)v;
    return n ? fmi2Error : fmi2OK;
}

EXPORT fmi2Status fmi2DoStep(fmi2Component c, fmi2Real t, fmi2Real h, fmi2Boolean noSetPrior) {
    Model *m = c;
    (void)noSetPrior;
    if (fabs(t - m->t) > 1e-9 * (1 + fabs(t))) return fmi2Error; /* steps must join up */
    misbehave(m);
    if (m->mode == 5 && m->t >= 0.5) return fmi2Error;
    double a = m->tau > 0 ? 1.0 - exp(-h / m->tau) : 1.0;
    m->y += (m->k * m->u - m->y) * a;
    m->t = t + h;
    return fmi2OK;
}

EXPORT fmi2Status fmi2CancelStep(fmi2Component c) { (void)c; return fmi2OK; }

/* The rest of the FMI 2.0 Co-Simulation interface: not used by this model. */
typedef void *fmi2FMUstate;
typedef unsigned char fmi2Byte;

EXPORT fmi2Status fmi2GetString(fmi2Component c, const fmi2ValueReference vr[], size_t n, fmi2String v[]) {
    (void)c; (void)vr; (void)v;
    return n ? fmi2Error : fmi2OK;
}
EXPORT fmi2Status fmi2SetString(fmi2Component c, const fmi2ValueReference vr[], size_t n, const fmi2String v[]) {
    (void)c; (void)vr; (void)v;
    return n ? fmi2Error : fmi2OK;
}
EXPORT fmi2Status fmi2GetFMUstate(fmi2Component c, fmi2FMUstate *s) { (void)c; (void)s; return fmi2Error; }
EXPORT fmi2Status fmi2SetFMUstate(fmi2Component c, fmi2FMUstate s) { (void)c; (void)s; return fmi2Error; }
EXPORT fmi2Status fmi2FreeFMUstate(fmi2Component c, fmi2FMUstate *s) { (void)c; (void)s; return fmi2Error; }
EXPORT fmi2Status fmi2SerializedFMUstateSize(fmi2Component c, fmi2FMUstate s, size_t *n) {
    (void)c; (void)s; (void)n;
    return fmi2Error;
}
EXPORT fmi2Status fmi2SerializeFMUstate(fmi2Component c, fmi2FMUstate s, fmi2Byte b[], size_t n) {
    (void)c; (void)s; (void)b; (void)n;
    return fmi2Error;
}
EXPORT fmi2Status fmi2DeSerializeFMUstate(fmi2Component c, const fmi2Byte b[], size_t n, fmi2FMUstate *s) {
    (void)c; (void)b; (void)n; (void)s;
    return fmi2Error;
}
EXPORT fmi2Status fmi2GetDirectionalDerivative(fmi2Component c, const fmi2ValueReference u[], size_t nu,
                                               const fmi2ValueReference z[], size_t nz, const fmi2Real du[],
                                               fmi2Real dz[]) {
    (void)c; (void)u; (void)nu; (void)z; (void)nz; (void)du; (void)dz;
    return fmi2Error;
}
EXPORT fmi2Status fmi2SetRealInputDerivatives(fmi2Component c, const fmi2ValueReference vr[], size_t n,
                                              const fmi2Integer order[], const fmi2Real v[]) {
    (void)c; (void)vr; (void)n; (void)order; (void)v;
    return fmi2Error;
}
EXPORT fmi2Status fmi2GetRealOutputDerivatives(fmi2Component c, const fmi2ValueReference vr[], size_t n,
                                               const fmi2Integer order[], fmi2Real v[]) {
    (void)c; (void)vr; (void)n; (void)order; (void)v;
    return fmi2Error;
}
EXPORT fmi2Status fmi2GetStatus(fmi2Component c, const fmi2StatusKind s, fmi2Status *v) {
    (void)c; (void)s; (void)v;
    return fmi2Discard;
}
EXPORT fmi2Status fmi2GetRealStatus(fmi2Component c, const fmi2StatusKind s, fmi2Real *v) {
    (void)c; (void)s; (void)v;
    return fmi2Discard;
}
EXPORT fmi2Status fmi2GetIntegerStatus(fmi2Component c, const fmi2StatusKind s, fmi2Integer *v) {
    (void)c; (void)s; (void)v;
    return fmi2Discard;
}
EXPORT fmi2Status fmi2GetBooleanStatus(fmi2Component c, const fmi2StatusKind s, fmi2Boolean *v) {
    (void)c; (void)s; (void)v;
    return fmi2Discard;
}
EXPORT fmi2Status fmi2GetStringStatus(fmi2Component c, const fmi2StatusKind s, fmi2String *v) {
    (void)c; (void)s; (void)v;
    return fmi2Discard;
}
