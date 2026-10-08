"""The closed-form building blocks the exact answers are made of.

Standard library only (math, cmath), so the answers need nothing installed
and can be ported line by line:

- :class:`ExpPoly`: sums of a·tⁿ·e^(λt) (λ may be complex), closed under
  +, −, × and ∫₀ᵗ — every linear problem's states, powers and energies are
  one of these, so energies are integrated exactly, not by quadrature;
- :func:`linear1` and :func:`linear2`: x' = M x + f with constant M and f,
  solved through M's eigenvalues (two distinct ones for 2 × 2);
- :class:`Piecewise`: a signal made of phases (before and after an event);
- :func:`lambert_w0`: the principal branch of Lambert's W (Halley's method);
- :func:`rational_integral`: ∫ N(x) / Πⱼ(x − rⱼ) dx with distinct real
  poles, by polynomial division and partial fractions;
- :func:`first_crossing`: the first time a smooth signal reaches a level,
  bracketed on a grid and bisected to the last bit.
"""
from __future__ import annotations

import cmath
import math
from typing import Callable, Sequence

Number = float | complex


class ExpPoly:
    """f(t) = Σ a · tⁿ · e^(λ t), with complex a and λ; f(t) is the real part.

    Terms are keyed by (n, λ). λ = 0 terms are polynomials. The algebra is
    exact; only the final evaluation rounds."""

    __slots__ = ("terms",)

    def __init__(self, terms: dict[tuple[int, complex], complex] | None = None):
        self.terms: dict[tuple[int, complex], complex] = {}
        for (n, lam), a in (terms or {}).items():
            self._add(n, lam, a)

    # -- construction -----------------------------------------------------------
    @classmethod
    def const(cls, c: Number) -> "ExpPoly":
        return cls({(0, 0j): complex(c)})

    @classmethod
    def exp(cls, lam: Number, a: Number = 1.0) -> "ExpPoly":
        """a · e^(λ t)."""
        return cls({(0, complex(lam)): complex(a)})

    @classmethod
    def t(cls, a: Number = 1.0) -> "ExpPoly":
        """a · t."""
        return cls({(1, 0j): complex(a)})

    def _add(self, n: int, lam: Number, a: Number) -> None:
        key = (n, complex(lam))
        self.terms[key] = self.terms.get(key, 0j) + complex(a)

    # -- algebra ------------------------------------------------------------------
    def __add__(self, other) -> "ExpPoly":
        other = _ep(other)
        out = ExpPoly(self.terms)
        for (n, lam), a in other.terms.items():
            out._add(n, lam, a)
        return out

    __radd__ = __add__

    def __neg__(self) -> "ExpPoly":
        return ExpPoly({k: -a for k, a in self.terms.items()})

    def __sub__(self, other) -> "ExpPoly":
        return self + (-_ep(other))

    def __rsub__(self, other) -> "ExpPoly":
        return _ep(other) - self

    def __mul__(self, other) -> "ExpPoly":
        if isinstance(other, (int, float, complex)):
            return ExpPoly({k: a * other for k, a in self.terms.items()})
        out = ExpPoly()
        for (n1, l1), a1 in self.terms.items():
            for (n2, l2), a2 in other.terms.items():
                out._add(n1 + n2, l1 + l2, a1 * a2)
        return out

    __rmul__ = __mul__

    def __truediv__(self, c: Number) -> "ExpPoly":
        return self * (1.0 / c)

    def __call__(self, t: float) -> float:
        s = 0j
        for (n, lam), a in self.terms.items():
            term = a * (t ** n if n else 1.0)
            if lam != 0:
                term *= cmath.exp(lam * t)
            s += term
        return s.real

    def integral(self) -> "ExpPoly":
        """F(t) = ∫₀ᵗ f(s) ds, exactly:
        ∫ tⁿ e^(λt) dt = e^(λt) Σₖ (−1)ᵏ n!/(n−k)! tⁿ⁻ᵏ / λᵏ⁺¹ (λ ≠ 0)."""
        out = ExpPoly()
        for (n, lam), a in self.terms.items():
            if lam == 0:
                out._add(n + 1, 0j, a / (n + 1))
                continue
            for k in range(n + 1):
                coef = a * (-1) ** k * math.factorial(n) / math.factorial(n - k) / lam ** (k + 1)
                out._add(n - k, lam, coef)
        at_zero = sum((a for (n, _), a in out.terms.items() if n == 0), 0j)
        out._add(0, 0j, -at_zero)
        return out

    def shifted(self, t0: float) -> "ExpPoly":
        """g(t) = f(t − t0), for a phase that starts at t0 (n ≤ 1 terms only)."""
        out = ExpPoly()
        for (n, lam), a in self.terms.items():
            a0 = a * cmath.exp(-lam * t0)
            if n == 0:
                out._add(0, lam, a0)
            elif n == 1:  # (t − t0) e^(λ(t − t0))
                out._add(1, lam, a0)
                out._add(0, lam, -t0 * a0)
            else:
                raise ValueError("shifted() handles terms up to t¹")
        return out


def _ep(x) -> ExpPoly:
    return x if isinstance(x, ExpPoly) else ExpPoly.const(x)


# ---- linear systems with constant coefficients ------------------------------------

def linear1(a: float, f: float, x0: float) -> ExpPoly:
    """x' = a x + f, x(0) = x0 (a ≠ 0): x = x∞ + (x0 − x∞) e^(a t), x∞ = −f/a."""
    if a == 0:
        return ExpPoly.const(x0) + ExpPoly.t(f)
    x_inf = -f / a
    return ExpPoly.const(x_inf) + ExpPoly.exp(a, x0 - x_inf)


def eig2(m: Sequence[Sequence[float]]) -> tuple[complex, complex]:
    """The two eigenvalues of a real 2 × 2 matrix."""
    tr = m[0][0] + m[1][1]
    det = m[0][0] * m[1][1] - m[0][1] * m[1][0]
    root = cmath.sqrt(tr * tr - 4.0 * det)
    # the larger-magnitude root first, the other from the product (no cancellation)
    l1 = (tr + root) / 2 if tr.real >= 0 else (tr - root) / 2
    l2 = det / l1 if l1 != 0 else (tr - l1)
    return l1, l2


def linear2(m: Sequence[Sequence[float]], f: Sequence[float],
            x0: Sequence[float]) -> tuple[ExpPoly, ExpPoly]:
    """x' = M x + f, x(0) = x0, for a 2 × 2 M with distinct eigenvalues λ₁,₂
    and det M ≠ 0: x(t) = x∞ + e^(λ₁t) P₁ (x0 − x∞) + e^(λ₂t) P₂ (x0 − x∞),
    with x∞ = −M⁻¹ f and the spectral projectors P₁ = (M − λ₂I)/(λ₁ − λ₂),
    P₂ = (M − λ₁I)/(λ₂ − λ₁)."""
    (a, b), (c, d) = m
    det = a * d - b * c
    if det == 0:
        raise ValueError("linear2 needs a non-singular matrix")
    x_inf = (-(d * f[0] - b * f[1]) / det, -(-c * f[0] + a * f[1]) / det)
    l1, l2 = eig2(m)
    if abs(l1 - l2) <= 1e-9 * max(abs(l1), abs(l2)):
        raise ValueError("linear2 needs distinct eigenvalues")
    dx = (x0[0] - x_inf[0], x0[1] - x_inf[1])
    out = []
    for i in range(2):
        poly = ExpPoly.const(x_inf[i])
        for lam, other in ((l1, l2), (l2, l1)):
            # row i of (M − other·I) / (lam − other) applied to dx
            row = (a - other, b) if i == 0 else (c, d - other)
            coef = (row[0] * dx[0] + row[1] * dx[1]) / (lam - other)
            poly = poly + ExpPoly.exp(lam, coef)
        out.append(poly)
    return out[0], out[1]


# ---- phases --------------------------------------------------------------------------

class Piecewise:
    """A signal made of phases: ``phases`` is [(t_start, fn), …] in time
    order, each fn taking the absolute time. At a phase boundary the value
    is the new phase's (the right limit)."""

    def __init__(self, phases: list[tuple[float, Callable[[float], float]]]):
        self.phases = phases

    def __call__(self, t: float) -> float:
        fn = self.phases[0][1]
        for t0, f in self.phases:
            if t >= t0:
                fn = f
        return fn(t)


def after(t0: float, poly: ExpPoly) -> Callable[[float], float]:
    """A phase given in its own time τ = t − t0."""
    return lambda t: poly(t - t0)


# ---- special functions and solvers --------------------------------------------------------

def lambert_w0(z: float) -> float:
    """W₀(z) for z ≥ 0: the w ≥ 0 with w·e^w = z (Halley's iteration)."""
    if z < 0:
        raise ValueError("lambert_w0 is used here for z ≥ 0 only")
    if z == 0:
        return 0.0
    w = math.log1p(z) if z < 3 else math.log(z) - math.log(math.log(z))
    for _ in range(100):
        ew = math.exp(w)
        f = w * ew - z
        step = f / (ew * (w + 1) - (w + 2) * f / (2 * w + 2))
        w -= step
        if abs(step) <= 1e-16 * (1 + abs(w)):
            break
    return w


def _poly_eval(p: Sequence[float], x: float) -> float:
    """p[0] + p[1] x + p[2] x² + …"""
    s = 0.0
    for c in reversed(p):
        s = s * x + c
    return s


def _poly_mul(p: Sequence[float], q: Sequence[float]) -> list[float]:
    out = [0.0] * (len(p) + len(q) - 1)
    for i, a in enumerate(p):
        for j, b in enumerate(q):
            out[i + j] += a * b
    return out


def rational_integral(num: Sequence[float], poles: Sequence[float]
                      ) -> Callable[[float, float], float]:
    """∫ₓ₀ˣ¹ N(x) / Πⱼ (x − rⱼ) dx for distinct real poles rⱼ (none inside
    [x0, x1]). ``num`` holds N's coefficients from the constant up. By
    polynomial division N = Q·D + R, then R/D = Σⱼ R(rⱼ)/D'(rⱼ) / (x − rⱼ):
    the integral is that of Q plus Σⱼ cⱼ ln|x − rⱼ|."""
    den = [1.0]
    for r in poles:
        den = _poly_mul(den, [-r, 1.0])
    rem = list(num)
    quot = [0.0] * max(1, len(num) - len(den) + 1)
    while len(rem) >= len(den) and any(rem):
        shift = len(rem) - len(den)
        c = rem[-1] / den[-1]
        quot[shift] = c
        for i, dcoef in enumerate(den):
            rem[shift + i] -= c * dcoef
        rem.pop()
    residues = []
    for j, r in enumerate(poles):
        d_prime = 1.0
        for k, s in enumerate(poles):
            if k != j:
                d_prime *= r - s
        residues.append(_poly_eval(rem, r) / d_prime)
    anti = [0.0] + [c / (i + 1) for i, c in enumerate(quot)]

    def integral(x0: float, x1: float) -> float:
        total = _poly_eval(anti, x1) - _poly_eval(anti, x0)
        for c, r in zip(residues, poles):
            if c:
                total += c * math.log(abs(x1 - r) / abs(x0 - r))
        return total

    return integral


def bisect(fn: Callable[[float], float], lo: float, hi: float) -> float:
    """The root of fn in [lo, hi] (fn(lo) and fn(hi) of opposite signs or
    zero), bisected until the interval stops shrinking."""
    f_lo = fn(lo)
    if f_lo == 0:
        return lo
    for _ in range(2000):
        mid = 0.5 * (lo + hi)
        if mid <= lo or mid >= hi:
            break
        f_mid = fn(mid)
        if f_mid == 0:
            return mid
        if (f_mid > 0) == (f_lo > 0):
            lo, f_lo = mid, f_mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def first_crossing(fn: Callable[[float], float], level: float, t0: float, t1: float,
                   samples: int = 4000) -> float:
    """The first t in [t0, t1] at which fn reaches ``level`` (from either
    side): bracketed on a grid of ``samples`` points, then bisected. Raises
    ValueError when fn does not reach it."""
    prev_t, prev = t0, fn(t0) - level
    if prev == 0:
        return t0
    for k in range(1, samples + 1):
        t = t0 + (t1 - t0) * k / samples
        cur = fn(t) - level
        if cur == 0 or (cur > 0) != (prev > 0):
            return bisect(lambda s: fn(s) - level, prev_t, t)
        prev_t, prev = t, cur
    raise ValueError(f"the signal does not reach {level} between {t0} and {t1}")
