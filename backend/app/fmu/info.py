"""Read an FMU without running it: what it is, its variables, where it runs.

Everything here only reads the archive (the XML description and the list of
files in it); none of the FMU's own code is loaded, so it is safe to call in
the engine process for an FMU nobody has allowed yet.
"""
from __future__ import annotations

import platform as _platform
import sys
import zipfile
from pathlib import Path

from . import NOT_INSTALLED, fmpy_available

#: FMI 2.0 folder names and FMI 3.0 platform tuples, by operating system.
_OS_OF_FOLDER = {
    "win32": "Windows", "win64": "Windows",
    "linux32": "Linux", "linux64": "Linux",
    "darwin32": "macOS", "darwin64": "macOS",
}
_OS_OF_TUPLE_SUFFIX = {"windows": "Windows", "linux": "Linux", "darwin": "macOS"}
_LIB_EXT = {"Windows": ".dll", "Linux": ".so", "macOS": ".dylib"}

#: Variable types LightSim can pass through a pin (as a number).
NUMERIC_TYPES = frozenset({
    "Real", "Integer", "Boolean", "Enumeration",  # FMI 2.0
    "Float32", "Float64", "Int8", "UInt8", "Int16", "UInt16",
    "Int32", "UInt32", "Int64", "UInt64",  # FMI 3.0
})

#: The largest modelDescription.xml LightSim reads (FMUs with tens of
#: thousands of variables stay well under it).
MAX_DESCRIPTION_BYTES = 256 * 1024 * 1024

_cache: dict[tuple[str, int, int], dict] = {}


def this_os() -> str:
    if sys.platform == "win32":
        return "Windows"
    if sys.platform == "darwin":
        return "macOS"
    return "Linux"


def this_folders(fmi_version: str) -> list[str]:
    """The binaries/ folder names that hold code for this computer."""
    machine = _platform.machine().lower()
    arm = machine in ("arm64", "aarch64")
    os_name = this_os()
    if fmi_version.startswith("3"):
        arch = "aarch64" if arm else ("x86_64" if sys.maxsize > 2**32 else "x86")
        return [f"{arch}-{ {'Windows': 'windows', 'Linux': 'linux', 'macOS': 'darwin'}[os_name]}"]
    if os_name == "Windows":
        return ["win64" if sys.maxsize > 2**32 else "win32"]
    if os_name == "macOS":
        return ["darwin64", "aarch64-darwin" if arm else "x86_64-darwin"]
    return ["linux64" if sys.maxsize > 2**32 else "linux32"]


def _os_of(folder: str) -> str | None:
    if folder in _OS_OF_FOLDER:
        return _OS_OF_FOLDER[folder]
    return _OS_OF_TUPLE_SUFFIX.get(folder.rsplit("-", 1)[-1])


def platforms(names: list[str], model_identifier: str, fmi_version: str) -> dict:
    """Which operating systems the FMU has code for, from its file list."""
    folders: set[str] = set()
    by_os: dict[str, list[str]] = {}
    for name in names:
        parts = name.replace("\\", "/").split("/")
        if len(parts) == 3 and parts[0] == "binaries" and parts[2]:
            folder, file = parts[1], parts[2]
            os_name = _os_of(folder)
            if os_name and file == model_identifier + _LIB_EXT[os_name]:
                folders.add(folder)
                by_os.setdefault(os_name, []).append(folder)
    has_sources = any(n.replace("\\", "/").startswith("sources/") and n.endswith(".c")
                      for n in names)
    here = this_os()
    runs_here = any(f in folders for f in this_folders(fmi_version))
    oses = sorted(by_os)
    if runs_here:
        badge = "runs here"
    elif oses:
        badge = " and ".join(oses) + " only"
    elif has_sources:
        badge = "source only"
    else:
        badge = "no code"
    missing = [o for o in ("Windows", "Linux", "macOS") if o not in by_os]
    return {
        "badge": badge,
        "runsHere": runs_here,
        "thisOs": here,
        "operatingSystems": oses,
        "binaryFolders": sorted(folders),
        "missingOperatingSystems": missing,
        "hasSources": has_sources,
    }


def _plain(problem: str) -> str:
    problem = str(problem).strip().rstrip(".")
    return f"Its description does not follow the FMI standard: {problem}."


def describe(path: Path) -> dict:
    """Everything the import dialog and Data Checks show about an FMU.

    Never raises for a bad file: problems are listed under ``problems`` (in
    plain words) and ``ok`` is False when the FMU cannot run in LightSim."""
    try:
        st = path.stat()
    except OSError:
        return _broken(f"The file {path} cannot be read.")
    key = (str(path), st.st_size, st.st_mtime_ns)
    if key in _cache:
        return _cache[key]
    info = _describe(path)
    if len(_cache) > 64:
        _cache.clear()
    _cache[key] = info
    return info


def _broken(problem: str, **extra) -> dict:
    return {"ok": False, "problems": [problem], "variables": [], **extra}


def _describe(path: Path) -> dict:
    try:
        with zipfile.ZipFile(path) as zf:
            names = zf.namelist()
            xml_size = zf.getinfo("modelDescription.xml").file_size if (
                "modelDescription.xml" in names) else 0
    except (OSError, zipfile.BadZipFile):
        return _broken("This is not an FMU: an FMU is a zip file, and this file is not one.")
    if "modelDescription.xml" not in names:
        return _broken("This is not an FMU: it has no modelDescription.xml.")
    if xml_size > MAX_DESCRIPTION_BYTES:
        return _broken(f"Its description (modelDescription.xml) is larger than "
                       f"{MAX_DESCRIPTION_BYTES >> 20} MB.")
    if not fmpy_available():
        return _broken(NOT_INSTALLED, fmpyMissing=True)

    from fmpy import read_model_description
    from fmpy.validation import validate_fmu

    try:
        md = read_model_description(str(path), validate=False)
    except Exception as e:  # noqa: BLE001 — any parse failure is the file's
        return _broken(f"Its description (modelDescription.xml) cannot be read: {e}")

    problems: list[str] = []
    warnings: list[str] = []
    try:
        found = validate_fmu(str(path))
    except Exception as e:  # noqa: BLE001
        found = [str(e)]
    # FMPy's checks are strict (they flag some of the standard's own Reference
    # FMUs), so what they find is a warning to pass on to the supplier, not
    # a reason to refuse the FMU
    for p in found[:10]:
        warnings.append(_plain(p))
    if len(found) > 10:
        warnings.append(f"… and {len(found) - 10} more problems in its description.")

    version = str(md.fmiVersion or "")
    cs = md.coSimulation
    kinds = [k for k, v in (("Co-Simulation", cs), ("Model Exchange", md.modelExchange))
             if v is not None]
    if getattr(md, "scheduledExecution", None) is not None:
        kinds.append("Scheduled Execution")
    if not version.startswith(("2.", "3.")):
        problems.append(f"It is an FMI {version} FMU; LightSim runs FMI 2.0 and 3.0 FMUs. "
                        f"Ask for an FMI 2.0 or 3.0 export.")
    if cs is None:
        problems.append("It is a Model Exchange FMU only; LightSim runs Co-Simulation FMUs "
                        "(the FMU brings its own solver). Ask for a Co-Simulation export.")
    model_identifier = cs.modelIdentifier if cs is not None else (
        md.modelExchange.modelIdentifier if md.modelExchange is not None else "")
    plat = platforms(names, model_identifier, version)
    if cs is not None and not plat["runsHere"]:
        if plat["badge"] == "source only":
            problems.append(f"It has only source code, no code compiled for {plat['thisOs']}; "
                            f"ask the supplier for an FMU built for {plat['thisOs']}.")
        else:
            problems.append(f"It has no code for {plat['thisOs']} ({plat['badge']}); ask the "
                            f"supplier for an FMU built for {plat['thisOs']}.")
    if cs is not None and plat["runsHere"] and plat["missingOperatingSystems"]:
        miss = " and ".join(plat["missingOperatingSystems"])
        warnings.append(f"It has no code for {miss}: a colleague on {miss} cannot run this "
                        f"model. Ask the supplier for the FMU built for {miss} as well.")

    variables = []
    for v in md.modelVariables:
        unit = v.unit or (v.declaredType.unit if v.declaredType is not None else None)
        start = v.start
        try:
            start_num = float(start) if start is not None and not isinstance(start, str) else (
                None if start is None else float({"true": 1, "false": 0}.get(start, start)))
        except (TypeError, ValueError):
            start_num = None
        numeric = v.type in NUMERIC_TYPES and not getattr(v, "dimensions", None)
        causality = v.causality or "local"
        pin = None
        if numeric and causality == "input":
            pin = "input"
        elif numeric and causality in ("output", "local") and v.variability != "constant":
            pin = "output"
        variables.append({
            "name": v.name,
            "valueReference": int(v.valueReference),
            "type": v.type,
            "causality": causality,
            "variability": v.variability or "continuous",
            "unit": unit or "",
            "start": start_num,
            "description": v.description or "",
            # the pin it can be: an input, an output, or none (not a number)
            "pin": pin,
            # a value the user may set before the run (parameters and inputs)
            "settable": bool(numeric and causality in ("parameter", "input")
                             and v.variability != "constant"),
        })

    de = md.defaultExperiment
    step = None
    if de is not None and de.stepSize:
        try:
            step = float(de.stepSize)
        except (TypeError, ValueError):
            step = None
    ok = not problems
    return {
        "ok": ok,
        "problems": problems,
        "warnings": warnings,
        "fmiVersion": version,
        "kinds": kinds,
        "modelName": md.modelName or "",
        "description": md.description or "",
        "generationTool": md.generationTool or "",
        "guid": md.guid or "",
        "modelIdentifier": model_identifier,
        "canGetAndSetState": bool(cs is not None and cs.canGetAndSetFMUstate),
        "defaultStepSize": step,
        "platform": plat,
        "variables": variables,
    }
