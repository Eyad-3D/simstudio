"""Transcribes an IVP Test Set problem's Fortran into the engine's text
format by running its `feval` symbolically: y(i), yprime(i) and t are
symbols, every other value is computed as Fortran computes it (integer
division, REAL*4 literals rounded to single precision), loops are run, calls
are inlined, and a branch on a symbol becomes an `if` expression. Each value
that depends on a symbol becomes one variable of the model (`w<k>`), so the
model grows as the code does, never by substitution.

Nothing is typed by hand: the model is what the Fortran computes. What the
script cannot run (a GOTO, a loop bound that depends on a symbol, a call to
a routine the file does not hold) stops it with the line it stopped at.
"""
from __future__ import annotations

import copy
import math
import re
import struct


class Unsupported(Exception):
    pass


def f32(x: float) -> float:
    return struct.unpack("f", struct.pack("f", x))[0]


# ---------------------------------------------------------------------------
# Reading fixed-form source
# ---------------------------------------------------------------------------

def statements(src: str) -> list[tuple[str | None, str, int]]:
    """(label, statement in lower case, line number) of fixed-form source."""
    out: list[list] = []
    for no, raw in enumerate(src.splitlines(), 1):
        if not raw.strip() or raw[0] in "cC*!":
            continue
        line = raw.expandtabs(6).rstrip()
        # an inline comment (no strings with ! in the test set's feval code)
        if "!" in line[6:] and "'" not in line:
            line = line[:6] + line[6:].split("!", 1)[0]
        body = line[6:72] if len(line) > 6 else ""
        if len(line) > 5 and line[5] not in " 0" and out:
            out[-1][1] += body.strip() if False else body
            continue
        label = line[:5].strip() or None
        out.append([label, body, no])
    res = []
    for label, body, no in out:
        s = lower_outside_strings(body).strip()
        if s:
            res.append((label, s, no))
    return res


def lower_outside_strings(s: str) -> str:
    parts = s.split("'")
    return "'".join(p.lower() if i % 2 == 0 else p for i, p in enumerate(parts))


def units(src: str) -> dict[str, dict]:
    """The program units: name -> {kind, args, body, line}."""
    res, cur = {}, None
    for label, s, no in statements(src):
        m = re.match(r"(?:(?:double precision|integer|real\*8|real|logical)\s+)?"
                     r"(subroutine|function)\s+(\w+)\s*(?:\((.*)\))?$", s)
        if cur is None and m:
            cur = {"kind": m.group(1), "name": m.group(2),
                   "args": [a.strip() for a in (m.group(3) or "").split(",") if a.strip()],
                   "body": [], "line": no,
                   "type": "d" if s.startswith(("double", "real*8")) else
                           "i" if s.startswith("integer") else
                           "r" if s.startswith("real") else None}
            continue
        if cur is not None and re.fullmatch(r"end(\s*(subroutine|function)(\s+\w+)?)?", s):
            res[cur["name"]] = cur
            cur = None
            continue
        if cur is not None:
            cur["body"].append((label, s, no))
    return res


# ---------------------------------------------------------------------------
# Expressions
# ---------------------------------------------------------------------------

TOKEN = re.compile(r"""\s*(?:
  (?P<num>(?:\d+\.\d*|\.\d+|\d+)(?:[ed][+-]?\d+)?)(?![a-z_])
 |(?P<dop>\.(?:lt|le|gt|ge|eq|ne|and|or|not|true|false|eqv|neqv)\.)
 |(?P<name>[a-z_][a-z0-9_]*)
 |(?P<op>\*\*|<=|>=|==|/=|//|[-+*/(),<>=:])
 |(?P<str>'[^']*')
)""", re.X)


def tokens(s: str) -> list[tuple[str, str]]:
    out, pos = [], 0
    s = re.sub(r"\.\s*(lt|le|gt|ge|eq|ne|and|or|not|true|false|eqv|neqv)\s*\.", r".\1.", s.rstrip())
    while pos < len(s):
        m = TOKEN.match(s, pos)
        if not m or m.end() == pos:
            raise Unsupported(f"cannot read {s[pos:]!r}")
        pos = m.end()
        kind = m.lastgroup
        out.append((kind, m.group(kind)))
    return out


class Parser:
    def __init__(self, s: str):
        self.t = tokens(s)
        self.i = 0

    def peek(self, k=0):
        j = self.i + k
        return self.t[j] if j < len(self.t) else (None, None)

    def take(self, val=None):
        tok = self.peek()
        if val is not None and tok[1] != val:
            raise Unsupported(f"expected {val!r}, read {tok[1]!r}")
        self.i += 1
        return tok

    def done(self):
        return self.i >= len(self.t)

    # precedence: eqv < or < and < not < relational < +- < */ < ** < unary
    def expr(self):
        return self.or_()

    def or_(self):
        a = self.and_()
        while self.peek()[1] == ".or.":
            self.take()
            a = ("or", a, self.and_())
        return a

    def and_(self):
        a = self.not_()
        while self.peek()[1] == ".and.":
            self.take()
            a = ("and", a, self.not_())
        return a

    def not_(self):
        if self.peek()[1] == ".not.":
            self.take()
            return ("not", self.not_())
        return self.rel()

    RELS = {".lt.": "<", ".le.": "<=", ".gt.": ">", ".ge.": ">=", ".eq.": "==", ".ne.": "<>",
            "<": "<", "<=": "<=", ">": ">", ">=": ">=", "==": "==", "/=": "<>"}

    def rel(self):
        a = self.add()
        if self.peek()[1] in self.RELS:
            op = self.RELS[self.take()[1]]
            return ("cmp", op, a, self.add())
        return a

    def add(self):
        if self.peek()[1] in ("+", "-"):
            sign = self.take()[1]
            a = self.mul()
            if sign == "-":
                a = ("neg", a)
        else:
            a = self.mul()
        while self.peek()[1] in ("+", "-"):
            op = self.take()[1]
            a = (op, a, self.mul())
        return a

    def mul(self):
        a = self.pow_()
        while self.peek()[1] in ("*", "/"):
            op = self.take()[1]
            a = (op, a, self.pow_())
        return a

    def pow_(self):
        a = self.atom()
        if self.peek()[1] == "**":
            self.take()
            # right associative; a unary sign is allowed after ** by gfortran
            if self.peek()[1] in ("+", "-"):
                sign = self.take()[1]
                b = self.pow_()
                b = ("neg", b) if sign == "-" else b
            else:
                b = self.pow_()
            return ("**", a, b)
        return a

    def atom(self):
        kind, val = self.take()
        if kind == "num":
            return ("num", val)
        if kind == "dop" and val in (".true.", ".false."):
            return ("bool", val == ".true.")
        if kind == "op" and val == "(":
            e = self.expr()
            self.take(")")
            return e
        if kind == "op" and val in ("-", "+"):
            # a sign after * or / (gfortran's extension)
            a = self.pow_()
            return ("neg", a) if val == "-" else a
        if kind == "str":
            return ("str", val[1:-1])
        if kind == "name":
            if self.peek()[1] == "(":
                self.take()
                args = []
                if self.peek()[1] != ")":
                    args.append(self.expr())
                    while self.peek()[1] == ",":
                        self.take()
                        args.append(self.expr())
                self.take(")")
                return ("ref", val, args)
            return ("ref", val, None)
        raise Unsupported(f"unexpected {val!r}")


def parse_expr(s: str):
    p = Parser(s)
    e = p.expr()
    if not p.done():
        raise Unsupported(f"left over in {s!r}: {p.t[p.i:]}")
    return e


# ---------------------------------------------------------------------------
# Values: a number of a Fortran kind, or a symbolic double (model text)
# ---------------------------------------------------------------------------

class V:
    __slots__ = ("kind", "c", "s", "tf")

    def __init__(self, kind, c=None, s=None, tf=None):
        self.kind, self.c, self.s = kind, c, s  # kind: i r d l (s: a string)
        self.tf = tf  # for a symbol that depends on time alone: its value as a function of t

    @property
    def const(self):
        return self.s is None

    def text(self) -> str:
        if self.s is not None:
            return self.s
        if self.kind == "l":
            return "true" if self.c else "false"
        x = float(self.c)
        if math.isnan(x) or math.isinf(x):
            raise Unsupported(f"constant {x}")
        r = repr(x)
        return f"({r})" if x < 0 or r.startswith("-") else r

    def __repr__(self):
        return f"V({self.kind},{self.c!r},{self.s!r})"


def literal(tok: str) -> V:
    if re.fullmatch(r"\d+", tok):
        return V("i", int(tok))
    if "d" in tok:
        return V("d", float(tok.replace("d", "e")))
    return V("r", f32(float(tok)))


RANK = {"i": 0, "r": 1, "d": 2}


def coerce(x, kind):
    if kind == "i":
        return int(x)
    if kind == "r":
        return f32(float(x))
    return float(x)


def ieee_div(x: float, y: float) -> float:
    if y != 0:
        return x / y
    if x == 0 or math.isnan(x):
        return math.nan
    return math.copysign(math.inf, x) * math.copysign(1.0, y)


def fortran_int_div(a, b):
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b >= 0) else -q


def atom_text(t: str) -> bool:
    return re.fullmatch(r"[a-z_][a-z0-9_]*|\(?-?[0-9.e+-]+\)?|der\(\w+\)|time", t) is not None


# ---------------------------------------------------------------------------
# The machine
# ---------------------------------------------------------------------------

INTRINSIC_1 = {
    "sin": "sin", "dsin": "sin", "cos": "cos", "dcos": "cos", "tan": "tan", "dtan": "tan",
    "asin": "asin", "dasin": "asin", "acos": "acos", "dacos": "acos", "atan": "atan",
    "datan": "atan", "sinh": "sinh", "dsinh": "sinh", "cosh": "cosh", "dcosh": "cosh",
    "tanh": "tanh", "dtanh": "tanh", "exp": "exp", "dexp": "exp", "log": "log",
    "dlog": "log", "alog": "log", "log10": "log10", "dlog10": "log10", "sqrt": "sqrt",
    "dsqrt": "sqrt", "abs": "abs", "dabs": "abs", "iabs": "abs",
}
PYMATH = {"sin": math.sin, "cos": math.cos, "tan": math.tan, "asin": math.asin,
          "acos": math.acos, "atan": math.atan, "sinh": math.sinh, "cosh": math.cosh,
          "tanh": math.tanh, "exp": math.exp, "log": math.log, "log10": math.log10,
          "sqrt": math.sqrt, "abs": abs}


class Array:
    """A view of storage: (key of the storage, offset, dims as (lo, hi))."""
    __slots__ = ("key", "off", "dims", "kind")

    def __init__(self, key, off, dims, kind):
        self.key, self.off, self.dims, self.kind = key, off, dims, kind

    def flat(self, idx: list[int]) -> int:
        if len(idx) != len(self.dims) and not (len(self.dims) == 1 and len(idx) == 1):
            raise Unsupported(f"index rank {len(idx)} for dims {self.dims}")
        k, mult = 0, 1
        for d, (lo, hi) in enumerate(self.dims):
            k += (idx[d] - lo) * mult
            if hi is not None:
                mult *= hi - lo + 1
        return self.off + k


class Frame:
    def __init__(self):
        self.scal: dict[str, object] = {}     # name -> ("store", key) or Array
        self.types: dict[str, str] = {}
        self.params: dict[str, V] = {}
        self.dims: dict[str, list] = {}
        self.imp: dict[str, str] = {}


class Return(Exception):
    pass


class Goto(Exception):
    def __init__(self, label):
        self.label = label


def time_fn(op, a: V, b: V):
    """The value of `a op b` as a function of time, where both are."""
    fa = a.tf if a.tf else (lambda t, c=a.c: float(c)) if a.const and a.kind != "l" else None
    fb = b.tf if b.tf else (lambda t, c=b.c: float(c)) if b.const and b.kind != "l" else None
    if fa is None or fb is None or (a.const and b.const):
        return None
    f = {"+": lambda x, y: x + y, "-": lambda x, y: x - y, "*": lambda x, y: x * y,
         "/": ieee_div, "**": lambda x, y: x ** y}[op]
    return lambda t: f(fa(t), fb(t))


class Machine:
    def __init__(self, units_: dict, neqn: int, trange=(0.0, 1.0)):
        self.trange = trange
        self.unset: set[str] = set()
        self.single: set[str] = set()  # REAL*4 variables given a symbolic value
        self.units = units_
        self.neqn = neqn
        self.store: dict = {}          # (key, index) -> V
        self.eqs: list[str] = []       # "w<k> = expr"
        self.nw = 0
        self.notes: list[str] = []
        self.read_yp: set[int] = set()
        self.nkeys = 0
        self.commons: dict[str, str] = {}
        self.cond: list[str] = []      # the symbolic conditions we are inside (for notes)

    # -- storage -----------------------------------------------------------
    def newkey(self, name):
        self.nkeys += 1
        return f"{name}#{self.nkeys}"

    def new_w(self, text: str, tf=None) -> V:
        self.nw += 1
        name = f"w{self.nw}"
        self.eqs.append(f"{name} = {text}")
        return V("d", s=name, tf=tf)

    def sym(self, text: str, tf=None) -> V:
        return V("d", s=text, tf=tf) if atom_text(text) else self.new_w(text, tf)

    def int_of_time(self, a: V, what: str) -> V:
        """int(x) for x >= 0 a function of time alone, over the run's span:
        k0 plus one for each integer x passes (each an event at that time)."""
        t0, t1 = self.trange
        xs = [a.tf(t0 + (t1 - t0) * k / 20000) for k in range(20001)]
        lo, hi = min(xs), max(xs)
        if lo < 0:
            raise Unsupported(f"{what} of a value of time below 0")
        k0, k1 = int(lo), int(hi)
        if k1 - k0 > 200:
            raise Unsupported(f"{what} of time crosses {k1 - k0} integers")
        self.notes.append(f"{what} of a function of time written as {k1 - k0} steps over "
                          f"the run's span [{t0!r}, {t1!r}]")
        text = repr(float(k0))
        for k in range(k0 + 1, k1 + 1):
            text += f" + (if ({a.text()}) >= {float(k)!r} then 1.0 else 0.0)"
        w = self.new_w(text, tf=lambda t, f=a.tf: float(int(f(t))))
        w.kind = "i"
        return w

    # -- declarations ------------------------------------------------------
    def implicit(self, name, fr=None):
        if fr is not None and name[0] in fr.imp:
            return fr.imp[name[0]]
        return "i" if name[0] in "ijklmn" else "r"

    def kind_of(self, fr, name):
        return fr.types.get(name) or self.implicit(name, fr)

    def declare(self, fr: Frame, s: str, no: int) -> bool:
        m = re.match(r"(double precision|real\*8|integer|real|logical|character(?:\*\S+)?|"
                     r"dimension|external|intrinsic|save|implicit|data|common|parameter)\b\s*(.*)$", s)
        if not m:
            return False
        what, rest = m.group(1), m.group(2)
        if what in ("external", "intrinsic", "save") or what.startswith("character"):
            return True
        if what == "implicit":
            m2 = re.match(r"(double precision|real\*8|integer|real|logical|character)(?:\*\d+)?\s*"
                          r"\((.*)\)$", rest.strip())
            if rest.strip() == "none":
                return True
            if not m2:
                raise Unsupported(f"line {no}: implicit {rest}")
            k = {"double precision": "d", "real*8": "d", "integer": "i", "real": "r",
                 "logical": "l", "character": "s"}[m2.group(1)]
            for rng in m2.group(2).split(","):
                a, _, b = rng.strip().partition("-")
                for c in range(ord(a), ord(b or a) + 1):
                    fr.imp[chr(c)] = k
            return True
        if what == "parameter":
            body = rest.strip()[1:-1]
            for item in split_top(body):
                n, e = item.split("=", 1)
                n = n.strip()
                v = self.ev(fr, parse_expr(e))
                if not v.const:
                    raise Unsupported(f"line {no}: parameter {n} not constant")
                k = self.kind_of(fr, n)
                fr.params[n] = V(k, coerce(v.c, k)) if k != "l" else v
            return True
        if what == "data":
            self.data(fr, rest, no)
            return True
        if what == "common":
            m2 = re.match(r"/\s*(\w*)\s*/\s*(.*)", rest)
            if not m2:
                raise Unsupported(f"line {no}: common {rest}")
            blk = m2.group(1)
            for i, item in enumerate(split_top(m2.group(2))):
                n, dims = self.decl_item(fr, item)
                key = f"common/{blk}/{i}"
                if dims:
                    fr.dims[n] = dims
                    fr.scal[n] = Array(key, 0, dims, self.kind_of(fr, n))
                else:
                    fr.scal[n] = ("store", key, 0)
            return True
        kind = {"double precision": "d", "real*8": "d", "integer": "i", "real": "r",
                "logical": "l", "dimension": None}[what]
        for item in split_top(rest):
            n, dims = self.decl_item(fr, item)
            if kind:
                fr.types[n] = kind
            if dims:
                fr.dims[n] = dims
        return True

    def decl_item(self, fr, item):
        item = item.strip()
        m = re.match(r"(\w+)\s*(?:\((.*)\))?$", item)
        if not m:
            raise Unsupported(f"declaration {item}")
        dims = None
        if m.group(2):
            dims = []
            for d in split_top(m.group(2)):
                d = d.strip()
                if d == "*":
                    dims.append((1, None))
                    continue
                lo, _, hi = d.rpartition(":") if ":" in d else ("1", "", d)
                lo_v = self.ev(fr, parse_expr(lo))
                hi_v = None if hi.strip() == "*" else self.ev(fr, parse_expr(hi))
                if not lo_v.const or (hi_v is not None and not hi_v.const):
                    raise Unsupported(f"dimension {d} not constant")
                dims.append((int(lo_v.c), None if hi_v is None else int(hi_v.c)))
        return m.group(1), dims

    def data(self, fr, rest, no):
        for m in re.finditer(r"([^/]+)/([^/]*)/\s*,?", rest):
            names, vals = split_top(m.group(1)), []
            for v in split_top(m.group(2)):
                v = v.strip()
                rep = 1
                if "*" in v and re.match(r"\d+\*", v):
                    r, v = v.split("*", 1)
                    rep = int(r)
                x = self.ev(fr, parse_expr(v))
                vals += [x] * rep
            targets = []
            for n in names:
                n = n.strip()
                if "(" in n:
                    targets.append(("el", n))
                elif n in fr.dims:
                    a = self.arr(fr, n)
                    size = 1
                    for lo, hi in a.dims:
                        size *= hi - lo + 1
                    targets += [("flat", n, k) for k in range(size)]
                else:
                    targets.append(("sc", n))
            if len(targets) != len(vals):
                raise Unsupported(f"line {no}: data counts differ")
            for t, v in zip(targets, vals):
                if t[0] == "sc":
                    self.assign(fr, ("ref", t[1], None), v)
                elif t[0] == "flat":
                    a = self.arr(fr, t[1])
                    self.store[(a.key, a.off + t[2])] = self.conv(v, a.kind)
                else:
                    self.assign(fr, parse_expr(t[1]), v)

    # -- names ---------------------------------------------------------------
    def arr(self, fr: Frame, name) -> Array:
        a = fr.scal.get(name)
        if isinstance(a, Array):
            return a
        if name not in fr.dims:
            raise Unsupported(f"{name} is not an array")
        a = Array(self.newkey(name), 0, fr.dims[name], self.kind_of(fr, name))
        fr.scal[name] = a
        return a

    def slot(self, fr: Frame, name):
        s = fr.scal.get(name)
        if s is None:
            s = ("store", self.newkey(name), 0)
            fr.scal[name] = s
        return s

    def conv(self, v: V, kind: str) -> V:
        if v.kind == "s":
            return v
        if v.const:
            if kind == "l" or v.kind == "l":
                return v
            return V(kind, coerce(v.c, kind))
        if kind == "i":
            if v.tf is None or v.kind != "i":
                raise Unsupported("a symbolic value assigned to an integer")
        return v

    # -- evaluation ----------------------------------------------------------
    def ev(self, fr: Frame, e) -> V:
        op = e[0]
        if op == "num":
            return literal(e[1])
        if op == "bool":
            return V("l", e[1])
        if op == "str":
            return V("s", e[1])
        if op == "neg":
            a = self.ev(fr, e[1])
            return V(a.kind, -a.c) if a.const else self.sym(f"-({a.text()})")
        if op in ("+", "-", "*", "/", "**"):
            return self.arith(op, self.ev(fr, e[1]), self.ev(fr, e[2]))
        if op == "cmp":
            a, b = self.ev(fr, e[2]), self.ev(fr, e[3])
            if a.const and b.const:
                x, y = a.c, b.c
                return V("l", {"<": x < y, "<=": x <= y, ">": x > y, ">=": x >= y,
                               "==": x == y, "<>": x != y}[e[1]])
            rel = f"({a.text()}) {e[1]} ({b.text()})"
            # Fortran evaluates a relation as it stands: on the state, no
            # event (noEvent); on time alone, an event, as the test set's
            # drivers restart their solvers at those times (ndisc)
            timed = all(v.const or v.tf is not None for v in (a, b))
            return V("l", s=rel if timed else f"noEvent({rel})")
        if op in ("and", "or"):
            a, b = self.ev(fr, e[1]), self.ev(fr, e[2])
            if a.const and b.const:
                return V("l", (a.c and b.c) if op == "and" else (a.c or b.c))
            return V("l", s=f"({a.text()}) {op} ({b.text()})")
        if op == "not":
            a = self.ev(fr, e[1])
            return V("l", not a.c) if a.const else V("l", s=f"not ({a.text()})")
        if op == "ref":
            return self.ref(fr, e[1], e[2])
        raise Unsupported(f"expression {op}")

    def arith(self, op, a: V, b: V) -> V:
        if a.const and b.const:
            k = max(a.kind, b.kind, key=lambda x: RANK[x])
            x, y = coerce(a.c, k), coerce(b.c, k)
            if op == "**":
                if b.kind == "i":
                    k = a.kind
                    x = coerce(a.c, k)
                    r = x ** b.c if k == "i" and b.c >= 0 else float(x) ** b.c
                    # gfortran evaluates x**n by repeated products
                    if k != "i":
                        r = ipow(float(x), b.c, k)
                    return V(k, coerce(r, k) if k != "i" else int(r))
                return V(k, coerce(float(x) ** float(y), k))
            if op == "/" and k == "i":
                return V("i", fortran_int_div(x, y))
            r = {"+": x + y, "-": x - y, "*": x * y, "/": ieee_div(x, y) if k != "i" else None}[op]
            return V(k, coerce(r, k))
        if op == "**":
            if b.const and b.kind == "i":
                n = b.c
                if n == 1:
                    return a
                if n == 2:
                    return self.sym(f"({a.text()}) * ({a.text()})")
                return self.sym(f"({a.text()}) ^ {n}" if n >= 0 else f"({a.text()}) ^ ({n})")
            return self.sym(f"({a.text()}) ^ ({b.text()})")
        if op == "/" and a.kind == "i" and b.kind == "i":
            if a.tf and b.const and b.c > 0:
                return self.int_of_time(self.arith("/", V("d", s=a.s, tf=a.tf), V("d", float(b.c))),
                                        "integer division")
            raise Unsupported("integer division of a symbol")
        if a.kind == "i" and b.kind == "i" and op != "**":
            r = self.sym(f"({a.text()}) {op} ({b.text()})", tf=time_fn(op, a, b))
            r.kind = "i"
            return r
        # exact simplifications (x + 0 = x, x * 1 = x, x * 0 = 0 for finite x),
        # which keep sums over a sparse matrix from making a variable per zero
        if op in ("+", "-") and b.const and b.c == 0:
            return a
        if op == "+" and a.const and a.c == 0:
            return b
        if op == "*" and ((a.const and a.c == 0) or (b.const and b.c == 0)):
            return V("d", 0.0)
        if op in ("*", "/") and b.const and b.c == 1:
            return a
        if op == "*" and a.const and a.c == 1:
            return b
        o = {"+": "+", "-": "-", "*": "*", "/": "/"}[op]
        return self.sym(f"({a.text()}) {o} ({b.text()})", tf=time_fn(op, a, b))

    def ref(self, fr: Frame, name: str, args):
        if args is None:
            if name in fr.params:
                return fr.params[name]
            if name in fr.scal and isinstance(fr.scal[name], tuple):
                _, key, off = fr.scal[name]
                if (key, off) not in self.store:
                    raise Unsupported(f"{name} read before it is set")
                return self.store[(key, off)]
            if name in fr.scal and isinstance(fr.scal[name], Array):
                return fr.scal[name]  # an array passed on whole
            if name in fr.dims:
                return self.arr(fr, name)
            raise Unsupported(f"{name} read before it is set")
        # an array element, an intrinsic or a function
        if name in fr.dims or isinstance(fr.scal.get(name), Array):
            a = self.arr(fr, name)
            idx = [self.ev(fr, x) for x in args]
            if not all(i.const for i in idx):
                raise Unsupported(f"{name} indexed by a symbol")
            k = a.flat([int(i.c) for i in idx])
            if (a.key, k) not in self.store:
                self.unset.add(name)
                return V(a.kind, coerce(0, a.kind) if a.kind != "l" else False)
            return self.store[(a.key, k)]
        if name in self.units and self.units[name]["kind"] == "function":
            return self.call(name, [self.actual_ast(fr, x) for x in args], func=True)
        vals = [self.ev(fr, x) for x in args]
        return self.intrinsic(name, vals)

    def intrinsic(self, name, vals):
        if name in INTRINSIC_1 and len(vals) == 1:
            f = INTRINSIC_1[name]
            a = vals[0]
            if a.const:
                k = a.kind if a.kind != "i" or f == "abs" else "d"
                x = PYMATH[f](float(a.c) if f != "abs" else a.c)
                return V(k, coerce(x, k))
            tf = (lambda t, g=PYMATH[f], h=a.tf: g(h(t))) if a.tf else None
            return self.sym(f"{f}({a.text()})", tf=tf)
        if name in ("dble", "dfloat", "real", "float", "sngl"):
            k = "d" if name in ("dble", "dfloat") else "r"
            a = vals[0]
            return V(k, coerce(a.c, k)) if a.const else a
        if name in ("int", "idint", "ifix", "nint", "idnint"):
            a = vals[0]
            if not a.const and a.tf and name in ("int", "idint", "ifix"):
                return self.int_of_time(a, name)
            if not a.const:
                raise Unsupported(f"{name} of a symbol")
            if name in ("nint", "idnint"):
                return V("i", int(math.floor(abs(a.c) + 0.5)) * (1 if a.c >= 0 else -1))
            return V("i", int(a.c))
        if name in ("mod", "amod", "dmod"):
            a, b = vals
            if not a.const and a.tf and b.const and b.c > 0:
                q = self.int_of_time(self.arith("/", a, b), name)
                return self.arith("-", a, self.arith("*", q, b))
            if not (a.const and b.const):
                raise Unsupported("mod of a symbol")
            k = max(a.kind, b.kind, key=lambda x: RANK[x])
            return V(k, a.c - int(a.c / b.c) * b.c if k != "i" else a.c - fortran_int_div(a.c, b.c) * b.c)
        if name in ("max", "dmax1", "amax1", "max0", "min", "dmin1", "amin1", "min0"):
            big = name.startswith(("max", "dmax", "amax"))
            if all(v.const for v in vals):
                k = max((v.kind for v in vals), key=lambda x: RANK[x])
                return V(k, coerce((max if big else min)(v.c for v in vals), k))
            out = vals[0]
            for v in vals[1:]:
                out = self.sym(f"{'max' if big else 'min'}({out.text()}, {v.text()})")
            return out
        if name in ("sign", "dsign", "isign"):
            a, b = vals
            if a.const and b.const:
                k = max(a.kind, b.kind, key=lambda x: RANK[x])
                return V(k, coerce(abs(a.c) if b.c >= 0 else -abs(a.c), k))
            c = f"({b.text()}) >= 0" if b.tf is not None else f"noEvent(({b.text()}) >= 0)"
            return self.sym(f"if {c} then abs({a.text()}) else -abs({a.text()})")
        if name in ("atan2", "datan2"):
            a, b = vals
            if a.const and b.const:
                return V("d", math.atan2(a.c, b.c))
            return self.sym(f"atan2({a.text()}, {b.text()})")
        if name in self.units and self.units[name]["kind"] == "function":
            return self.call(name, vals, func=True)
        raise Unsupported(f"function {name}")

    # -- assignment ----------------------------------------------------------
    def assign(self, fr: Frame, target, v: V):
        _, name, args = target
        if args is None:
            if name in fr.params:
                raise Unsupported(f"assignment to parameter {name}")
            _, key, off = self.slot(fr, name)
            if self.kind_of(fr, name) == "r" and not v.const:
                self.single.add(name)
            self.store[(key, off)] = self.conv(v, self.kind_of(fr, name))
            return
        a = self.arr(fr, name)
        idx = [self.ev(fr, x) for x in args]
        if not all(i.const for i in idx):
            raise Unsupported(f"{name} indexed by a symbol")
        self.store[(a.key, a.flat([int(i.c) for i in idx]))] = self.conv(v, a.kind)

    # -- calls -------------------------------------------------------------
    def call(self, name, actuals, func=False, fr_caller=None, actual_exprs=None):
        u = self.units.get(name)
        if u is None:
            raise Unsupported(f"call to {name}, which the file does not hold")
        fr = Frame()
        body = u["body"]
        if len(actuals) != len(u["args"]):
            raise Unsupported(f"{name} called with {len(actuals)} of {len(u['args'])} arguments")
        bind(self, fr, u["args"], actuals, body)
        if func:
            fr.types.setdefault(name, u["type"] or self.implicit(name))
        self.run(fr, body, skip_decl=True)
        if func:
            return self.ref(fr, name, None)
        return None


    # -- statements ----------------------------------------------------------
    def run(self, fr: Frame, body, skip_decl=False):
        i = 0
        while i < len(body) and DECL.match(body[i][1]):
            i += 1
        try:
            self.block(fr, body[i:])
        except Return:
            pass

    def block(self, fr: Frame, body):
        i = 0
        while i < len(body):
            try:
                i = self.stmt(fr, body, i)
            except Goto as g:
                at = [k for k, st in enumerate(body) if st[0] == g.label]
                if not at:
                    raise
                i = at[0]

    def stmt(self, fr, body, i):
        label, s, no = body[i]
        try:
            return self._stmt(fr, body, i)
        except Unsupported as e:
            if str(e).startswith("line "):
                raise
            raise Unsupported(f"line {no}: {s}: {e}") from None

    def _stmt(self, fr, body, i):
        label, s, no = body[i]
        if s in ("continue", "return") or re.fullmatch(r"end\s*do|enddo", s):
            if s == "return":
                raise Return()
            return i + 1
        if re.match(r"(write|print|format|open|close|stop)\b", s):
            if s.startswith("stop"):
                raise Unsupported("stop")
            return i + 1
        m = re.match(r"go\s*to\s*(\d+)$", s)
        if m:
            raise Goto(m.group(1))
        if re.match(r"go\s*to\b", s):
            raise Unsupported("computed goto")
        m = re.match(r"do\s+(\d+\s*,?\s*)?(\w+)\s*=\s*(.*)$", s)
        if m:
            end_label = m.group(1).strip(" ,") if m.group(1) else None
            var = m.group(2)
            parts = [self.ev(fr, parse_expr(x)) for x in split_top(m.group(3))]
            if not all(p.const for p in parts):
                raise Unsupported("loop bounds depend on a symbol")
            a, b = int(parts[0].c), int(parts[1].c)
            step = int(parts[2].c) if len(parts) > 2 else 1
            # the loop's body
            j, depth = i + 1, 0
            while True:
                if j >= len(body):
                    raise Unsupported("unterminated do")
                lab, s2, _ = body[j]
                if end_label is not None:
                    if lab == end_label:
                        break
                else:
                    if re.match(r"do\s+\w+\s*=", s2):
                        depth += 1
                    elif re.fullmatch(r"end\s*do|enddo", s2):
                        if depth == 0:
                            break
                        depth -= 1
                j += 1
            inner = body[i + 1:j]
            last = body[j]
            if end_label is not None and not re.fullmatch(r"continue|end\s*do|enddo", last[1]):
                inner = inner + [(None, last[1], last[2])]
            k = a
            while (k <= b) if step > 0 else (k >= b):
                self.assign(fr, ("ref", var, None), V("i", k))
                self.block(fr, inner)
                k += step
            self.assign(fr, ("ref", var, None), V("i", k))
            # shared terminal labels: other loops end here too
            return j + 1
        if s.startswith("if") and re.match(r"if\s*\(", s):
            cond_text, rest = split_paren(s[s.index("("):])
            rest = rest.strip()
            if rest == "then":
                return self.if_block(fr, body, i, cond_text)
            # a logical if
            c = self.ev(fr, parse_expr(cond_text))
            if c.const:
                if c.c:
                    self._stmt(fr, [(None, rest, no)], 0)
                return i + 1
            return self.branches(fr, [(c, [(None, rest, no)])], [], no, i + 1)
        m = re.match(r"call\s+(\w+)\s*(?:\((.*)\))?$", s)
        if m:
            acts = [self.actual(fr, x) for x in split_top(m.group(2) or "") if x.strip()]
            self.call(m.group(1), acts)
            return i + 1
        m = re.match(r"([a-z_]\w*)\s*(\(.*?\))?\s*=(?!=)(.*)$", s)
        if m:
            lhs_text = m.group(1) + (m.group(2) or "")
            # an array element's index may itself hold parentheses
            lhs, rhs = split_assign(s)
            target = parse_expr(lhs)
            if target[0] != "ref":
                raise Unsupported("assignment target")
            self.assign(fr, target, self.ev(fr, parse_expr(rhs)))
            return i + 1
        raise Unsupported("statement")

    def actual(self, fr, text):
        return self.actual_ast(fr, parse_expr(text))

    def actual_ast(self, fr, e):
        """An actual argument as Fortran passes it: by reference where it
        names a variable, an array or an element."""
        if e[0] == "ref":
            name, args = e[1], e[2]
            if args is None and (name in fr.dims or isinstance(fr.scal.get(name), Array)):
                return self.arr(fr, name)
            if args is not None and (name in fr.dims or isinstance(fr.scal.get(name), Array)):
                a = self.arr(fr, name)
                idx = [self.ev(fr, x) for x in args]
                return ("elem", a.key, a.flat([int(x.c) for x in idx]), a.kind)
            if args is None and name not in fr.params:
                return self.slot(fr, name)
        return self.ev(fr, e)

    def if_block(self, fr, body, i, cond_text):
        arms, cur_cond, cur, depth = [], cond_text, [], 0
        j = i + 1
        else_body = None
        while True:
            if j >= len(body):
                raise Unsupported("unterminated if")
            lab, s2, no2 = body[j]
            if re.match(r"if\s*\(", s2) and s2.rstrip().endswith("then") and \
                    split_paren(s2[s2.index("("):])[1].strip() == "then":
                depth += 1
            elif re.fullmatch(r"end\s*if", s2):
                if depth == 0:
                    break
                depth -= 1
            elif depth == 0 and re.match(r"else\s*if\s*\(", s2):
                arms.append((cur_cond, cur))
                t = s2[s2.index("("):]
                cur_cond, cur = split_paren(t)[0], []
                j += 1
                continue
            elif depth == 0 and re.fullmatch(r"else", s2):
                arms.append((cur_cond, cur))
                cur_cond, cur = None, []
                j += 1
                continue
            cur.append(body[j])
            j += 1
        if cur_cond is None:
            else_body = cur
        else:
            arms.append((cur_cond, cur))
        # constant conditions decide at once
        evald = []
        for c, b in arms:
            cv = self.ev(fr, parse_expr(c))
            if cv.const:
                if cv.c:
                    if not evald:
                        self.block(fr, b)
                        return j + 1
                    else_body = b
                    break
                continue
            evald.append((cv, b))
        if not evald:
            if else_body:
                self.block(fr, else_body)
            return j + 1
        return self.branches(fr, evald, else_body or [], body[i][2], j + 1)

    def branches(self, fr, arms, else_body, no, nxt):
        """Branches on symbols: each arm run on a copy, then merged with if
        expressions. An arm that rejects the point (ierr set, return) is a
        guard of the solver's, not of the model: left out, and noted."""
        def guard(b):
            texts = [s for _, s, _ in b]
            return any(re.match(r"ierr\s*=\s*-?\s*[1-9]", t) or re.match(r"stop\b", t)
                       for t in texts)
        live = []
        for c, b in arms:
            if guard(b):
                self.notes.append(f"line {no}: the step rejection where {c.text()} left out "
                                  "(a solver guard, not part of the model)")
                continue
            live.append((c, b))
        if not live:
            if else_body:
                self.block(fr, else_body)
            return nxt
        before = self.store
        results = []
        for c, b in live + [(None, else_body)]:
            self.store = dict(before)
            try:
                self.block(fr, b)
            except Return:
                raise Unsupported("return inside a branch on a symbol")
            results.append((c, self.store))
        keys = set().union(*[set(r.keys()) for _, r in results])
        merged = dict(before)
        for k in keys:
            vals = [r.get(k) for _, r in results]
            if all(v is vals[0] or (v is not None and vals[0] is not None and v.c == vals[0].c
                                    and v.s == vals[0].s) for v in vals):
                merged[k] = vals[0]
                continue
            if any(v is None for v in vals):
                if k[0].startswith("ierr"):
                    continue
                if before.get(k) is not None:
                    vals = [v if v is not None else before[k] for v in vals]
                else:
                    # Fortran leaves it undefined there: the arms without it
                    # take the value of the nearest arm that sets it
                    filled = [v for v in vals if v is not None]
                    vals = [v if v is not None else filled[-1] for v in vals]
                    self.notes.append(f"line {no}: {k[0].split('#')[0]} undefined in some arms; "
                                      "those take the nearest arm's value")
            text = vals[-1].text()
            for (c, _), v in reversed(list(zip(results[:-1], vals[:-1]))):
                text = f"if {c.text()} then {v.text()} else {text}"
            merged[k] = self.new_w(text)
        self.store = {k: v for k, v in merged.items() if v is not None}
        return nxt


DECL = re.compile(r"(double precision|real\*8|integer|real|logical|character|dimension|external|"
                  r"intrinsic|save|implicit|data|common|parameter)\b")


def bind(m: "Machine", fr: Frame, dummies, actuals, body):
    """Binds a unit's dummies to the caller's actuals: scalars first (so
    that declarations can size arrays by them), then the declarations, then
    each array dummy as a view of the caller's storage."""
    for dummy, act in zip(dummies, actuals):
        if isinstance(act, V):
            key = m.newkey(dummy)
            fr.scal[dummy] = ("store", key, 0)
            m.store[(key, 0)] = act
        elif isinstance(act, tuple) and act[0] == "store":
            fr.scal[dummy] = act
        elif isinstance(act, tuple) and act[0] == "elem":
            fr.scal[dummy] = ("store", act[1], act[2])
    for label, s, no in body:
        if not DECL.match(s):
            break
        m.declare(fr, s, no)
    for dummy, act in zip(dummies, actuals):
        if isinstance(act, Array):
            fr.scal[dummy] = Array(act.key, act.off, fr.dims.get(dummy) or [(1, None)], act.kind)
        elif isinstance(act, tuple) and act[0] == "elem" and dummy in fr.dims:
            fr.scal[dummy] = Array(act[1], act[2], fr.dims[dummy], act[3])


def ipow(x: float, n: int, k: str) -> float:
    """x**n for an integer n as gfortran computes it (binary powering)."""
    if n < 0:
        return 1.0 / ipow(x, -n, k)
    r, base = 1.0, x
    first = True
    while n:
        if n & 1:
            r = base if first else (r * base if k == "d" else f32(r * base))
            first = False
        n >>= 1
        if n:
            base = base * base if k == "d" else f32(base * base)
    return r


def split_top(s: str) -> list[str]:
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur)
    return out


def split_paren(s: str) -> tuple[str, str]:
    depth = 0
    for i, ch in enumerate(s):
        depth += ch == "("
        depth -= ch == ")"
        if depth == 0:
            return s[1:i], s[i + 1:]
    raise Unsupported("unbalanced parentheses")


def split_assign(s: str) -> tuple[str, str]:
    depth = 0
    for i, ch in enumerate(s):
        depth += ch == "("
        depth -= ch == ")"
        if ch == "=" and depth == 0 and s[i + 1:i + 2] != "=" and s[i - 1:i] not in "<>/=":
            return s[:i], s[i + 1:]
    raise Unsupported("no assignment")


# ---------------------------------------------------------------------------
# A problem's feval, run on symbols
# ---------------------------------------------------------------------------

def run_setup(m: "Machine", us: dict, neqn: int) -> None:
    """Runs the problem's `prob` and `init` on numbers first: some problems set
    their constants there, in COMMON blocks feval reads."""
    for unit, acts in (("prob", None), ("init", None)):
        u = us.get(unit)
        if u is None:
            continue
        acts = []
        for k, a in enumerate(u["args"]):
            acts.append(Array(m.newkey(a), 0, [(0, None)] if a == "t" and unit == "prob"
                              else [(1, None)], "d"))
        # scalars: neqn and the like are set by prob itself; give init its neqn
        if unit == "init":
            acts[0] = V("i", neqn)
            acts[1] = V("d", 0.0)
            acts[-1] = V("l", False)
        else:
            acts = [V("s", "") if a in ("fullnm", "problm", "type") else
                    ("store", m.newkey(a), 0) if a not in ("t", "ind") else acts[k]
                    for k, a in enumerate(u["args"])]
        try:
            m.call(unit, acts)
        except (Unsupported, Return) as e:
            m.notes.append(f"{unit} not run through ({e}); its COMMON values up to there kept")


def feval_symbolic(src: str, neqn: int, kind: str, edit_params: dict | None = None,
                   trange=(0.0, 1.0)):
    """The residual rows of `feval` as model text: (rows, equations, notes,
    the y' read). kind: ODE, DAE or IDE."""
    us = units(src)
    m = Machine(us, neqn, trange)
    run_setup(m, us, neqn)
    m.notes = [n for n in m.notes if "not run through" not in n]
    fe = us["feval"]
    fr = Frame()
    body = fe["body"]
    # the arguments: neqn, t, y, yprime, f, ierr, rpar, ipar
    args = fe["args"]
    acts = [V("i", neqn), V("d", s="(time / s1)", tf=lambda t: t), Array("y", 0, [(1, neqn)], "d"),
             Array("yprime", 0, [(1, neqn)], "d"), Array("f", 0, [(1, neqn)], "d"), V("i", 0)]
    acts += [Array(f"unused{k}", 0, [(1, None)], "d") for k in range(len(args) - 6)]
    bind(m, fr, args, acts, body)
    if edit_params:
        for k, v in edit_params.items():
            if k not in fr.params:
                raise Unsupported(f"edit: no parameter {k}")
            fr.params[k] = V(fr.params[k].kind, coerce(v, fr.params[k].kind))
    for k in range(neqn):
        m.store[("y", k)] = V("d", s=f"y{k + 1}")
        m.store[("yprime", k)] = V("d", s=f"(s1 * der(y{k + 1}))")
    m.run(fr, body, skip_decl=True)
    if m.single:
        m.notes.append("SINGLE PRECISION: Fortran rounds these to REAL*4, the model keeps double: "
                       + ", ".join(sorted(m.single)))
    if m.unset:
        m.notes.append("array elements read before any assignment, taken as 0 (static storage): "
                       + ", ".join(sorted(m.unset)))
    rows = []
    for k in range(neqn):
        v = m.store.get(("f", k))
        if v is None:
            raise Unsupported(f"f({k + 1}) never set")
        rows.append(v.text())
    reads = set()
    text = "\n".join(m.eqs + rows)
    for k in range(neqn):
        if re.search(rf"der\(y{k + 1}\)", text):
            reads.add(k)
    return rows, m.eqs, m.notes, reads


def model_text(name: str, title: str, neqn: int, kind: str, rows: list[str], eqs: list[str],
               mass: list[list[float]] | None, y0: list[float], fixed: list[bool],
               drop_rows=(), extra=()) -> tuple[str, list[int]]:
    """The model; and for each problem row its equation's index in the
    equation section (-1 where left out)."""
    nw = len(eqs)
    lines = [f'model {name} "{title}"',
             '  parameter Real s1(unit = "s") = 1 "one second: the time unit of the problem, '
             'whose variables are dimensionless";']
    for k in range(neqn):
        fx = "true" if fixed[k] else "false"
        lines.append(f"  Real y{k + 1}(start = {repr(float(y0[k]))}, fixed = {fx});")
    for k in range(1, nw + 1):
        lines.append(f"  Real w{k};")
    lines.append("equation")
    index, n = [], 0
    for e in eqs:
        lines.append(f"  {e};")
        n += 1
    for i in range(neqn):
        if i in drop_rows:
            index.append(-1)
            continue
        if kind == "IDE":
            lhs = "0"
        elif kind == "ODE":
            lhs = f"s1 * der(y{i + 1})"
        else:
            terms = []
            for j in range(neqn):
                c = mass[i][j]
                if c == 0:
                    continue
                terms.append(f"s1 * der(y{j + 1})" if c == 1 else
                             f"({repr(c)}) * (s1 * der(y{j + 1}))")
            lhs = " + ".join(terms) if terms else "0"
        lines.append(f"  {lhs} = {rows[i]};")
        index.append(n)
        n += 1
    for e in extra:
        lines.append(f"  {e};")
    lines.append(f"end {name};")
    return "\n".join(lines) + "\n", index
