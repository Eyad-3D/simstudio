# Format changelog

What changed in each version of LightSim's file formats and of its
Python API. The rules for when a version changes are in the
[specification](README.md#versions).

## LightSim 0.3.0

| Format | Version | Change |
|---|---|---|
| Project | 1 | First published. Optional `noAi` (true hides the project from AI tools) |
| Result (`SimResult`) | 1 | First published. Each summary row has a stable `key` (empty on runs stored by 0.2) |
| Stored run | 1 | First published |
| Study | 1 | First published. Its table is keyed by the summary rows' labels, not yet by their keys |
| Library, Data Checks | 1 | First published |
| `lightsim-result` (JSON export) | 1 | New |
| Python API (`lightsim`) | 1 | New: `run`, `check`, `load`, `read_run`, `Project`, `Result` |
