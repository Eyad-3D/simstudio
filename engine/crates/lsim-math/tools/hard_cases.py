"""Writes tests/data/hard_cases.txt: inputs of the engine's 13 library
functions (and of the software fused multiply-add) with their correctly
rounded results, computed twice, independently:

* MPFR (through gmpy2) in an IEEE binary64 context (53 bits, subnormals,
  round to nearest, ties to even), which rounds correctly by design;
* mpmath at 4000 bits, rounded to the nearest double here (ties to even,
  subnormals, overflow);

and refuses to write anything when the two disagree on a finite nonzero
result of finite nonzero inputs (special values, zeros among them, come
from MPFR, which follows C's Annex F; mpmath has no signed zeros).

The inputs, per function:
* `wc`: an even sample of CORE-MATH's hard-to-round cases (`<f>.wc` in a
  CORE-MATH checkout: the inputs whose exact value lies closest to a
  rounding boundary, where a library that is not correctly rounded errs);
* `special`: zeros, infinities, NaN, subnormals, domain edges, overflow and
  underflow thresholds;
* `random`: values over the ranges the engine's models use;
* `regression`: inputs a CORE-MATH revision got wrong (exp at 37384b5);
* `glibc`: inputs where glibc 2.39 (x86-64, FMA variants) is more than 1
  ulp off (up to 1.4e5 ulps for sin and cos of huge arguments);
* `libm.rs`: the 543 references of lsim-solve's former platform test
  (computed with Python's decimal module: a third reference, which must
  agree).

usage (offline; MPFR and mpmath from pip):
    python -m venv v && v/bin/pip install gmpy2 mpmath
    git clone https://gitlab.inria.fr/core-math/core-math
    v/bin/python -I tools/hard_cases.py core-math/src/binary64 tests/data/hard_cases.txt
"""
import math
import random
import re
import struct
import sys
from pathlib import Path

import gmpy2
import mpmath
from gmpy2 import mpfr

FUNCS = ["sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh",
         "exp", "log", "pow"]
BIVARIATE = {"atan2", "pow"}
PER_FUNCTION_WC = 150

gmpy2.set_context(gmpy2.ieee(64))
mpmath.mp.prec = 4000

MPFR = {
    "sin": gmpy2.sin, "cos": gmpy2.cos, "tan": gmpy2.tan, "asin": gmpy2.asin,
    "acos": gmpy2.acos, "atan": gmpy2.atan, "atan2": gmpy2.atan2, "sinh": gmpy2.sinh,
    "cosh": gmpy2.cosh, "tanh": gmpy2.tanh, "exp": gmpy2.exp, "log": gmpy2.log,
    "pow": lambda x, y: x ** y, "fma": gmpy2.fma,
}
MPMATH = {
    "sin": mpmath.sin, "cos": mpmath.cos, "tan": mpmath.tan, "asin": mpmath.asin,
    "acos": mpmath.acos, "atan": mpmath.atan, "atan2": mpmath.atan2, "sinh": mpmath.sinh,
    "cosh": mpmath.cosh, "tanh": mpmath.tanh, "exp": mpmath.exp, "log": mpmath.log,
    "pow": mpmath.power, "fma": lambda a, b, c: a * b + c,
}

EXTRA = {
    "regression": [("exp", "-0x1.07e05e6e03b83p-2"), ("exp", "-0x1.40a990279c2c0p-6"),
                   ("exp", "0x1.aca7ae8da5a7bp+0")],
    "glibc": [("sin", "0x1.4c96c11134d36p+578"), ("cos", "0x1.69eab0985179bp+246"),
              ("cos", "0x1.6ac5b262ca1ffp+849"), ("tan", "0x1.6ac5b262ca1ffp+849"),
              ("sinh", "-0x1.6901f13839aa0p-1"), ("sinh", "0x1.c5fb4e5a38e4fp-2"),
              ("tanh", "0x1.d60b6c022a900p-2"), ("tanh", "0x1.d81713ce42067p-3"),
              ("cosh", "0x1.f570168b89987p+0")],
}


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def from_bits(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def nearest_double(e):
    """The double nearest to the mpmath value e (finite, nonzero)."""
    neg = e < 0
    a = abs(e)
    _, ex = mpmath.frexp(a)  # a = m 2^ex, 1/2 <= m < 1
    q = max(int(ex) - 53, -1074)
    n = mpmath.ldexp(a, -q)
    k = int(mpmath.floor(n))
    frac = n - k
    if frac > 0.5 or (frac == 0.5 and k % 2 == 1):
        k += 1
    try:
        r = math.ldexp(float(k), q)  # exact: k has at most 53 bits
    except OverflowError:
        r = math.inf
    return -r if neg else r


def reference(f, args):
    m = [mpfr(a) for a in args]
    r = float(MPFR[f](*m))
    if math.isfinite(r) and r != 0 and all(math.isfinite(a) and a != 0 for a in args):
        e = MPMATH[f](*[mpmath.mpf(a) for a in args])
        c = nearest_double(e)
        if bits(c) != bits(r):
            raise SystemExit(f"MPFR and mpmath disagree on {f}{tuple(a.hex() for a in args)}: "
                             f"{r.hex()} against {c.hex()}")
    return r


def parse_wc(tok):
    t = tok.strip()
    a = t.lstrip("+-")
    if a in ("snan",):
        return None  # IEEE's signalling NaN: MPFR has none
    if a == "nan":
        return -math.nan if t.startswith("-") else math.nan
    if a == "inf":
        return -math.inf if t.startswith("-") else math.inf
    return float.fromhex(t)


def worst_cases(cm, f):
    rows = []
    for line in open(Path(cm) / f / f"{f}.wc"):
        line = line.split("#")[0].strip()
        if not line:
            continue
        parts = [p.strip() for p in line.split(",")] if "," in line else [line.split()[0]]
        vals = [parse_wc(p) for p in parts[:2 if f in BIVARIATE else 1]]
        if any(v is None for v in vals):
            continue
        rows.append(tuple(vals))
    step = max(1, len(rows) // PER_FUNCTION_WC)
    return rows[::step][:PER_FUNCTION_WC]


SPECIAL = [0.0, -0.0, math.inf, -math.inf, math.nan, 1.0, -1.0, 0.5, -0.5, 2.0, -2.0, 3.0,
           5e-324, -5e-324, 2.2250738585072014e-308, -2.2250738585072014e-308,
           1.7976931348623157e308, -1.7976931348623157e308, 1e-300, -1e-300, 1e300, -1e300,
           math.pi, -math.pi, math.pi / 2, -math.pi / 2, math.pi / 4, 0.9999999999999999,
           1.0000000000000002, -0.9999999999999999, 709.782712893384, 709.7827128933841,
           710.4758600739439, 710.475860073944, -708.3964185322641, -745.1332191019411,
           -745.1332191019412, 1e-8, 1e8, 1e22, float.fromhex("0x1.6ac5b262ca1ffp+849"), 22.0, 19.0, 0.1]
SPECIAL_Y = [0.0, -0.0, math.inf, -math.inf, math.nan, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 0.5,
             -0.5, 1.5, 1e10, -1e10, 2.0**53, 2.0**53 + 2, -1075.0, 5e-324]
SPECIAL_X2 = [0.0, -0.0, math.inf, -math.inf, math.nan, 1.0, -1.0, 2.0, -2.0, 0.5, -0.5, 10.0,
              -10.0, 5e-324, -5e-324, 1.7976931348623157e308, 0.9999999999999999,
              1.0000000000000002]


def special_inputs(f):
    if f in BIVARIATE:
        return [(x, y) for x in SPECIAL_X2 for y in SPECIAL_Y]
    return [(x,) for x in SPECIAL]


def logu(rng, lo, hi):
    return 10.0 ** rng.uniform(math.log10(lo), math.log10(hi))


def random_inputs(f, rng, n=50):
    s = lambda v: -v if rng.random() < 0.5 else v  # noqa: E731
    if f in ("sin", "cos", "tan"):
        return [(rng.uniform(-10, 10),) for _ in range(n)]
    if f in ("asin", "acos"):
        return [(rng.uniform(-1, 1),) for _ in range(n)]
    if f == "atan":
        return [(s(logu(rng, 1e-6, 1e6)),) for _ in range(n)]
    if f == "atan2":
        return [(rng.uniform(-10, 10), rng.uniform(-10, 10)) for _ in range(n)]
    if f in ("sinh", "cosh", "tanh"):
        return [(rng.uniform(-20, 20),) for _ in range(n)]
    if f == "exp":
        return [(rng.uniform(-50, 50),) for _ in range(n)]
    if f == "log":
        return [(logu(rng, 1e-10, 1e10),) for _ in range(n)]
    if f == "pow":
        return [(logu(rng, 1e-3, 1e3), rng.uniform(-5, 5)) for _ in range(n)]
    raise ValueError(f)


def fma_inputs(rng):
    out = []
    for _ in range(200):
        a, b = rng.uniform(-4, 4), rng.uniform(-4, 4)
        out.append((a, b, -(a * b)))  # the exact error of a product
    for _ in range(100):
        out.append((logu(rng, 1e-300, 1e300), rng.choice([-1, 1]) * logu(rng, 1e-300, 1e300),
                    rng.choice([-1, 1]) * logu(rng, 1e-300, 1e300)))
    eps = 2.0**-52
    out += [(1 + eps, 1 + eps, -(1 + 2 * eps)), (1e308, 10.0, -1e308), (5e-324, 0.5, 0.0),
            (2.0**-537, 2.0**-537, 0.0), (3.0, 5e-324, -5e-324), (math.inf, 0.0, 1.0),
            (1.0, math.inf, -math.inf), (0.0, -1.0, 0.0), (-0.0, 1.0, 0.0), (math.nan, 1.0, 1.0),
            (2.0**-511, 2.0**-511, -(2.0**-1022)), (1 - 2.0**-53, 1 - 2.0**-53, -1.0),
            (2.0**-1022, -0.5, 2.0**-1074)]
    return out


def libm_rs_references(path):
    text = Path(path).read_text()
    names = {"ln": "log"}
    return [(names.get(n, n), int(x, 16), int(r, 16))
            for n, x, r in re.findall(r'\("(\w+)", 0x([0-9a-f]+), 0x([0-9a-f]+)\)', text)]


def main():
    cm, out = sys.argv[1], sys.argv[2]
    old = sys.argv[3] if len(sys.argv) > 3 else None
    rng = random.Random(20261014)
    lines = []

    def emit(f, tag, args):
        r = reference(f, args)
        lines.append(" ".join([f, tag] + [f"{bits(a):016x}" for a in args] + ["=", f"{bits(r):016x}"]))

    for f in FUNCS:
        for a in special_inputs(f):
            emit(f, "special", a)
        for a in random_inputs(f, rng):
            emit(f, "random", a)
        for a in worst_cases(cm, f):
            emit(f, "wc", a)
        print(f, file=sys.stderr, flush=True)
    for tag, rows in EXTRA.items():
        for f, x in rows:
            emit(f, tag, (float.fromhex(x),))
    if old:
        for f, x, r in libm_rs_references(old):
            got = reference(f, (from_bits(x),))
            if bits(got) != r:
                raise SystemExit(f"the old reference for {f}({from_bits(x).hex()}) disagrees")
            emit(f, "libm.rs", (from_bits(x),))
    for a in fma_inputs(rng):
        emit("fma", "fma", a)
    header = [
        "# Correctly rounded results of the engine's library functions: written by",
        "# tools/hard_cases.py (MPFR and mpmath at 4000 bits, which agree on every",
        "# finite nonzero result). Each line: function, source, the inputs and the",
        "# result as the bits of doubles in hex. Do not edit by hand.",
    ]
    Path(out).write_text("\n".join(header + lines) + "\n")
    print(len(lines), "cases", file=sys.stderr)


if __name__ == "__main__":
    main()
