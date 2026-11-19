"""SUNDIALS' own serial dense examples on the engine (step 3, item 3; the
manifest's sun-* cases, criteria.sundials_ours).

Each example's right-hand side or residual, its root functions and its
quadrature are transcribed by script from its C source (this module runs the
C function's statements on symbols); the transcription is checked against
the C function itself at points near the example's start (the example is
built with a probe that evaluates its own functions there: hard.py's check,
1e-13), then run on the engine at the example's own tolerances, and every
value its .out prints is compared (hard.py sunours, after hard.py sundials
has unpacked and built SUNDIALS). What a C main program does between solver
calls (cvDisc_dns's change of right-hand side at t = 1 and its reset of the
solution there) is not a right-hand side: it is stated per example below
(PHASES) and written into the model as an event.
"""
from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path

# per example: its file, its package, the functions the probe and the
# transcription read, how its .out prints the solution
EXAMPLES = {
    "cvRoberts_dns": dict(path="cvode/serial/cvRoberts_dns", pkg="cv", fn="f", roots="g",
                          out="at"),
    "idaRoberts_dns": dict(path="ida/serial/idaRoberts_dns", pkg="ida", fn="resrob",
                           roots="grob", out="cols", ncols=3),
    "idasAkzoNob_dns": dict(path="idas/serial/idasAkzoNob_dns", pkg="ida", fn="res",
                            quad="rhsQ", out="cols", ncols=6),
    "idaSlCrank_dns": dict(path="ida/serial/idaSlCrank_dns", pkg="ida", fn="ressc", out="cols",
                           ncols=3),
    "cvPendulum_dns": dict(path="cvode/serial/cvPendulum_dns", pkg="cv", fn="f",
                           ref="RefSol", t_ref=30.0, out="pendulum"),
    "cvDisc_dns": dict(path="cvode/serial/cvDisc_dns", pkg="cv", fn="f", out="disc"),
}
# cvDisc_dns's main program, per section of its .out: the flag its f
# switches on, by time, and its reset of y at t = 1 (time, value)
PHASES = {
    "cvDisc_dns": {
        "solution": {"title": "Discontinuity in solution", "flags": [(0.0, "RHS1")],
                     "reset": (1.0, 1.0)},
        "rhs": {"title": "Discontinuity in RHS", "flags": [(0.0, "RHS1"), (1.0, "RHS2")]},
    },
}


class Unsupported(Exception):
    pass


# ---------------------------------------------------------------------------
# the C source
# ---------------------------------------------------------------------------

def strip_comments(src: str) -> str:
    src = re.sub(r"/\*.*?\*/", " ", src, flags=re.S)
    return re.sub(r"//[^\n]*", " ", src)


def function(src: str, name: str) -> tuple[list[str], list[str], str]:
    """A function's parameter names, their declarations and its body (the
    definition, not a prototype)."""
    for m in re.finditer(rf"\b{name}\s*\(([^;{{]*?)\)\s*\{{", src):
        depth, i = 1, m.end()
        while depth:
            depth += {"{": 1, "}": -1}.get(src[i], 0)
            i += 1
        decls = [p.strip() for p in m.group(1).split(",")]
        return [re.findall(r"\w+", p)[-1] for p in decls], decls, src[m.end():i - 1]
    raise Unsupported(f"no function {name}")


def defines(src: str) -> dict[str, str]:
    return {m.group(1): m.group(2).strip()
            for m in re.finditer(r"^#define\s+(\w+)\s+([^\n]+)$", src, flags=re.M)}


TOK = re.compile(r"\s*(?:(\d+\.\d*(?:[eE][-+]?\d+)?|\.\d+(?:[eE][-+]?\d+)?|\d+[eE][-+]?\d+|\d+)"
                 r"|([A-Za-z_]\w*)|(->|[-+*/(),\[\]]))")
FUNCS = {"SUNRsqrt": "sqrt", "sqrt": "sqrt", "SQRT": "sqrt", "sin": "sin", "SIN": "sin",
         "cos": "cos", "COS": "cos", "SUNRabs": "abs", "fabs": "abs", "ABS": "abs", "exp": "exp",
         "SUNRexp": "exp", "atan": "atan", "asin": "asin", "acos": "acos", "log": "log"}


def tokens(text: str) -> list[str]:
    out, pos = [], 0
    while text[pos:].strip():
        mt = TOK.match(text, pos)
        if not mt or mt.end() == pos:
            raise Unsupported(f"cannot read {text[pos:]!r}")
        out.append(next(g for g in mt.groups() if g is not None))
        pos = mt.end()
    return out


class Expr:
    """A C expression as model text, by recursive descent; each name read
    is replaced by the machine's text for it, in parentheses."""

    def __init__(self, m: "Machine", text: str):
        self.m, self.t, self.i = m, tokens(text), 0

    def peek(self):
        return self.t[self.i] if self.i < len(self.t) else None

    def take(self, want=None):
        tok = self.peek()
        if want is not None and tok != want:
            raise Unsupported(f"expected {want}, found {tok} in {' '.join(self.t)}")
        self.i += 1
        return tok

    def parse(self) -> str:
        e = self.sum()
        if self.peek() is not None:
            raise Unsupported(f"left over: {self.t[self.i:]}")
        return e

    def sum(self):
        e = self.product()
        while self.peek() in ("+", "-"):
            op = self.take()
            e = f"({e}) {op} ({self.product()})"
        return e

    def product(self):
        e = self.unary()
        while self.peek() in ("*", "/"):
            op = self.take()
            e = f"({e}) {op} ({self.unary()})"
        return e

    def unary(self):
        if self.peek() == "-":
            self.take()
            return f"-({self.unary()})"
        if self.peek() == "+":
            self.take()
        return self.primary()

    def args(self) -> list[str]:
        self.take("(")
        out, depth, start = [], 0, self.i
        while True:
            tok = self.peek()
            if tok is None:
                raise Unsupported("unclosed call")
            if tok in "([":
                depth += 1
            elif tok in ")]" and depth:
                depth -= 1
            elif tok == ")" or (tok == "," and depth == 0):
                out.append(" ".join(self.t[start:self.i]))
                self.take()
                start = self.i
                if tok == ")":
                    return out
                continue
            self.i += 1

    def primary(self):
        tok = self.take()
        m = self.m
        if tok == "(":
            e = self.sum()
            self.take(")")
            return e
        if re.match(r"[\d.]", tok):
            return repr(float(tok))
        if self.peek() == "(":
            a = self.args()
            if tok == "SUN_RCONST":
                return Expr(m, a[0]).parse()
            if tok in ("Ith", "NV_Ith_S"):
                return m.read(a[0].strip(), m.index(a[1]) - (tok == "Ith"))
            if tok == "SUNRpowerI":
                return f"({Expr(m, a[0]).parse()})^({m.index(a[1])})"
            if tok in ("SUNRpowerR", "pow"):
                return f"({Expr(m, a[0]).parse()})^({Expr(m, a[1]).parse()})"
            if tok in FUNCS:
                return f"{FUNCS[tok]}({', '.join(Expr(m, x).parse() for x in a)})"
            raise Unsupported(f"call of {tok}")
        if self.peek() == "->":
            self.take()
            return m.field(tok, self.take())
        if self.peek() == "[":
            self.take()
            start = self.i
            while self.peek() != "]":
                self.take()
            idx = " ".join(self.t[start:self.i])
            self.take("]")
            return m.read(tok, m.index(idx))
        return m.name(tok)


class Machine:
    """Runs a C function's statements on symbols: each local holds the model
    text of its value; the vectors' elements are the model's variables (y,
    their derivatives) and the function's outputs."""

    def __init__(self, src: str, main_body: str):
        self.src = src
        self.defs = defines(src)
        # the user data main fills (`data->k1 = SUN_RCONST(18.7);`) and its
        # constants (`const sunrealtype y01 = ...;`)
        self.fields = {}
        for m in re.finditer(r"\b\w+->(\w+)\s*=\s*([^;]+);", main_body):
            self.fields.setdefault(m.group(1), m.group(2))
        self.consts = dict(re.findall(r"const\s+sunrealtype\s+(\w+)\s*=\s*([^;]+);", main_body))
        self.env: dict[str, str] = {}
        self.vec: dict[str, str] = {}  # a vector or its data pointer: 'y', 'yp' or 'out'
        self.arr: dict[tuple[str, int], str] = {}
        self.out: dict[int, str] = {}
        self.data: set[str] = set()
        self.time = None

    def index(self, text: str) -> int:
        """An index of the source: an integer expression of its macros."""
        for _ in range(10):
            text = re.sub(r"\b([A-Za-z_]\w*)\b",
                          lambda mt: f"({self.defs[mt.group(1)]})" if mt.group(1) in self.defs
                          else mt.group(1), text)
        text = re.sub(r"SUN_RCONST\s*\(([^()]*)\)", r"\1", text)
        if not re.fullmatch(r"[\d\s()+\-*/.]*", text):
            raise Unsupported(f"index {text}")
        return int(eval(text, {"__builtins__": {}}))  # noqa: S307 - arithmetic of numbers only

    def field(self, owner: str, f: str) -> str:
        if owner not in self.data or f not in self.fields:
            raise Unsupported(f"{owner}->{f}")
        return Expr(self, self.fields[f]).parse()

    def name(self, tok: str) -> str:
        if tok == self.time:
            return "(time / s1)"
        if tok in self.env:
            return f"({self.env[tok]})"
        if tok in self.consts:
            return Expr(self, self.consts[tok]).parse()
        if tok in self.defs:
            return Expr(self, self.defs[tok]).parse()
        raise Unsupported(f"unknown name {tok}")

    def read(self, v: str, k: int) -> str:
        role = self.vec.get(v)
        if role == "y":
            return f"y{k + 1}"
        if role == "yp":
            return f"(s1 * der(y{k + 1}))"
        if role == "out" and k in self.out:
            return f"({self.out[k]})"
        if (v, k) in self.arr:
            return f"({self.arr[(v, k)]})"
        raise Unsupported(f"read of {v}[{k}] before it is set")

    def target(self, lhs: str):
        lhs = lhs.strip()
        m = re.fullmatch(r"(Ith|NV_Ith_S)\s*\(\s*(\w+)\s*,(.*)\)", lhs)
        if m:
            return ("v", m.group(2), self.index(m.group(3)) - (m.group(1) == "Ith"))
        m = re.fullmatch(r"(\w+)\s*\[(.*)\]", lhs)
        if m:
            return ("v", m.group(1), self.index(m.group(2)))
        if re.fullmatch(r"\w+", lhs):
            return ("s", lhs, None)
        raise Unsupported(f"assignment to {lhs}")

    def get(self, tg) -> str:
        kind, n, k = tg
        return self.read(n, k) if kind == "v" else self.name(n)

    def put(self, tg, value: str):
        kind, n, k = tg
        if kind == "s":
            self.env[n] = value
        elif self.vec.get(n) == "out":
            self.out[k] = value
        elif n in self.vec:
            raise Unsupported(f"assignment to the input vector {n}")
        else:
            self.arr[(n, k)] = value

    def run(self, params: list[str], decls: list[str], body: str, roles: dict[int, str]):
        """Runs `body`, the parameters in their roles ('time', 'y', 'yp',
        'out', 'data'; a pointer parameter not named here is an array of
        the caller's, already renamed in `body`)."""
        for i, p in enumerate(params):
            r = roles.get(i)
            if r == "time":
                self.time = p
            elif r == "data":
                self.data.add(p)
            elif r is not None:
                self.vec[p] = r
        for st in statements(strip_comments(body)):
            self.statement(st)

    def statement(self, st: str):
        if not st or st.startswith("return") or st == "break":
            return
        m = re.match(r"(?:const\s+)?(?:sunrealtype|realtype|double|int|UserData|N_Vector|void)\b"
                     r"\s*\**\s*(.*)$", st, flags=re.S)
        if m:
            if "=" not in st:
                return  # a declaration
            st = m.group(1)  # a declaration with a value
        if re.fullmatch(r"\**\s*\w+\s*=\s*NULL", st):
            return
        m = re.fullmatch(r"(\w+)\s*=\s*N_VGetArrayPointer\s*\(\s*(\w+)\s*\)", st)
        if m:
            self.vec[m.group(1)] = self.vec[m.group(2)]
            return
        m = re.fullmatch(r"\**\s*(\w+)\s*=\s*\(\s*\w+\s*\**\s*\)\s*(\w+)", st)
        if m:  # data = (UserData)user_data
            self.data.add(m.group(1))
            return
        m = re.fullmatch(r"(\w+)\s*\((.*)\)", st, flags=re.S)
        if m and "=" not in st:
            self.call(m.group(1), [a.strip() for a in m.group(2).split(",")])
            return
        parts = split_assign(st)
        lhs, rhs = parts[:-1], parts[-1]
        op = None
        mo = re.fullmatch(r"(.*?)([-+*/])\s*", lhs[-1])
        if len(lhs) == 1 and mo:
            lhs, op = [mo.group(1)], mo.group(2)
        value = Expr(self, rhs).parse()
        for t in reversed(lhs):
            tg = self.target(t)
            if op:
                value = f"({self.get(tg)}) {op} ({value})"
            self.put(tg, value)

    def call(self, name: str, args: list[str]):
        """A call of another function of the source: run in place, its
        vectors and user data the caller's, its pointer parameters the
        caller's arrays."""
        params, decls, body = function(self.src, name)
        roles, renames = {}, {}
        for i, (p, a, decl) in enumerate(zip(params, args, decls)):
            if "N_Vector" in decl:
                roles[i] = self.vec[a]
            elif "UserData" in decl or "void" in decl:
                roles[i] = "data"
            elif "*" in decl:
                renames[p] = a
            else:
                raise Unsupported(f"argument {a} of {name}")
        saved = (self.env, dict(self.vec), self.time, set(self.data))
        self.env = {}
        for p, a in renames.items():
            body = re.sub(rf"\b{p}\b", a, body)
        self.run(params, decls, body, roles)
        self.env, self.vec, self.time, self.data = saved


def statements(body: str) -> list[str]:
    out, cur, depth = [], [], 0
    for ch in body:
        if ch in "{}" and depth == 0:
            out.append("".join(cur))
            cur = []
            continue
        depth += {"(": 1, ")": -1}.get(ch, 0)
        if ch == ";" and depth == 0:
            out.append("".join(cur))
            cur = []
            continue
        cur.append(ch)
    out.append("".join(cur))
    return [s for s in (x.strip() for x in out) if s and not s.startswith("#")]


def split_assign(st: str) -> list[str]:
    """`a = b = expr` into [a, b, expr]; a compound `a -= e` into
    [`a -`, e]."""
    parts, cur, depth = [], "", 0
    for i, ch in enumerate(st):
        depth += {"(": 1, ")": -1}.get(ch, 0)
        if ch == "=" and depth == 0 and st[i + 1:i + 2] != "=" and st[i - 1:i] not in "<>!=":
            parts.append(cur)
            cur = ""
        else:
            cur += ch
    parts.append(cur)
    if len(parts) < 2:
        raise Unsupported(f"not an assignment: {st}")
    return parts


def transcribe(src_text: str, ex: dict, n: int, phases: dict | None = None):
    """(rows, root rows, quadrature rows, the y' read): each output of the
    example's functions as model text (an ODE's f, a DAE's residual)."""
    src = strip_comments(src_text)
    main_body = function(src, "main")[2]
    cv = ex["pkg"] == "cv"
    roles = {0: "time", 1: "y", 2: "out", 3: "data"} if cv else \
        {0: "time", 1: "y", 2: "yp", 3: "out", 4: "data"}

    def outputs(fn: str, count: int, body: str | None = None) -> list[str]:
        mach = Machine(src, main_body)
        params, decls, b = function(src, fn)
        mach.run(params, decls, b if body is None else body, roles)
        if sorted(mach.out) != list(range(count)):
            raise Unsupported(f"{fn}: outputs {sorted(mach.out)}, {count} expected")
        return [mach.out[k] for k in range(count)]

    body = function(src, ex["fn"])[2]
    sw = re.search(r"switch\s*\(\s*\*\s*\w+\s*\)\s*\{(.*)\}", body, flags=re.S)
    if sw:  # the arms, chosen by time as the main program's phases do
        arms = {m.group(1): outputs(ex["fn"], n, m.group(2))
                for m in re.finditer(r"case\s+(\w+)\s*:(.*?)break\s*;", sw.group(1), flags=re.S)}
        flags = phases["flags"]
        rows = []
        for k in range(n):
            e = arms[flags[-1][1]][k]
            for (_, f), (t_next, _) in reversed(list(zip(flags, flags[1:]))):
                e = f"if (time / s1) < {t_next!r} then {arms[f][k]} else {e}"
            rows.append(e)
    else:
        rows = outputs(ex["fn"], n)
    roots = []
    if ex.get("roots"):
        g = function(src, ex["roots"])[2]
        roots = outputs(ex["roots"], len(set(re.findall(r"gout\s*\[\s*(\d+)\s*\]\s*=", g))))
    quad = outputs(ex["quad"], 1) if ex.get("quad") else []
    reads = {k for k in range(n) if any(re.search(rf"der\(y{k + 1}\)", r) for r in rows)}
    return rows, roots, quad, reads


# ---------------------------------------------------------------------------
# the probe: the example's own functions at points
# ---------------------------------------------------------------------------

PROBE_H = """#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <%(header)s>
#include <nvector/nvector_serial.h>
static void* lsim_ud = NULL;
static double lsim_t0 = 0, lsim_rtol = -1, lsim_satol = -1;
static N_Vector lsim_y0 = NULL, lsim_yp0 = NULL, lsim_atol = NULL, lsim_id = NULL, lsim_q0 = NULL;
static int lsim_nr = 0;
static void* lsim_fn = NULL;
static void lsim_probe_now(void);
static N_Vector lsim_keep(N_Vector v) { N_Vector c = N_VClone(v); N_VScale(1.0, v, c); return c; }
/* LSIM_TRACE: the example runs as shipped, and each solver call's return
   time is printed in full (the .out rounds the times it prints) */
static int lsim_tracing(void) { return getenv("LSIM_TRACE") != NULL; }
static int lsim_traced(int r, sunrealtype* t) { printf("LSIMT %%.17g\\n", (double)*t); return r; }
%(macros)s
"""
CV_MACROS = """#define CVodeInit(m, f, t0, y) (lsim_fn = (void*)(f), lsim_t0 = (t0), lsim_y0 = lsim_keep(y), CVodeInit(m, f, t0, y))
#define CVodeSStolerances(m, r, a) (lsim_rtol = (r), lsim_satol = (a), CVodeSStolerances(m, r, a))
#define CVodeSVtolerances(m, r, a) (lsim_rtol = (r), lsim_atol = lsim_keep(a), CVodeSVtolerances(m, r, a))
#define CVodeSetUserData(m, d) (lsim_ud = (void*)(d), CVodeSetUserData(m, d))
#define CVodeRootInit(m, n, g) (lsim_nr = (n), CVodeRootInit(m, n, g))
#define CVode(m, tout, y, t, task) (lsim_tracing() ? lsim_traced(CVode(m, tout, y, t, task), t) : lsim_fn == (void*)%(fn)s ? (lsim_probe_now(), 0) : CVode(m, tout, y, t, task))
"""
IDA_MACROS = """#define IDAInit(m, r, t0, yy, yp) (lsim_fn = (void*)(r), lsim_t0 = (t0), lsim_y0 = lsim_keep(yy), lsim_yp0 = lsim_keep(yp), IDAInit(m, r, t0, yy, yp))
#define IDASStolerances(m, r, a) (lsim_rtol = (r), lsim_satol = (a), IDASStolerances(m, r, a))
#define IDASVtolerances(m, r, a) (lsim_rtol = (r), lsim_atol = lsim_keep(a), IDASVtolerances(m, r, a))
#define IDASetUserData(m, d) (lsim_ud = (void*)(d), IDASetUserData(m, d))
#define IDARootInit(m, n, g) (lsim_nr = (n), IDARootInit(m, n, g))
#define IDASetId(m, id) (lsim_id = lsim_keep(id), IDASetId(m, id))
#define IDAQuadInit(m, f, q) (lsim_q0 = lsim_keep(q), IDAQuadInit(m, f, q))
#define IDASolve(m, tout, tret, yy, yp, task) (lsim_tracing() ? lsim_traced(IDASolve(m, tout, tret, yy, yp, task), tret) : lsim_fn == (void*)%(fn)s ? (lsim_probe_now(), 0) : IDASolve(m, tout, tret, yy, yp, task))
"""
# (one line per item: a key and numbers)
PROBE_C = r"""
static void lsim_pv(const char* k, N_Vector v) {
  printf("%%s", k);
  if (v) for (sunindextype i = 0; i < N_VGetLength(v); i++) printf(" %%.17g", NV_Ith_S(v, i));
  printf("\n");
}
static void lsim_pa(const char* k, sunrealtype* a, int n) {
  printf(" %%s", k);
  for (int i = 0; i < n; i++) printf(" %%.17g", a[i]);
}
static void lsim_probe_now(void) {
  printf("LSIM\nT0 %%.17g\nRTOL %%.17g\nSATOL %%.17g\nNROOTS %%d\n", lsim_t0, lsim_rtol, lsim_satol,
         lsim_nr);
  lsim_pv("Y0", lsim_y0); lsim_pv("YP0", lsim_yp0); lsim_pv("ATOL", lsim_atol);
  lsim_pv("ID", lsim_id); lsim_pv("Q0", lsim_q0);
  N_Vector y = N_VClone(lsim_y0), yp = N_VClone(lsim_y0), out = N_VClone(lsim_y0);
  sunindextype n = N_VGetLength(lsim_y0);
  int np = 0;
  FILE* fp = getenv("LSIM_POINTS") ? fopen(getenv("LSIM_POINTS"), "r") : NULL;
  if (fp && fscanf(fp, "%%d", &np) != 1) np = 0;
  for (int k = 0; k < np; k++) {
    double t; char flag[64];
    if (fscanf(fp, "%%lf %%63s", &t, flag) != 2) break;
    for (sunindextype i = 0; i < n; i++) if (fscanf(fp, "%%lf", &NV_Ith_S(y, i)) != 1) exit(3);
    for (sunindextype i = 0; i < n; i++) if (fscanf(fp, "%%lf", &NV_Ith_S(yp, i)) != 1) exit(3);
%(setflag)s
    printf("P %%.17g", t);
    %(call)s
    lsim_pa("F", N_VGetArrayPointer(out), (int)n);
%(roots)s%(quad)s
    printf("\n");
  }
%(ref)s
  fflush(stdout);
  exit(0);
}
"""


def probe(sun: Path, d: Path, ex: dict, pts: list[dict] | None, prefix: Path) -> dict:
    """The example built with the probe and run up to its first solver call:
    its start, tolerances, ids and root count as main sets them, and its
    functions at `pts` (each with the flag a switch reads, '-' for none)."""
    src = sun / "examples" / f"{ex['path']}.c"
    d.mkdir(parents=True, exist_ok=True)
    pkg = Path(ex["path"]).parts[0]
    header = {"cvode": "cvode/cvode.h", "ida": "ida/ida.h", "idas": "idas/idas.h"}[pkg]
    macros = (CV_MACROS if ex["pkg"] == "cv" else IDA_MACROS) % {"fn": ex["fn"]}
    (d / "lsim_probe.h").write_text(PROBE_H % {"header": header, "macros": macros})
    yv = "y" if ex["pkg"] == "cv" else "y, yp"
    call = f"{ex['fn']}(t, {yv}, out, lsim_ud);"
    roots = (f"    {{ sunrealtype lsim_g[16]; {ex['roots']}(t, {yv}, lsim_g, lsim_ud); "
             "lsim_pa(\"G\", lsim_g, lsim_nr); }\n") if ex.get("roots") else ""
    quad = (f"    {{ N_Vector q = N_VClone(lsim_q0); {ex['quad']}(t, y, yp, q, lsim_ud); "
            "lsim_pa(\"Q\", N_VGetArrayPointer(q), 1); }\n") if ex.get("quad") else ""
    setflag = ("    if (flag[0] != '-') *(int*)lsim_ud = strcmp(flag, \"RHS2\") == 0 ? RHS2 : RHS1;"
               if "cvDisc" in ex["path"] else "")
    ref = (f"  {{ N_Vector r = N_VNew_Serial(4, sunctx); {ex['ref']}({ex['t_ref']!r}, r, 1); "
           "lsim_pv(\"REF\", r); }") if ex.get("ref") else ""
    body = PROBE_C % {"call": call, "roots": roots, "quad": quad, "setflag": setflag, "ref": ref}
    (d / "probe.c").write_text(f'#include "lsim_probe.h"\n#include "{src}"\n{body}')
    lib = prefix / "lib" if (prefix / "lib").exists() else prefix / "lib64"
    libs = [str(p) for p in sorted(lib.glob("libsundials_*.a"))
            if p.name.split("_")[1].split(".")[0] not in ({"cvode", "ida", "idas"} - {pkg})]
    b = subprocess.run(["cc", "-O0", "-w", "-I", str(prefix / "include"), "-I", str(d),
                        str(d / "probe.c"), "-o", str(d / "probe"), "-Wl,--start-group"] + libs
                       + ["-Wl,--end-group", "-lm"], capture_output=True, text=True)
    if b.returncode:
        raise Unsupported(f"probe build: {b.stderr[-1500:]}")
    env = dict(os.environ)
    if pts:
        n = len(pts[0]["y"])
        feed = [str(len(pts))]
        for p in pts:
            feed += [f"{p['t']!r} {p.get('flag', '-')}", " ".join(map(repr, p["y"])),
                     " ".join(map(repr, p.get("yp") or [0.0] * n))]
        (d / "points.txt").write_text("\n".join(feed) + "\n")
        env["LSIM_POINTS"] = str(d / "points.txt")
    r = subprocess.run([str(d / "probe")], capture_output=True, text=True, timeout=600, cwd=d,
                       env=env)
    if "LSIM\n" not in r.stdout:
        raise Unsupported(f"probe run: {r.stdout[-500:]} {r.stderr[-500:]}")
    out: dict = {"points": []}
    for line in r.stdout.split("LSIM\n", 1)[1].splitlines():
        key, *rest = line.split()
        if key == "P":
            p, cur = {"t": float(rest[0])}, None
            for w in rest[1:]:
                if w in ("F", "G", "Q"):
                    cur = w.lower()
                    p[cur] = []
                else:
                    p[cur].append(float(w))
            out["points"].append(p)
        else:
            vals = [float(w) for w in rest]
            out[key.lower()] = vals if key in ("Y0", "YP0", "ATOL", "ID", "Q0", "REF") else vals[0]
    if not pts:  # the times of every solver return, unrounded
        r = subprocess.run([str(d / "probe")], capture_output=True, text=True, timeout=600, cwd=d,
                           env={**env, "LSIM_TRACE": "1"})
        out["trace"] = [float(ln.split()[1]) for ln in r.stdout.splitlines()
                        if ln.startswith("LSIMT ")]
    return out


# ---------------------------------------------------------------------------
# what the .out prints
# ---------------------------------------------------------------------------

NUM = r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?"


def half_unit(tok: str) -> float:
    """Half a unit in the last digit a printed number shows."""
    m = re.fullmatch(r"[-+]?\d*\.?(\d*)[eE]([-+]?\d+)", tok)
    if m:
        return 0.5 * 10.0 ** (int(m.group(2)) - len(m.group(1)))
    m = re.fullmatch(r"[-+]?\d*\.(\d+)", tok)
    return 0.5 * 10.0 ** (-len(m.group(1))) if m else 0.5


def printed(name: str, ex: dict, text: str) -> dict:
    """The .out's numbers: per model (its PHASES key, or 'main') the rows
    (time, its half unit, [(component, value, half unit)]) and the roots
    (time, half unit) of each section; the quadrature (idasAkzoNob_dns);
    the error rows per rtol (cvPendulum_dns)."""
    out: dict = {"models": {}, "quad": None, "pendulum": {}}
    if ex["out"] == "pendulum":
        rt = None
        for line in text.splitlines():
            m = re.match(rf"\s*rtol = ({NUM}), atol = ({NUM})", line)
            if m:
                rt = float(m.group(1))
                continue
            m = re.match(rf"\s*(YES|NO)\s+({NUM})\s+({NUM})\s+({NUM})\s+({NUM})", line)
            if m and rt is not None:
                out["pendulum"].setdefault(rt, {})[m.group(1)] = [float(m.group(k)) for k in range(2, 6)]
        return out
    phases = PHASES.get(name)
    model, section = ("main", "main") if not phases else (None, None)
    for line in text.splitlines():
        if phases and line.strip() and not re.match(rf"\s*{NUM}", line):
            hit = [k for k, ph in phases.items() if line.strip().startswith(ph["title"])]
            if hit:
                model, section = hit[0], line.strip()
                out["models"].setdefault(model, {})[section] = {"rows": [], "roots": []}
            continue
        if model is None:
            continue
        sec = out["models"].setdefault(model, {}).setdefault(section, {"rows": [], "roots": []})
        if ex["out"] == "at":
            m = re.match(rf"\s*At t = ({NUM})\s+y =((?:\s+{NUM})+)", line)
            toks = [m.group(1)] + m.group(2).split() if m else None
        elif ex["out"] == "cols":
            m = re.match(rf"\s*({NUM}(?:\s+{NUM}){{{ex['ncols']}}})(?:\s|$)", line)
            toks = m.group(1).split() if m else None
        else:
            m = re.fullmatch(rf"\s*({NUM})\s+({NUM})\s*", line)
            toks = [m.group(1), m.group(2)] if m else None
        if toks:
            sec["rows"].append((float(toks[0]), half_unit(toks[0]),
                                [(k, float(v), half_unit(v)) for k, v in enumerate(toks[1:])]))
            continue
        if "rootsfound" in line and sec["rows"]:
            sec["roots"].append(sec["rows"][-1][:2])
        m = re.match(rf"\s*G:\s+({NUM})", line)
        if m:
            out["quad"] = (float(m.group(1)), half_unit(m.group(1)))
    return out
