"""Correctly rounded references (MPFR through gmpy2, IEEE binary64 context
with subnormals) for the engine's 13 library functions.

usage: gen_refs.py CORE_MATH_BINARY64_DIR OUT_DIR
writes OUT_DIR/wc/<f>.bin (CORE-MATH's worst cases, but signalling NaNs,
which MPFR does not have), OUT_DIR/rand/<f>.bin
(random inputs over wide and typical ranges) and OUT_DIR/special/<f>.bin;
each record three little-endian u64: x, y (0 for one argument), result.
"""
import math
import os
import random
import struct
import sys

import gmpy2
from gmpy2 import mpfr

FUNCS = ["sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh",
         "exp", "log", "pow"]
BIVARIATE = {"atan2", "pow"}

CTX = gmpy2.ieee(64)
gmpy2.set_context(CTX)

F = {
    "sin": gmpy2.sin, "cos": gmpy2.cos, "tan": gmpy2.tan, "asin": gmpy2.asin,
    "acos": gmpy2.acos, "atan": gmpy2.atan, "atan2": gmpy2.atan2, "sinh": gmpy2.sinh,
    "cosh": gmpy2.cosh, "tanh": gmpy2.tanh, "exp": gmpy2.exp, "log": gmpy2.log,
    "pow": lambda x, y: x ** y,
}


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def from_bits(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def parse(tok):
    t = tok.strip()
    neg = t.startswith("-")
    a = t.lstrip("+-")
    if a == "snan":
        return None  # a signalling NaN: MPFR has none (IEEE 754 returns a quiet NaN)
    if a == "nan":
        return -math.nan if neg else math.nan
    if a == "inf":
        return -math.inf if neg else math.inf
    return float.fromhex(t)


def reference(f, x, y):
    if f in BIVARIATE:
        r = F[f](mpfr(x), mpfr(y))
    else:
        r = F[f](mpfr(x))
    return float(r)


def write(path, f, pairs):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as out:
        n = 0
        for x, y in pairs:
            gmpy2.get_context().clear_flags()
            r = reference(f, x, y)
            out.write(struct.pack("<QQQ", bits(x), bits(y), bits(r)))
            n += 1
    return n


def worst_cases(cm, f):
    out = []
    for line in open(f"{cm}/{f}/{f}.wc"):
        line = line.split("#")[0].strip()
        if not line:
            continue
        if "," in line:
            parts = [t.strip() for t in line.split(",")]
        else:
            parts = [line.split()[0]]
        x = parse(parts[0])
        y = parse(parts[1]) if f in BIVARIATE else 0.0
        if x is None or y is None:
            continue
        out.append((x, y))
    return out


def logu(rng, lo, hi):
    return 10.0 ** rng.uniform(math.log10(lo), math.log10(hi))


def sgn(rng, v):
    return -v if rng.random() < 0.5 else v


def random_inputs(f, rng):
    xs = []
    if f in ("sin", "cos", "tan"):
        xs += [(rng.uniform(-10, 10), 0.0) for _ in range(100_000)]
        xs += [(sgn(rng, logu(rng, 1e-10, 1e6)), 0.0) for _ in range(50_000)]
        xs += [(sgn(rng, logu(rng, 1e6, 1e300)), 0.0) for _ in range(20_000)]
    elif f in ("asin", "acos"):
        xs += [(rng.uniform(-1, 1), 0.0) for _ in range(100_000)]
        xs += [(sgn(rng, 1 - logu(rng, 1e-16, 1)), 0.0) for _ in range(50_000)]
        xs += [(sgn(rng, logu(rng, 1e-300, 1e-5)), 0.0) for _ in range(20_000)]
    elif f == "atan":
        xs += [(sgn(rng, logu(rng, 1e-10, 1e10)), 0.0) for _ in range(150_000)]
        xs += [(sgn(rng, logu(rng, 1e-300, 1e300)), 0.0) for _ in range(20_000)]
    elif f == "atan2":
        xs += [(rng.uniform(-10, 10), rng.uniform(-10, 10)) for _ in range(100_000)]
        xs += [(sgn(rng, logu(rng, 1e-20, 1e20)), sgn(rng, logu(rng, 1e-20, 1e20)))
               for _ in range(50_000)]
        xs += [(sgn(rng, logu(rng, 1e-300, 1e300)), sgn(rng, logu(rng, 1e-300, 1e300)))
               for _ in range(20_000)]
    elif f in ("sinh", "cosh", "tanh"):
        xs += [(rng.uniform(-20, 20), 0.0) for _ in range(100_000)]
        xs += [(sgn(rng, logu(rng, 1e-10, 711)), 0.0) for _ in range(50_000)]
        xs += [(sgn(rng, logu(rng, 1e-300, 1e-10)), 0.0) for _ in range(20_000)]
    elif f == "exp":
        xs += [(rng.uniform(-20, 20), 0.0) for _ in range(100_000)]
        xs += [(rng.uniform(-746, 710), 0.0) for _ in range(50_000)]
        xs += [(sgn(rng, logu(rng, 1e-300, 1e-5)), 0.0) for _ in range(20_000)]
    elif f == "log":
        xs += [(logu(rng, 1e-300, 1e300), 0.0) for _ in range(100_000)]
        xs += [(rng.uniform(0.5, 2), 0.0) for _ in range(50_000)]
        xs += [(1 + sgn(rng, logu(rng, 1e-16, 1e-3)), 0.0) for _ in range(20_000)]
    elif f == "pow":
        xs += [(logu(rng, 1e-5, 1e5), rng.uniform(-10, 10)) for _ in range(100_000)]
        xs += [(rng.uniform(0.5, 2), rng.uniform(-1000, 1000)) for _ in range(50_000)]
        xs += [(rng.uniform(0.1, 10), float(rng.randint(-40, 40))) for _ in range(10_000)]
        xs += [(rng.uniform(0.1, 10), rng.randint(-40, 40) + 0.5) for _ in range(10_000)]
        xs += [(-rng.uniform(0.1, 10), float(rng.randint(-40, 40))) for _ in range(10_000)]
        xs += [(float(rng.randint(1, 2**20)), float(rng.randint(-8, 8))) for _ in range(10_000)]
    return xs


def special_inputs(f):
    vals = [0.0, -0.0, math.inf, -math.inf, math.nan, 1.0, -1.0, 0.5, -0.5, 2.0, -2.0,
            5e-324, -5e-324, 2.2250738585072014e-308, -2.2250738585072014e-308,
            1.7976931348623157e308, -1.7976931348623157e308, 1e-300, 1e300,
            math.pi, -math.pi, math.pi / 2, -math.pi / 2, math.pi / 4, 709.782712893384,
            709.7827128933841, -745.1332191019411, -745.1332191019412, -708.3964185322641,
            710.4758600739439, 710.475860073944, 0.9999999999999999, 1.0000000000000002,
            3.0, 10.0, 0.1, 1e-8, 1e8, 1e22, 6381956970095103 * 2.0**797, 22.0, 19.0]
    if f in BIVARIATE:
        ys = vals + [3.0, -3.0, 0.5, -0.5, 1.5, 1e10, -1e10, 2**53, 2**53 + 2.0, -1075.0]
        return [(x, y) for x in vals for y in ys]
    return [(x, 0.0) for x in vals]


def main():
    cm, out = sys.argv[1], sys.argv[2]
    rng = random.Random(20261014)
    only = sys.argv[3:]
    for f in FUNCS:
        if only and f not in only:
            continue
        n1 = write(f"{out}/special/{f}.bin", f, special_inputs(f))
        n2 = write(f"{out}/rand/{f}.bin", f, random_inputs(f, rng))
        n3 = write(f"{out}/wc/{f}.bin", f, worst_cases(cm, f))
        print(f, n1, n2, n3, flush=True)


if __name__ == "__main__":
    main()
