#!/usr/bin/env python3
"""Check that every third-party crate the engine builds with is under a
permissive licence, from `cargo metadata` alone (offline, no extra tools).

A crate passes when its SPDX licence expression can be satisfied using only
the licences in ALLOWED: for `A OR B` one side must pass, for `A AND B`
both, and `X WITH exception` passes when `X WITH exception` or `X` is
allowed. A crate with no licence expression fails (unless it is one of the
engine's own crates). GPL, LGPL, AGPL, EUPL and SSPL never pass.

Usage: licences.py [--table] [manifest ...]   (default: engine/Cargo.toml)
Exit status 1 when any crate fails.
"""
from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ALLOWED = {
    "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause",
    "BSD-3-Clause", "ISC", "Zlib", "0BSD", "BSL-1.0", "CC0-1.0", "Unicode-3.0",
    "Unicode-DFS-2016",
}
NEVER = re.compile(r"\b(A?GPL|LGPL|EUPL|SSPL)", re.I)


def tokens(expr: str) -> list[str]:
    return re.findall(r"\(|\)|[A-Za-z0-9.\-+]+", expr.replace("/", " OR "))


def parse(toks: list[str], i: int = 0):
    """expr := term (OR term)* ; term := factor (AND factor)* ;
    factor := '(' expr ')' | id [WITH id]. Returns (ok, next index)."""
    def factor(i):
        if toks[i] == "(":
            ok, i = expr(i + 1)
            return ok, i + 1  # ')'
        lic = toks[i]
        i += 1
        if i < len(toks) and toks[i] == "WITH":
            full = f"{lic} WITH {toks[i + 1]}"
            return (full in ALLOWED or lic in ALLOWED) and not NEVER.search(lic), i + 2
        return lic in ALLOWED and not NEVER.search(lic), i

    def term(i):
        ok, i = factor(i)
        while i < len(toks) and toks[i] == "AND":
            ok2, i = factor(i + 1)
            ok = ok and ok2
        return ok, i

    def expr(i):
        ok, i = term(i)
        while i < len(toks) and toks[i] == "OR":
            ok2, i = term(i + 1)
            ok = ok or ok2
        return ok, i

    return expr(i)


def check(manifest: Path) -> list[tuple[str, str, str, bool]]:
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--manifest-path", str(manifest)],
        check=True, capture_output=True, text=True).stdout)
    members = set(meta["workspace_members"])
    # only packages that are actually in the build graph (resolve), not
    # every optional dependency listed in the lockfile's universe
    used = {n["id"] for n in meta["resolve"]["nodes"]}
    rows = []
    for p in meta["packages"]:
        if p["id"] in members or p["id"] not in used:
            continue
        lic = p.get("license") or ""
        ok = bool(lic) and parse(tokens(lic))[0]
        rows.append((p["name"], p["version"], lic or "(none)", ok))
    return sorted(rows)


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    table = "--table" in sys.argv
    manifests = [Path(a) for a in args] or [Path(__file__).resolve().parent.parent / "Cargo.toml"]
    bad = 0
    for m in manifests:
        rows = check(m)
        print(f"{m}: {len(rows)} third-party crates")
        for name, ver, lic, ok in rows:
            if table or not ok:
                print(f"  {'ok ' if ok else 'BAD'} {name} {ver}: {lic}")
            bad += not ok
    if bad:
        print(f"{bad} crate(s) without an allowed licence")
        return 1
    print("every crate is under an allowed licence")
    return 0


if __name__ == "__main__":
    sys.exit(main())
