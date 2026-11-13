"""Step 3's hard-problem harness (phase 1: the IVP Test Set 2.4 and
SUNDIALS' examples). One command runs it all:

    python3 benchmarks/hard/hard.py all CACHE WORK

and each stage alone:

    fetch CACHE            the sources, checked by SHA-256 (research-use files stay in CACHE)
    probe CACHE WORK       each problem's own Fortran built with gfortran: start, reference,
                           mass matrix, and its residual at the check points
    transcribe CACHE WORK  each problem's feval run on symbols into a model (fortran.py)
    check WORK             every model's equations against the Fortran at the points (1e-13)
    outside CACHE WORK     RADAU5, DASSL and PSIDE from the test set at every tolerance
    run WORK               ours: ida (every block implicit), diffsol and default (the
                           engine's own choice) at every tolerance
    sundials CACHE WORK    SUNDIALS 7.1.1's C examples, built against upstream and against
                           the engine's own copy, against their .out files
    sunours WORK           the same examples transcribed by script (csun.py), checked
                           against their C functions, run on ours, against their .out
    score WORK             digits and counts against the manifest's expectations

`all` builds the engine's runners first (cargo, release examples `judge` and
`hard_eval` into CARGO_TARGET_DIR) and runs every stage in this order. CACHE
and WORK are folders outside the repository. HARD_CPUS pins the runs
(taskset), HARD_OURS names the folder of our runs. Needs gfortran, a C
compiler, cmake (CMAKE, the PATH's, or pip's next to this Python) and the
engine's Rust toolchain.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import random
import re
import shutil
import subprocess
import sys
import tarfile
import tomllib
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))
import fortran  # noqa: E402

TESTSET = "https://archimede.uniba.it/~testset"
SOURCES = [
    # The IVP Test Set 2.4 (F. Mazzia, C. Magherini, University of Bari, 2008):
    # research use only, fetched here and never committed.
    ("ivptestset/IVPtestset_2.4.tar.gz", f"{TESTSET}/IVPtestset_2.4.tar.gz",
     "7da75f54b5221b0c4b9de3698c1c83b5d8748b160a360c50735ded3af433999b"),
]
TS_DIR = "ivptestset/IVPtestset_2.4"
RTOLS = [1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10]
PROBLEMS = ["hires", "pollu", "ringmod", "vdpol", "vdpolm", "orego", "rober", "e5", "medakzo",
            "emep", "plei", "beam", "chemakzo", "andrews", "transamp", "caraxis", "crank",
            "fekete", "pump", "tba", "water", "nand", "wheel"]
# the components the test set's getscd skips (src/auxil/report.f), 1-based
SKIP = {
    "emep": lambda i: i in (36, 38), "crank": lambda i: 8 <= i <= 21,
    "andrews": lambda i: i > 7, "nand": lambda i: i != 5, "pump": lambda i: i == 9,
    "tba": lambda i: i not in (49 + 175, 130 + 175, 148 + 175),
    "fekete": lambda i: i > 60, "plei": lambda i: i > 14, "beam": lambda i: i > 40,
}
# the derived cases: (id, problem, parameter edits, rows left out, extra equations, start edits)
DERIVED = {
    "derived-chemakzo-no-algebraic": ("chemakzo", {}, [5], [], {}),
    "derived-hires-extra-equation": ("hires", {}, [], ["y8 = 0.0057"], {}),
    "derived-andrews-massless": ("andrews", {"m1": 0.0}, [], [], {}),
    "derived-chemakzo-y6-start": ("chemakzo", {}, [], [], {6: (1.0, True)}),
}
OURS = os.environ.get("HARD_OURS", "ours")  # where our runs go (a second run beside the first)
WITHDRAWN = {"derived-andrews-massless"}  # see the manifest dated changes
GFORTRAN = os.environ.get("GFORTRAN", "gfortran")
CPUS = os.environ.get("HARD_CPUS")  # e.g. "16-23": runs are pinned with taskset


def sha256(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def pinned(cmd: list[str]) -> list[str]:
    return ["taskset", "-c", CPUS] + cmd if CPUS else cmd


# ---------------------------------------------------------------------------
# fetch
# ---------------------------------------------------------------------------

def get(cache: Path, rel: str, url: str, digest: str) -> Path:
    """cache/rel, downloaded from url unless the cache holds it with this
    SHA-256 already; a download that fails says where to put the file."""
    p = cache / rel
    if not (p.exists() and sha256(p) == digest):
        p.parent.mkdir(parents=True, exist_ok=True)
        tmp = p.with_suffix(".part")
        try:
            with urllib.request.urlopen(url, timeout=300) as r, open(tmp, "wb") as f:
                shutil.copyfileobj(r, f)
        except OSError as e:
            sys.exit(f"{rel}: could not download it from {url} ({getattr(e, 'reason', e)}). "
                     f"Put the file (SHA-256 {digest}) at {p} and run again.")
        got = sha256(tmp)
        if got != digest:
            sys.exit(f"{rel}: SHA-256 {got}, expected {digest}: changed upstream, not used")
        tmp.replace(p)
    return p


def fetch(cache: Path) -> None:
    for rel, url, digest in SOURCES:
        get(cache, rel, url, digest)
    ts = cache / TS_DIR
    if not ts.exists():
        with tarfile.open(cache / SOURCES[0][0]) as t:
            t.extractall(cache / "ivptestset", filter="data")


def problem_file(cache: Path, name: str) -> Path:
    return cache / TS_DIR / "src" / "problems" / f"{name}.f"


# ---------------------------------------------------------------------------
# probe: the original, built with gfortran
# ---------------------------------------------------------------------------

PROBE = """      program probe
      integer nmax
      parameter (nmax=500)
      character*80 fullnm
      character*20 problm
      character*3 tp
      integer neqn,ndisc,mljac,mujac,mlmas,mumas,ind(nmax),ipar(nmax)
      integer ierr,i,j,k,np,nindsl,indsol(nmax)
      double precision t(0:400),y(nmax),yp(nmax),f(nmax),yref(nmax)
      double precision rpar(nmax),dm(nmax,nmax),tt,y0(nmax),yp0(nmax)
      logical numjac,nummas,consis,solref,prsol
      do 1 i=1,nmax
         ind(i)=0
    1 continue
      consis=.false.
      call prob(fullnm,problm,tp,neqn,ndisc,t,numjac,mljac,mujac,
     +          nummas,mlmas,mumas,ind)
      call init(neqn,t(0),y,yp,consis)
      call setoutput(neqn,solref,prsol,nindsl,indsol)
      write(*,'(a)') 'TYPE '//tp
      write(*,'(a,i6,i6,i6,i6)') 'NEQN ',neqn,ndisc,mlmas,mumas
      write(*,'(a,l2,l2)') 'FLAGS ',consis,solref
      write(*,'(a)') 'T'
      write(*,'(es26.17e3)') (t(i),i=0,ndisc+1)
      write(*,'(a)') 'IND'
      write(*,'(i4)') (ind(i),i=1,neqn)
      write(*,'(a)') 'Y0'
      write(*,'(es26.17e3)') (y(i),i=1,neqn)
      write(*,'(a)') 'YP0'
      write(*,'(es26.17e3)') (yp(i),i=1,neqn)
      do 2 i=1,neqn
         y0(i)=y(i)
         yp0(i)=yp(i)
    2 continue
      if (solref) then
         call solut(neqn,t(ndisc+1),yref)
         write(*,'(a)') 'YREF'
         write(*,'(es26.17e3)') (yref(i),i=1,neqn)
      endif
      if (tp.eq.'DAE') then
         do 4 j=1,nmax
            do 3 i=1,nmax
               dm(i,j)=0d0
    3       continue
    4    continue
         call meval(nmax,neqn,t(0),y0,yp0,dm,ierr,rpar,ipar)
         write(*,'(a)') 'MASS'
         do 6 j=1,neqn
            do 5 i=1,neqn
               if (mlmas.eq.neqn) then
                  tt=dm(i,j)
               elseif (i-j+mumas+1.ge.1.and.i-j+mumas+1.le.
     +                 mlmas+mumas+1) then
                  tt=dm(i-j+mumas+1,j)
               else
                  tt=0d0
               endif
               if (tt.ne.0d0) write(*,'(2i6,es26.17e3)') i,j,tt
    5       continue
    6    continue
         write(*,'(a)') 'END'
      endif
      read(*,*) np
      do 9 k=1,np
         read(*,*) tt
         read(*,*) (y(i),i=1,neqn)
         read(*,*) (yp(i),i=1,neqn)
         ierr=0
         call feval(neqn,tt,y,yp,f,ierr,rpar,ipar)
         write(*,'(a,i4)') 'F ',ierr
         write(*,'(es26.17e3)') (f(i),i=1,neqn)
    9 continue
      end
"""


def parse_probe(out: str) -> dict:
    lines = out.split("\n")
    d: dict = {"F": []}
    i = 0
    nums = lambda k: [float(x.replace("D", "E")) for x in lines[i + 1:i + 1 + k]]
    while i < len(lines):
        ln = lines[i].strip()
        if ln.startswith("TYPE"):
            d["type"] = ln.split()[1]
        elif ln.startswith("NEQN"):
            d["neqn"], d["ndisc"], d["mlmas"], d["mumas"] = map(int, ln.split()[1:])
        elif ln.startswith("FLAGS"):
            d["consis"], d["solref"] = [x == "T" for x in ln.split()[1:]]
        elif ln == "T":
            d["t"] = nums(d["ndisc"] + 2)
        elif ln == "IND":
            d["ind"] = [int(x) for x in lines[i + 1:i + 1 + d["neqn"]]]
        elif ln in ("Y0", "YP0", "YREF"):
            d[ln.lower()] = nums(d["neqn"])
        elif ln == "MASS":
            n = d["neqn"]
            mass = [[0.0] * n for _ in range(n)]
            i += 1
            while lines[i].strip() != "END":
                a, b, v = lines[i].split()
                mass[int(a) - 1][int(b) - 1] = float(v)
                i += 1
            d["mass"] = mass
        elif ln.startswith("F "):
            d["F"].append({"ierr": int(ln.split()[1]), "f": nums(d["neqn"])})
            i += d["neqn"]
        i += 1
    return d


def points(d: dict) -> list[dict]:
    """The check points: the start, the reference at the end, and three
    points near each moved by a relative 1e-3 (seeds 1, 2, 3)."""
    base = [(d["t"][0], d["y0"], d["yp0"])]
    if d.get("yref"):
        base.append((d["t"][-1], d["yref"], d["yp0"]))
    pts = []
    for t, y, yp in base:
        pts.append({"t": t, "y": list(y), "yp": list(yp), "seed": 0})
        for seed in (1, 2, 3):
            r = random.Random(seed)
            pts.append({"t": t, "seed": seed,
                        "y": [v * (1 + 1e-3 * r.uniform(-1, 1)) for v in y],
                        "yp": [v * (1 + 1e-3 * r.uniform(-1, 1)) for v in yp]})
    return pts


def probe_one(cache: Path, work: Path, name: str) -> dict:
    d = work / "probe" / name
    d.mkdir(parents=True, exist_ok=True)
    (d / "probe.f").write_text(PROBE)
    exe = d / "probe"
    src = problem_file(cache, name)
    r = subprocess.run(pinned([GFORTRAN, "-O0", "-ffpe-summary=none", "-std=legacy", "-o", str(exe),
                        str(d / "probe.f"), str(src)]), capture_output=True, text=True)
    if r.returncode:
        raise RuntimeError(f"gfortran: {r.stderr[-2000:]}")
    # once for the header, once more with the points
    first = subprocess.run([str(exe)], input="0\n", capture_output=True, text=True, timeout=60)
    head = parse_probe(first.stdout)
    pts = points(head)
    feed = [str(len(pts))]
    for p in pts:
        feed += [repr(p["t"]), " ".join(repr(v) for v in p["y"]), " ".join(repr(v) for v in p["yp"])]
    second = subprocess.run([str(exe)], input="\n".join(feed) + "\n", capture_output=True,
                            text=True, timeout=60)
    full = parse_probe(second.stdout)
    for p, f in zip(pts, full["F"]):
        p["f"], p["ierr"] = f["f"], f["ierr"]
    full["points"] = pts
    full["gfortran"] = subprocess.run([GFORTRAN, "--version"], capture_output=True,
                                      text=True).stdout.split("\n")[0]
    full["source_sha256"] = sha256(src)
    del full["F"]
    (work / "probe" / f"{name}.json").write_text(json.dumps(full))
    return full


def probe(cache: Path, work: Path, only: list[str]) -> None:
    for name in PROBLEMS:
        if only and name not in only:
            continue
        try:
            d = probe_one(cache, work, name)
            print(f"{name}: {d['type']} {d['neqn']}, t {d['t'][0]} .. {d['t'][-1]}, "
                  f"{len(d['points'])} points", file=sys.stderr)
        except Exception as e:  # noqa: BLE001 - reported, the others go on
            print(f"{name}: probe failed: {e}", file=sys.stderr)


# ---------------------------------------------------------------------------
# transcribe
# ---------------------------------------------------------------------------

def model_name(case_id: str) -> str:
    return "Hard." + "".join(p.capitalize() for p in re.split(r"[-_]", case_id))


def transcribe_one(cache: Path, work: Path, case_id: str, name: str, params=None, drop=(),
                   extra=(), starts=None) -> dict:
    d = json.loads((work / "probe" / f"{name}.json").read_text())
    src = problem_file(cache, name).read_text(errors="replace")
    n, kind = d["neqn"], d["type"]
    rows, eqs, notes, reads = fortran.feval_symbolic(src, n, kind, params,
                                                     trange=(d["t"][0], d["t"][-1]))
    if kind == "ODE":
        fixed = [True] * n
    elif kind == "DAE":
        fixed = [any(d["mass"][i][j] != 0 for i in range(n)) for j in range(n)]
    else:
        fixed = [j in reads for j in range(n)]
    y0 = list(d["y0"])
    for k, (v, fx) in (starts or {}).items():
        y0[k - 1], fixed[k - 1] = v, fx
    title = (f"IVP Test Set 2.4 problem {name} ({kind}, {n} equations), "
             f"transcribed by script from its Fortran feval")
    text, index = fortran.model_text(model_name(case_id), title, n, kind, rows, eqs,
                                     d.get("mass"), y0, fixed, drop_rows=set(drop), extra=extra)
    out = work / "cases"
    out.mkdir(parents=True, exist_ok=True)
    (out / f"{case_id}.mo").write_text(text)
    t0, t1 = d["t"][0], d["t"][-1]
    case = {"text": f"{case_id}.mo", "model": model_name(case_id), "t0": t0, "t_end": t1,
            "dt": (t1 - t0) / 10, "rtols": RTOLS, "atol_factor": 1.0, "record": [],
            "repeat": 1, "problem": name, "rows": index, "notes": notes,
            "n_w": len(eqs), "type": kind}
    (out / f"{case_id}.json").write_text(json.dumps(case, indent=1))
    return case


def transcribe(cache: Path, work: Path, only: list[str]) -> None:
    jobs = [(f"ivp-{p}", p, None, (), (), None) for p in PROBLEMS]
    jobs += [(cid, p, prm, dr, ex, st) for cid, (p, prm, dr, ex, st) in DERIVED.items()]
    for cid, p, prm, dr, ex, st in jobs:
        if only and p not in only and cid not in only:
            continue
        if not (work / "probe" / f"{p}.json").exists():
            continue
        try:
            c = transcribe_one(cache, work, cid, p, prm, dr, ex, st)
            print(f"{cid}: {c['n_w']} intermediate variables; notes {len(c['notes'])}", file=sys.stderr)
        except fortran.Unsupported as e:
            (work / "cases").mkdir(parents=True, exist_ok=True)
            (work / "cases" / f"{cid}.untranscribed").write_text(str(e))
            print(f"{cid}: NOT transcribed: {e}", file=sys.stderr)


# ---------------------------------------------------------------------------
# check: our equations against the Fortran, 1e-13
# ---------------------------------------------------------------------------

def target_dir() -> Path:
    return Path(os.environ.get("CARGO_TARGET_DIR", REPO / "engine" / "target")) / "release" / "examples"


def check_one(work: Path, case_id: str, cases: str = "cases") -> dict:
    case = json.loads((work / cases / f"{case_id}.json").read_text())
    d = json.loads((work / "probe" / f"{case['problem']}.json").read_text())
    n, kind = d["neqn"], d["type"]
    pts = d["points"]
    feed = [{"t": p["t"], "vars": {f"y{i + 1}": p["y"][i] for i in range(n)},
             "ders": {f"y{i + 1}": p["yp"][i] for i in range(n)}} for p in pts]
    pf = work / "check" / f"{case_id}.points.json"
    pf.parent.mkdir(parents=True, exist_ok=True)
    pf.write_text(json.dumps(feed))
    of = work / "check" / f"{case_id}.eval.json"
    r = subprocess.run(pinned([str(target_dir() / "hard_eval"), str(work / cases / case["text"]),
                               case["model"], str(pf), str(of)]), capture_output=True, text=True)
    if r.returncode:
        return {"id": case_id, "ok": False, "why": f"hard_eval: {r.stderr[-1500:]}"}
    ev = json.loads(of.read_text())
    # the Fortran's rows: lhs (M y', y' or 0) and rhs (f)
    worst, where, bad = 0.0, None, []
    deps = term_deps(work / cases / case["text"])
    fort = []
    for p in pts:
        lhs = []
        for i in range(n):
            if kind == "ODE":
                lhs.append(p["yp"][i])
            elif kind == "DAE":
                lhs.append(sum(d["mass"][i][j] * p["yp"][j] for j in range(n) if d["mass"][i][j]))
            else:
                lhs.append(0.0)
        rhs = p["f"]
        fort.append((lhs, rhs))
    for k, (p, e) in enumerate(zip(pts, ev)):
        # the largest term a row's computation adds, through every
        # intermediate it reads (they are defined before they are read)
        big = []
        for q, r in enumerate(e["rows"]):
            m = (r[2] or 0.0) if r is not None and len(r) > 2 else 0.0
            for dq in deps[q]:
                m = max(m, big[dq])
            big.append(m)
        if p["ierr"] != 0:
            bad.append(f"point {k}: the Fortran rejected it (ierr {p['ierr']})")
            continue
        lhs, rhs = fort[k]
        for i in range(n):
            row = case["rows"][i]
            if row < 0:
                continue
            got = e["rows"][row]
            if got is None:
                bad.append(f"point {k} row {i + 1}: not evaluable ({e['unknown'][:5]})")
                continue
            s = max(big[row], abs(lhs[i]), abs(rhs[i]))
            s = s if s > 0 else 1.0
            err = max(abs(got[0] - lhs[i]), abs(got[1] - rhs[i])) / s
            if not math.isfinite(err):
                err = math.inf
            if err > worst:
                worst, where = err, (k, i + 1)
    return {"id": case_id, "ok": not bad and worst <= 1e-13, "worst": worst, "where": where,
            "problems": bad[:10], "points": len(pts), "notes": case["notes"]}


def term_deps(model: Path) -> list[list[int]]:
    """For each equation of a transcribed model, the equations of the
    intermediates it reads (w<k> is defined by equation k - 1)."""
    eqs = model.read_text().split("\nequation\n", 1)[1].split("\nend ", 1)[0]
    out = []
    for line in eqs.strip().split("\n"):
        rhs = line.split("=", 1)[1] if line.lstrip().startswith("w") else line
        out.append([int(x) - 1 for x in re.findall(r"\bw(\d+)\b", rhs)])
    return out


def check(work: Path, only: list[str]) -> None:
    res = []
    for f in sorted((work / "cases").glob("ivp-*.json")):
        cid = f.stem
        if only and cid not in only and cid[4:] not in only:
            continue
        c = check_one(work, cid)
        res.append(c)
        w = c.get("worst")
        print(f"{cid}: {'MATCH' if c['ok'] else 'DIFFERS'} worst {w if w is None else f'{w:.2e}'} "
              f"at {c.get('where')} {c.get('why', '')} {c.get('problems', '')}", file=sys.stderr)
    old = {}
    p = work / "check.json"
    if p.exists():
        old = {c["id"]: c for c in json.loads(p.read_text())}
    old.update({c["id"]: c for c in res})
    p.write_text(json.dumps(sorted(old.values(), key=lambda c: c["id"]), indent=1))


# ---------------------------------------------------------------------------
# outside: the test set's own solvers, built here
# ---------------------------------------------------------------------------

OUTSIDE = {  # solver: (driver, solver file, auxiliary file, reads h0)
    "RADAU5": ("radau5d", "radau5", "radaua", True),
    "DASSL": ("dassld", "ddassl", "dassla", False),
    "PSIDE": ("psided", "pside", "psidea", False),
}


def outside_one(exe: Path, rtol: float, pump: bool, h0: bool, timeout: float) -> dict:
    feed = [repr(rtol)] if pump else [repr(rtol), repr(rtol)]
    if h0:
        feed.append(repr(rtol))
    d = exe.parent / f"run-{rtol:g}"
    d.mkdir(exist_ok=True)
    try:
        r = subprocess.run(pinned([str(exe), str(d) + "/"]), input="\n".join(feed) + "\n",
                           capture_output=True, text=True, timeout=timeout, cwd=d)
    except subprocess.TimeoutExpired:
        return {"rtol": rtol, "ok": False, "error": f"no answer within {timeout:g} s"}
    out = r.stdout
    ys = {int(i): float(v.replace("D", "E")) for i, v in
          re.findall(r"y\(\s*(\d+)\)\s*=\s*([-+0-9.EeDd]+)", out)}
    counts = {}
    for key, pat in (("steps", r"number of integration steps\s+(\d+)"),
                     ("accepted", r"number of accepted steps\s+(\d+)"),
                     ("rhs_evals", r"number of f evaluations\s+(\d+)"),
                     ("jac_evals", r"number of Jacobian evaluations\s+(\d+)"),
                     ("lu", r"number of LU decompositions\s+(\d+)")):
        m = re.search(pat, out)
        if m:
            counts[key] = int(m.group(1))
    if not ys or "ERROR" in out:
        err = [ln.strip() for ln in out.splitlines() if "ERROR" in ln or "error" in ln.lower()]
        return {"rtol": rtol, "ok": False, "error": " | ".join(err[-3:]) or out[-300:]}
    return {"rtol": rtol, "ok": True, "y": [ys[i] for i in sorted(ys)], **counts}


def outside(cache: Path, work: Path, only: list[str], jobs: int = 8, timeout: float = 900) -> None:
    from concurrent.futures import ThreadPoolExecutor
    src = cache / TS_DIR / "src"
    tasks = []
    for solver, (drv, sol, aux, h0) in OUTSIDE.items():
        for p in PROBLEMS:
            if only and p not in only and solver not in only:
                continue
            d = work / "outside" / solver / p
            d.mkdir(parents=True, exist_ok=True)
            exe = d / "solve"
            r = subprocess.run(pinned([GFORTRAN, "-O2", "-std=legacy", "-w", "-o", str(exe),
                                str(src / "drivers" / f"{drv}.f"), str(src / "solvers" / f"{sol}.f"),
                                str(src / "auxil" / f"{aux}.f"), str(src / "auxil" / "report.f"),
                                str(problem_file(cache, p))]), capture_output=True, text=True)
            if r.returncode:
                (d / "result.json").write_text(json.dumps({"solver": solver, "problem": p,
                                                           "build": r.stderr[-1500:], "runs": []}))
                continue
            tasks.append((solver, p, exe, h0))

    def go(t):
        solver, p, exe, h0 = t
        runs = [outside_one(exe, rt, p == "pump", h0, timeout) for rt in RTOLS]
        (exe.parent / "result.json").write_text(json.dumps({"solver": solver, "problem": p,
                                                            "runs": runs}))
        ok = sum(r["ok"] for r in runs)
        print(f"{solver} {p}: {ok} of {len(runs)} ran", file=sys.stderr)

    with ThreadPoolExecutor(jobs) as ex:
        list(ex.map(go, tasks))


# ---------------------------------------------------------------------------
# run: ours
# ---------------------------------------------------------------------------

# ida: every block kept implicit, so every problem runs on IDA (its
# intermediates are IDA's algebraic unknowns too); default: the engine's own
# choice (CVODE for an explicit ODE, IDA otherwise), as a user's run
BACKENDS = {"ida": {"force_implicit": True}, "diffsol": {"backend": "diffsol"}, "default": {}}


def run_ours(work: Path, only: list[str], jobs: int = 8, timeout: float = 1200) -> None:
    from concurrent.futures import ThreadPoolExecutor
    runner = target_dir() / "judge"
    tasks = []
    for f in sorted((work / "cases").glob("*.json")):
        cid = f.stem
        if only and cid not in only and not any(cid.endswith(o) for o in only):
            continue
        for be in BACKENDS:
            for rt in RTOLS:
                tasks.append((cid, be, rt))

    def go(t):
        cid, be, rt = t
        case = json.loads((work / "cases" / f"{cid}.json").read_text())
        d = work / OURS / be / cid
        d.mkdir(parents=True, exist_ok=True)
        one = dict(case, rtols=[rt], text=str((work / "cases" / case["text"]).resolve()),
                   options=BACKENDS[be])
        cf, of = d / f"{rt:g}.case.json", d / f"{rt:g}.json"
        cf.write_text(json.dumps(one))
        try:
            r = subprocess.run(pinned([str(runner), str(cf), str(of)]), capture_output=True,
                               text=True, timeout=timeout)
            if r.returncode:
                of.write_text(json.dumps({"stage": "crash", "error":
                                          f"exit status {r.returncode}: {r.stderr[-1500:]}"}))
        except subprocess.TimeoutExpired:
            of.write_text(json.dumps({"stage": "hang", "error": f"no answer within {timeout:g} s"}))
        res = json.loads(of.read_text())
        # keep the end values of the problem's own variables only
        for run in res.get("runs", []):
            if "final" in run:
                run["final"] = {k: v for k, v in run["final"].items() if re.fullmatch(r"y\d+", k)}
            run.pop("series", None)
        of.write_text(json.dumps(res))
        cf.unlink()
        stage = res.get("stage")
        okr = [r.get("ok") for r in res.get("runs", [])]
        print(f"{cid} {be} {rt:g}: {stage} {okr}", file=sys.stderr, flush=True)

    with ThreadPoolExecutor(jobs) as ex:
        list(ex.map(go, tasks))


# ---------------------------------------------------------------------------
# score
# ---------------------------------------------------------------------------

def digits(problem: str, yref: list[float], y: list[float]) -> tuple[float, float]:
    """scd and mescd as the test set's getscd computes them (atol = rtol)."""
    skip = SKIP.get(problem, lambda i: False)
    rel, mixed = 0.0, 0.0
    for i, (r, v) in enumerate(zip(yref, y), 1):
        if v is None or not math.isfinite(v):
            return -math.inf, -math.inf
        if abs(r) >= 1e-200 and not skip(i):
            rel = max(rel, abs((r - v) / r))
        mixed = max(mixed, abs(r - v) / (1.0 + abs(r)))
    lg = lambda e: 16.0 if e == 0 else -math.log10(e)  # noqa: E731
    return lg(rel), lg(mixed)


def load_ours(work: Path, be: str, cid: str) -> dict:
    out = {}
    for rt in RTOLS:
        f = work / OURS / be / cid / f"{rt:g}.json"
        if not f.exists():
            continue
        res = json.loads(f.read_text())
        runs = res.get("runs") or [{"ok": False, "stage": res.get("stage"), "error": res.get("error"),
                                    "codes": res.get("codes")}]
        r = dict(runs[0])
        r["prepare_warnings"] = res.get("prepare_warnings", [])
        if res.get("stage") != "run":
            r["stage"] = res.get("stage")
            r["codes"] = res.get("codes")
        out[rt] = r
    return out


def score(work: Path, cache: Path | None = None) -> dict:
    man = tomllib.loads((HERE / "manifest.toml").read_text())
    table = {"cases": {}, "outside": {}}
    probed = [p for p in PROBLEMS if (work / "probe" / f"{p}.json").exists()]  # (--only)
    for p in probed:
        d = json.loads((work / "probe" / f"{p}.json").read_text())
        yref = d.get("yref")
        best, best_m = {}, {}
        for solver in OUTSIDE:
            f = work / "outside" / solver / p / "result.json"
            if not f.exists():
                continue
            res = json.loads(f.read_text())
            rows = {}
            for r in res["runs"]:
                if r["ok"] and yref:
                    r["scd"], r["mescd"] = digits(p, yref, r["y"])
                    best[r["rtol"]] = max(best.get(r["rtol"], -math.inf), r["scd"])
                    best_m[r["rtol"]] = max(best_m.get(r["rtol"], -math.inf), r["mescd"])
                rows[r["rtol"]] = {k: v for k, v in r.items() if k != "y"}
            table["outside"].setdefault(p, {})[solver] = rows
        table["outside"].setdefault(p, {})["best"] = best
        table["outside"][p]["best_mescd"] = best_m
    checks = {c["id"]: c for c in json.loads((work / "check.json").read_text())} \
        if (work / "check.json").exists() else {}
    for c in man["case"]:
        cid = c["id"]
        if not (cid.startswith("ivp-") or cid.startswith("derived-")):
            continue
        p = cid[4:] if cid.startswith("ivp-") else DERIVED[cid][0]
        if p not in probed or not (work / "cases" / f"{cid}.json").exists():
            continue
        d = json.loads((work / "probe" / f"{p}.json").read_text())
        yref, n = d.get("yref"), d["neqn"]
        row = {"transcription": checks.get(f"ivp-{p}", {}).get("ok")}
        for be in BACKENDS:
            runs = load_ours(work, be, cid)
            rows, verdicts = {}, []
            for rt in RTOLS:
                r = runs.get(rt)
                if r is None:
                    continue
                o = {k: r.get(k) for k in ("ok", "steps", "rhs_evals", "jac_evals", "lin_setups",
                                           "err_test_fails", "nonlin_fails", "events", "code",
                                           "stage", "codes")}
                o["error"] = r.get("error") or None
                o["warnings"] = [w["code"] for w in r.get("prepare_warnings", [])]
                o["warning_parts"] = [w["parts"] for w in r.get("prepare_warnings", [])]
                o["run_warnings"] = r.get("warnings") or []
                if r.get("ok") and yref:
                    y = [r["final"].get(f"y{i + 1}") for i in range(n)]
                    o["scd"], o["mescd"] = digits(p, yref, y)
                rows[rt] = o
            row[be] = rows
            row[be + "_verdict"] = ({"pass": None, "fails": ["withdrawn (manifest changes)"], "notes": []}
                                    if cid in WITHDRAWN else
                                    judge_case(c, be, rows, table["outside"].get(p, {}).get("best", {}),
                                               table["outside"].get(p, {}).get("best_mescd", {})))
        table["cases"][cid] = row
    (work / "score.json").write_text(json.dumps(table, indent=1, default=str))
    return table


def judge_case(c: dict, be: str, rows: dict, best: dict, best_mescd: dict | None = None) -> dict:
    """The manifest's expectation for one backend, applied: pass or the
    failures, each classed as the triage orders them."""
    exp = c.get("expect", {}).get(be, "") or c.get("expect", {}).get("ida", "")
    fails, notes = [], []
    best_mescd = best_mescd or {}
    if not rows:
        return {"pass": None, "fails": ["not run"]}
    if exp == "result" or exp.endswith("then result"):
        for rt, o in rows.items():
            if not o.get("ok"):
                kind = "crash" if o.get("stage") == "crash" else "hang" if o.get("stage") == "hang" \
                    else "false refusal"
                fails.append(f"{kind} at {rt:g}: {o.get('code') or o.get('codes')} {(o.get('error') or '')[:120]}")
                continue
            sc = o.get("scd")
            if sc is None:
                continue
            want = (best[rt] - 1) if rt in best and math.isfinite(best[rt]) else -math.log10(rt) - 3
            if sc < want:
                # (a run whose warnings name a variable of the problem told
                # the user: not silent; the mixed digits beside the best
                # outside solver's show whether the error lies in components
                # far below the absolute tolerance)
                told = any(re.search(r"\by\d+\b", w) for w in o.get("run_warnings", []))
                kind = ("short of digits" if sc >= 1 else
                        "wrong answer, warned" if told else "silent wrong answer")
                fails.append(f"{kind} at {rt:g}: scd {sc:.2f}, wanted {want:.2f}; mescd "
                             f"{o.get('mescd', math.nan):.2f}, best outside "
                             f"{best_mescd.get(rt, math.nan):.2f}")
        s4, s10 = rows.get(1e-4, {}).get("scd"), rows.get(1e-10, {}).get("scd")
        if s4 is not None and s10 is not None and s10 - s4 < 3:
            fails.append(f"error does not fall with the tolerance: scd {s4:.2f} at 1e-4, {s10:.2f} at 1e-10")
        prev = None
        for rt in RTOLS:
            sc = rows.get(rt, {}).get("scd")
            if sc is not None and prev is not None and sc < prev - 1:
                fails.append(f"loses {prev - sc:.2f} digits at {rt:g}")
            prev = sc if sc is not None else prev
        if exp.startswith("warning"):
            m = re.match(r"warning (\S+) naming (\S+)", exp)
            for rt, o in rows.items():
                if m.group(1) not in o.get("warnings", []):
                    fails.append(f"no {m.group(1)} warning at {rt:g} (warnings {o.get('warnings')})")
                    break
    elif exp.startswith("refusal"):
        m = re.match(r"refusal (\S+) naming (\S+)", exp)
        code, part = m.group(1), m.group(2)
        for rt, o in rows.items():
            codes = o.get("codes") or ([{"code": o.get("code"), "parts": []}] if o.get("code") else [])
            if o.get("ok"):
                fails.append(f"silent wrong answer at {rt:g}: no refusal (scd {o.get('scd')})")
                break
            got = [x["code"] for x in codes]
            word = re.compile(rf"\b{re.escape(part)}\b")
            text = json.dumps([x.get("parts") for x in codes]) + (o.get("error") or "")
            named = bool(word.search(text))
            among = len(set(re.findall(r"\b(?:y|w)\d+\b", o.get("error") or "")))
            if code not in got:
                fails.append(f"refusal {got} instead of {code} at {rt:g}; names {part}: {named}")
                break
            if not named:
                fails.append(f"refusal {code} at {rt:g} does not name {part}")
                break
            if among > 3:
                fails.append(f"refusal {code} at {rt:g} names {part} among {among} variables: "
                             "it does not point at the planted part")
            break
    return {"pass": not fails, "fails": fails, "notes": notes}


def report(t: dict, work: Path) -> None:
    """The scores as a table (WORK/report.md): each case's verdicts, then its
    work-precision rows (digits against steps, evaluations and LU)."""
    def cell(o, counts_lu=True):
        if not o:
            return "-"
        if not o.get("ok"):
            return f"FAIL {o.get('code') or o.get('stage') or o.get('error', '')[:30]}"
        sc = o.get("scd")
        lu = o.get("lin_setups", o.get("lu")) if counts_lu else "n/c"
        return (f"{sc:.2f}" if sc is not None else "?") + f" / {o.get('steps')} / {o.get('rhs_evals')} / {lu}"
    lines = ["# Hard problems, phase 1", "",
             "Cell: scd / steps / function evaluations / LU factorisations (n/c: not counted; "
             "DASSL counts none, the test set's report.f sets nlu = 0 for it).", ""]
    npass = {be: 0 for be in BACKENDS}
    for cid, row in t["cases"].items():
        for be in BACKENDS:
            npass[be] += bool(row.get(be + "_verdict", {}).get("pass"))
    lines.append(f"Cases passing their expectation: " +
                 ", ".join(f"{be} {npass[be]} of {len(t['cases'])}" for be in BACKENDS))
    lines.append("")
    for cid, row in t["cases"].items():
        p = cid[4:] if cid.startswith("ivp-") else DERIVED[cid][0]
        lines.append(f"## {cid} (transcription {'matches' if row['transcription'] else 'DIFFERS'})")
        for be in BACKENDS:
            v = row.get(be + "_verdict", {})
            lines.append(f"- {be}: {'pass' if v.get('pass') else 'FAIL'}"
                         + ("".join(f"\n  - {f}" for f in v.get("fails", []))))
        out = t["outside"].get(p, {})
        lines.append("")
        lines.append("| rtol | " + " | ".join(BACKENDS) + " | " + " | ".join(OUTSIDE) + " |")
        lines.append("|---" * (1 + len(BACKENDS) + len(OUTSIDE)) + "|")
        for rt in RTOLS:
            cells = [cell(row.get(be, {}).get(rt)) for be in BACKENDS]
            cells += [cell(out.get(s, {}).get(rt), s != "DASSL") for s in OUTSIDE]
            lines.append(f"| {rt:g} | " + " | ".join(cells) + " |")
        lines.append("")
    if (work / "sundials.json").exists():
        lines += ["## SUNDIALS' C examples (criteria.sundials_c)", ""]
        for r in json.loads((work / "sundials.json").read_text()):
            lines.append(f"- {r['example']} ({r.get('copy', 'upstream')}): "
                         + ("identical" if r["ok"] else f"{r.get('differ', '?')} lines differ "
                            f"{r.get('first', r.get('why', ''))}"))
        lines.append("")
    if (work / "sun-ours.json").exists():
        lines += ["## SUNDIALS' examples on ours (criteria.sundials_ours)", ""]
        for m in json.loads((work / "sun-ours.json").read_text()):
            chk = m.get("check", {})
            lines.append(f"- {m['id']}: transcription "
                         + ("MATCH" if chk.get("ok") else f"not right: {m.get('why') or chk}"))
            for be, v in m.get("verdicts", {}).items():
                lines.append(f"  - {be}: {'pass' if v['pass'] else 'FAIL'}"
                             + "".join(f"\n    - {f}" for f in v["fails"][:4]))
        lines.append("")
    (work / "report.md").write_text("\n".join(lines))
    print("\n".join(lines[:6]), file=sys.stderr)


# ---------------------------------------------------------------------------
# sundials: SUNDIALS' own C examples, built here, against their .out files
# ---------------------------------------------------------------------------

SUNDIALS_TGZ = ("sundials/sundials-7.1.1.tar.gz",
                "https://github.com/LLNL/sundials/releases/download/v7.1.1/sundials-7.1.1.tar.gz",
                "ea7d6edfb52448ddfdc1ec48f89a721fe6c0a259c10f8ef56f60fcded87a94bb")
SUN_EXAMPLES = ["cvode/serial/cvRoberts_dns", "ida/serial/idaRoberts_dns",
                "idas/serial/idasAkzoNob_dns", "ida/serial/idaSlCrank_dns",
                "cvode/serial/cvPendulum_dns", "cvode/serial/cvDisc_dns"]


def sundials_c(cache: Path, work: Path) -> list[dict]:
    """Builds each example as shipped twice: against upstream SUNDIALS
    7.1.1 (CVODE, IDA, IDAS; the release the engine vendors) and against
    the engine's own copy (the vendored sources with the engine's patch,
    CVODES and IDAS, its mathematics lsim-math's: the static library
    `sundials_c` of lsim-sundials-sys), runs each, and compares what it
    prints with its .out, line by line: every printed number and count."""
    tgz = get(cache, *SUNDIALS_TGZ)
    base = work / "sundials"
    src, build, prefix = base / "src", base / "build", base / "install"
    # (CMAKE, else one on the PATH, else pip's next to this Python)
    cmake = (os.environ.get("CMAKE") or shutil.which("cmake")
             or str(Path(sys.executable).with_name("cmake")))
    if not (prefix / "lib").exists() and not (prefix / "lib64").exists():
        shutil.rmtree(base, ignore_errors=True)
        src.mkdir(parents=True)
        subprocess.run(["tar", "-xzf", str(tgz), "-C", str(src), "--strip-components=1"], check=True)
        flags = ["-DBUILD_SHARED_LIBS=OFF", "-DBUILD_STATIC_LIBS=ON", "-DEXAMPLES_ENABLE_C=OFF",
                 "-DEXAMPLES_INSTALL=OFF", "-DBUILD_ARKODE=OFF", "-DBUILD_CVODES=OFF",
                 "-DBUILD_KINSOL=OFF", "-DBUILD_TESTING=OFF", "-DCMAKE_BUILD_TYPE=Release",
                 f"-DCMAKE_INSTALL_PREFIX={prefix}"]
        subprocess.run(pinned([cmake, "-S", str(src), "-B", str(build)] + flags), check=True,
                       stdout=subprocess.DEVNULL)
        subprocess.run(pinned([cmake, "--build", str(build), "-j", "4", "--target", "install"]),
                       check=True, stdout=subprocess.DEVNULL)
    lib = prefix / "lib" if (prefix / "lib").exists() else prefix / "lib64"
    libs = sorted(str(p) for p in lib.glob("libsundials_*.a"))
    # the engine's own copy (vendored, patched, CVODES and IDAS, its
    # mathematics lsim-math's) as one static library; the examples' cvode/
    # and ida/ headers lead to cvodes/ and idas/, whose interface holds theirs
    subprocess.run(pinned(["cargo", "build", "--release", "--locked", "-p", "lsim-sundials-sys",
                           "--example", "sundials_c"]), cwd=REPO / "engine", check=True)
    shim = base / "engine-include"
    for h, to in (("cvode/cvode.h", "cvodes/cvodes.h"), ("ida/ida.h", "idas/idas.h")):
        (shim / h).parent.mkdir(parents=True, exist_ok=True)
        (shim / h).write_text(f"#include <{to}>\n")
    vendored = REPO / "engine" / "crates" / "lsim-sundials-sys" / "sundials" / "include"
    engine_lib = str(target_dir() / "libsundials_c.a")
    res = []
    for ex in SUN_EXAMPLES:
        c = src / "examples" / f"{ex}.c"
        shipped = (src / "examples" / f"{ex}.out").read_text()
        pkg = ex.split("/")[0]
        mine = [x for x in libs if Path(x).name.split("_")[1].split(".")[0]
                not in ({"cvode", "ida", "idas"} - {pkg})]
        builds = {
            "upstream": ["-I", str(prefix / "include"), str(c), "-Wl,--start-group"] + mine
            + ["-Wl,--end-group", "-lm"],
            "engine": ["-I", str(shim), "-I", str(vendored), "-DSUNDIALS_STATIC_DEFINE", str(c),
                       engine_lib, "-lm", "-lpthread", "-ldl"],
        }
        for copy, args in builds.items():
            d = base / "run" / copy / Path(ex).name
            d.mkdir(parents=True, exist_ok=True)
            exe = d / "example"
            b = subprocess.run(pinned(["cc", "-O2", "-o", str(exe)] + args), capture_output=True,
                               text=True)
            if b.returncode:
                res.append({"example": ex, "copy": copy, "ok": False, "why": b.stderr[-800:]})
                print(f"{ex} ({copy}): not built: {b.stderr[-300:]}", file=sys.stderr)
                continue
            out = subprocess.run(pinned([str(exe)]), capture_output=True, text=True, timeout=600,
                                 cwd=d).stdout
            (d / "printed.out").write_text(out)
            a = [ln.rstrip() for ln in shipped.splitlines() if ln.strip()]
            o = [ln.rstrip() for ln in out.splitlines() if ln.strip()]
            diff = [(x, y) for x, y in zip(a, o) if x != y]
            if len(a) != len(o):
                diff.append((f"{len(a)} lines", f"{len(o)} lines"))
            res.append({"example": ex, "copy": copy, "ok": not diff, "lines": len(a),
                        "differ": len(diff), "first": diff[:6]})
            print(f"{ex} ({copy}): {'identical' if not diff else f'{len(diff)} lines differ'}",
                  file=sys.stderr)
    (work / "sundials.json").write_text(json.dumps(res, indent=1))
    return res


def sun_model(cid: str, n: int, kind: str, rows, roots, quad, base: dict, phase, quad_atol):
    """A SUNDIALS example's transcription as model text (fortran.model_text's
    form): a vector absolute tolerance as nominal values (atol = rtol x
    nominal), the quadrature a state, each root function's crossings either
    way recorded at events, the main program's reset of the solution an
    event."""
    reads = {k for k in range(n) if any(f"der(y{k + 1})" in r for r in rows)}
    fixed = [True] * n if kind == "ODE" else [k in reads for k in range(n)]
    title = f"SUNDIALS 7.1.1 example {cid[4:]}, transcribed by script from its C source"
    text, index = fortran.model_text(model_name(cid), title, n, kind, rows, [], None, base["y0"],
                                     fixed)
    rtol = base["rtol"]
    if base.get("atol"):
        for k, a in enumerate(base["atol"]):
            text = re.sub(rf"(  Real y{k + 1}\(start = [^,]+, fixed = \w+)\)",
                          rf"\1, nominal = {a / rtol!r})", text)
    decl, eqs = [], []
    if quad:
        q0 = (base.get("q0") or [0.0])[0]
        nom = f", nominal = {quad_atol / rtol!r}" if quad_atol else ""
        decl.append(f"  Real q1(start = {q0!r}, fixed = true{nom});")
        eqs.append(f"  s1 * der(q1) = {quad[0]};")
    for j, g in enumerate(roots):
        for way, rel in (("up", ">="), ("down", "<=")):
            decl.append(f"  discrete Real r{j + 1}{way}(start = -1, fixed = true);")
            eqs.append(f"  when ({g}) {rel} 0 then\n    r{j + 1}{way} = time / s1;\n  end when;")
    if phase and phase.get("reset"):
        t, v = phase["reset"]
        eqs.append(f"  when time >= {t!r} * s1 then\n    reinit(y1, {v!r});\n  end when;")
    text = text.replace("\nequation\n", "\n" + "".join(x + "\n" for x in decl) + "equation\n")
    text = text.replace(f"\nend {model_name(cid)};", "\n" + "\n".join(eqs) + f"\nend {model_name(cid)};")
    return text.replace("\n\n", "\n"), index


def sun_ours(work: Path, only: list[str], jobs: int = 8) -> list[dict]:
    """Each SUNDIALS example transcribed (csun), checked against its C
    function at points near its start (1e-13, as the test set's), run on
    every backend at the example's own tolerances to each time its .out
    prints, and judged (criteria.sundials_ours, with the dated changes of
    13 November: half a unit of the last printed digit allowed besides;
    cvPendulum_dns on its errors at tf)."""
    import csun  # noqa: PLC0415
    from concurrent.futures import ThreadPoolExecutor  # noqa: PLC0415
    sun, prefix = work / "sundials" / "src", work / "sundials" / "install"
    (work / "suncases").mkdir(parents=True, exist_ok=True)
    runner = target_dir() / "judge"
    tasks, models = [], []
    for name, ex in csun.EXAMPLES.items():
        if only and name not in only:
            continue
        src_text = (sun / "examples" / f"{ex['path']}.c").read_text()
        shown = csun.printed(name, ex, (sun / "examples" / f"{ex['path']}.out").read_text())
        try:
            base = csun.probe(sun, work / "sunprobe" / name, ex, None, prefix)
        except csun.Unsupported as e:
            models.append({"id": f"sun-{name}", "transcribed": False, "why": str(e)})
            print(f"sun-{name}: NOT probed: {e}", file=sys.stderr)
            continue
        # the times the .out rounds, as the example's solver returned them
        trace = base.get("trace", [])

        def exact(t, th):
            near = [x for x in trace if abs(x - t) <= th * (1 + 1e-9)]
            return min(near, key=lambda x: abs(x - t)) if near else t
        for secs in shown["models"].values():
            for sec in secs.values():
                sec["rows"] = [(exact(t, th), th, v) for t, th, v in sec["rows"]]
                sec["roots"] = [(exact(t, th), th) for t, th in sec["roots"]]
        n = len(base["y0"])
        kind = "ODE" if ex["pkg"] == "cv" else "IDE"
        defs = csun.defines(src_text)
        quad_atol = float(re.sub(r"SUN_RCONST\((.*)\)", r"\1", defs["ATOLQ"])) if ex.get("quad") else None
        for key, phase in (csun.PHASES.get(name) or {"main": None}).items():
            cid = f"sun-{name}" + ("" if key == "main" else f"-{key}")
            pts = []
            for t_from, flag in (phase["flags"] if phase else [(base["t0"], "-")]):
                t = base["t0"] if t_from <= base["t0"] else t_from + 0.5
                for seed in (0, 1, 2, 3):
                    r = random.Random(seed)
                    mv = (lambda v: v) if seed == 0 else \
                        (lambda v: v * (1 + 1e-3 * r.uniform(-1, 1)) + 1e-3 * r.uniform(-1, 1))
                    pts.append({"t": t, "flag": flag, "seed": seed, "y": [mv(v) for v in base["y0"]],
                                "yp": [mv(v) for v in (base["yp0"] or [0.0] * n)]})
            try:
                at = csun.probe(sun, work / "sunprobe" / name / key, ex, pts, prefix)
                rows, roots, quad, _ = csun.transcribe(src_text, ex, n, phase)
            except csun.Unsupported as e:
                models.append({"id": cid, "transcribed": False, "why": str(e)})
                print(f"{cid}: NOT transcribed: {e}", file=sys.stderr)
                continue
            for p, q in zip(pts, at["points"]):
                p["f"], p["ierr"] = q["f"], 0
                if kind == "ODE":
                    p["yp"] = q["f"]
            text, index = sun_model(cid, n, kind, rows, roots, quad, base, phase, quad_atol)
            (work / "suncases" / f"{cid}.mo").write_text(text)
            (work / "suncases" / f"{cid}.json").write_text(json.dumps(
                {"text": f"{cid}.mo", "model": model_name(cid), "problem": cid, "rows": index,
                 "notes": [], "type": kind, "n_w": 0}, indent=1))
            (work / "probe" / f"{cid}.json").write_text(json.dumps(
                {"type": kind, "neqn": n, "t": [base["t0"]], "y0": base["y0"], "yp0": base["yp0"],
                 "mass": None, "points": pts}))
            chk = check_one(work, cid, "suncases")
            print(f"{cid}: transcription {'MATCH' if chk['ok'] else 'DIFFERS'} worst "
                  f"{chk.get('worst')} {chk.get('why', '')} {chk.get('problems', '')}", file=sys.stderr)
            m = {"id": cid, "transcribed": True, "check": chk, "n": n, "kind": kind,
                 "rtol": base["rtol"], "atol": base.get("atol") or [base["satol"]] * n,
                 "printed": shown["models"].get(key, {}), "quad": shown["quad"],
                 "pendulum": shown["pendulum"], "ref": base.get("ref")}
            models.append(m)
            if not chk["ok"]:
                continue
            factor = 1.0 if base.get("atol") else base["satol"] / base["rtol"]
            if ex["out"] == "pendulum":
                ends = [(rt, 30.0) for rt in shown["pendulum"]]
            else:
                times = sorted({r[0] for sec in m["printed"].values() for r in sec["rows"]})
                ends = [(base["rtol"], t) for t in times if t > base["t0"]]
            for be in BACKENDS:
                for rt, t_end in ends:
                    tasks.append((cid, be, rt, t_end, factor if ex["out"] != "pendulum" else 1.0))

    def go(task):
        cid, be, rt, t_end, factor = task
        d = work / "sunours" / be / cid
        d.mkdir(parents=True, exist_ok=True)
        case = {"text": str((work / "suncases" / f"{cid}.mo").resolve()), "model": model_name(cid),
                "t0": 0.0, "t_end": t_end, "dt": t_end, "rtols": [rt], "atol_factor": factor,
                "record": [], "repeat": 1, "options": BACKENDS[be]}
        cf, of = d / f"{rt:g}-{t_end!r}.case.json", d / f"{rt:g}-{t_end!r}.json"
        cf.write_text(json.dumps(case))
        try:
            r = subprocess.run(pinned([str(runner), str(cf), str(of)]), capture_output=True,
                               text=True, timeout=600)
            if r.returncode:
                of.write_text(json.dumps({"stage": "crash", "error": r.stderr[-800:]}))
        except subprocess.TimeoutExpired:
            of.write_text(json.dumps({"stage": "hang", "error": "no answer within 600 s"}))
        cf.unlink()
        res = json.loads(of.read_text())
        run = (res.get("runs") or [{}])[0]
        return task, {"ok": bool(run.get("ok")), "final": run.get("final", {}),
                      "events": run.get("event_times", []), "warnings": run.get("warnings", []),
                      "error": run.get("error") or res.get("error"), "code": run.get("code")
                      or [c.get("code") for c in res.get("codes") or []], "steps": run.get("steps")}

    with ThreadPoolExecutor(jobs) as pool:
        done = list(pool.map(go, tasks))
    got = {}
    for (cid, be, rt, t_end, _), r in done:
        got.setdefault((cid, be), {})[(rt, t_end)] = r
    out = []
    for m in models:
        if not m.get("transcribed") or not m["check"]["ok"]:
            out.append({**m, "verdicts": {}})
            continue
        m["verdicts"] = {be: sun_judge(m, got.get((m["id"], be), {})) for be in BACKENDS}
        for be, v in m["verdicts"].items():
            print(f"{m['id']} {be}: {'pass' if v['pass'] else 'FAIL'} {v['fails'][:3]}", file=sys.stderr)
        out.append(m)
    (work / "sun-ours.json").write_text(json.dumps(out, indent=1, default=str))
    return out


def sun_judge(m: dict, runs: dict) -> dict:
    """criteria.sundials_ours on one backend's runs (and the dated change of
    13 November: half a unit in the last digit the .out prints is allowed
    besides; cvPendulum_dns: each error at tf at most ten times the
    example's own without projection)."""
    fails, compared = [], 0
    if m["pendulum"]:
        for rt, rowsets in m["pendulum"].items():
            r = runs.get((rt, 30.0))
            if not r or not r["ok"]:
                fails.append(f"rtol {rt:g}: {r and (r['code'] or r['error'])}")
                continue
            errs = [abs(r["final"][f"y{k + 1}"] - m["ref"][k]) for k in range(4)]
            compared += 4
            if any(e > 10 * no for e, no in zip(errs, rowsets["NO"])):
                fails.append(f"rtol {rt:g}: errors {['%.2e' % e for e in errs]}, the example's "
                             f"without projection {rowsets['NO']}, with {rowsets['YES']}")
        return {"pass": not fails and compared > 0, "fails": fails or
                ([] if compared else ["nothing compared"]), "compared": compared}
    rtol, atol = m["rtol"], m["atol"]
    for section, sec in m["printed"].items():
        by_t: dict = {}
        for t, th, vals in sec["rows"]:
            by_t.setdefault(t, []).append(vals)
        for t, alts in by_t.items():
            r = runs.get((rtol, t))
            if t <= 0.0:
                continue
            if not r or not r["ok"]:
                fails.append(f"{section} t = {t:g}: {r and (r['code'] or (r['error'] or '')[:120])}")
                continue
            # (a time printed twice, before and after the main program's
            # reset: either value)
            ok_any, worst = False, None
            for vals in alts:
                bad = []
                for k, v, hu in vals:
                    u = r["final"].get(f"y{k + 1}")
                    lim = 100 * rtol * abs(v) + atol[k] + hu
                    if u is None or abs(u - v) > lim:
                        bad.append(f"y{k + 1} {u} against {v} (allowed {lim:.1e})")
                compared += len(vals)
                ok_any |= not bad
                worst = worst or bad
            if not ok_any:
                fails.append(f"{section} t = {t:g}: {worst[:2]}")
        if sec["roots"]:
            t_last = max(t for t, _, _ in sec["rows"])
            ev = (runs.get((rtol, t_last)) or {}).get("events", [])
            for t, hu in sec["roots"]:
                compared += 1
                if not any(abs(e - t) <= 100 * rtol * abs(t) + hu for e in ev):
                    fails.append(f"{section}: no root near t = {t:g} (events {ev[:6]})")
    if m.get("quad"):
        v, hu = m["quad"]
        t_last = max(t for sec in m["printed"].values() for t, _, _ in sec["rows"])
        r = runs.get((rtol, t_last))
        u = r and r["final"].get("q1")
        compared += 1
        if u is None or abs(u - v) > 100 * rtol * abs(v) + hu:
            fails.append(f"quadrature G at t = {t_last:g}: {u} against {v}")
    if not compared:
        fails.append("nothing compared (no printed value read)")
    return {"pass": not fails, "fails": fails, "compared": compared}


def run_probe_points(work: Path, name: str, pts: list[dict]) -> list[list[float]]:
    """f at the given points, from the problem's gfortran-built probe."""
    exe = work / "probe" / name / "probe"
    feed = [str(len(pts))]
    for p in pts:
        feed += [repr(p["t"]), " ".join(map(repr, p["y"])), " ".join(map(repr, p["yp"]))]
    out = subprocess.run([str(exe)], input="\n".join(feed) + "\n", capture_output=True,
                         text=True, timeout=60).stdout
    return [f["f"] for f in parse_probe(out)["F"]]


def check_judge(work: Path) -> None:
    """Step 1's hand transcriptions (engine/judge/judge.py) against the same
    originals. Their equations are written another way (other names, other
    scaling, the second derivative instead of the test set's w), so each of
    their equations must equal one Fortran row times a constant at every
    point: the points are the probe's, with each y' the judge reads as a
    second derivative set to the test set's value of it."""
    sys.path.insert(0, str(REPO / "engine" / "judge"))
    import judge  # noqa: PLC0415
    res = []
    for name, fn in (("andrews", judge.andrews), ("caraxis", judge.caraxis),
                     ("chemakzo", judge.chemakzo)):
        text, case = fn()
        d = json.loads((work / "probe" / f"{name}.json").read_text())
        n = d["neqn"]
        chans = case["judge"]["channels"]
        pts = [dict(p) for p in d["points"]]
        names, ders = {}, {}
        for i, c in enumerate(chans):
            if isinstance(c, list):  # component i is der(X): X's y' is y(i)
                x = c[0][4:-1]
                j = chans.index(x)
                for p in pts:
                    p["yp"] = list(p["yp"])
                    p["yp"][j] = p["y"][i]
            else:
                names[c] = i
        fs = run_probe_points(work, name, pts)
        feed = [{"t": p["t"], "vars": {c: p["y"][i] for c, i in names.items()},
                 "ders": {c: p["yp"][i] for c, i in names.items()}} for p in pts]
        dd = work / "check" / "judge"
        dd.mkdir(parents=True, exist_ok=True)
        (dd / f"{name}.mo").write_text(text)
        (dd / f"{name}.points.json").write_text(json.dumps(feed))
        r = subprocess.run(pinned([str(target_dir() / "hard_eval"), str(dd / f"{name}.mo"), case["model"],
                                   str(dd / f"{name}.points.json"), str(dd / f"{name}.eval.json")]),
                           capture_output=True, text=True)
        if r.returncode:
            res.append({"id": f"judge-{name}", "ok": False, "why": r.stderr[-800:]})
            continue
        ev = json.loads((dd / f"{name}.eval.json").read_text())
        mass = d.get("mass")
        rf = []  # the Fortran's residual rows, M y' - f
        for p, f in zip(pts, fs):
            rf.append([sum(mass[i][j] * p["yp"][j] for j in range(n)) - f[i] for i in range(n)])
        worst, unmatched = 0.0, []
        for q in range(len(ev[0]["rows"])):
            rows = [e["rows"][q] for e in ev]
            if any(r is None for r in rows):
                unmatched.append(f"equation {q + 1}: not evaluable")
                continue
            rj = [r[0] - r[1] for r in rows]
            sj = [r[2] for r in rows]
            best = None
            for i in range(n):
                k = max(range(len(pts)), key=lambda k: abs(rf[k][i]))
                if rf[k][i] == 0:
                    continue
                c = rj[k] / rf[k][i]
                err = max(abs(rj[k2] - c * rf[k2][i]) / max(sj[k2], abs(c) * abs(rf[k2][i]), 1e-300)
                          for k2 in range(len(pts)))
                if best is None or err < best[0]:
                    best = (err, i + 1, c)
            if best is None or best[0] > 1e-13:
                unmatched.append(f"equation {q + 1}: nearest row {best and best[1]} at {best and best[0]:.1e}")
            else:
                worst = max(worst, best[0])
        res.append({"id": f"judge-{name}", "ok": not unmatched, "worst": worst,
                    "problems": unmatched[:8], "equations": len(ev[0]["rows"])})
        print(f"judge-{name}: {'MATCH' if not unmatched else 'DIFFERS'} worst {worst:.2e} "
              f"{unmatched[:4]}", file=sys.stderr)
    p = work / "check.json"
    old = {c["id"]: c for c in json.loads(p.read_text())} if p.exists() else {}
    old.update({c["id"]: c for c in res})
    p.write_text(json.dumps(sorted(old.values(), key=lambda c: c["id"]), indent=1))


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("stage")
    ap.add_argument("dirs", nargs="+")
    ap.add_argument("--only", nargs="*", default=[])
    a = ap.parse_args()
    st = a.stage
    if st == "fetch":
        fetch(Path(a.dirs[0]))
    elif st == "probe":
        probe(Path(a.dirs[0]), Path(a.dirs[1]), a.only)
    elif st == "transcribe":
        transcribe(Path(a.dirs[0]), Path(a.dirs[1]), a.only)
    elif st == "check":
        check(Path(a.dirs[0]), a.only)
        if not a.only:
            check_judge(Path(a.dirs[0]))
    elif st == "outside":
        outside(Path(a.dirs[0]), Path(a.dirs[1]), a.only)
    elif st == "run":
        run_ours(Path(a.dirs[0]), a.only)
    elif st == "sundials":
        sundials_c(Path(a.dirs[0]), Path(a.dirs[1]))
    elif st == "sunours":
        sun_ours(Path(a.dirs[0]), a.only)
    elif st == "score":
        report(score(Path(a.dirs[0])), Path(a.dirs[0]))
    elif st == "all":
        cache, work = Path(a.dirs[0]), Path(a.dirs[1])
        subprocess.run(pinned(["cargo", "build", "--release", "--locked", "-p", "lsim-project",
                               "--example", "judge", "--example", "hard_eval"]),
                       cwd=REPO / "engine", check=True)
        fetch(cache)
        probe(cache, work, a.only)
        transcribe(cache, work, a.only)
        check(work, a.only)
        if not a.only:
            check_judge(work)
        outside(cache, work, a.only)
        run_ours(work, a.only)
        sundials_c(cache, work)
        sun_ours(work, a.only)
        report(score(work), work)
    else:
        sys.exit(f"unknown stage {st}")


if __name__ == "__main__":
    main()
