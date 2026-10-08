# Format changelog

What changed in each version of LightSim's file formats and of its
Python API. The rules for when a version changes are in the
[specification](README.md#versions).

## LightSim 0.3.0

| Format | Version | Change |
|---|---|---|
| Project | 1 | First published. Optional `noAi` (true hides the project from AI tools), `card` (an example's card, CON-15), `attachments` (files kept with it, STD-02), `cycles` (drive cycles of its own, against time or distance, CON-11) and each part's `parameterSources` (CON-13). Studies are no longer in the project file: they are kept with its runs (PLT-34) |
| Result (`SimResult`) | 1 | First published. Each summary row has a stable `key` (empty on runs stored by 0.2) and keeps full precision (ENG-16). New: `partEnergy` (each part's energy books, MOD-10), `energy`, `duty` and `limits` (the Energy and Duty views and the limit band, RES-22, RES-39, RES-38) and `references` (expected values and hand checks, VAL-35) |
| Stored run | 1 | First published |
| Study | 1 | First published. Its table is keyed by the summary rows' labels, not yet by their keys. A study from the parallel runner records `workers` and `wallS`, each point its `wallS` (ENG-05) |
| Library, Data Checks | 1 | First published. A Data Check has an optional `caseId`: the case whose own values or kind it is about (it stops only that case's runs) |
| `lightsim-result` (JSON export) | 1 | New |
| Python API (`lightsim`) | 1 | New: `run`, `check`, `load`, `read_run`, `Project`, `Result` |
