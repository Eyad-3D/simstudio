"""Build the LightSimTest FMU (lightsim_test_fmu.c) for the tests.

Compiled with the system C compiler into a temporary folder; the tests that
need it skip when there is no compiler (Windows runners) or no FMPy.
"""
from __future__ import annotations

import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

HERE = Path(__file__).parent
GUID = "{7a1c6a0e-5f0b-4b8e-9d3e-1c2b3a4d5e6f}"

MODEL_DESCRIPTION = f"""<?xml version="1.0" encoding="UTF-8"?>
<fmiModelDescription fmiVersion="2.0" modelName="LightSimTest" guid="{GUID}"
    description="First-order lag with gain (LightSim's test FMU)"
    generationTool="LightSim tests" numberOfEventIndicators="0">
  <CoSimulation modelIdentifier="LightSimTest" canHandleVariableCommunicationStepSize="true"/>
  <UnitDefinitions>
    <Unit name="s"><BaseUnit s="1"/></Unit>
  </UnitDefinitions>
  <DefaultExperiment startTime="0" stopTime="2" stepSize="0.01"/>
  <ModelVariables>
    <ScalarVariable name="u" valueReference="0" causality="input" variability="continuous"
        description="Input signal"><Real start="0"/></ScalarVariable>
    <ScalarVariable name="k" valueReference="1" causality="parameter" variability="tunable"
        description="Gain"><Real start="2"/></ScalarVariable>
    <ScalarVariable name="tau" valueReference="2" causality="parameter" variability="fixed"
        description="Time constant"><Real start="0.5" unit="s"/></ScalarVariable>
    <ScalarVariable name="y" valueReference="3" causality="output" variability="continuous"
        initial="exact" description="Lagged output"><Real start="0"/></ScalarVariable>
    <ScalarVariable name="t_fmu" valueReference="4" causality="output" variability="continuous"
        initial="exact" description="The FMU's own time"><Real start="0" unit="s"/></ScalarVariable>
    <ScalarVariable name="escaped" valueReference="5" causality="output" variability="discrete"
        initial="exact" description="1: read a file, 2: wrote a file (mode 3)"><Real start="0"/></ScalarVariable>
    <ScalarVariable name="mode" valueReference="6" causality="parameter" variability="fixed"
        description="0 behave, 1 crash, 2 hang, 3 files, 4 memory, 5 error"><Integer start="0"/></ScalarVariable>
  </ModelVariables>
  <ModelStructure>
    <Outputs>
      <Unknown index="4" dependencies=""/>
      <Unknown index="5" dependencies=""/>
      <Unknown index="6" dependencies=""/>
    </Outputs>
  </ModelStructure>
</fmiModelDescription>
"""


def compiler() -> str | None:
    if sys.platform == "win32":
        return None
    return shutil.which("cc") or shutil.which("gcc") or shutil.which("clang")


def build(target_dir: Path) -> Path:
    """Compile and zip the test FMU; returns the .fmu path."""
    cc = compiler()
    if cc is None:
        raise RuntimeError("no C compiler")
    if sys.platform == "darwin":
        folder, ext, extra = "darwin64", ".dylib", ["-dynamiclib"]
    else:
        folder, ext, extra = "linux64", ".so", ["-shared"]
    lib = target_dir / f"LightSimTest{ext}"
    subprocess.run([cc, "-O2", "-fPIC", *extra, "-o", str(lib),
                    str(HERE / "lightsim_test_fmu.c"), "-lm"], check=True)
    fmu = target_dir / "LightSimTest.fmu"
    with zipfile.ZipFile(fmu, "w", zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("modelDescription.xml", MODEL_DESCRIPTION)
        zf.write(lib, f"binaries/{folder}/LightSimTest{ext}")
        zf.write(HERE / "lightsim_test_fmu.c", "sources/lightsim_test_fmu.c")
    return fmu
