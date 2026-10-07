# Use a model from another tool (FMU)

An FMU (Functional Mock-up Unit) is a model packed in one `.fmu` file by the
FMI standard (Functional Mock-up Interface). Simulink, Dymola, GT-SUITE,
OpenModelica and many suppliers export them: a battery's thermal model, a
supplier's inverter, a controller built in Simulink. LightSim runs such a
model as one part of your car, the *FMU* block, and passes signals to and
from it at every step.

LightSim runs **Co-Simulation** FMUs of **FMI 2.0 or 3.0**: FMUs that bring
their own solver. Ask whoever exports the model for that kind, built for
your operating system. A Model Exchange FMU (one that needs the importing
tool's solver) does not run yet.

## Add the FMU to your model

1. Drag the `.fmu` file from your file manager onto the diagram.
   Or: in *Components*, under *Driver & Signals*, add **FMU (Model from
   Another Tool)**, then click **Choose FMU file…** in *Properties*.
2. LightSim asks **Allow … to run?** An FMU contains compiled code from
   another tool or company. Click **Allow it to run** only if you trust
   where the file came from. Click **Not now** to look at the FMU first;
   you can allow it later in *Properties*.
3. The block appears, named after the model. LightSim keeps its own copy
   of the file, so moving or deleting the original does not break it.

## Check what the FMU is

Select the block. *Properties* shows:

- the FMI version, the kind (Co-Simulation, Model Exchange) and the tool
  that made it;
- where it runs: **Runs here** in green, or in red **Windows only**,
  **Linux only**, **macOS only** or **Source only** (only source code, no
  compiled model). A red badge comes with an error in *Problems* that names
  the file to ask the supplier for;
- **Allowed to run on this computer**, or the **Allow this FMU to run…**
  button;
- problems in red (the FMU cannot run) and warnings in amber. A warning
  that the description "does not follow the FMI standard" comes from
  FMPy's checks; the FMU may still run, but tell its supplier.

## Choose its pins and start values

1. In *Properties*, click **Variables and pins**. A dialog lists every
   variable of the FMU, in the FMU's own name tree, with its kind (input,
   output, parameter, internal), start value and unit. Rest the pointer on
   a row to read its description.
2. Tick a variable to make it a pin of the block. Inputs become input
   pins, outputs and internal values become output pins. A new block has
   all the FMU's inputs and outputs ticked; parameters cannot be pins.
3. To change a parameter's or input's start value, type it in *Start* and
   press Enter. It applies when the FMU starts, at the beginning of each
   run. A value you changed shows in bold; clear the field to go back to
   the FMU's own value.

## Wire it and run

1. Open *Data Bus Connections* and pick a source for each of the block's
   inputs, as for any part (see [Wire control signals](wire-control-signals.md)).
   An input with no source reads 0.
2. Link the block's outputs to the inputs that need them.
3. *Communication Step* in *Properties* sets how often LightSim and the FMU
   exchange values. At 0 they do so every solver step (at most 10 ms). If
   the FMU is slow, use the step the FMU suggests (shown under its details).
4. Click **Run**. The block's outputs are recorded: on the *Results* page,
   search for the block's name.

## When something goes wrong

- **The run stops with "crashed", "did not finish its step" or "reported
  an error".** The FMU runs in a separate, locked-down process, so its
  failure stops the run, not LightSim. Send the message to the FMU's
  supplier.
- **"… has not been allowed to run on this computer."** Select the block
  and click **Allow this FMU to run…**. LightSim asks once for each FMU on
  each computer; a changed FMU file is asked about again.
- **"… was not found … Import the FMU again".** The project came from
  another computer: FMU files are not yet saved inside the project. Get
  the `.fmu` file and choose it in *Properties*.
- **"FMU support is not installed".** This copy of LightSim does not have
  the FMU pack (FMPy). From source: `pip install -r requirements-fmu.txt`
  then `pip install --no-deps fmpy==0.3.32` in `backend/`.

[Known issues](../../KNOWN-LIMITS.md) lists what FMU blocks cannot do yet.
