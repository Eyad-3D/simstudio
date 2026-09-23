#!/usr/bin/env python3
"""Write THIRD-PARTY-NOTICES.txt and enforce SimStudio's licence policy.

Everything inside the desktop app that someone else wrote is listed with its
licence and licence text:

- the UI's npm packages: the production dependencies of frontend/ plus the
  build tools whose own code lands in the bundle (UI_BUILD_OUTPUT), read with
  license-checker-rseidelsohn;
- Electron, from desktop/. The notices for Chromium, Node.js and the other
  parts built into Electron ship as LICENSES.chromium.html next to the
  SimStudio executable (electron-builder copies it from Electron);
- everything PyInstaller froze into the engine, taken from the build's own
  table of contents: Python packages (licences read with pip-licenses), the
  Python runtime, the PyInstaller bootloader and native libraries
  (scripts/licenses/bundled-runtime.json).

The same list goes into sbom.cdx.json, a CycloneDX software bill of
materials that the installers also include. The run fails if any of these is
under a licence that is not on scripts/licenses/allowed.txt, or under a
licence it cannot determine.

Run it with the Python that froze the engine, after freezing it and after
`npm ci` in frontend/ and desktop/ (pip-licenses comes with
backend/requirements-build.txt):

    python scripts/third-party-notices.py          # check, then write both files
    python scripts/third-party-notices.py --check  # check only (CI)
"""
from __future__ import annotations

import argparse
import ast
import fnmatch
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import sysconfig
from dataclasses import dataclass, field
from importlib import metadata
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
POLICY = ROOT / "scripts" / "licenses"
NOTICES = ROOT / "THIRD-PARTY-NOTICES.txt"
SBOM = ROOT / "sbom.cdx.json"
WORK = ROOT / "backend" / "build" / "simstudio-backend"

LICENSE_CHECKER = "license-checker-rseidelsohn@4.4.2"
# Dev dependencies whose own code still ends up in the UI bundle: Tailwind's
# base styles, Vite's module-preload polyfill and Rolldown's runtime helpers.
UI_BUILD_OUTPUT = ["tailwindcss", "vite", "rolldown"]
# SimStudio's own files inside the frozen engine.
OWN_FILES = [ROOT / "backend" / "app", ROOT / "backend" / "projects",
             ROOT / "backend" / "run_backend.py", ROOT / "VERSION"]
# PyInstaller table-of-contents entries that are files in the bundle.
FILE_KINDS = {"PYMODULE", "PYSOURCE", "EXTENSION", "BINARY", "DATA", "EXECUTABLE"}

# Refused even if someone lists them in allowed.txt: copyleft that would
# reach SimStudio's own code. A "X WITH exception" term is judged as a whole.
REFUSED = re.compile(r"(A?GPL|SSPL|EUPL)-", re.IGNORECASE)
LICENSE_FILE = re.compile(r"(LICEN[CS]E|COPYING|NOTICE)", re.IGNORECASE)
UNKNOWN = {"", "UNKNOWN", "UNLICENSED"}

# Licence classifiers that older Python packages declare instead of an SPDX
# expression. "BSD License" is missing on purpose: it does not say which one.
CLASSIFIERS = {
    "MIT License": "MIT",
    "Apache Software License": "Apache-2.0",
    "ISC License (ISCL)": "ISC",
    "Python Software Foundation License": "PSF-2.0",
    "The Unlicense (Unlicense)": "Unlicense",
    "zlib/libpng License": "Zlib",
    "Mozilla Public License 2.0 (MPL 2.0)": "MPL-2.0",
    "GNU General Public License v2 (GPLv2)": "GPL-2.0-only",
    "GNU General Public License v2 or later (GPLv2+)": "GPL-2.0-or-later",
    "GNU General Public License v3 (GPLv3)": "GPL-3.0-only",
    "GNU General Public License v3 or later (GPLv3+)": "GPL-3.0-or-later",
    "GNU Lesser General Public License v2 or later (LGPLv2+)": "LGPL-2.0-or-later",
    "GNU Lesser General Public License v3 (LGPLv3)": "LGPL-3.0-only",
    "GNU Affero General Public License v3": "AGPL-3.0-only",
}

UI, SHELL, ENGINE = "User interface", "Desktop shell", "Simulation engine"


@dataclass
class Component:
    part: str
    name: str
    version: str
    license: str
    url: str = ""
    # Package URL (https://github.com/package-url/purl-spec) for the SBOM.
    purl: str = ""
    # Licence and notice files the component ships, as (file name, text).
    texts: list[tuple[str, str]] = field(default_factory=list)
    note: str = ""
    # The package this one is vendored or statically linked into.
    within: str = ""
    # A native library: its copyright file, where there is one, names the
    # authors but refers to the standard licence text instead of holding it.
    native: bool = False
    # The licence terms SimStudio uses it under, set by check().
    terms: list[str] = field(default_factory=list)

    @property
    def label(self) -> str:
        return f"{self.name} {self.version}".strip()

    @property
    def needs_standard_text(self) -> bool:
        return self.native or not self.texts


# --- SPDX expressions --------------------------------------------------------

TOKEN = re.compile(r"\s*(\(|\)|[A-Za-z0-9][A-Za-z0-9.+:-]*)")


def parse_expression(text: str):
    """An SPDX licence expression as nested ("or"/"and", [...]) and
    ("id", term) tuples, where a term may be "X WITH exception"; None if the
    text is not a valid expression."""
    tokens, pos = [], 0
    text = text.strip()
    while pos < len(text):
        match = TOKEN.match(text, pos)
        if not match:
            return None
        tokens.append(match.group(1))
        pos = match.end()
    at = 0

    def peek() -> str | None:
        return tokens[at].upper() if at < len(tokens) else None

    def take() -> str:
        nonlocal at
        at += 1
        return tokens[at - 1]

    def either():
        items = [both()]
        while peek() == "OR":
            take()
            items.append(both())
        return items[0] if len(items) == 1 else ("or", items)

    def both():
        items = [term()]
        while peek() == "AND":
            take()
            items.append(term())
        return items[0] if len(items) == 1 else ("and", items)

    def term():
        if peek() in (None, "AND", "OR", "WITH", ")"):
            raise ValueError
        token = take()
        if token == "(":
            node = either()
            if peek() != ")":
                raise ValueError
            take()
            return node
        if peek() == "WITH":
            take()
            if peek() in (None, "AND", "OR", "WITH", "(", ")"):
                raise ValueError
            return ("id", f"{token} WITH {take()}")
        return ("id", token)

    try:
        node = either()
    except ValueError:
        return None
    return node if at == len(tokens) else None


def choose(node, allowed: dict[str, str]) -> list[str] | None:
    """The terms of the first OR choice made only of allowed terms, spelled
    as in allowed.txt."""
    kind, value = node
    if kind == "id":
        refused = REFUSED.match(value) and " WITH " not in value
        return [allowed[value.lower()]] if value.lower() in allowed and not refused else None
    if kind == "and":
        terms = []
        for child in value:
            chosen = choose(child, allowed)
            if chosen is None:
                return None
            terms += chosen
        return terms
    for child in value:
        chosen = choose(child, allowed)
        if chosen is not None:
            return chosen
    return None


def standard_text_files(term: str) -> list[Path]:
    """scripts/licenses/texts/ files holding a term's standard licence text."""
    return [POLICY / "texts" / f"{part.strip()}.txt" for part in term.split(" WITH ")]


# --- the policy ---------------------------------------------------------------

def load_policy() -> tuple[dict[str, str], dict, dict, list[str]]:
    allowed, problems = {}, []
    for line in (POLICY / "allowed.txt").read_text(encoding="utf-8").splitlines():
        term = line.split("#", 1)[0].strip()
        if not term:
            continue
        node = parse_expression(term)
        if node is None or node[0] != "id":
            problems.append(f"allowed.txt: '{term}' is not a single SPDX term")
            continue
        if REFUSED.match(term) and " WITH " not in node[1]:
            problems.append(f"allowed.txt: '{term}' is refused whatever this file says")
            continue
        missing = [f.name for f in standard_text_files(term) if not f.is_file()]
        if missing:
            problems.append(f"allowed.txt: '{term}' has no standard text "
                            f"(add scripts/licenses/texts/{', '.join(missing)})")
        allowed[node[1].lower()] = node[1]
    clarified = json.loads((POLICY / "clarifications.json").read_text(encoding="utf-8"))
    runtime = json.loads((POLICY / "bundled-runtime.json").read_text(encoding="utf-8"))
    return allowed, clarified, runtime, problems


def check(components: list[Component], allowed: dict[str, str], clarified: dict) -> list[str]:
    problems = []
    for c in components:
        rule = clarified.get(c.name)
        if rule:
            if rule["expect"] not in "\n".join(text for _, text in c.texts):
                problems.append(f"{c.label}: its licence file no longer contains "
                                f"\"{rule['expect']}\"; review its entry in clarifications.json")
                continue
            c.license = rule["license"]
        node = None if c.license.upper() in UNKNOWN else parse_expression(c.license)
        if node is None:
            problems.append(f"{c.label} ({c.part.lower()}): licence unknown "
                            f"({c.license or 'none declared'})")
            continue
        chosen = choose(node, allowed)
        if chosen is None:
            problems.append(f"{c.label} ({c.part.lower()}): {c.license} is not allowed "
                            "(scripts/licenses/allowed.txt)")
            continue
        c.terms = chosen
    return problems


# --- npm ----------------------------------------------------------------------

def license_checker(folder: Path, *args: str) -> dict:
    if not (folder / "node_modules").is_dir():
        sys.exit(f"{folder.name}/node_modules is missing: run `npm ci` in {folder.name}/ first")
    npx = shutil.which("npx")
    if npx is None:
        sys.exit("npx not found: install Node.js 22 or later")
    run = subprocess.run([npx, "--yes", LICENSE_CHECKER, *args, "--json"], cwd=folder,
                         capture_output=True, text=True, encoding="utf-8")
    if run.returncode != 0:
        sys.exit(f"license-checker failed in {folder.name}/:\n{run.stderr}")
    return json.loads(run.stdout)


def read(path: str | Path) -> str:
    return Path(path).read_text(encoding="utf-8", errors="replace").strip()


def npm_components() -> list[Component]:
    ui = ROOT / "frontend"
    found = [(UI, license_checker(ui, "--production", "--excludePrivatePackages",
                                  "--excludePackagesStartingWith", "@types/")),
             (UI, license_checker(ui, "--includePackages", ";".join(UI_BUILD_OUTPUT))),
             (SHELL, license_checker(ROOT / "desktop", "--includePackages", "electron"))]
    components = []
    for part, packages in found:
        for key, info in sorted(packages.items()):
            name, _, version = key.rpartition("@")
            declared = info.get("licenses", "")
            if isinstance(declared, list):  # old "licenses" arrays list alternatives
                declared = " OR ".join(declared)
            texts = []
            for kind in ("licenseFile", "noticeFile"):
                path = info.get(kind)
                # Without a licence file license-checker falls back to the README.
                if path and LICENSE_FILE.match(Path(path).name):
                    text = read(path)
                    if name in UI_BUILD_OUTPUT:
                        # Vite appends the licences of what it bundles into
                        # itself; none of that reaches the UI.
                        text = text.split("\n# Licenses of bundled dependencies", 1)[0].strip()
                    texts.append((Path(path).name, text))
            note = ""
            if name in UI_BUILD_OUTPUT:
                note = "Build tool; the part of its code that is generated into the UI is covered."
            if name == "electron":
                note = ("The notices for Chromium, Node.js and the other components built into "
                        "Electron are in LICENSES.chromium.html, next to the SimStudio executable.")
            purl = f"pkg:npm/{name.replace('@', '%40', 1)}@{version}"
            components.append(Component(part, name, version, declared, purl=purl,
                                        url=info.get("repository", ""), texts=texts, note=note))
    return sorted(components, key=lambda c: (c.part != UI, c.name.lower(), c.version))


# --- the frozen engine --------------------------------------------------------

def norm(path: str | Path) -> str:
    return os.path.normcase(os.path.abspath(path))


def is_under(path: str | Path, roots: list[Path]) -> bool:
    path = norm(path)
    return any(path == norm(r) or path.startswith(norm(r) + os.sep) for r in roots)


def canonical(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def frozen_files(work: Path) -> list[tuple[str, str, str]]:
    """(name in the bundle, source file, kind) for everything PyInstaller put
    into the engine, read from the build's table-of-contents files."""
    if not (work / "COLLECT-00.toc").is_file():
        sys.exit(f"No PyInstaller build in {work}: freeze the engine first "
                 "(python -m PyInstaller --noconfirm --distpath dist --workpath build "
                 "simstudio-backend.spec, in backend/)")
    found: dict[tuple[str, str], str] = {}

    def walk(node) -> None:
        if not isinstance(node, (list, tuple)):
            return
        if (len(node) == 3 and all(isinstance(x, str) for x in node)
                and node[2] in FILE_KINDS and os.path.isabs(node[1])):
            found[(node[0], node[1])] = node[2]
            return
        for child in node:
            walk(child)

    for toc in sorted(work.glob("*.toc")):
        walk(ast.literal_eval(toc.read_text(encoding="utf-8")))
    return [(name, src, kind) for (name, src), kind in sorted(found.items())]


class Owners:
    """Which installed Python distribution a file belongs to. Copies vendored
    inside another package (setuptools/_vendor/…) count as their own
    projects, since they keep their own licences."""

    def __init__(self) -> None:
        self.files: dict[str, tuple] = {}
        self.dists: dict[tuple, metadata.Distribution] = {}
        installed = list(metadata.distributions())
        for dist in installed:
            self.add(dist, "")
        for dist in installed:
            for f in dist.files or []:
                if f.name == "METADATA" and "_vendor" in f.parts and f.parent.name.endswith(".dist-info"):
                    vendored = metadata.PathDistribution(Path(dist.locate_file(f)).parent)
                    self.add(vendored, dist.metadata["Name"])

    def add(self, dist: metadata.Distribution, within: str) -> None:
        key = (dist.metadata["Name"], dist.version, within)
        self.dists[key] = dist
        for f in dist.files or []:
            path = dist.locate_file(f)
            self.files[norm(path)] = key
            self.files[norm(os.path.realpath(path))] = key

    def owner(self, path: str) -> tuple | None:
        return self.files.get(norm(path)) or self.files.get(norm(os.path.realpath(path)))


def python_dirs() -> list[Path]:
    paths = sysconfig.get_paths()
    return [Path(paths["stdlib"]), Path(paths["platstdlib"]), Path(sys.base_prefix, "DLLs")]


def dist_texts(dist: metadata.Distribution) -> list[tuple[str, str]]:
    """The licence and notice files in a distribution's .dist-info folder."""
    texts = []
    for f in sorted(dist.files or [], key=str):
        # parts[0], not any part: setuptools also lists its vendored packages'
        # .dist-info folders, which get entries of their own.
        in_dist_info = len(f.parts) > 1 and f.parts[0].endswith(".dist-info")
        if in_dist_info and LICENSE_FILE.match(f.name) and Path(dist.locate_file(f)).is_file():
            texts.append((f.name, read(dist.locate_file(f))))
    return texts


def embedded_sbom(dist: metadata.Distribution, part: str) -> list[Component]:
    """Components a package declares as statically linked into it, from the
    CycloneDX files in its .dist-info/sboms/ (PEP 770; for example the Rust
    crates inside pydantic-core)."""
    components = []
    for f in dist.files or []:
        if f.parent.name != "sboms" or not f.name.endswith(".json"):
            continue
        sbom = json.loads(read(dist.locate_file(f)))
        for item in sbom.get("components", []):
            declared = []
            for entry in item.get("licenses", []):
                declared.append(entry.get("expression")
                                or entry.get("license", {}).get("id")
                                or entry.get("license", {}).get("name", ""))
            license_ = " AND ".join(f"({d})" if len(declared) > 1 else d for d in declared)
            components.append(Component(part, item.get("name", "?"), item.get("version", ""),
                                        license_, purl=item.get("purl", ""),
                                        within=dist.metadata["Name"]))
    return components


def identify(expression: str, classifiers: str, declared: str) -> str:
    """A Python package's licence as an SPDX expression: its
    License-Expression, else its licence classifiers (several mean a choice),
    else its License field if that is an expression; "" if none of these."""
    if expression and expression != "UNKNOWN":
        return expression
    if classifiers and classifiers != "UNKNOWN":
        spdx = [CLASSIFIERS.get(c.strip()) for c in classifiers.split(";")]
        if all(spdx):
            return " OR ".join(spdx)
    if declared and declared != "UNKNOWN" and parse_expression(declared):
        return declared
    return ""


def pip_licenses(names: list[str]) -> dict[str, dict]:
    run = subprocess.run([sys.executable, "-m", "piplicenses", "--from=all", "--format=json",
                          "--with-urls", "--with-system", "--packages", *names],
                         capture_output=True, text=True, encoding="utf-8")
    if run.returncode != 0:
        sys.exit("pip-licenses failed (install backend/requirements-build.txt into this "
                 f"Python):\n{run.stderr}")
    return {canonical(p["Name"]): p for p in json.loads(run.stdout)}


def system_copyright(src: str) -> tuple[str, str] | None:
    """On Debian and Ubuntu, the copyright file of the package that installed
    a native library."""
    if not shutil.which("dpkg"):
        return None
    for path in (src, os.path.realpath(src)):
        run = subprocess.run(["dpkg", "-S", path], capture_output=True, text=True)
        if run.returncode == 0:
            package = run.stdout.split(":", 1)[0].strip()
            copyright_ = Path("/usr/share/doc", package, "copyright")
            if copyright_.is_file():
                return (f"{copyright_} (from the {package} package)", read(copyright_))
    return None


def python_license() -> list[tuple[str, str]]:
    for folder in (Path(sysconfig.get_paths()["stdlib"]), Path(sys.base_prefix)):
        if (folder / "LICENSE.txt").is_file():
            return [("LICENSE.txt", read(folder / "LICENSE.txt"))]
    return []


def engine_components(work: Path, runtime: dict, problems: list[str]) -> list[Component]:
    owners = Owners()
    stdlib = python_dirs()
    shipped: dict[tuple, list[str]] = {}
    natives: dict[str, list[tuple[str, str]]] = {}
    runtime_files: list[str] = []

    def matches(patterns: list[str], name: str) -> bool:
        return any(fnmatch.fnmatch(name.lower(), p.lower()) for p in patterns)

    for name, src, kind in frozen_files(work):
        if is_under(src, [work, *OWN_FILES]):
            continue
        if not os.path.exists(src):
            problems.append(f"engine file {name}: its source {src} is gone; freeze again")
            continue
        base = Path(name).name
        if key := owners.owner(src):
            shipped.setdefault(key, []).append(name)
        elif matches(runtime["python"]["files"], base):
            runtime_files.append(name)
        elif entry := next((e for e in runtime["native"] if matches(e["files"], base)), None):
            natives.setdefault(entry["name"], []).append((name, src))
        elif (kind != "BINARY" and is_under(src, stdlib)
              and not re.search(r"[\\/](site|dist)-packages[\\/]", src)):
            runtime_files.append(name)
        else:
            problems.append(f"engine file {name} (from {src}) belongs to no known project: "
                            "add it to scripts/licenses/bundled-runtime.json")

    found = pip_licenses(sorted({key[0] for key in shipped if not key[2]}))
    components = []
    for key in sorted(shipped, key=lambda k: (canonical(k[0]), k[2])):
        name, version, within = key
        dist = owners.dists[key]
        info = found.get(canonical(name)) if not within else None
        if info is None:  # vendored copies are invisible to pip-licenses
            meta = dist.metadata
            classifiers = [c.split("::")[-1].strip() for c in meta.get_all("Classifier") or []
                           if c.startswith("License ::")]
            info = {"License-Expression": meta.get("License-Expression") or "UNKNOWN",
                    "License-Classifier": "; ".join(classifiers) or "UNKNOWN",
                    "License-Metadata": meta.get("License") or "UNKNOWN", "URL": ""}
        license_ = identify(info["License-Expression"], info["License-Classifier"],
                            info["License-Metadata"])
        url = info.get("URL", "")
        components.append(Component(ENGINE, name, version, license_,
                                    purl=f"pkg:pypi/{canonical(name)}@{version}",
                                    url="" if url == "UNKNOWN" else url,
                                    texts=dist_texts(dist), within=within))
        components += embedded_sbom(dist, ENGINE)

    if runtime_files:
        py = runtime["python"]
        version = platform.python_version()
        components.append(Component(ENGINE, py["name"], version, py["license"],
                                    purl=f"pkg:generic/cpython@{version}", url=py["url"],
                                    texts=python_license()))
    for entry in runtime["native"]:
        files = natives.get(entry["name"])
        if not files:
            continue
        texts = [t for t in (system_copyright(src) for _, src in files) if t]
        components.append(Component(ENGINE, entry["name"], "", entry["license"],
                                    url=entry["url"], texts=list(dict.fromkeys(texts)),
                                    note=f"{entry['copyright']} Files: "
                                         f"{', '.join(sorted(n for n, _ in files))}.",
                                    native=True))
    return components


# --- the notices file ---------------------------------------------------------

RULE, THIN = "=" * 78, "-" * 78


def render(components: list[Component]) -> str:
    version = read(ROOT / "VERSION")
    system = {"win32": "Windows", "darwin": "macOS"}.get(sys.platform, platform.system())
    top = [c for c in components if not c.within]
    inside: dict[str, list[Component]] = {}
    for c in components:
        if c.within:
            inside.setdefault(c.within, []).append(c)
    lines = [
        "THIRD-PARTY SOFTWARE NOTICES",
        f"SimStudio {version}",
        "",
        "SimStudio includes the third-party software listed below. Each component",
        "is used under its own licence, reproduced after the list. SimStudio's own",
        "terms (LICENSE and EULA.txt) do not apply to these components.",
        "",
        "Chromium, Node.js and the other components built into Electron are listed",
        "with their licences in LICENSES.chromium.html, next to the SimStudio",
        "executable.",
        "",
        f"Generated by scripts/third-party-notices.py from the {system} build.",
        "Do not edit by hand.",
        "",
    ]
    for part in (UI, SHELL, ENGINE):
        members = [c for c in top if c.part == part]
        if not members:
            continue
        lines += ["", part, "-" * len(part)]
        for c in members:
            lines.append(f"  {c.label} — {c.license}")
            for sub in inside.get(c.name, []):
                lines.append(f"      {sub.label} — {sub.license}  (inside {c.name})")

    standard: list[str] = []
    for c in components:
        if c.needs_standard_text:
            standard += [t for t in c.terms if t not in standard]

    for c in top:
        lines += ["", RULE, c.label, f"Licence: {c.license}"]
        if c.url:
            lines.append(f"Source: {c.url}")
        lines.append(f"Used in: {c.part.lower()}")
        if c.note:
            lines.append(c.note)
        lines.append(THIN)
        for heading, text in c.texts:
            lines += ["", f"[{heading}]", "", text]
        if c.needs_standard_text:
            shipped = "" if c.texts else "It ships no licence file of its own. "
            lines += ["", f"{shipped}The standard text of {' and '.join(c.terms)} "
                      "is in the appendix."]
        subs = inside.get(c.name, [])
        for s in (s for s in subs if s.texts):
            lines += ["", THIN, f"{s.label}, vendored inside {c.name}",
                      f"Licence: {s.license}", THIN]
            for heading, text in s.texts:
                lines += ["", f"[{heading}]", "", text]
        linked = [s for s in subs if not s.texts]
        if linked:
            lines += ["", THIN, f"Built into {c.name} (from the SBOM it ships). These parts",
                      "ship no licence files of their own; the standard texts of their",
                      "licences are in the appendix.", THIN, ""]
            lines += [f"  {s.label} — {s.license}" for s in linked]

    lines += ["", RULE, "APPENDIX: STANDARD LICENCE TEXTS", RULE]
    files = {path for term in standard for path in standard_text_files(term)}
    for path in sorted(files, key=lambda p: p.stem.lower()):
        lines += ["", THIN, path.stem, THIN, "", read(path)]
    return "\n".join(lines) + "\n"


def bill_of_materials(components: list[Component]) -> dict:
    """The components as a CycloneDX 1.5 SBOM; no timestamp, so that an
    unchanged build gives an unchanged file."""
    def entry(c: Component, parent: str = "") -> dict:
        ref = f"{parent}/{c.purl or c.name}" if parent else (c.purl or f"{c.part}/{c.name}")
        item = {"type": "library", "bom-ref": ref, "name": c.name}
        if c.version:
            item["version"] = c.version
        if c.purl:
            item["purl"] = c.purl
        item["licenses"] = [{"expression": c.license}]
        if c.url:
            item["externalReferences"] = [{"type": "website", "url": c.url}]
        item["properties"] = [{"name": "simstudio:part", "value": c.part}]
        subs = [entry(s, ref) for s in components if s.within == c.name and s.part == c.part]
        if subs:
            item["components"] = subs
        return item

    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": {
            "tools": {"components": [{"type": "application",
                                      "name": "scripts/third-party-notices.py"}]},
            "component": {"type": "application", "bom-ref": "simstudio", "name": "SimStudio",
                          "version": read(ROOT / "VERSION"),
                          "licenses": [{"license": {"name": "Proprietary; see LICENSE and EULA.txt"}}]},
        },
        "components": [entry(c) for c in components if not c.within],
    }


def contents(text: str) -> set[str]:
    """Component names, without versions, in a notices file's list."""
    names = set()
    for line in text.split(RULE, 1)[0].splitlines():
        if line.startswith("  ") and " — " in line:
            names.add(re.sub(r" \d[\w.+-]*$", "", line.strip().split(" — ")[0]))
    return names


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true",
                        help="check the policy without writing the notices or the SBOM")
    parser.add_argument("--work", type=Path, default=WORK,
                        help="PyInstaller work folder of the engine build (default: %(default)s)")
    args = parser.parse_args()

    allowed, clarified, runtime, problems = load_policy()
    components = npm_components() + engine_components(args.work, runtime, problems)
    problems += check([c for c in components if c.part != ENGINE], allowed, clarified["npm"])
    problems += check([c for c in components if c.part == ENGINE], allowed, clarified["python"])

    counts: dict[str, int] = {}
    for c in components:
        counts[c.part] = counts.get(c.part, 0) + 1
    print(", ".join(f"{n} {part.lower()}" for part, n in counts.items()) + " components")
    if problems:
        print(f"\n{len(problems)} licence problem(s):", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    text = render(components)
    if not args.check:
        NOTICES.write_text(text, encoding="utf-8", newline="\n")
        SBOM.write_text(json.dumps(bill_of_materials(components), indent=2, ensure_ascii=False)
                        + "\n", encoding="utf-8", newline="\n")
        print(f"All licences allowed. Wrote {NOTICES.name} and {SBOM.name}.")
        return 0
    print("All licences allowed.")
    committed = contents(NOTICES.read_text(encoding="utf-8")) if NOTICES.is_file() else set()
    added, gone = contents(text) - committed, committed - contents(text)
    if added or gone:
        # A warning, not a failure: the Python requirements are not pinned, and
        # the desktop build writes the file afresh before packaging anyway.
        def some(names: set[str]) -> str:
            names = sorted(names, key=str.lower)
            more = f" and {len(names) - 8} more" if len(names) > 8 else ""
            return ", ".join(names[:8]) + more if names else "none"

        message = ("THIRD-PARTY-NOTICES.txt is out of date (new: "
                   f"{some(added)}; gone: {some(gone)}). "
                   "Run scripts/third-party-notices.py and commit the result.")
        prefix = "::warning file=THIRD-PARTY-NOTICES.txt::" if os.environ.get("GITHUB_ACTIONS") else ""
        print(prefix + message)
    return 0


if __name__ == "__main__":
    sys.exit(main())
