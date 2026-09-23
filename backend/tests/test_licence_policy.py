"""The licence gate in scripts/third-party-notices.py: what it lets into the
desktop app and what it stops. The full run needs a frozen engine and npm
installs, so it runs in CI's licence job; these cover its decisions."""
from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "third-party-notices.py"


@pytest.fixture(scope="module")
def notices():
    spec = importlib.util.spec_from_file_location("third_party_notices", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module  # dataclasses look their module up there
    spec.loader.exec_module(module)
    yield module
    del sys.modules[spec.name]


@pytest.fixture(scope="module")
def allowed(notices):
    allowed, _, _, problems = notices.load_policy()
    assert not problems
    return allowed


def verdict(notices, allowed, expression):
    node = notices.parse_expression(expression)
    return None if node is None else notices.choose(node, allowed)


def test_the_policy_files_are_consistent(notices):
    allowed, clarified, runtime, problems = notices.load_policy()
    # Every allowed licence has a standard text for components that ship none.
    assert problems == []
    assert {"mit", "apache-2.0", "bsd-3-clause", "isc"} <= set(allowed)
    assert set(clarified) == {"_about", "python", "npm"}
    assert runtime["python"]["files"] and all(e["files"] for e in runtime["native"])


@pytest.mark.parametrize("expression", [
    "MIT", "(MIT OR Apache-2.0) AND Unicode-DFS-2016",
    "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT", "mit or isc",
])
def test_spdx_expressions_parse(notices, expression):
    assert notices.parse_expression(expression) is not None


@pytest.mark.parametrize("text", [
    "", "MIT License", "MIT*", "Custom: https://example.com/licence", "MIT OR",
    "(MIT", "MIT WITH", "UNKNOWN AND",
])
def test_anything_else_is_not_an_expression(notices, text):
    assert notices.parse_expression(text) is None


@pytest.mark.parametrize("expression, terms", [
    ("MIT", ["MIT"]),
    ("GPL-3.0-only OR MIT", ["MIT"]),  # a choice: we take the allowed one
    ("(MIT OR Apache-2.0) AND Unicode-DFS-2016", ["MIT", "Unicode-DFS-2016"]),
    ("GPL-2.0-or-later WITH Bootloader-exception", ["GPL-2.0-or-later WITH Bootloader-exception"]),
    ("MIT AND GPL-3.0-only", None),
    ("GPL-2.0-or-later", None),  # allowed only together with the exception
    ("AGPL-3.0-only", None),
    ("SSPL-1.0", None),
    ("EUPL-1.2", None),
    ("LGPL-3.0-only", None),
    ("UNKNOWN", None),
])
def test_the_policy_decides(notices, allowed, expression, terms):
    assert verdict(notices, allowed, expression) == terms


def test_copyleft_is_refused_even_if_listed(notices, allowed):
    node = notices.parse_expression("GPL-3.0-only")
    assert notices.choose(node, {**allowed, "gpl-3.0-only": "GPL-3.0-only"}) is None


@pytest.mark.parametrize("expression, classifiers, declared, spdx", [
    ("MIT", "UNKNOWN", "UNKNOWN", "MIT"),
    # Several classifiers do not say whether they are a choice or all apply,
    # so all of them must be allowed.
    ("UNKNOWN", "Apache Software License; MIT License", "MIT License", "Apache-2.0 AND MIT"),
    ("UNKNOWN", "GNU General Public License v3 (GPLv3); MIT License", "UNKNOWN",
     "GPL-3.0-only AND MIT"),
    ("UNKNOWN", "UNKNOWN", "BSD-3-Clause", "BSD-3-Clause"),
    # "BSD License" does not say which BSD licence, so it is not a licence.
    ("UNKNOWN", "BSD License", "UNKNOWN", ""),
    ("UNKNOWN", "UNKNOWN", "GPLv2-or-later with a special exception", ""),
])
def test_python_licences_come_from_the_metadata(notices, expression, classifiers, declared, spdx):
    assert notices.identify(expression, classifiers, declared) == spdx


def test_unknown_and_disallowed_components_fail_the_check(notices, allowed):
    engine = notices.ENGINE
    components = [
        notices.Component(engine, "fine", "1.0", "MIT"),
        notices.Component(engine, "mystery", "1.0", ""),
        notices.Component(engine, "guessed", "1.0", "MIT*"),
        notices.Component(engine, "copyleft", "1.0", "GPL-3.0-only"),
        notices.Component(engine, "relicensed", "2.0", "UNKNOWN", texts=[("LICENSE", "All rights reserved")]),
    ]
    clarified = {"relicensed": {"license": "MIT", "expect": "Permission is hereby granted"}}
    problems = notices.check(components, allowed, clarified)

    assert components[0].terms == ["MIT"]
    assert [p.split(" ", 1)[0] for p in problems] == ["mystery", "guessed", "copyleft", "relicensed"]
    assert "licence unknown" in problems[0] and "licence unknown" in problems[1]
    assert "is not allowed" in problems[2]
    assert "clarifications.json" in problems[3]


def test_several_licences_without_an_expression_must_all_be_allowed(notices, allowed):
    # A package whose parts are GPL but that also lists MIT must not pass as
    # MIT: without an SPDX expression it needs a clarifications.json entry.
    engine = notices.ENGINE
    spdx = notices.identify("UNKNOWN", "GNU General Public License v3 (GPLv3); MIT License",
                            "UNKNOWN")
    assert verdict(notices, allowed, spdx) is None
    problems = notices.check([notices.Component(engine, "mixed", "1.0", spdx)], allowed, {})
    assert len(problems) == 1 and "is not allowed" in problems[0]


def test_npm_licence_arrays_must_all_be_allowed(notices, allowed, monkeypatch):
    # Old package.json "licenses" arrays are just as ambiguous.
    found = {"mixed@1.0.0": {"licenses": ["GPL-3.0-only", "MIT"]},
             "dual@2.0.0": {"licenses": ["MIT", "Apache-2.0"]}}
    monkeypatch.setattr(notices, "license_checker", lambda folder, *args: found)
    components = {c.name: c for c in notices.npm_components()}
    assert components["mixed"].license == "GPL-3.0-only AND MIT"
    problems = notices.check(list(components.values()), allowed, {})
    assert [p.split(" ", 1)[0] for p in problems] == ["mixed"]
    assert components["dual"].terms == ["MIT", "Apache-2.0"]


def test_the_desktop_shells_runtime_dependencies_are_checked(notices, allowed, monkeypatch):
    # electron-builder ships every runtime dependency of desktop/ inside
    # app.asar, so the gate must see them, not just Electron.
    def fake(folder, *args):
        if folder.name == "desktop" and "--production" in args:
            return {"gpl-updater@1.0.0": {"licenses": "GPL-3.0-only"}}
        if folder.name == "desktop":
            return {"electron@38.0.0": {"licenses": "MIT"}}
        return {}
    monkeypatch.setattr(notices, "license_checker", fake)
    components = {c.name: c for c in notices.npm_components()}
    assert components["gpl-updater"].part == notices.SHELL
    problems = notices.check(list(components.values()), allowed, {})
    assert [p.split(" ", 1)[0] for p in problems] == ["gpl-updater"]


def test_example_data_is_credited_with_its_notice(notices, allowed):
    """Third-party data in the examples (scripts/licenses/bundled-data.json)
    goes through the same licence gate, and the notices reproduce the NOTICE
    its Apache-2.0 licence asks for, with the licence text in the appendix."""
    data = notices.data_components()
    assert data and all(c.part == notices.DATA and c.data for c in data)
    assert notices.check(data, allowed, {}) == []
    fastsim = next(c for c in data if c.name.startswith("FASTSim"))
    assert fastsim.terms == ["Apache-2.0"]
    notice = dict(fastsim.texts)["NOTICE"]
    assert notice.startswith("Copyright 2020 Alliance for Sustainable Energy, LLC")

    text = notices.render(data)
    assert f"  {fastsim.label} — Apache-2.0" in text.split(notices.RULE, 1)[0]
    assert notice in text
    appendix = text.split("APPENDIX: STANDARD LICENCE TEXTS", 1)[1]
    assert "Apache License" in appendix and "Version 2.0" in appendix
    bom = notices.bill_of_materials(data)["components"]
    assert [(c["type"], c["licenses"]) for c in bom] == [("data", [{"expression": "Apache-2.0"}])]

    # the committed notices file (Help → Third-Party Notices) carries it too
    committed = (SCRIPT.parents[1] / "THIRD-PARTY-NOTICES.txt").read_text(encoding="utf-8")
    assert fastsim.name in notices.contents(committed)
    assert notice in committed
