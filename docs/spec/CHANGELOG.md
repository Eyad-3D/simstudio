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
| Project history (`history.jsonl`) | 1 | New: one line per save of a project, beside its backups, with who saved (the app or an AI agent, its client and session), why, what changed and the change itself (step 6) |
| Python API (`lightsim`) | 1 | New: `run`, `check`, `load`, `read_run`, `Project`, `Result`. `Result.to_parquet` writes Parquet when pyarrow is installed (STD-09) |
| Result (`SimResult`) | 1 | New optional fields: `engine`, `engineVersion` and `solverOptions` (what ran it) and `liveEdits` (the live edits it made, each at the simulated time the run had reached when it took it) |
| Stored run | 1 | The snapshot's new optional fields: `engine`, `engineVersion` and `solverOptions`, from the result; its `liveEdits` from the result's own list when it has one |
| Result (`SimResult`) and stored run | 1 | A live edit's new optional field `note`: for an edit the run did not make (it takes effect on the next run), the result's note of it; the new engine's `liveEdits` list those edits too |
| Project | 1 | New optional fields: a case's `mode` (`full` or `quick`: quick mode, the new engine's fast mode) and a part's `textFile` (a text component the new engine builds the part from) |
| Result (`SimResult`) and stored run | 1 | A quick-mode run's `solverOptions` is `quickStep` alone |
| Run card (`lightsim-run-card`, and the MAT-file's `meta.live_edits`) | 1 | Each live edit's new field `note`, as in the stored run: why the run did not make it (null, in the MAT-file empty, for one it made) |
