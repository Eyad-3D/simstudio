/* Runs CORE-MATH compiled with clang-cl's semantics (run.sh) on the
   references of libm-eval (DIR/<f>.bin: x, y, correctly rounded result,
   as u64 triples) and counts the results that are not the reference.
   usage: harness DIR... */
#define _GNU_SOURCE
#include <fenv.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* the UCRT's floating-point environment, on glibc's */
#define U_INEXACT 0x01
#define U_UNDERFLOW 0x02
#define U_OVERFLOW 0x04
#define U_DIVBYZERO 0x08
#define U_INVALID 0x10
static int to_glibc(int u) {
  return (u & U_INEXACT ? FE_INEXACT : 0) | (u & U_UNDERFLOW ? FE_UNDERFLOW : 0) |
         (u & U_OVERFLOW ? FE_OVERFLOW : 0) | (u & U_DIVBYZERO ? FE_DIVBYZERO : 0) |
         (u & U_INVALID ? FE_INVALID : 0);
}
static int to_ucrt(int g) {
  return (g & FE_INEXACT ? U_INEXACT : 0) | (g & FE_UNDERFLOW ? U_UNDERFLOW : 0) |
         (g & FE_OVERFLOW ? U_OVERFLOW : 0) | (g & FE_DIVBYZERO ? U_DIVBYZERO : 0) |
         (g & FE_INVALID ? U_INVALID : 0);
}
int ucrt_fegetround(void) {
  switch (fegetround()) {
  case FE_TONEAREST: return 0;
  case FE_UPWARD: return 0x100;
  case FE_DOWNWARD: return 0x200;
  default: return 0x300;
  }
}
int ucrt_feraiseexcept(int e) { return feraiseexcept(to_glibc(e)); }
int ucrt_fetestexcept(int e) { return to_ucrt(fetestexcept(to_glibc(e))); }
int ucrt_feclearexcept(int e) { return feclearexcept(to_glibc(e)); }
static fenv_t held; /* the UCRT's fenv_t is 8 bytes: keep glibc's here */
int ucrt_feholdexcept(void *env) { (void)env; return feholdexcept(&held); }
int ucrt_feupdateenv(const void *env) { (void)env; return feupdateenv(&held); }
int ucrt_fegetexceptflag(uint32_t *f, int e) { *f = to_ucrt(fetestexcept(to_glibc(e))); return 0; }
int ucrt_fesetexceptflag(const uint32_t *f, int e) {
  int g = to_glibc(*f & e);
  feclearexcept(to_glibc(e) & ~g);
  if (g) feraiseexcept(g);
  return 0;
}
/* the patched sources report a case they cannot round instead of exiting */
unsigned long long unexpected;
void lsim_math_unexpected_case(int f, double x, double y) { (void)f; (void)x; (void)y; unexpected++; }

#define DECL(t) \
  double t##_sin(double); double t##_cos(double); double t##_tan(double); \
  double t##_asin(double); double t##_acos(double); double t##_atan(double); \
  double t##_atan2(double, double); double t##_sinh(double); double t##_cosh(double); \
  double t##_tanh(double); double t##_exp(double); double t##_log(double); \
  double t##_pow(double, double);
DECL(winF)
DECL(winS)

typedef double (*f1)(double);
typedef double (*f2)(double, double);
static struct { const char *name; f1 a1[2]; f2 a2[2]; } F[] = {
  {"sin", {winF_sin, winS_sin}}, {"cos", {winF_cos, winS_cos}}, {"tan", {winF_tan, winS_tan}},
  {"asin", {winF_asin, winS_asin}}, {"acos", {winF_acos, winS_acos}}, {"atan", {winF_atan, winS_atan}},
  {"atan2", {0}, {winF_atan2, winS_atan2}}, {"sinh", {winF_sinh, winS_sinh}},
  {"cosh", {winF_cosh, winS_cosh}}, {"tanh", {winF_tanh, winS_tanh}}, {"exp", {winF_exp, winS_exp}},
  {"log", {winF_log, winS_log}}, {"pow", {0}, {winF_pow, winS_pow}},
};

static uint64_t bits(double d) { uint64_t u; memcpy(&u, &d, 8); return u; }
static double dbl(uint64_t u) { double d; memcpy(&d, &u, 8); return d; }
static int same(double a, double r) { return r != r ? a != a : bits(a) == bits(r); }

int main(int argc, char **argv) {
  unsigned long long total = 0, bad = 0;
  for (int d = 1; d < argc; d++)
    for (unsigned i = 0; i < sizeof F / sizeof F[0]; i++) {
      char path[4096];
      snprintf(path, sizeof path, "%s/%s.bin", argv[d], F[i].name);
      FILE *fp = fopen(path, "rb");
      if (!fp) continue;
      fseek(fp, 0, SEEK_END);
      long sz = ftell(fp);
      fseek(fp, 0, SEEK_SET);
      uint64_t *v = malloc(sz);
      if (fread(v, 1, sz, fp) != (size_t)sz) { perror(path); return 2; }
      fclose(fp);
      size_t n = sz / 24;
      unsigned long long miss[2] = {0, 0};
      for (size_t k = 0; k < n; k++) {
        double x = dbl(v[3 * k]), y = dbl(v[3 * k + 1]), r = dbl(v[3 * k + 2]);
        for (int b = 0; b < 2; b++) {
          double a = F[i].a1[0] ? F[i].a1[b](x) : F[i].a2[b](x, y);
          if (!same(a, r)) {
            if (miss[b] < 3)
              printf("  MISS %s %s(%a, %a) = %a, not %a\n", b ? "winS" : "winF", F[i].name, x, y, a, r);
            miss[b]++;
          }
        }
      }
      printf("%-12s %-6s %9zu  winF %llu  winS %llu\n", argv[d], F[i].name, n, miss[0], miss[1]);
      total += 2 * n;
      bad += miss[0] + miss[1];
      free(v);
    }
  printf("%llu results, %llu not correctly rounded, %llu unexpected cases\n", total, bad, unexpected);
  return bad != 0;
}
