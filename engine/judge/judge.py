#!/usr/bin/env python3
"""The outside judge of the engine's solver: published problems with their
own reference solutions, run at rtol 1e-4 ... 1e-10 and scored the way the
problems' sources score solvers (error against the reference, significant
correct digits, steps, error-test failures, events, wall time).

    judge.py fetch    CACHE                  download the sources, checked by SHA-256
    judge.py cases    CACHE CASES [--sweep]  write the models (text format), the
                                             runs and their reference solutions
    judge.py run      CASES RESULTS [--runner EXE] [--timeout S] [case ...]
    judge.py score    CASES RESULTS [--before RESULTS0] [--out FILE]
    judge.py sundials CACHE WORK             SUNDIALS' own IDAS examples, built
                                             against the engine's IDAS and upstream's
    judge.py omc      CASES RESULTS          the same models in OpenModelica, if `omc`
                                             is on the PATH

`run` takes the runner built by `cargo build --release -p lsim-project
--example judge`. CACHE, CASES and RESULTS are folders outside the
repository: nothing the judge downloads or writes belongs in it (README.md
says why, source by source). Needs Python 3.10+ with mpmath and scipy (for
the reference solutions) and, for `sundials`, a C compiler and CMake.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
ENGINE = HERE.parent
RTOLS = [1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10]

# ---------------------------------------------------------------------------
# Sources: where each comes from, its terms, and the SHA-256 of the file the
# judge was built against (a changed file stops `fetch`)
# ---------------------------------------------------------------------------

TESTSET = "https://archimede.uniba.it/~testset"
IFTOMM = "https://www.iftomm-multibody.org/media"
RFMU = "https://raw.githubusercontent.com/modelica/Reference-FMUs/v0.0.41"
SUNDIALS = "https://raw.githubusercontent.com/LLNL/sundials"
SUNDIALS_REL = "https://github.com/LLNL/sundials/releases/download"

SOURCES = [
    # The Bari IVP Test Set (F. Mazzia, C. Magherini, release 2.4, 2008): free
    # to download, no licence stated. Its reference solutions are read from
    # these files when scoring; none of them is copied into the repository.
    ("ivptestset/andrews.f", f"{TESTSET}/src/problems/andrews.f",
     "d5c1ce9c1d849bb048f9a478b2f6c6918e6c3048b529e55658d50caef13280a6"),
    ("ivptestset/andrews.pdf", f"{TESTSET}/report/andrews.pdf",
     "265d826997921e54c7e7d0c57aee0f8519cfa8ae1b5be6b8145f23961cf9eda7"),
    ("ivptestset/caraxis.f", f"{TESTSET}/src/problems/caraxis.f",
     "d673be59873bbe50bd209c09ddb829f7b73155f0522f3d606cc3d26b9bfbe75c"),
    ("ivptestset/caraxis.pdf", f"{TESTSET}/report/caraxis.pdf",
     "12837551919e4c320e60012b8b5edde46b019373e1d4642f3c9d19352c4b6782"),
    ("ivptestset/chemakzo.f", f"{TESTSET}/src/problems/chemakzo.f",
     "2d3f9e5a764afd5ec708267c7667a6a5f8f4430621184c416883435d93c692ba"),
    ("ivptestset/chemakzo.pdf", f"{TESTSET}/report/chemakzo.pdf",
     "cae6184bfae961693439a3203a86be71d9d04e41d493b47f20aa509acf8d704c"),
    # The IFToMM Library of Computational Benchmark Problems (Masoudi et al.,
    # ECCOMAS Multibody Dynamics 2013): research use only. The descriptions
    # and the results other groups submitted stay in the cache.
    ("iftomm/pendulum.pdf", f"{IFTOMM}/problems/planar%20simple%20pendulum/planar%20simple%20pendulum.pdf",
     "392951abe273d4f3f9866852e4f554c2e27cde9343f6b00450018272eb5dbf57"),
    ("iftomm/pendulum-gonzalez-74.txt", f"{IFTOMM}/solutions/planar%20simple%20pendulum/francisco%20gonz%C3%A1lez/74/results.txt",
     "2c19faf741db9c231b1bcfa495585b425e3259f51510436d4170a6f979c45ac9"),
    ("iftomm/pendulum-tagliapietra-106.txt", f"{IFTOMM}/solutions/planar%20simple%20pendulum/luca%20tagliapietra/106/results.txt",
     "8f100f8ca903b2216e17c9a0f03548e365a638807838381d4267348815b4aee5"),
    ("iftomm/pendulum-gonzalez-129.txt", f"{IFTOMM}/solutions/planar%20simple%20pendulum/francisco%20gonz%C3%A1lez/129/results.txt",
     "d52177d6de7dcb4abc09655dec8b1eba5ea4c81529c9b65be5de32a81db7c828"),
    ("iftomm/fourbar.pdf", f"{IFTOMM}/problems/multiple%20four-bar%20mechanism/multiple%20four-bar%20mechanism.pdf",
     "71a04877540a2a6bb4669fe892b70ceed514a1ab12cc5c6d61fd43d4a8b8b699"),
    ("iftomm/fourbar-callejo-71.txt", f"{IFTOMM}/solutions/multiple%20four-bar%20mechanism/alfonso%20callejo/71/results.txt",
     "7e0e7eebb76694f6ad329eedc247c220228407e0aa2333b30eea97f1062522df"),
    ("iftomm/fourbar-burkhardt-95.txt", f"{IFTOMM}/solutions/multiple%20four-bar%20mechanism/markus%20burkhardt/95/results.txt",
     "7bef0e8238fc8fc66bec6d5534825307e36913b3161635d2bfbcb4084bac8965"),
    # The Reference FMUs v0.0.41 (Modelica Association, BSD-2-Clause)
    ("reference-fmus/LICENSE.txt", f"{RFMU}/LICENSE.txt",
     "3bd19affe120c783d1bb63f423ba1e5b31fed892d98d583001aac013a3432041"),
    ("reference-fmus/Stair/model.c", f"{RFMU}/Stair/model.c",
     "5cca4268af479bfc7a13793caabf7b85bac713755ae1e6a82ac51ad59a17ba53"),
    ("reference-fmus/Stair/FMI3.xml", f"{RFMU}/Stair/FMI3.xml",
     "b6ccd0d3e714f086d85310371d39e51e1667dd9e7fde7d3c7e7d44d129626ffc"),
    ("reference-fmus/Stair/Stair_out.csv", f"{RFMU}/Stair/Stair_out.csv",
     "15e6142e3422925c88cd73b0c7993016228988cc2a5832d99cbdf23170828f30"),
    ("reference-fmus/BouncingBall/model.c", f"{RFMU}/BouncingBall/model.c",
     "64a2072f95310bb0d045e3d5cdd2e7fa8a3bddb2dc0c24eda574851a19be9d23"),
    ("reference-fmus/BouncingBall/FMI3.xml", f"{RFMU}/BouncingBall/FMI3.xml",
     "a3ea534af11ccb72f41b9cb6aba6c37f14b6c1c78e3321a592d96daf54763e52"),
    ("reference-fmus/BouncingBall/BouncingBall_out.csv", f"{RFMU}/BouncingBall/BouncingBall_out.csv",
     "7123af5e548a120e20a8167a7ece4df442474b7788a31bf57d2c0c177702a150"),
    # SUNDIALS (LLNL, BSD-3-Clause): the release the engine vendors, the
    # latest, and their IDAS examples with quadratures under error control
    ("sundials/sundials-7.1.1.tar.gz", f"{SUNDIALS_REL}/v7.1.1/sundials-7.1.1.tar.gz",
     "ea7d6edfb52448ddfdc1ec48f89a721fe6c0a259c10f8ef56f60fcded87a94bb"),
    ("sundials/sundials-7.9.0.tar.gz", f"{SUNDIALS_REL}/v7.9.0/sundials-7.9.0.tar.gz",
     "13f898a27b48fe3449483f9e438a800ed545abf93bc2e2ceec2d1e00ae8db5ef"),
    ("sundials/v7.1.1/idasSlCrank_dns.c", f"{SUNDIALS}/v7.1.1/examples/idas/serial/idasSlCrank_dns.c",
     "360f7fc4bfdf30a2d77aff399fff947a8bba2438d6a8b8cfa579b6bc2046d9d4"),
    ("sundials/v7.1.1/idasSlCrank_dns.out", f"{SUNDIALS}/v7.1.1/examples/idas/serial/idasSlCrank_dns.out",
     "a10754d1065e7e74cee97cad5cdc1eb4c8bc1962ffae77bcabda328984c69683"),
    ("sundials/v7.1.1/idasAkzoNob_dns.c", f"{SUNDIALS}/v7.1.1/examples/idas/serial/idasAkzoNob_dns.c",
     "e3adff468d0e9e78926ea3b0e496ca38978ec2c56eeeb89c290d3ba3acf2a525"),
    ("sundials/v7.1.1/idasAkzoNob_dns.out", f"{SUNDIALS}/v7.1.1/examples/idas/serial/idasAkzoNob_dns.out",
     "dd69bfec8d242eca7dbc7cde8571d6963e2778ed2b7ebe1a0d63c35ff1074f58"),
    ("sundials/v7.9.0/idasSlCrank_dns.c", f"{SUNDIALS}/v7.9.0/examples/idas/serial/idasSlCrank_dns.c",
     "78616be92f3507e5801b9a3d9b2cb3f09621426e625cf34dca1ffd05483024a9"),
    ("sundials/v7.9.0/idasSlCrank_dns.out", f"{SUNDIALS}/v7.9.0/examples/idas/serial/idasSlCrank_dns.out",
     "90c92504aa8887bcfe7d8da0b9fe9457f915d801e50fe0ad94fda256d85f262e"),
    ("sundials/v7.9.0/idasAkzoNob_dns.c", f"{SUNDIALS}/v7.9.0/examples/idas/serial/idasAkzoNob_dns.c",
     "845429ae5ed59d5d36f43ba1d6d928f7b6f92f98aa9616d3e81531ffd6780e05"),
    ("sundials/v7.9.0/idasAkzoNob_dns.out", f"{SUNDIALS}/v7.9.0/examples/idas/serial/idasAkzoNob_dns.out",
     "dd69bfec8d242eca7dbc7cde8571d6963e2778ed2b7ebe1a0d63c35ff1074f58"),
]


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def fetch(cache: Path) -> None:
    bad = []
    for rel, url, digest in SOURCES:
        path = cache / rel
        if path.exists() and sha256(path) == digest:
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        print(f"fetching {url}", file=sys.stderr)
        tmp = path.with_suffix(path.suffix + ".part")
        with urllib.request.urlopen(url, timeout=300) as r, open(tmp, "wb") as f:
            shutil.copyfileobj(r, f)
        got = sha256(tmp)
        if got != digest:
            bad.append(f"{rel}: SHA-256 {got}, expected {digest} ({url})")
            continue
        tmp.replace(path)
    (cache / "SHA256SUMS").write_text(
        "".join(f"{d}  {rel}\n" for rel, _, d in SOURCES))
    if bad:
        sys.exit("changed upstream, not used:\n  " + "\n  ".join(bad))


def cached(cache: Path, rel: str) -> Path:
    path = cache / rel
    digest = next(d for r, _, d in SOURCES if r == rel)
    if not path.exists() or sha256(path) != digest:
        sys.exit(f"{path}: missing or changed; run `judge.py fetch {cache}`")
    return path


# ---------------------------------------------------------------------------
# The models, in the engine's text format (engine/docs/text-format.md). Each
# transcribes a published problem statement, cited in its description; the
# reference solutions stay in the cache or are computed here.
# ---------------------------------------------------------------------------

def num(x) -> str:
    """A number as the text format reads it, to the last digit."""
    return repr(float(x)) if not isinstance(x, str) else x


def andrews() -> tuple[str, dict]:
    """Andrews' squeezing mechanism, index 3: E. Hairer, G. Wanner, Solving
    Ordinary Differential Equations II (1996), pp. 530-536, as the Bari IVP
    Test Set states it (problem 13, report/andrews.pdf)."""
    consts = [  # name, value, unit
        ("m1", ".04325", "kg"), ("m2", ".00365", "kg"), ("m3", ".02373", "kg"),
        ("m4", ".00706", "kg"), ("m5", ".07050", "kg"), ("m6", ".00706", "kg"),
        ("m7", ".05498", "kg"),
        ("xa", "-.06934", "m"), ("ya", "-.00227", "m"), ("xb", "-.03635", "m"),
        ("yb", ".03273", "m"), ("xc", ".014", "m"), ("yc", ".072", "m"),
        ("c0", "4530", "N/m"),
        ("i1", "2.194e-6", "kg.m2"), ("i2", "4.410e-7", "kg.m2"), ("i3", "5.255e-6", "kg.m2"),
        ("i4", "5.667e-7", "kg.m2"), ("i5", "1.169e-5", "kg.m2"), ("i6", "5.667e-7", "kg.m2"),
        ("i7", "1.912e-5", "kg.m2"),
        ("d", "28e-3", "m"), ("da", "115e-4", "m"), ("e", "2e-2", "m"), ("ea", "1421e-5", "m"),
        ("rr", "7e-3", "m"), ("ra", "92e-5", "m"), ("l0", "7785e-5", "m"),
        ("ss", "35e-3", "m"), ("sa", "1874e-5", "m"), ("sb", "1043e-5", "m"),
        ("sc", "18e-3", "m"), ("sd", "2e-2", "m"), ("ta", "2308e-5", "m"),
        ("tb", "916e-5", "m"), ("u", "4e-2", "m"), ("ua", "1228e-5", "m"),
        ("ub", "449e-5", "m"), ("zf", "2e-2", "m"), ("zt", "4e-2", "m"),
        ("fa", "1421e-5", "m"), ("mom", "33e-3", "N.m"),
    ]
    # the consistent start (the test set's y0 and y0'), to its 30 digits
    q0 = ["-0.0617138900142764496358948458001", "0", "0.455279819163070380255912382449",
          "0.222668390165885884674473185609", "0.487364979543842550225598953530",
          "-0.222668390165885884674473185609", "1.23054744454982119249735015568"]
    lam0 = ["98.5668703962410896057654982170", "-6.12268834425566265503114393122",
            "0", "0", "0", "0"]
    names = ["beta", "Theta", "gamma", "Phi", "delta", "Omega", "epsilon"]
    lines = ['model Judge.Andrews "Andrews\' squeezing mechanism, index 3: 7 bodies, '
             '7 angles, 6 constraints (Hairer & Wanner 1996, pp. 530-536; '
             'Bari IVP Test Set, problem 13)."']
    for n, v, u in consts:
        lines.append(f'  parameter Real {n}(unit = "{u}") = {v};')
    for i in range(7):
        fixed = ", fixed = true" if i == 0 else ""
        lines.append(f'  Real q{i+1}(unit = "rad", start = {q0[i]}{fixed}) "{names[i]}";')
    for i in range(7):
        fixed = ", fixed = true" if i == 0 else ""
        lines.append(f'  Real v{i+1}(unit = "rad/s", start = 0{fixed}) "d{names[i]}/dt";')
    for k in range(6):
        lines.append(f'  Real lam{k+1}(unit = "N", start = {lam0[k]}) "constraint force {k+1}";')
    lines += [
        '  Real xd(unit = "m") = sd * cos(q3) + sc * sin(q3) + xb;',
        '  Real yd(unit = "m") = sd * sin(q3) - sc * cos(q3) + yb;',
        '  Real ls(unit = "m") = sqrt((xd - xc) ^ 2 + (yd - yc) ^ 2) "the spring\'s length";',
        '  Real fs(unit = "N/m") = -c0 * (ls - l0) / ls;',
        '  Real fx(unit = "N") = fs * (xd - xc);',
        '  Real fy(unit = "N") = fs * (yd - yc);',
        "equation",
    ]
    for i in range(7):
        lines.append(f"  der(q{i+1}) = v{i+1};")
    # M(q) q'' = f(q, q') - G(q)^T lambda, G = dg/dq (the test set's gp)
    s12, c12 = "sin(q1 + q2)", "cos(q1 + q2)"
    s45, c45 = "sin(q4 + q5)", "cos(q4 + q5)"
    s67, c67 = "sin(q6 + q7)", "cos(q6 + q7)"
    gx1, gy1 = f"(-rr * sin(q1) + d * {s12})", f"(rr * cos(q1) - d * {c12})"
    gx2, gy2 = f"d * {s12}", f"(-d * {c12})"
    eqs = [
        ("(m1 * ra ^ 2 + m2 * (rr ^ 2 - 2 * da * rr * cos(q2) + da ^ 2) + i1 + i2) * der(v1)"
         " + (m2 * (da ^ 2 - da * rr * cos(q2)) + i2) * der(v2)",
         "mom - m2 * da * rr * v2 * (v2 + 2 * v1) * sin(q2)",
         f"{gx1} * (lam1 + lam3 + lam5) + {gy1} * (lam2 + lam4 + lam6)"),
        ("(m2 * (da ^ 2 - da * rr * cos(q2)) + i2) * der(v1) + (m2 * da ^ 2 + i2) * der(v2)",
         "m2 * da * rr * v1 ^ 2 * sin(q2)",
         f"{gx2} * (lam1 + lam3 + lam5) + {gy2} * (lam2 + lam4 + lam6)"),
        ("(m3 * (sa ^ 2 + sb ^ 2) + i3) * der(v3)",
         "fx * (sc * cos(q3) - sd * sin(q3)) + fy * (sd * cos(q3) + sc * sin(q3))",
         "(-ss * cos(q3)) * lam1 + (-ss * sin(q3)) * lam2"),
        ("(m4 * (e - ea) ^ 2 + i4) * der(v4) + (m4 * ((e - ea) ^ 2 + zt * (e - ea) * sin(q4)) + i4) * der(v5)",
         "m4 * zt * (e - ea) * v5 ^ 2 * cos(q4)",
         f"(-e * {c45}) * lam3 + (-e * {s45}) * lam4"),
        ("(m4 * ((e - ea) ^ 2 + zt * (e - ea) * sin(q4)) + i4) * der(v4)"
         " + (m4 * (zt ^ 2 + 2 * zt * (e - ea) * sin(q4) + (e - ea) ^ 2) + m5 * (ta ^ 2 + tb ^ 2) + i4 + i5) * der(v5)",
         "-m4 * zt * (e - ea) * v4 * (v4 + 2 * v5) * cos(q4)",
         f"(-e * {c45} + zt * sin(q5)) * lam3 + (-e * {s45} - zt * cos(q5)) * lam4"),
        ("(m6 * (zf - fa) ^ 2 + i6) * der(v6) + (m6 * ((zf - fa) ^ 2 - u * (zf - fa) * sin(q6)) + i6) * der(v7)",
         "-m6 * u * (zf - fa) * v7 ^ 2 * cos(q6)",
         f"zf * {s67} * lam5 + (-zf * {c67}) * lam6"),
        ("(m6 * ((zf - fa) ^ 2 - u * (zf - fa) * sin(q6)) + i6) * der(v6)"
         " + (m6 * ((zf - fa) ^ 2 - 2 * u * (zf - fa) * sin(q6) + u ^ 2) + m7 * (ua ^ 2 + ub ^ 2) + i6 + i7) * der(v7)",
         "m6 * u * (zf - fa) * v6 * (v6 + 2 * v7) * cos(q6)",
         f"(zf * {s67} - u * cos(q7)) * lam5 + (-zf * {c67} - u * sin(q7)) * lam6"),
    ]
    for i, (lhs, f, gl) in enumerate(eqs):
        lines.append(f'  {lhs} = {f} - ({gl}) "the motion of {names[i]}";')
    cons = [
        f"rr * cos(q1) - d * {c12} - ss * sin(q3) - xb",
        f"rr * sin(q1) - d * {s12} + ss * cos(q3) - yb",
        f"rr * cos(q1) - d * {c12} - e * {s45} - zt * cos(q5) - xa",
        f"rr * sin(q1) - d * {s12} + e * {c45} - zt * sin(q5) - ya",
        f"rr * cos(q1) - d * {c12} - zf * {c67} - u * sin(q7) - xa",
        f"rr * sin(q1) - d * {s12} - zf * {s67} + u * cos(q7) - ya",
    ]
    for k, g in enumerate(cons):
        lines.append(f'  0 = {g} "loop closure {k+1}";')
    lines.append("end Judge.Andrews;")
    case = {
        "model": "Judge.Andrews", "t_end": 0.03, "dt": 0.0003, "atol_factor": 1.0,
        "record": [f"q{i+1}" for i in range(7)],
        "judge": {"kind": "testset", "file": "ivptestset/andrews.f",
                  # y(1..7) = q, y(8..14) = q', y(15..21) = q'', y(22..27) = lambda
                  # (q'' is a channel by either name, as index reduction keeps it)
                  "channels": [f"q{i+1}" for i in range(7)] + [f"v{i+1}" for i in range(7)]
                  + [[f"der(v{i+1})", f"der(der(q{i+1}))"] for i in range(7)]
                  + [f"lam{k+1}" for k in range(6)],
                  # the test set's scd counts the first seven components only
                  "scd": list(range(7))},
    }
    return "\n".join(lines) + "\n", case


def caraxis() -> tuple[str, dict]:
    """The car axis, index 3: S. Schneider, PhD thesis, Geneva 1994, as the
    Bari IVP Test Set states it (problem 17, report/caraxis.pdf)."""
    text = '''model Judge.CarAxis "The car axis, index 3: a bar on two springs over a bumpy road (Schneider 1994; Bari IVP Test Set, problem 17)."
  parameter Real L(unit = "m") = 1 "the axis' length";
  parameter Real L0(unit = "m") = 0.5 "the springs' length at rest";
  parameter Real k(unit = "N/m") = 1 "the springs' stiffness";
  parameter Real eps = 1e-2;
  parameter Real M(unit = "kg") = 10;
  parameter Real r(unit = "m") = 0.1 "the bump's height";
  parameter Real w(unit = "rad/s") = 10 "the bumps' angular frequency";
  parameter Real g(unit = "m/s2") = 1;
  Real xl(unit = "m", start = 0);
  Real yl(unit = "m", start = 0.5, fixed = true);
  Real xr(unit = "m", start = 1);
  Real yr(unit = "m", start = 0.5, fixed = true);
  Real ul(unit = "m/s", start = -0.5) "dxl/dt";
  Real vl(unit = "m/s", start = 0, fixed = true) "dyl/dt";
  Real ur(unit = "m/s", start = -0.5) "dxr/dt";
  Real vr(unit = "m/s", start = 0, fixed = true) "dyr/dt";
  Real lam1(unit = "N/m", start = 0);
  Real lam2(unit = "N/m", start = 0);
  Real yb(unit = "m") = r * sin(w * time) "the right wheel's height";
  Real xb(unit = "m") = sqrt(L * L - yb * yb);
  Real Ll(unit = "m") = sqrt(xl ^ 2 + yl ^ 2);
  Real Lr(unit = "m") = sqrt((xr - xb) ^ 2 + (yr - yb) ^ 2);
equation
  der(xl) = ul;
  der(yl) = vl;
  der(xr) = ur;
  der(yr) = vr;
  M * eps ^ 2 / 2 * der(ul) = k * (L0 - Ll) * xl / Ll + lam1 * xb + 2 * lam2 * (xl - xr);
  M * eps ^ 2 / 2 * der(vl) = k * (L0 - Ll) * yl / Ll + lam1 * yb + 2 * lam2 * (yl - yr) - M * eps ^ 2 * g / 2;
  M * eps ^ 2 / 2 * der(ur) = k * (L0 - Lr) * (xr - xb) / Lr - 2 * lam2 * (xl - xr);
  M * eps ^ 2 / 2 * der(vr) = k * (L0 - Lr) * (yr - yb) / Lr - 2 * lam2 * (yl - yr) - M * eps ^ 2 * g / 2;
  0 = xb * xl + yb * yl "the left spring stays at right angles to the axis";
  0 = (xl - xr) ^ 2 + (yl - yr) ^ 2 - L * L "the bar keeps its length";
end Judge.CarAxis;
'''
    case = {
        "model": "Judge.CarAxis", "t_end": 3.0, "dt": 0.01, "atol_factor": 1.0,
        "record": ["xl", "yl", "xr", "yr"],
        "judge": {"kind": "testset", "file": "ivptestset/caraxis.f",
                  "channels": ["xl", "yl", "xr", "yr", "ul", "vl", "ur", "vr", "lam1", "lam2"],
                  "scd": list(range(10))},
    }
    return text, case


def chemakzo() -> tuple[str, dict]:
    """The chemical Akzo Nobel problem, index 1 (Bari IVP Test Set, problem 3;
    also SUNDIALS' idasAkzoNob_dns)."""
    text = '''model Judge.ChemAkzo "The chemical Akzo Nobel problem, index 1 (Bari IVP Test Set, problem 3)."
  parameter Real k1(unit = "s-1") = 18.7;
  parameter Real k2(unit = "s-1") = 0.58;
  parameter Real k3(unit = "s-1") = 0.09;
  parameter Real k4(unit = "s-1") = 0.42;
  parameter Real K = 34.4;
  parameter Real klA(unit = "s-1") = 3.3;
  parameter Real Ks = 115.83;
  parameter Real pCO2 = 0.9;
  parameter Real H = 737;
  Real y1(start = 0.444, fixed = true);
  Real y2(start = 0.00123, fixed = true);
  Real y3(start = 0, fixed = true);
  Real y4(start = 0.007, fixed = true);
  Real y5(start = 0, fixed = true);
  Real y6(start = 0.36);
  Real r1(unit = "s-1") = k1 * y1 ^ 4 * sqrt(y2);
  Real r2(unit = "s-1") = k2 * y3 * y4;
  Real r3(unit = "s-1") = k2 / K * y1 * y5;
  Real r4(unit = "s-1") = k3 * y1 * y4 ^ 2;
  Real r5(unit = "s-1") = k4 * y6 ^ 2 * sqrt(y2);
  Real Fin(unit = "s-1") = klA * (pCO2 / H - y2);
equation
  der(y1) = -2 * r1 + r2 - r3 - r4;
  der(y2) = -0.5 * r1 - r4 - 0.5 * r5 + Fin;
  der(y3) = r1 - r2 + r3;
  der(y4) = -r2 + r3 - 2 * r4;
  der(y5) = r2 - r3 + r5;
  0 = Ks * y1 * y4 - y6;
end Judge.ChemAkzo;
'''
    case = {
        "model": "Judge.ChemAkzo", "t_end": 180.0, "dt": 1.0, "atol_factor": 1.0,
        "record": ["y1"],
        "judge": {"kind": "testset", "file": "ivptestset/chemakzo.f",
                  "channels": [f"y{i}" for i in range(1, 7)], "scd": list(range(6))},
    }
    return text, case


PENDULUM = '''model Judge.Pendulum "A point mass on a massless rod of length L in a vertical plane, in x and y (index 3): the IFToMM planar simple pendulum with x0 = -L sin(theta0), or given the speed v0 at the bottom."
  parameter Real m(unit = "kg") = 1;
  parameter Real L(unit = "m") = 1;
  parameter Real g(unit = "m/s2") = 9.81;
  parameter Real theta0 = {theta0} "the angle from the downward vertical at the start";
  parameter Real v0(unit = "m/s") = {v0} "the speed at the start, at right angles to the rod";
  Real x(unit = "m", start = L * sin(theta0), fixed = true);
  Real y(unit = "m", start = -L * cos(theta0));
  Real vx(unit = "m/s", start = v0 * cos(theta0), fixed = true);
  Real vy(unit = "m/s", start = v0 * sin(theta0));
  Real F(unit = "N", start = m * g) "the rod's tension";
equation
  der(x) = vx;
  der(y) = vy;
  m * der(vx) = -x * F / L "the rod pulls along x";
  m * der(vy) = -y * F / L - m * g "the rod and gravity pull along y";
  x * x + y * y = L * L "the rod keeps its length";
end Judge.Pendulum;
'''


def pendulum(theta0: float, v0: float, t_end: float, kind: dict) -> tuple[str, dict]:
    text = PENDULUM.replace("{theta0}", num(theta0)).replace("{v0}", num(v0))
    case = {"model": "Judge.Pendulum", "t_end": t_end, "dt": 0.01, "atol_factor": 1e-2,
            "record": ["x", "y", "vx", "vy"],
            "judge": dict(kind="pendulum", theta0=theta0, v0=v0, m=1.0, L=1.0, g=9.81, **kind)}
    return text, case


def fourbar() -> tuple[str, dict]:
    """The IFToMM multiple four-bar mechanism (A. Callejo): a grid of 5 x 5
    four-bar loops of 1 m, 1 kg uniform bars hanging from a fixed top row,
    under gravity, in natural coordinates (the nodes' positions): 60
    coordinates, 55 constraints (each bar's length), 5 degrees of freedom.
    Each bar between nodes a and b adds m/6 (2 a'' + b'') to a's equation of
    motion and the force -2 lambda (a - b) of its length constraint
    (|a - b|^2 = L^2); its weight is shared by its ends. It passes through
    the singular positions where a row's bars line up."""
    n = 5
    w0 = 2 * math.pi / 3
    lines = ['model Judge.FourBar "The IFToMM multiple four-bar mechanism (Callejo): 5 x 5 four-bar loops '
             'of 1 m, 1 kg uniform bars hanging from a fixed top row, in natural coordinates."',
             '  parameter Real m(unit = "kg") = 1 "each bar\'s mass";',
             '  parameter Real L(unit = "m") = 1 "each bar\'s length";',
             '  parameter Real g(unit = "m/s2") = 9.81;',
             f'  parameter Real w0(unit = "rad/s") = {num(w0)} "the bars\' angular velocity at the start";']

    def node(k, j):
        return None if k == 0 else f"{k}_{j}"

    for k in range(1, n + 1):
        for j in range(n + 1):
            p = node(k, j)
            fy = ", fixed = true" if j == 0 else ""
            lines.append(f'  Real y_{p}(unit = "m", start = {j}{fy});')
            lines.append(f'  Real z_{p}(unit = "m", start = {-k});')
            lines.append(f'  Real vy_{p}(unit = "m/s", start = {k} * w0 * L{fy});')
            lines.append(f'  Real vz_{p}(unit = "m/s", start = 0);')
    bars = []  # (a, b): a moving, b moving or a fixed node (k = 0)
    for k in range(1, n + 1):
        for j in range(n + 1):
            bars.append(((k, j), (k - 1, j), f"vertical bar {k},{j}"))
    for k in range(1, n + 1):
        for j in range(n):
            bars.append(((k, j), (k, j + 1), f"horizontal bar {k},{j}"))
    for i, (_, _, label) in enumerate(bars):
        lines.append(f'  Real lam{i}(unit = "N/m", start = 0) "the force per metre in the {label}";')
    lines.append("equation")

    def pos(c, k, j):
        return f"{c}_{k}_{j}" if k > 0 else (num(j) if c == "y" else "0")

    for k in range(1, n + 1):
        for j in range(n + 1):
            p = f"{k}_{j}"
            lines.append(f"  der(y_{p}) = vy_{p};")
            lines.append(f"  der(z_{p}) = vz_{p};")
    for k in range(1, n + 1):
        for j in range(n + 1):
            for c in ("y", "z"):
                inertia, force, weight = [], [], 0
                for i, (a, b, _) in enumerate(bars):
                    for me, other in ((a, b), (b, a)):
                        if me != (k, j):
                            continue
                        weight += 1
                        if other[0] > 0:
                            inertia.append(f"2 * der(v{c}_{k}_{j}) + der(v{c}_{other[0]}_{other[1]})")
                        else:
                            inertia.append(f"2 * der(v{c}_{k}_{j})")
                        force.append(f"lam{i} * ({c}_{k}_{j} - {pos(c, *other)})")
                lhs = f"m / 6 * ({' + '.join(inertia)})"
                rhs = f"-2 * ({' + '.join(force)})"
                if c == "z":
                    rhs += f" - {weight} * m * g / 2"
                lines.append(f'  {lhs} = {rhs} "the node {k},{j} along {c}";')
    for i, (a, b, label) in enumerate(bars):
        dy = f"({pos('y', *a)} - {pos('y', *b)})"
        dz = f"({pos('z', *a)} - {pos('z', *b)})"
        lines.append(f'  {dy} ^ 2 + {dz} ^ 2 = L ^ 2 "the {label} keeps its length";')
    lines.append("end Judge.FourBar;")
    record = []
    for k in range(1, n + 1):
        for j in range(n + 1):
            record += [f"y_{k}_{j}", f"z_{k}_{j}", f"vy_{k}_{j}", f"vz_{k}_{j}"]
    case = {"model": "Judge.FourBar", "t_end": 10.0, "dt": 0.01, "atol_factor": 1e-2,
            "record": record, "judge": {"kind": "fourbar", "n": n, "w0": w0, "g": 9.81}}
    return "\n".join(lines) + "\n", case


def stair() -> tuple[str, dict]:
    """The Reference FMUs' Stair (Stair/model.c): no continuous state, a
    counter from 1 that a time event raises each second; the FMU ends the
    run when it reaches 10 (at 9 s)."""
    text = '''model Judge.Stair "The Reference FMUs' Stair: a counter from 1, raised by a time event each second (no continuous state)."
  discrete Real counter(start = 1, fixed = true) "counts the seconds";
  discrete Real t_next(unit = "s", start = 1, fixed = true) "the next event";
equation
  when time >= pre(t_next) then
    counter = pre(counter) + 1;
    t_next = pre(t_next) + 1;
  end when "a second has passed";
end Judge.Stair;
'''
    case = {"model": "Judge.Stair", "t_end": 9.0, "dt": 0.2, "atol_factor": 1e-2,
            "record": ["counter"],
            "judge": {"kind": "stair", "csv": "reference-fmus/Stair/Stair_out.csv"}}
    return text, case


def bouncing_ball() -> tuple[str, dict]:
    """The Reference FMUs' BouncingBall (BouncingBall/model.c): h' = v,
    v' = g; at h <= 0 the ball bounces with v := -e v, and rests (v := 0,
    g := 0) once the speed it bounces with is below v_min."""
    text = '''model Judge.BouncingBall "The Reference FMUs' BouncingBall: a ball dropped from 1 m bounces back with 0.7 of its speed until that is below 0.1 m/s, then rests."
  parameter Real e = 0.7 "coefficient of restitution";
  parameter Real v_min(unit = "m/s") = 0.1 "the speed below which it rests";
  Real h(unit = "m", start = 1, fixed = true) "height";
  Real v(unit = "m/s", start = 0, fixed = true) "velocity";
  discrete Real g(unit = "m/s2", start = -9.81, fixed = true) "gravity, until it rests";
equation
  der(h) = v;
  der(v) = g;
  when h <= 0 then
    reinit(h, 0);
    reinit(v, if -e * pre(v) < v_min then 0 else -e * pre(v));
    g = if -e * pre(v) < v_min then 0 else pre(g);
  end when "it bounces";
end Judge.BouncingBall;
'''
    case = {"model": "Judge.BouncingBall", "t_end": 3.0, "dt": 0.01, "atol_factor": 1e-2,
            "record": ["h", "v"],
            "judge": {"kind": "bouncingball", "h0": 1.0, "g": -9.81, "e": 0.7, "v_min": 0.1,
                      "csv": "reference-fmus/BouncingBall/BouncingBall_out.csv"}}
    return text, case


def all_cases(sweep: bool) -> dict[str, tuple[str, dict]]:
    out = {
        "testset-andrews": andrews(),
        "testset-caraxis": caraxis(),
        "testset-chemakzo": chemakzo(),
        # the IFToMM planar simple pendulum: released from x = -1 m, y = 0
        "iftomm-pendulum": pendulum(-math.pi / 2, 0.0, 10.0, {"iftomm": True}),
        "iftomm-fourbar": fourbar(),
        "fmu-stair": stair(),
        "fmu-bouncingball": bouncing_ball(),
    }
    # the pendulum's closed form: released at rest from theta0 (swinging),
    # and given the speed v0 at the bottom so that m = 4 g L / v0^2 < 1
    # (looping)
    for theta0 in ([round(0.01 * i, 2) for i in range(1, 315)] if sweep
                   else [0.4, 1.5, 1.55, 3.0, 3.13]):
        out[f"pendulum-swing-{theta0:.2f}"] = pendulum(theta0, 0.0, 10.0, {})
    for m in [0.5, 0.9, 0.99, 0.999]:
        v0 = math.sqrt(4 * 9.81 * 1.0 / m)
        out[f"pendulum-loop-{m}"] = pendulum(0.0, v0, 10.0, {"loop_m": m})
    return out


# ---------------------------------------------------------------------------
# Reference solutions computed here (mpmath, 30+ digits)
# ---------------------------------------------------------------------------

def pendulum_reference(j: dict, times: list[float]) -> dict:
    """x, y, vx, vy of the pendulum in closed form (Jacobi elliptic
    functions), libration and rotation."""
    import mpmath as mp
    mp.mp.dps = 34
    g, L = mp.mpf(j["g"]), mp.mpf(j["L"])
    w = mp.sqrt(g / L)
    theta0 = mp.mpf(j["theta0"])
    v0 = mp.mpf(j["v0"])
    out = {"x": [], "y": [], "vx": [], "vy": []}

    if v0 == 0:
        # released at rest: sin(theta/2) = k sn(K - w t | k^2), k = sin(theta0/2)
        k = mp.sin(theta0 / 2)
        m = k * k
        K = mp.ellipk(m)

        def state(t):
            u = K - w * t
            sn, cn, dn = (mp.ellipfun(f, u, m=m) for f in ("sn", "cn", "dn"))
            s = k * sn  # sin(theta/2)
            th = 2 * mp.asin(s)
            # dtheta/dt = 2 d/dt asin(k sn) = -2 w k cn dn / cos(theta/2)
            thd = -2 * w * k * cn * dn / mp.sqrt(1 - s * s) if s * s < 1 else mp.mpf(0)
            return th, thd
    else:
        # rotation from the bottom (theta0 = 0) with v0 = L theta'(0):
        # theta / 2 = am(v0 t / (2 L) | m), m = 4 g L / v0^2
        m = 4 * g * L / v0 ** 2
        K = mp.ellipk(m)

        def am(u):
            n = mp.floor((u + K) / (2 * K))
            r = u - 2 * K * n
            sn, cn = mp.ellipfun("sn", r, m=m), mp.ellipfun("cn", r, m=m)
            return mp.pi * n + mp.atan2(sn, cn)

        def state(t):
            u = v0 * t / (2 * L)
            phi = am(u)
            dn = mp.sqrt(1 - m * mp.sin(phi) ** 2)
            return 2 * phi, (v0 / L) * dn

    for t in times:
        th, thd = state(mp.mpf(t))
        out["x"].append(float(L * mp.sin(th)))
        out["y"].append(float(-L * mp.cos(th)))
        out["vx"].append(float(L * mp.cos(th) * thd))
        out["vy"].append(float(L * mp.sin(th) * thd))
    return out


def fourbar_reference(j: dict, times: list[float], tol: str = "1e-28") -> dict:
    """The four-bar grid in its minimal coordinates: each row's bars turn
    together by the angle phi_k (the parallelograms keep their shape), the
    rows' horizontal bars translate. Lagrange's equations
    sum_b C_ab cos(phi_a - phi_b) phi_b'' + C_ab sin(phi_a - phi_b) phi_b'^2
    + dU/dphi_a = 0, integrated by Gragg-Bulirsch-Stoer extrapolation at 40
    digits with the step halved until the extrapolation's own error
    estimate is below `tol`. The bars point along (-cos phi, -sin phi) in
    (y, z): phi = pi/2 hangs them straight down."""
    import mpmath as mp
    mp.mp.dps = 40
    n = j["n"]
    g = mp.mpf(j["g"])
    C = [[0] * n for _ in range(n)]
    W = [0] * n
    for k in range(1, n + 1):
        for a in range(1, n + 1):
            W[a - 1] += 3 * ((a <= k - 1) + (a <= k)) + 5 * (a <= k)
            for b in range(1, n + 1):
                # vertical bars of row k: 6 x m/6 (|V_{k-1}|^2 + V_{k-1}.V_k + |V_k|^2),
                # horizontal bars: 5 x m/2 |V_k|^2, with T = 1/2 phi' M phi'
                C[a - 1][b - 1] += (2 * (a <= k - 1 and b <= k - 1)
                                    + (a <= k - 1 and b <= k) + (a <= k and b <= k - 1)
                                    + 7 * (a <= k and b <= k))

    def rhs(s):
        p, w = s[:n], s[n:]
        M = mp.matrix(n, n)
        r = mp.matrix(n, 1)
        for a in range(n):
            acc = g * W[a] * mp.cos(p[a])
            for b in range(n):
                d = p[a] - p[b]
                M[a, b] = C[a][b] * mp.cos(d)
                acc -= C[a][b] * mp.sin(d) * w[b] ** 2
            r[a] = acc
        x = mp.lu_solve(M, r)
        return list(w) + [x[i] for i in range(n)]

    def gbs(s, H, K=10):
        T = []
        for jj in range(1, K + 1):
            nn = 2 * jj
            h = H / nn
            z0 = s
            f0 = rhs(z0)
            z1 = [a + h * b for a, b in zip(z0, f0)]
            for _ in range(1, nn):
                f1 = rhs(z1)
                z0, z1 = z1, [a + 2 * h * b for a, b in zip(z0, f1)]
            f1 = rhs(z1)
            T.append([[(a + b + h * c) / 2 for a, b, c in zip(z0, z1, f1)]])
            for kk in range(1, jj):
                r = (mp.mpf(nn) / (2 * (jj - kk))) ** 2
                prev, prevm = T[jj - 1][kk - 1], T[jj - 2][kk - 1]
                T[jj - 1].append([a + (a - b) / (r - 1) for a, b in zip(prev, prevm)])
        best, second = T[K - 1][K - 1], T[K - 1][K - 2]
        return best, max(abs(a - b) for a, b in zip(best, second))

    def energy(s):
        p, w = s[:n], s[n:]
        T = sum(w[a] * C[a][b] * mp.cos(p[a] - p[b]) * w[b] for a in range(n) for b in range(n)) / 2
        Z = [mp.mpf(0)]
        for a in range(n):
            Z.append(Z[-1] - mp.sin(p[a]))
        U = g * sum(3 * (Z[k - 1] + Z[k]) + 5 * Z[k] for k in range(1, n + 1))
        return T + U

    tol = mp.mpf(tol)
    s = [mp.pi / 2] * n + [mp.mpf(2) * mp.pi / 3] * n
    e0 = energy(s)
    t = mp.mpf(0)
    out = {"phi": [], "dphi": [], "energy0": float(e0), "max_energy_drift": 0.0}
    H = mp.mpf("0.02")
    for tk in times:
        tk = mp.mpf(tk)
        while t < tk - mp.mpf("1e-30"):
            h = min(H, tk - t)
            new, err = gbs(s, h)
            if err > tol:
                H = h / 2
                continue
            s, t = new, t + h
            if err < tol / 1000:
                H = min(H * 2, mp.mpf("0.05"))
        out["phi"].append([float(x) for x in s[:n]])
        out["dphi"].append([float(x) for x in s[n:]])
        out["max_energy_drift"] = max(out["max_energy_drift"], float(abs(energy(s) - e0)))
    return out


def fourbar_text_check(text: str, j: dict) -> float:
    """The four-bar model as written, checked against its mechanics: the
    accelerations and bar forces at the start, solved independently from
    the bars' mass matrices, weights and length constraints (index 1),
    must zero every equation of the text. Returns the largest residual."""
    import numpy as np
    n, g = j["n"], j["g"]
    nodes = [(k, jj) for k in range(1, n + 1) for jj in range(n + 1)]
    at = {p: i for i, p in enumerate(nodes)}
    bars = ([((k, jj), (k - 1, jj)) for k in range(1, n + 1) for jj in range(n + 1)]
            + [((k, jj), (k, jj + 1)) for k in range(1, n + 1) for jj in range(n)])
    size = 2 * len(nodes)
    q = np.zeros(size)
    v = np.zeros(size)
    for (k, jj), i in at.items():
        q[2 * i], q[2 * i + 1], v[2 * i] = jj, -k, k * j["w0"]

    def pos(x, p):
        return np.array([float(p[1]), 0.0]) if p[0] == 0 else x[2 * at[p]:2 * at[p] + 2]

    M = np.zeros((size, size))
    Q = np.zeros(size)
    J = np.zeros((len(bars), size))
    Jdv = np.zeros(len(bars))
    for r, (a, b) in enumerate(bars):
        d = pos(q, a) - pos(q, b)
        dv = pos(v, a) - (pos(v, b) if b[0] > 0 else 0.0)
        Jdv[r] = 2 * float(dv @ dv)
        for me, other, sign in ((a, b, 1), (b, a, -1)):
            if me[0] == 0:
                continue
            i = at[me]
            J[r, 2 * i:2 * i + 2] += 2 * sign * d
            Q[2 * i + 1] -= g / 2
            for c in range(2):
                M[2 * i + c, 2 * i + c] += 2 / 6
                if other[0] > 0:
                    M[2 * i + c, 2 * at[other] + c] += 1 / 6
    K = np.block([[M, J.T], [J, np.zeros((len(bars), len(bars)))]])
    sol = np.linalg.solve(K, np.concatenate([Q, -Jdv]))
    env = {"m": 1.0, "L": 1.0, "g": g, "w0": j["w0"]}
    for (k, jj), i in at.items():
        env.update({f"y_{k}_{jj}": q[2 * i], f"z_{k}_{jj}": q[2 * i + 1],
                    f"vy_{k}_{jj}": v[2 * i], f"vz_{k}_{jj}": v[2 * i + 1],
                    f"ay_{k}_{jj}": sol[2 * i], f"az_{k}_{jj}": sol[2 * i + 1]})
    for r in range(len(bars)):
        env[f"lam{r}"] = sol[size + r]
    body = text[text.index("equation\n") + len("equation\n"):text.index("end Judge.FourBar")]
    worst = 0.0
    for line in body.strip().splitlines():
        eq = re.sub(r'\s*"[^"]*"\s*;\s*$', "", line.strip()).rstrip(";")
        eq = re.sub(r"der\(v(y|z)_", r"a\1_", eq)
        eq = re.sub(r"der\((y|z)_(\d+_\d+)\)", r"v\1_\2", eq).replace("^", "**")
        eq = re.sub(r"\ba(y|z)_(\d+_\d+)\)", r"a\1_\2", eq)
        lhs, rhs = eq.split(" = ")
        worst = max(worst, abs(eval(lhs, {}, env) - eval(rhs, {}, env)))  # noqa: S307 (our own text)
    return worst


def bouncing_ball_reference(j: dict) -> dict:
    """The ball's bounces, exactly: the times it reaches h = 0 and the speed
    it leaves with, until that is below v_min."""
    import mpmath as mp
    mp.mp.dps = 34
    g, e, vmin, h0 = (mp.mpf(str(j[k])) for k in ("g", "e", "v_min", "h0"))
    t = mp.sqrt(2 * h0 / -g)
    v = g * t  # arriving
    bounces = []
    while True:
        out = -e * v
        rest = out < vmin
        bounces.append({"t": t, "v_in": v, "v_out": mp.mpf(0) if rest else out})
        if rest:
            break
        v = -out
        t = t + 2 * out / -g
    return {"bounces": [{k: float(x) for k, x in b.items()} for b in bounces],
            "g": float(g)}


def bouncing_ball_at(ref: dict, t: float) -> tuple[float, float]:
    """h and v at t from the exact bounces (the value after a bounce at its
    instant)."""
    g = ref["g"]
    t0, v0 = 0.0, 0.0
    h0 = 1.0
    for b in ref["bounces"]:
        if t < b["t"]:
            break
        t0, v0, h0 = b["t"], b["v_out"], 0.0
        if b["v_out"] == 0.0:
            return 0.0, 0.0
    dt = t - t0
    return h0 + v0 * dt + 0.5 * g * dt * dt, v0 + g * dt


def testset_reference(path: Path) -> list[float]:
    """The reference solution at the end time from a test set problem's
    `solut` subroutine."""
    text = path.read_text()
    body = text[text.index("subroutine solut"):]
    vals = {}
    for i, v in re.findall(r"y\(\s*(\d+)\)\s*=\s*([-+0-9.dDeE]+)", body):
        vals[int(i)] = float(v.replace("d", "e").replace("D", "e"))
    return [vals[i] for i in sorted(vals)]


def cases(cache: Path, out: Path, sweep: bool) -> None:
    out.mkdir(parents=True, exist_ok=True)
    (out / "refs").mkdir(exist_ok=True)
    index = {}
    for name, (text, case) in all_cases(sweep).items():
        case = dict(case)
        case["text"] = f"{name}.mo"
        case["rtols"] = RTOLS
        case.setdefault("repeat", 3)
        (out / f"{name}.mo").write_text(text)
        (out / f"{name}.json").write_text(json.dumps(case, indent=1))
        index[name] = case
        ref = out / "refs" / f"{name}.json"
        if ref.exists():
            continue
        j = case["judge"]
        n = round(case["t_end"] / case["dt"])
        times = [round(k * case["dt"], 12) for k in range(n + 1)]
        started = time.time()
        if j["kind"] == "pendulum":
            r = pendulum_reference(j, times)
        elif j["kind"] == "fourbar":
            r = fourbar_reference(j, times)
            r["text_residual"] = fourbar_text_check(text, j)
        elif j["kind"] == "bouncingball":
            r = bouncing_ball_reference(j)
        elif j["kind"] == "testset":
            r = {"y": testset_reference(cached(cache, j["file"]))}
        elif j["kind"] == "stair":
            r = {}
        else:
            raise ValueError(j["kind"])
        r["times"] = times
        r["seconds"] = time.time() - started
        ref.write_text(json.dumps(r))
        print(f"{name}: reference in {r['seconds']:.1f} s", file=sys.stderr)
    (out / "index.json").write_text(json.dumps(sorted(index), indent=1))


# ---------------------------------------------------------------------------
# Running the engine
# ---------------------------------------------------------------------------

def default_runner() -> Path:
    target = Path(os.environ.get("CARGO_TARGET_DIR", ENGINE / "target"))
    return target / "release" / "examples" / ("judge.exe" if os.name == "nt" else "judge")


def run(cases_dir: Path, results: Path, runner: Path, timeout: float, only: list[str]) -> None:
    results.mkdir(parents=True, exist_ok=True)
    names = json.loads((cases_dir / "index.json").read_text())
    for name in names:
        if only and not any(name == o or name.startswith(o) for o in only):
            continue
        case_file = cases_dir / f"{name}.json"
        out = results / f"{name}.json"
        started = time.time()
        try:
            p = subprocess.run([str(runner), str(case_file), str(out)], timeout=timeout,
                               capture_output=True, text=True)
            if p.returncode != 0:
                out.write_text(json.dumps({"stage": "crash", "error":
                                           f"exit status {p.returncode}: {p.stderr[-2000:]}"}))
        except subprocess.TimeoutExpired:
            # once more, a tolerance at a time, so the others are recorded
            case = json.loads(case_file.read_text())
            runs, last = [], {}
            for rtol in case["rtols"]:
                one = dict(case, rtols=[rtol], text=str((cases_dir / case["text"]).resolve()))
                tmp = results / f"{name}.{rtol:g}.case.json"
                tmp.write_text(json.dumps(one))
                part = results / f"{name}.{rtol:g}.json"
                try:
                    subprocess.run([str(runner), str(tmp), str(part)], timeout=timeout,
                                   capture_output=True, text=True)
                    last = json.loads(part.read_text())
                    runs += last.get("runs", [])
                except subprocess.TimeoutExpired:
                    runs.append({"rtol": rtol, "ok": False, "error": f"no answer within {timeout:g} s"})
                for f in (tmp, part):
                    f.unlink(missing_ok=True)
            out.write_text(json.dumps(dict(last, stage="run", runs=runs)))
        print(f"{name}: {time.time() - started:.1f} s", file=sys.stderr)


# ---------------------------------------------------------------------------
# Scoring
# ---------------------------------------------------------------------------

def log_digits(err: float) -> float | None:
    """Significant correct digits from a relative error: -log10(err)."""
    if err is None or not math.isfinite(err):
        return None
    return 16.0 if err <= 0 else -math.log10(err)


def score_testset(case: dict, ref: dict, r: dict) -> dict:
    """The test set's own measures at the end time: scd over its chosen
    components (relative), mescd over all (mixed, atol/rtol as run)."""
    j = case["judge"]
    y = ref["y"]

    def value(c):
        for name in [c] if isinstance(c, str) else c:
            if r["final"].get(name) is not None:
                return r["final"][name]
        raise KeyError(f"no channel {c}")

    got = [value(c) for c in j["channels"]]
    rel = max(abs(got[i] - y[i]) / abs(y[i]) for i in j["scd"])
    ratio = r["atol"] / r["rtol"]
    mixed = max(abs(g - v) / (ratio + abs(v)) for g, v in zip(got, y))
    return {"error": rel, "scd": log_digits(rel), "mescd": log_digits(mixed)}


def energy_pendulum(j, x, y, vx, vy):
    return j["m"] * (vx * vx + vy * vy) / 2 + j["m"] * j["g"] * y


def score_pendulum(case: dict, ref: dict, r: dict) -> dict:
    j = case["judge"]
    s = r["series"]["channels"]
    n = min(len(s["x"]), len(ref["x"]))
    pos = max(max(abs(s["x"][k] - ref["x"][k]), abs(s["y"][k] - ref["y"][k])) for k in range(n))
    vel = max(max(abs(s["vx"][k] - ref["vx"][k]), abs(s["vy"][k] - ref["vy"][k])) for k in range(n))
    e = [energy_pendulum(j, s["x"][k], s["y"][k], s["vx"][k], s["vy"][k]) for k in range(n)]
    drift = max(abs(v - e[0]) for v in e)
    c0 = max(abs(s["x"][k] ** 2 + s["y"][k] ** 2 - j["L"] ** 2) for k in range(n))
    c1 = max(abs(s["x"][k] * s["vx"][k] + s["y"][k] * s["vy"][k]) for k in range(n))
    out = {"error": pos / j["L"], "scd": log_digits(pos / j["L"]), "vel_error": vel,
           "energy_drift": drift, "constraint": c0, "constraint_v": c1}
    if j.get("iftomm"):
        out["iftomm_pass"] = drift < 5e-5  # the benchmark's own limit, J
    return out


def score_fourbar(case: dict, ref: dict, r: dict) -> dict:
    j = case["judge"]
    n = j["n"]
    s = r["series"]["channels"]
    nt = min(len(s["y_1_0"]), len(ref["phi"]))

    def P(c, k, jj, t):
        if k == 0:
            return float(jj) if c == "y" else 0.0
        return s[f"{c}_{k}_{jj}"][t]

    def V(c, k, jj, t):
        return 0.0 if k == 0 else s[f"v{c}_{k}_{jj}"][t]

    err = 0.0
    err_a = 0.0
    drift = 0.0
    e0 = None
    for t in range(nt):
        for k in range(1, n + 1):
            dy = P("y", k, 0, t) - P("y", k - 1, 0, t)
            dz = P("z", k, 0, t) - P("z", k - 1, 0, t)
            phi = math.atan2(-dz, -dy)
            d = (phi - ref["phi"][t][k - 1] + math.pi) % (2 * math.pi) - math.pi
            err = max(err, abs(d))
        ay = sum(-math.cos(p) for p in ref["phi"][t])
        az = sum(-math.sin(p) for p in ref["phi"][t])
        err_a = max(err_a, math.hypot(P("y", n, 0, t) - ay, P("z", n, 0, t) - az))
        # the energy from the nodes: each uniform bar m/6 (|va|^2 + va.vb + |vb|^2)
        T = U = 0.0
        g = j["g"]
        for k in range(1, n + 1):
            for jj in range(n + 1):
                bars = [((k, jj), (k - 1, jj))] + ([((k, jj), (k, jj + 1))] if jj < n else [])
                for a, b in bars:
                    va = (V("y", *a, t), V("z", *a, t))
                    vb = (V("y", *b, t), V("z", *b, t))
                    T += (va[0] ** 2 + va[1] ** 2 + va[0] * vb[0] + va[1] * vb[1]
                          + vb[0] ** 2 + vb[1] ** 2) / 6
                    U += g * (P("z", *a, t) + P("z", *b, t)) / 2
        e = T + U
        if e0 is None:
            e0 = e
        drift = max(drift, abs(e - e0))
    return {"error": err, "scd": log_digits(err), "error_A": err_a, "energy_drift": drift,
            "energy0": e0, "iftomm_pass": drift < 1.0}


def read_csv(path: Path) -> tuple[list[str], list[list[float]]]:
    lines = path.read_text().strip().splitlines()
    head = lines[0].split(",")
    return head, [[float(x) for x in ln.split(",")] for ln in lines[1:]]


def score_stair(case: dict, cache: Path, r: dict) -> dict:
    _, rows = read_csv(cached(cache, case["judge"]["csv"]))
    s = r["series"]
    got = s["channels"]["counter"]
    times = s["times"]
    n = min(len(rows), len(got))
    err = max(abs(got[k] - rows[k][1]) for k in range(n))
    dt = max(abs(times[k] - rows[k][0]) for k in range(n))
    ev = r["event_times"]
    ev_err = max((abs(t - (i + 1)) for i, t in enumerate(ev)), default=0.0)
    return {"error": err, "scd": 16.0 if err == 0 else None, "points": n, "csv_points": len(rows),
            "grid_offset": dt, "event_time_error": ev_err, "events_expected": 9}


def score_bouncingball(case: dict, cache: Path, ref: dict, r: dict) -> dict:
    exact = [b["t"] for b in ref["bounces"]]
    ev = r["event_times"]
    terr = max((abs(a - b) for a, b in zip(ev, exact)), default=float("nan"))
    s = r["series"]
    hs, vs = s["channels"]["h"], s["channels"]["v"]
    herr = verr = 0.0
    for k, t in enumerate(s["times"]):
        # skip the output points within the event-location error of a bounce
        if any(abs(t - b) < 1e-6 for b in exact):
            continue
        h, v = bouncing_ball_at(ref, t)
        herr, verr = max(herr, abs(hs[k] - h)), max(verr, abs(vs[k] - v))
    _, rows = read_csv(cached(cache, case["judge"]["csv"]))
    csv_dev = max(abs(hs[k] - rows[k][1]) for k in range(min(len(rows), len(hs))))
    csv_bounces = sum(1 for a, b in zip(rows, rows[1:]) if a[2] < 0 and b[2] >= 0)
    return {"error": terr, "scd": log_digits(terr / exact[-1]), "h_error": herr, "v_error": verr,
            "bounces": len(ev), "bounces_exact": len(exact), "csv_bounces": csv_bounces,
            "csv_max_dev": csv_dev}


def score_run(case: dict, ref: dict, cache: Path, r: dict) -> dict:
    kind = case["judge"]["kind"]
    if kind == "testset":
        return score_testset(case, ref, r)
    if kind == "pendulum":
        return score_pendulum(case, ref, r)
    if kind == "fourbar":
        return score_fourbar(case, ref, r)
    if kind == "stair":
        return score_stair(case, cache, r)
    if kind == "bouncingball":
        return score_bouncingball(case, cache, ref, r)
    raise ValueError(kind)


def scored(cases_dir: Path, results: Path, cache: Path) -> dict:
    names = json.loads((cases_dir / "index.json").read_text())
    out = {}
    for name in names:
        res = results / f"{name}.json"
        if not res.exists():
            continue
        case = json.loads((cases_dir / f"{name}.json").read_text())
        ref = json.loads((cases_dir / "refs" / f"{name}.json").read_text())
        d = json.loads(res.read_text())
        rows = []
        if d.get("stage") != "run":
            for rtol in case["rtols"]:
                rows.append({"rtol": rtol, "ok": False,
                             "failure": f"{d.get('stage')}: {d.get('error', '')}"})
        for r in d.get("runs", []):
            values = [v for ch in r.get("series", {}).get("channels", {}).values() for v in ch]
            if r["ok"] and not all(math.isfinite(v) for v in values):
                # the run said it finished, its results are not numbers
                r = dict(r, ok=False, error="finished without a message, its results NaN")
            n_grid = round((case["t_end"] - case.get("t0", 0.0)) / case["dt"]) + 1
            if r["ok"] and len(r.get("series", {}).get("times", [])) < n_grid:
                r = dict(r, ok=False, error="finished without a message, its results end early")
            row = {"rtol": r["rtol"], "ok": r["ok"], "wall_ms": 1e3 * r.get("wall_s", float("nan"))}
            if r["ok"]:
                row.update({k: r[k] for k in ("steps", "err_test_fails", "events", "nonlin_fails")})
                try:
                    row.update(score_run(case, ref, cache, r))
                except Exception as e:  # a run whose results cannot be scored
                    row["note"] = f"not scored: {e!r}"
            else:
                row["failure"] = r.get("error", "")
            rows.append(row)
        out[name] = {"states": d.get("states"), "prepare_s": d.get("prepare_s"),
                     "compile_s": d.get("compile_s"), "rows": rows}
    return out


def fmt(x, spec=".1e"):
    if x is None:
        return "–"
    if isinstance(x, bool):
        return "yes" if x else "no"
    if isinstance(x, float) and not math.isfinite(x):
        return "–"
    if isinstance(x, int):
        return str(x)
    return format(x, spec)


def failure_text(row) -> str:
    f = " ".join(row.get("failure", "").split())
    m = re.search(r"stopped at t = ([0-9.e+-]+) s", f)
    where = f" at t = {float(m.group(1)):.4g} s" if m else ""
    short = f.split(": ", 1)[-1] if m else f
    return f"fails{where}: {short[:300]}"


def report(cases_dir: Path, cache: Path, after: dict, before: dict | None) -> str:
    md = []
    for name, a in after.items():
        case = json.loads((cases_dir / f"{name}.json").read_text())
        md.append(f"### {name}\n")
        md.append(f"States chosen: {', '.join(a['states'] or []) or '(none)'}.\n")
        extra = {"pendulum": ("energy_drift", "energy drift J"),
                 "fourbar": ("energy_drift", "energy drift J"),
                 "testset": ("mescd", "mescd"),
                 "bouncingball": ("h_error", "h error m"),
                 "stair": ("event_time_error", "event time error s")}[case["judge"]["kind"]]
        cols = ["rtol", "error", "scd", extra[1], "steps", "ETF", "events", "wall ms"]
        b_rows = {r["rtol"]: r for r in (before or {}).get(name, {}).get("rows", [])}
        md.append("| " + " | ".join(cols) + " |")
        md.append("|" + "---|" * len(cols))
        for r in a["rows"]:
            b = b_rows.get(r["rtol"])

            def cell(key, spec=".1e", rr=r, bb=b):
                now = fmt(rr.get(key), spec) if rr.get("ok") else "–"
                if bb is None:
                    return now
                was = fmt(bb.get(key), spec) if bb.get("ok") else "fails"
                return now if was == now else f"{was} → {now}"

            if not r.get("ok") and (b is None or not b.get("ok")):
                text = failure_text(r)
                if b is not None and b.get("failure") != r.get("failure"):
                    text = f"before: {failure_text(b)}; after: {text}"
                md.append(f"| {r['rtol']:.0e} | {text} |" + " |" * (len(cols) - 2))
                continue
            if not r.get("ok"):
                md.append(f"| {r['rtol']:.0e} | before ok (error {fmt(b.get('error'))}); after {failure_text(r)} |"
                          + " |" * (len(cols) - 2))
                continue
            error = cell("error")
            if case["judge"]["kind"] in ("pendulum", "fourbar") and (r.get("error") or 0) >= 1e-2:
                # a position 1 cm or more off on 1 m bars, and the run said nothing
                error += " **wrong, no message**"
            md.append("| " + " | ".join([
                f"{r['rtol']:.0e}", error, cell("scd", ".2f"), cell(extra[0]),
                cell("steps", "d"), cell("err_test_fails", "d"), cell("events", "d"),
                cell("wall_ms", ".2f")]) + " |")
        md.append("")
    return "\n".join(md)


def score(cases_dir: Path, cache: Path, results: Path, before: Path | None, out: Path | None) -> None:
    a = scored(cases_dir, results, cache)
    b = scored(cases_dir, before, cache) if before else None
    md = report(cases_dir, cache, a, b)
    if out:
        out.write_text(md)
        out.with_suffix(".json").write_text(json.dumps({"after": a, "before": b}, indent=1))
    else:
        print(md)


# ---------------------------------------------------------------------------
# SUNDIALS' own IDAS examples with quadratures under error control, built
# against upstream IDAS (the release the engine vendors, and the latest) and
# against the engine's patched copy (sundials/lsim.patch)
# ---------------------------------------------------------------------------

VENDORED = ENGINE / "crates" / "lsim-sundials-sys" / "sundials"

# the examples' tolerance macros: (states' rtol, atol), (quadratures' rtol, atol)
EXAMPLES = {
    "idasSlCrank_dns": (("RTOLF", "ATOLF"), ("RTOLQ", "ATOLQ"), (1e-1, 1.0, 1e-2)),
    "idasAkzoNob_dns": (("RTOL", "ATOL"), ("RTOLQ", "ATOLQ"), (1e-2, 1e-2, 1e-4)),
}


def build_sundials(tarball: Path, work: Path, name: str, patch: Path | None) -> Path:
    """IDAS from a release tarball, with `patch` applied: a static install."""
    prefix = work / name / "install"
    if (prefix / "lib").exists():
        return prefix
    src = work / name / "src"
    shutil.rmtree(work / name, ignore_errors=True)
    src.mkdir(parents=True)
    subprocess.run(["tar", "-xzf", str(tarball), "-C", str(src), "--strip-components=1"], check=True)
    if patch:
        subprocess.run(["patch", "-p1", "-i", str(patch)], cwd=src, check=True,
                       stdout=subprocess.DEVNULL)
    build = work / name / "build"
    flags = ["-DBUILD_SHARED_LIBS=OFF", "-DBUILD_STATIC_LIBS=ON", "-DEXAMPLES_ENABLE_C=OFF",
             "-DEXAMPLES_INSTALL=OFF", "-DBUILD_ARKODE=OFF", "-DBUILD_CVODE=OFF",
             "-DBUILD_CVODES=OFF", "-DBUILD_IDA=OFF", "-DBUILD_IDAS=ON", "-DBUILD_KINSOL=OFF",
             "-DBUILD_TESTING=OFF", "-DSUNDIALS_TEST_ENABLE_UNIT_TESTS=OFF",
             "-DCMAKE_BUILD_TYPE=Release",
             # the engine's own options for its copy (lsim-sundials-sys/build.rs)
             "-DCMAKE_C_FLAGS_RELEASE=-O2 -ffp-contract=off",
             f"-DCMAKE_INSTALL_PREFIX={prefix}"]
    subprocess.run(["cmake", "-S", str(src), "-B", str(build)] + flags, check=True,
                   stdout=subprocess.DEVNULL)
    jobs = os.environ.get("CARGO_BUILD_JOBS", "2")
    subprocess.run(["cmake", "--build", str(build), "-j", jobs, "--target", "install"], check=True,
                   stdout=subprocess.DEVNULL)
    return prefix


def run_example(prefix: Path, source: Path, work: Path, tag: str, rtol: float | None) -> dict:
    """Builds the example against `prefix` with its tolerances scaled to rtol
    (None: as shipped) and returns what it prints."""
    name = source.stem
    text = source.read_text()
    if rtol is not None:
        (fr, fa), (qr, qa), (ra, rqr, rqa) = EXAMPLES[name]
        for macro, value in ((fr, rtol), (fa, rtol * ra), (qr, rtol * rqr), (qa, rtol * rqa)):
            text, n = re.subn(rf"(#define\s+{macro}\s+)SUN_RCONST\([^)]*\)",
                              rf"\g<1>SUN_RCONST({value!r})", text)
            assert n == 1, (name, macro)
    exe_dir = work / "examples" / tag
    exe_dir.mkdir(parents=True, exist_ok=True)
    stem = f"{name}-{'shipped' if rtol is None else f'{rtol:g}'}"
    c = exe_dir / f"{stem}.c"
    c.write_text(text)
    libs = sorted(str(p) for p in (prefix / "lib").glob("libsundials_*.a"))
    if not libs:
        libs = sorted(str(p) for p in (prefix / "lib64").glob("libsundials_*.a"))
    exe = exe_dir / stem
    subprocess.run(["cc", "-O2", "-ffp-contract=off", "-I", str(prefix / "include"), str(c), "-o",
                    str(exe), "-Wl,--start-group"] + libs + ["-Wl,--end-group", "-lm"], check=True)
    started = time.time()
    p = subprocess.run([str(exe)], capture_output=True, text=True, timeout=600)
    wall = time.time() - started
    out = p.stdout
    (exe_dir / f"{stem}.out").write_text(out + p.stderr)
    return parse_example(out, p.returncode, wall)


def parse_example(out: str, code: int = 0, wall: float | None = None) -> dict:
    stat = {}
    for key, label in (("steps", "Number of steps"), ("res", "Number of residual evaluations"),
                       ("jac", "Number of Jacobian evaluations"),
                       ("nni", "Number of nonlinear iterations"),
                       ("etf", "Number of error test failures"),
                       ("ncf", "Number of nonlinear conv. failures")):
        m = re.search(rf"{label}\s*=\s*(\d+)", out)
        stat[key] = int(m.group(1)) if m else None
    m = re.search(r"G\s*[:=]\s*([-+0-9.eE]+)", out)
    stat["G"] = float(m.group(1)) if m else None
    stat["ok"] = code == 0 and stat["G"] is not None
    stat["wall_ms"] = None if wall is None else 1e3 * wall
    return stat


def slider_crank_G(t_end: float = 10.0) -> float:
    """idasSlCrank_dns' quadrature G = int 1/2 (J1 q'^2 + m2 x'^2 + J2 p'^2) dt
    from its minimal coordinates: the crank angle q, with x and p from the
    constraints x = cos p + a cos q, sin p = -a sin q, and the accelerations
    and constraint forces from the example's own equations of motion
    (its force()), integrated by Gragg-Bulirsch-Stoer at 34 digits."""
    import mpmath as mp
    mp.mp.dps = 34
    a, J1, m2, J2, k, c, l0, F = (mp.mpf(x) for x in ("0.5", 1, 1, 2, 1, 1, 1, 1))

    def rhs(s):
        q, qd, _ = s
        s1, c1 = mp.sin(q), mp.cos(q)
        p = -mp.asin(a * s1)
        s2, c2 = mp.sin(p), mp.cos(p)
        x = c2 + a * c1
        pd = -a * c1 * qd / c2
        xd = -(a * s1 * qd + s2 * pd)
        s21, c21 = s2 * c1 - c2 * s1, c2 * c1 + s2 * s1
        l2 = x * x - x * (c2 + a * c1) + (1 + a * a) / 4 + a * c21 / 2
        ll = mp.sqrt(l2)
        ld = (2 * x * xd - xd * (c2 + a * c1) + x * (s2 * pd + a * s1 * qd)
              - a * s21 * (pd - qd) / 2) / (2 * ll)
        fl = (k * (ll - l0) + c * ld) / ll
        Q0 = -fl * a * (s21 / 2 + x * s1) / 2
        Q1 = fl * (c2 / 2 - x + a * c1 / 2) + F
        Q2 = -fl * (x * s2 - a * s21 / 2) / 2 - F * s2
        # unknowns q'', x'', p'', lam1, lam2
        A = mp.matrix([[J1, 0, 0, a * s1, -a * c1],
                       [0, m2, 0, 1, 0],
                       [0, 0, J2, s2, -c2],
                       [a * s1, 1, s2, 0, 0],
                       [-a * c1, 0, -c2, 0, 0]])
        b = mp.matrix([Q0, Q1, Q2, -(c2 * pd ** 2 + a * c1 * qd ** 2),
                       -(s2 * pd ** 2 + a * s1 * qd ** 2)])
        acc = mp.lu_solve(A, b)
        return [qd, acc[0], (J1 * qd ** 2 + m2 * xd ** 2 + J2 * pd ** 2) / 2]

    def gbs(s, H, K=10):
        T = []
        for jj in range(1, K + 1):
            nn = 2 * jj
            h = H / nn
            z0, f0 = s, rhs(s)
            z1 = [u + h * v for u, v in zip(z0, f0)]
            for _ in range(1, nn):
                f1 = rhs(z1)
                z0, z1 = z1, [u + 2 * h * v for u, v in zip(z0, f1)]
            f1 = rhs(z1)
            T.append([[(u + v + h * w) / 2 for u, v, w in zip(z0, z1, f1)]])
            for kk in range(1, jj):
                r = (mp.mpf(nn) / (2 * (jj - kk))) ** 2
                prev, prevm = T[jj - 1][kk - 1], T[jj - 2][kk - 1]
                T[jj - 1].append([u + (u - v) / (r - 1) for u, v in zip(prev, prevm)])
        return T[K - 1][K - 1], max(abs(u - v) for u, v in zip(T[K - 1][K - 1], T[K - 1][K - 2]))

    s = [mp.pi / 2, mp.mpf(0), mp.mpf(0)]
    t, H, tol = mp.mpf(0), mp.mpf("0.05"), mp.mpf("1e-26")
    while t < t_end - mp.mpf("1e-30"):
        h = min(H, t_end - t)
        new, err = gbs(s, h)
        if err > tol:
            H = h / 2
            continue
        s, t = new, t + h
        if err < tol / 1000:
            H = min(2 * H, mp.mpf("0.1"))
    return float(s[2])


def akzo_G(cache: Path) -> tuple[float, float]:
    """idasAkzoNob_dns' quadrature G = int_0^180 y1 dt, the problem reduced to
    its five differential equations (y6 = Ks y1 y4) and integrated by scipy's
    Radau IIA at rtol 1e-13; also y1(180) against the test set's reference."""
    from scipy.integrate import solve_ivp
    k1, k2, k3, k4, K, klA, Ks, pCO2, H = 18.7, 0.58, 0.09, 0.42, 34.4, 3.3, 115.83, 0.9, 737.0

    def f(t, y):
        y1, y2, y3, y4, y5 = y[:5]
        y6 = Ks * y1 * y4
        r1 = k1 * y1 ** 4 * math.sqrt(max(y2, 0.0))
        r2 = k2 * y3 * y4
        r3 = k2 / K * y1 * y5
        r4 = k3 * y1 * y4 * y4
        r5 = k4 * y6 * y6 * math.sqrt(max(y2, 0.0))
        fin = klA * (pCO2 / H - y2)
        return [-2 * r1 + r2 - r3 - r4, -0.5 * r1 - r4 - 0.5 * r5 + fin, r1 - r2 + r3,
                -r2 + r3 - 2 * r4, r2 - r3 + r5, y1]

    sol = solve_ivp(f, (0, 180), [0.444, 0.00123, 0, 0.007, 0, 0], method="Radau",
                    rtol=1e-13, atol=1e-18)
    y1_ref = testset_reference(cached(cache, "ivptestset/chemakzo.f"))[0]
    return float(sol.y[5, -1]), abs(sol.y[0, -1] - y1_ref) / y1_ref


def sundials(cache: Path, work: Path) -> None:
    work.mkdir(parents=True, exist_ok=True)
    patch = VENDORED / "lsim.patch"
    builds = {
        "7.1.1": build_sundials(cached(cache, "sundials/sundials-7.1.1.tar.gz"), work, "7.1.1", None),
        # (built afresh whenever the patch changes)
        "7.1.1+lsim": build_sundials(cached(cache, "sundials/sundials-7.1.1.tar.gz"), work,
                                     f"7.1.1-lsim-{sha256(patch)[:12]}", patch),
        "7.9.0": build_sundials(cached(cache, "sundials/sundials-7.9.0.tar.gz"), work, "7.9.0", None),
    }
    # the engine's copy is upstream 7.1.1 with its patch and nothing else
    upstream = work / f"7.1.1-lsim-{sha256(patch)[:12]}" / "src"
    differ = []
    for f in sorted(VENDORED.rglob("*")):
        rel = f.relative_to(VENDORED)
        # (the two headers CMake writes: the crate's copies are its own)
        if (f.is_dir() or rel.parts[0] not in ("src", "include")
                or rel.name in ("sundials_config.h", "sundials_export.h")):
            continue
        u = upstream / rel
        if not u.exists() or u.read_bytes() != f.read_bytes():
            differ.append(str(rel))
    refs = {"idasSlCrank_dns": slider_crank_G(), "idasAkzoNob_dns": None}
    refs["idasAkzoNob_dns"], akzo_y1 = akzo_G(cache)
    result = {"vendored_differs_from_upstream_plus_patch": differ, "G_reference": refs,
              "akzo_y1_rel_error_vs_testset": akzo_y1, "runs": {}}
    for name in EXAMPLES:
        for version in ("7.1.1", "7.9.0"):
            shipped = parse_example(cached(cache, f"sundials/v{version}/{name}.out").read_text())
            result["runs"].setdefault(name, {})[f"LLNL's {version} .out"] = {"shipped": shipped}
        src = cached(cache, f"sundials/v7.1.1/{name}.c")
        for tag, prefix in builds.items():
            src_v = cached(cache, f"sundials/v{'7.9.0' if tag == '7.9.0' else '7.1.1'}/{name}.c")
            rows = {"shipped": run_example(prefix, src_v, work, tag, None)}
            for rtol in RTOLS:
                rows[f"{rtol:g}"] = run_example(prefix, src, work, tag, rtol)
            result["runs"][name][tag] = rows
    (work / "sundials.json").write_text(json.dumps(result, indent=1))
    print(sundials_report(result))


def sundials_report(res: dict) -> str:
    md = []
    if res["vendored_differs_from_upstream_plus_patch"]:
        md.append("The engine's SUNDIALS differs from upstream 7.1.1 with lsim.patch in: "
                  + ", ".join(res["vendored_differs_from_upstream_plus_patch"]) + "\n")
    else:
        md.append("The engine's SUNDIALS is upstream 7.1.1 with lsim.patch, file for file "
                  "(sundials_config.h aside).\n")
    md.append(f"Akzo Nobel: y1(180) by the reference integration is "
              f"{res['akzo_y1_rel_error_vs_testset']:.1e} off the test set's reference.\n")
    for name, runs in res["runs"].items():
        g_ref = res["G_reference"][name]
        md.append(f"### {name} (G reference {g_ref!r})\n")
        md.append("| IDAS | tolerances | G | G error (rel.) | steps | ETF | NCF | res. evals | wall ms |")
        md.append("|---|---|---|---|---|---|---|---|---|")
        for tag, rows in runs.items():
            for tol, r in rows.items():
                if r["G"] is None:
                    md.append(f"| {tag} | {tol} | fails | | | | | | |")
                    continue
                err = abs(r["G"] - g_ref) / abs(g_ref)
                md.append(f"| {tag} | {tol} | {r['G']!r} | {err:.1e} | {fmt(r['steps'])} | "
                          f"{fmt(r['etf'])} | {fmt(r['ncf'])} | {fmt(r['res'])} | "
                          f"{fmt(r['wall_ms'], '.2f')} |")
        md.append("")
    return "\n".join(md)


# ---------------------------------------------------------------------------
# The same models in OpenModelica (for checking only: never shipped, never
# linked), with its dynamic state selection and with static dummy
# derivatives; results in the runner's form, so `score` reads them
# ---------------------------------------------------------------------------

OMC_METHODS = ("dynamicStateSelection", "dummyDerivatives")


def omc_env() -> tuple[str, dict]:
    exe = shutil.which("omc")
    if exe is None:
        sys.exit("omc is not on the PATH")
    root = Path(exe).resolve().parent.parent
    env = dict(os.environ, OPENMODELICAHOME=str(root))
    libs = [str(root / "lib"), str(root / "lib" / "omc")]
    env["LD_LIBRARY_PATH"] = ":".join(libs + [env.get("LD_LIBRARY_PATH", "")])
    return exe, env


def omc_csv(path: Path, record: list[str], dt: float) -> dict:
    """OpenModelica's result file as the runner's series: the output grid,
    with the values before and after an event at a grid time, and the
    events' instants (OpenModelica writes a row on each side of an event)."""
    with open(path, newline="") as f:
        table = list(csv.reader(f))
    head = table[0]
    rows = [[float(x) for x in row] for row in table[1:]]
    col = {h: i for i, h in enumerate(head)}
    times, after, before, events = [], [], {}, []
    for i, row in enumerate(rows):
        t = row[0]
        # an event: a row on each side of an instant, the values changed
        # (OpenModelica also writes two equal rows where a relation goes
        # back after it, and the end time twice)
        if (i > 0 and t == rows[i - 1][0] and row != rows[i - 1]
                and (not events or t - events[-1] > 1e-9 * max(1.0, abs(t)))):
            events.append(t)
        on_grid = abs(t / dt - round(t / dt)) < 1e-9
        if not on_grid:
            continue
        if times and t == times[-1]:
            before.setdefault(len(times) - 1, after[-1])
            after[-1] = row
        else:
            times.append(t)
            after.append(row)
    chans = {n: [r[col[n]] for r in after] for n in record if n in col}
    left = {n: [(k, b[col[n]]) for k, b in before.items() if b[col[n]] != after[k][col[n]]]
            for n in record if n in col}
    return {"times": times, "channels": chans, "before": {n: v for n, v in left.items() if v},
            "final": {h: rows[-1][i] for h, i in col.items()}, "event_times": events}


def omc_stats(log: str) -> dict:
    def grab(pattern):
        m = re.search(pattern, log)
        return int(m.group(1)) if m else None
    wall = re.search(r"([0-9.e+-]+)s\s*\[100\.0%\] total", log)
    events = (grab(r"(\d+) state events") or 0) + (grab(r"(\d+) time events") or 0)
    return {"steps": grab(r"(\d+) steps taken"), "err_test_fails": grab(r"(\d+) error test failures"),
            "nonlin_fails": grab(r"(\d+) convergence test failures"), "events": events,
            "wall_s": float(wall.group(1)) if wall else None}


def omc(cases_dir: Path, results: Path, only: list[str]) -> None:
    exe, env = omc_env()
    names = json.loads((cases_dir / "index.json").read_text())
    for name in names:
        if only and not any(name == o or name.startswith(o) for o in only):
            continue
        case = json.loads((cases_dir / f"{name}.json").read_text())
        model = case["model"].replace("Judge.", "")
        text = (cases_dir / case["text"]).read_text().replace("Judge.", "")
        for method in OMC_METHODS:
            work = results / "work" / name / method
            shutil.rmtree(work, ignore_errors=True)
            work.mkdir(parents=True)
            (work / f"{model}.mo").write_text(text)
            n = round(case["t_end"] / case["dt"])
            (work / "build.mos").write_text(
                'setCompiler("cc"); setCXXCompiler("c++");\n'
                f'loadFile("{model}.mo"); getErrorString();\n'
                f'setCommandLineOptions("--indexReductionMethod={method}"); getErrorString();\n'
                f'buildModel({model}, startTime=0, stopTime={case["t_end"]!r}, '
                f'numberOfIntervals={n}, outputFormat="csv"); getErrorString();\n')
            started = time.time()
            p = subprocess.run([exe, "build.mos"], cwd=work, env=env, capture_output=True,
                               text=True, timeout=1800)
            build_s = time.time() - started
            out = {"stage": "run", "tool": f"OpenModelica ({method})", "build_s": build_s,
                   "states": [], "runs": []}
            if not (work / model).exists():
                out = {"stage": "prepare", "error": p.stdout[-3000:] + p.stderr[-1000:]}
            else:
                for rtol in case["rtols"]:
                    res = work / f"{model}-{rtol:g}.csv"
                    started = time.time()
                    try:
                        q = subprocess.run([f"./{model}", f"-tolerance={rtol!r}", "-lv=LOG_STATS",
                                            f"-r={res.name}"], cwd=work, env=env,
                                           capture_output=True, text=True, timeout=600)
                        log, code = q.stdout + q.stderr, q.returncode
                    except subprocess.TimeoutExpired:
                        log, code = "no answer within 600 s", -1
                    wall = time.time() - started
                    st = omc_stats(log)
                    ok = (code == 0 and res.exists() and "simulation finished successfully" in log
                          and len(res.read_text().strip().splitlines()) > 1)
                    run = {"rtol": rtol, "atol": rtol, "ok": ok, **st}
                    run["wall_s"] = st["wall_s"] if st["wall_s"] is not None else wall
                    if ok:
                        data = omc_csv(res, case["record"], case["dt"])
                        run["final"] = data.pop("final")
                        run["event_times"] = data.pop("event_times")
                        run["series"] = data
                    else:
                        errors = [ln for ln in log.splitlines() if "error" in ln.lower()]
                        run["error"] = " | ".join(errors[-3:]) or log[-500:]
                    out["runs"].append(run)
                    res.unlink(missing_ok=True)
            (results / method).mkdir(parents=True, exist_ok=True)
            (results / method / f"{name}.json").write_text(json.dumps(out))
            print(f"{name} ({method}): {time.time() - started:.1f} s", file=sys.stderr)


# ---------------------------------------------------------------------------

def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("fetch")
    p.add_argument("cache", type=Path)
    p = sub.add_parser("cases")
    p.add_argument("cache", type=Path)
    p.add_argument("cases", type=Path)
    p.add_argument("--sweep", action="store_true",
                   help="the pendulum released at every 0.01 rad from 0.01 to 3.14")
    p = sub.add_parser("run")
    p.add_argument("cases", type=Path)
    p.add_argument("results", type=Path)
    p.add_argument("only", nargs="*")
    p.add_argument("--runner", type=Path, default=None)
    p.add_argument("--timeout", type=float, default=600)
    p = sub.add_parser("score")
    p.add_argument("cache", type=Path)
    p.add_argument("cases", type=Path)
    p.add_argument("results", type=Path)
    p.add_argument("--before", type=Path)
    p.add_argument("--out", type=Path)
    p = sub.add_parser("sundials")
    p.add_argument("cache", type=Path)
    p.add_argument("work", type=Path)
    p = sub.add_parser("omc")
    p.add_argument("cases", type=Path)
    p.add_argument("results", type=Path)
    p.add_argument("only", nargs="*")
    a = ap.parse_args()
    if a.cmd == "fetch":
        fetch(a.cache)
    elif a.cmd == "cases":
        cases(a.cache, a.cases, a.sweep)
    elif a.cmd == "run":
        run(a.cases, a.results, a.runner or default_runner(), a.timeout, a.only)
    elif a.cmd == "score":
        score(a.cases, a.cache, a.results, a.before, a.out)
    elif a.cmd == "sundials":
        sundials(a.cache, a.work)
    elif a.cmd == "omc":
        omc(a.cases, a.results, a.only)


if __name__ == "__main__":
    main()
