"""The error in ulps of every mismatch a candidate made (libm-eval check's
.mis files), against the exact value (MPFR at 300 bits).

usage: ulps.py DIR [f ...]   -> per function and candidate: count, max ulp
error, the input that gives it, and how many results are more than 1 ulp
off (or wrong in a special case: NaN, infinity, zero's sign).
"""
import glob
import math
import os
import struct
import sys

import gmpy2
from gmpy2 import mpfr

CANDS = ["glibc", "pxfm", "libm", "coremath", "coremath_fma", "coremath_soft", "lsim_math"]
BIV = {"atan2", "pow"}
HI = gmpy2.context(precision=300, emin=-100000, emax=100000)
F = {
    "sin": gmpy2.sin, "cos": gmpy2.cos, "tan": gmpy2.tan, "asin": gmpy2.asin,
    "acos": gmpy2.acos, "atan": gmpy2.atan, "atan2": gmpy2.atan2, "sinh": gmpy2.sinh,
    "cosh": gmpy2.cosh, "tanh": gmpy2.tanh, "exp": gmpy2.exp, "log": gmpy2.log,
    "pow": lambda x, y: x ** y,
}


def fb(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def ulp_err(f, x, y, got):
    """|got - exact| / ulp(exact); inf for a wrong special value."""
    with gmpy2.context(HI):
        e = F[f](mpfr(x), mpfr(y)) if f in BIV else F[f](mpfr(x))
        if gmpy2.is_nan(e) or gmpy2.is_infinite(e) or math.isnan(got) or math.isinf(got):
            same = (gmpy2.is_nan(e) and math.isnan(got)) or (
                gmpy2.is_infinite(e) and math.isinf(got) and (e > 0) == (got > 0))
            if same:
                return 0.0
            # an overflowing exact value: compare with the largest double
            if gmpy2.is_infinite(e) or gmpy2.is_nan(e):
                return math.inf
            if math.isinf(got):
                big = mpfr(1.7976931348623157e308)
                if abs(e) > big:  # exact beyond the largest double: rounding to inf is right
                    return float((abs(e) - big) / mpfr(2.0) ** 971)
            return math.inf
        if e == 0:
            return 0.0 if got == 0 else math.inf
        ex = gmpy2.get_exp(e) - 1  # e = m 2^ex with 1 <= |m| < 2
        q = max(ex, -1022) - 52
        return float(abs(mpfr(got) - e) / (mpfr(2) ** q))


def main():
    d = sys.argv[1]
    only = sys.argv[2:]
    for f in F:
        if only and f not in only:
            continue
        for c in CANDS:
            p = f"{d}/{f}.{c}.mis"
            if not os.path.exists(p):
                continue
            b = open(p, "rb").read()
            n = len(b) // 24
            worst, at, over1 = 0.0, None, 0
            for k in range(n):
                x, y, g = struct.unpack_from("<QQQ", b, 24 * k)
                x, y, g = fb(x), fb(y), fb(g)
                u = ulp_err(f, x, y, g)
                if u > 1.0:
                    over1 += 1
                if u > worst or at is None:
                    worst, at = u, (x.hex(), y.hex() if f in BIV else None, g.hex())
            if n:
                print(f"{f:6} {c:13} mismatches {n:8}  max {worst:.4g} ulp  >1ulp {over1:6}  at {at}")
            else:
                print(f"{f:6} {c:13} mismatches        0")


if __name__ == "__main__":
    main()
