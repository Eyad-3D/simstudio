"""A transcribed model's equations evaluated in high precision (mpmath), the
tie-breaker where the engine and the gfortran-built original differ by more
than rounding should allow: whichever of the two the 50-digit value is
nearer to is right.

    python3 benchmarks/hard/mpeval.py WORK CASE_ID POINT ROW
"""
import json
import re
import sys
from pathlib import Path

import mpmath as mp

mp.mp.dps = 50
TOK = re.compile(r"\s*(?:(\d+\.\d*(?:e[-+]?\d+)?|\d+e[-+]?\d+|\d+)|(der\(\w+\))|([A-Za-z_]\w*)|"
                 r"(<=|>=|<>|==|[-+*/^(),<>=]))")


class P:
    def __init__(self, text, env):
        self.t = [m for m in TOK.findall(text)]
        self.i, self.env = 0, env

    def peek(self):
        if self.i >= len(self.t):
            return None
        n, d, w, o = self.t[self.i]
        return n or d or w or o

    def take(self):
        v = self.peek()
        self.i += 1
        return v

    def expr(self):
        if self.peek() == "if":
            self.take()
            c = self.expr()
            assert self.take() == "then"
            a = self.expr()
            assert self.take() == "else"
            b = self.expr()
            return a if c else b
        return self.or_()

    def or_(self):
        a = self.and_()
        while self.peek() == "or":
            self.take()
            b = self.and_()
            a = bool(a) or bool(b)
        return a

    def and_(self):
        a = self.not_()
        while self.peek() == "and":
            self.take()
            b = self.not_()
            a = bool(a) and bool(b)
        return a

    def not_(self):
        if self.peek() == "not":
            self.take()
            return not self.not_()
        return self.rel()

    def rel(self):
        a = self.add()
        op = self.peek()
        if op in ("<", "<=", ">", ">=", "==", "<>"):
            self.take()
            b = self.add()
            return {"<": a < b, "<=": a <= b, ">": a > b, ">=": a >= b, "==": a == b, "<>": a != b}[op]
        return a

    def add(self):
        if self.peek() == "-":
            self.take()
            a = -self.mul()
        else:
            a = self.mul()
        while self.peek() in ("+", "-"):
            op = self.take()
            b = self.mul()
            a = a + b if op == "+" else a - b
        return a

    def mul(self):
        a = self.pow_()
        while self.peek() in ("*", "/"):
            op = self.take()
            b = self.pow_()
            a = a * b if op == "*" else a / b
        return a

    def pow_(self):
        a = self.atom()
        if self.peek() == "^":
            self.take()
            b = self.atom()
            return a ** b
        return a

    def atom(self):
        v = self.take()
        if v == "(":
            e = self.expr()
            assert self.take() == ")"
            return e
        if v == "-":
            return -self.atom()
        if re.fullmatch(r"\d.*", v):
            return mp.mpf(v)
        if v.startswith("der("):
            return self.env["der"][v[4:-1]]
        if self.peek() == "(":
            self.take()
            args = [self.expr()]
            while self.peek() == ",":
                self.take()
                args.append(self.expr())
            assert self.take() == ")"
            f = {"sin": mp.sin, "cos": mp.cos, "tan": mp.tan, "exp": mp.exp, "log": mp.log,
                 "log10": mp.log10, "sqrt": mp.sqrt, "abs": abs, "atan": mp.atan, "asin": mp.asin,
                 "acos": mp.acos, "sinh": mp.sinh, "cosh": mp.cosh, "tanh": mp.tanh,
                 "atan2": mp.atan2, "max": max, "min": min, "noEvent": lambda x: x}[v]
            return f(*args)
        return self.env["var"][v]


def main():
    work, cid, point, row = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
    case = json.loads((work / "cases" / f"{cid}.json").read_text())
    d = json.loads((work / "probe" / f"{case['problem']}.json").read_text())
    p = d["points"][point]
    text = (work / "cases" / case["text"]).read_text()
    env = {"var": {f"y{i + 1}": mp.mpf(v) for i, v in enumerate(p["y"])},
           "der": {f"y{i + 1}": mp.mpf(v) for i, v in enumerate(p["yp"])}}
    env["var"]["time"], env["var"]["s1"] = mp.mpf(p["t"]), mp.mpf(1)
    eqs = text.split("\nequation\n", 1)[1].split("\nend ", 1)[0].strip().split("\n")
    rhs_of_row = None
    for k, line in enumerate(eqs):
        lhs, rhs = line.strip().rstrip(";").split(" = ", 1)
        if re.fullmatch(r"w\d+", lhs.strip()):
            env["var"][lhs.strip()] = P(rhs, env).expr()
        if k == case["rows"][row - 1]:
            rhs_of_row = P(rhs, env).expr()
    ev = json.loads((work / "check" / f"{cid}.eval.json").read_text())[point]["rows"][case["rows"][row - 1]]
    f = p["f"][row - 1]
    print(f"row {row} at point {point}: 50 digits {mp.nstr(rhs_of_row, 20)}")
    print(f"  engine   {ev[1]!r}  off {mp.nstr(abs(ev[1] - rhs_of_row), 3)}")
    print(f"  gfortran {f!r}  off {mp.nstr(abs(f - rhs_of_row), 3)}")


if __name__ == "__main__":
    main()
