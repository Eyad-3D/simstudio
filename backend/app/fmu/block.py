"""The FMU block (``signal.fmu``): from its parameters and pins to a run.

The block's parameters:

* ``fmu_path`` / ``fmu_sha256`` / ``fmu_name`` — which FMU (see ``store``);
* ``sample_time_s`` — how often the FMU steps (0: every solver step);
* ``start:<variable>`` — a start value the user set for an FMU parameter or
  input, applied before the FMU initialises (the FMU's own start value
  otherwise). Being ordinary parameter values, they can be changed per case.

Its pins are ``dynamicPorts`` whose **name** is the FMU variable's name, so a
pin keeps its variable whatever id the canvas gives it. Inputs feed FMU
inputs; outputs read FMU outputs (or locals the user chose to watch).
"""
from __future__ import annotations

from pathlib import Path

from . import NOT_INSTALLED, FmuError, fmpy_available
from .info import describe, this_folders
from .sandbox import FmuSpec
from .store import FmuFileError, is_allowed, locate, sha256_of_cached, unpacked

COMPONENT_ID = "signal.fmu"
START_PREFIX = "start:"


def not_allowed_text(name: str) -> str:
    return (f"'{name}' has not been allowed to run on this computer. An FMU contains "
            f"compiled code from another company or tool; allow it only if you trust "
            f"where it came from.")


def problems(label: str, params: dict, ports: list) -> list[tuple[str, str, str | None]]:
    """Data Checks for one FMU block: (level, text, fix)."""
    out: list[tuple[str, str, str | None]] = []
    if not fmpy_available():
        return [("error", f"'{label}': {NOT_INSTALLED}",
                 "Install the FMU pack (see the help page 'Use models from other tools').")]
    try:
        path = locate(params)
    except FmuFileError as e:
        return [("error", f"'{label}': {e}", "Select the block and choose its FMU file.")]
    info = describe(path)
    for p in info.get("problems", []):
        out.append(("error", f"FMU '{label}': {p}", None))
    for w in info.get("warnings", []):
        out.append(("warning", f"FMU '{label}': {w}", None))
    if not info.get("ok"):
        return out
    by_name = {v["name"]: v for v in info["variables"]}
    for port in ports:
        v = by_name.get(port.name)
        if v is None:
            out.append(("error", f"FMU '{label}' has a pin '{port.name}' that is not a "
                                 f"variable of its FMU (the FMU may have changed).",
                        "Untick the pin in the block's FMU variables list."))
        elif v["pin"] != port.direction:
            out.append(("error", f"FMU '{label}': '{port.name}' cannot be an "
                                 f"{port.direction} pin (it is a {v['causality']} "
                                 f"{v['type']}).", "Untick it in the block's FMU variables list."))
    for key in params:
        if key.startswith(START_PREFIX):
            name = key[len(START_PREFIX):]
            v = by_name.get(name)
            if v is None or not v["settable"]:
                out.append(("warning", f"FMU '{label}' has a start value for '{name}', which "
                                       f"is not a parameter or input of its FMU; it is ignored.",
                            None))
    sha = str(params.get("fmu_sha256") or "") or sha256_of_cached(path)
    if not is_allowed(sha):
        out.append(("error", f"FMU {not_allowed_text(label)}",
                    "Select the block and click 'Allow this FMU to run'."))
    return out


def library_path(unzip_dir: Path, info: dict) -> Path:
    ext = {"Windows": ".dll", "macOS": ".dylib"}.get(info["platform"]["thisOs"], ".so")
    for folder in this_folders(info["fmiVersion"]):
        lib = unzip_dir / "binaries" / folder / (info["modelIdentifier"] + ext)
        if lib.is_file():
            return lib
    raise FmuError(f"The FMU has no code for {info['platform']['thisOs']}.")


def spec_for(el_id: str, label: str, params: dict, ports: list) -> FmuSpec:
    """What the worker needs to run this block. Raises FmuError/FmuFileError
    with a message for the user (the same ones Data Checks give)."""
    if not fmpy_available():
        raise FmuError(f"'{label}': {NOT_INSTALLED}")
    path = locate(params)
    info = describe(path)
    if not info.get("ok"):
        raise FmuError(f"FMU '{label}': " + " ".join(info.get("problems", [])))
    sha = sha256_of_cached(path)
    if not is_allowed(sha):
        raise FmuError(f"FMU {not_allowed_text(label)}")
    by_name = {v["name"]: v for v in info["variables"]}
    inputs, outputs = [], []
    for port in ports:
        v = by_name.get(port.name)
        if v is None or v["pin"] != port.direction:
            raise FmuError(f"FMU '{label}': pin '{port.name}' does not match its FMU.")
        (inputs if port.direction == "input" else outputs).append(
            (v["valueReference"], v["type"]))
    starts = []
    for key, value in params.items():
        if not key.startswith(START_PREFIX):
            continue
        v = by_name.get(key[len(START_PREFIX):])
        if v is None or not v["settable"]:
            continue
        try:
            starts.append((v["valueReference"], v["type"], float(value)))
        except (TypeError, ValueError):
            raise FmuError(f"FMU '{label}': the start value of '{v['name']}' is not a "
                           f"number.") from None
    unzip = unpacked(path, sha)
    return FmuSpec(
        el_id=el_id, label=label, unzip_dir=str(unzip),
        library_path=str(library_path(unzip, info)),
        fmi_version=info["fmiVersion"], guid=info["guid"],
        model_identifier=info["modelIdentifier"],
        inputs=inputs, outputs=outputs, starts=starts,
    )
