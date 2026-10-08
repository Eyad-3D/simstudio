# What LightSim reads and writes

Which file formats and standards LightSim works with today, which are
planned, and which are built only when someone asks for them. Each
release's notes update this page.

- **Works**: in this release, with an automatic test in LightSim's CI that
  writes the format and reads it back (or reads a reference file). The
  *Tested by* column names the test.
- **Partial**: some of it works, or it works without such a test yet.
- **Planned**: on the roadmap, not built yet.
- **On request**: not planned; built if users need it. Tell us.

Last updated for version 0.3.0.

## Works today

| Format | Reads | Writes | Where | Tested by |
|---|---|---|---|---|
| LightSim project (JSON, [spec](spec/project.md)) | yes | yes | the app, the Python package, the command line | `backend/tests/test_project_roundtrip.py::test_nothing_in_a_file_is_dropped` |
| Stored run (gzip JSON, [spec](spec/results.md#stored-run)) | yes | yes | the app's run history, `lightsim export`, `lightsim.read_run` | `backend/tests/test_runs.py::test_a_stored_run_comes_back_unchanged` |
| Results as CSV, units in the header | — | yes | `lightsim run -o x.csv`, `Result.to_csv` | `backend/tests/test_python_api.py::test_csv_has_units_in_the_header_and_a_row_per_point` |
| Results as a MATLAB MAT-file (Level 5, `save -v6`) | — | yes | `lightsim run -o x.mat`, `Result.to_mat` | `backend/tests/test_python_api.py::test_mat_file_round_trips_through_scipy` |
| Results as JSON (`lightsim-result`, [spec](spec/results.md#json-lightsim-result)) | yes | yes | `lightsim run -o x.json`, `lightsim.read_run` | `backend/tests/test_python_api.py::test_json_export_matches_its_schema_and_reads_back` |
| JSON Schemas of every file ([spec](spec/README.md)) | — | yes | `docs/spec/schemas/`, `lightsim schema` | `backend/tests/test_spec.py::test_committed_schemas_are_up_to_date` |
| Project import from a file (*Import*) | yes | — | the app | `frontend/src/store/projectStore.test.ts::Import loads a project file as unsaved work` |
| Drive cycle of your own from CSV or Excel (`.xlsx`): speed and grade against time or distance, units in the header | yes | — | the app (*Drive Cycle → Import a cycle from a file…*), kept in the project file ([spec](spec/project.md#drive-cycle-of-the-projects-own)) | `backend/tests/test_own_cycles.py::test_import_reads_time_speed_and_grade` |

## Partly there

| Format | What works | What is missing |
|---|---|---|
| Results CSV from the *Results* tab | The app exports the plotted channels along the chart's x axis | A test that reads the file back |
| Paste a table from a spreadsheet | The table editor takes a pasted block of cells | A test of the paste |
| Standard drive cycles (WLTC class 3b, EPA UDDS, HWFET) | Bundled; a Driving Task drives them | — (your own cycles: above) |
| Results as Apache Parquet | `Result.to_parquet` and `lightsim run -o x.parquet` in the Python package, when pyarrow is installed (`backend/tests/test_python_api.py::test_parquet_round_trips_through_pyarrow`) | pyarrow is not in LightSim's installer or its CI, so the app cannot write it and the test runs only where pyarrow is installed |

## Planned

For students and small teams:

- CSV import of measured signals, with units in the header
- `.mat` export from the *Results* tab (the command line and Python already write it)
- Charts as PNG and SVG
- GPX tracks (GPS recordings) as drive cycles and road profiles
- Data-logger and lap-simulator CSV import
- One spreadsheet of all parameters, to edit outside the app

For carmaker and supplier engineers:

- FMI 2.0 and 3.0 Co-Simulation FMU import (functional mock-up units, a
  standard way to swap simulation models)
- MDF4 import and export (ASAM measurement data files)
- Apache Parquet results from the app itself (the Python package writes them, above)
- FMU export (FMI 2.0 Co-Simulation first)
- SSP 2.0 (system structure and parameters) and requirements, from a
  spreadsheet first, then ReqIF

## On request only

Built if users ask for them: DCP (distributed co-simulation protocol),
OPC UA, ASAM XIL, SysML v2, CDF/DCM calibration files, OpenDRIVE road
networks, CAN and FMI-LS-BUS, WebAssembly FMUs, CAD pipe import, and
export to hardware-in-the-loop (HIL) systems.

## The licence rule for exchange code

LightSim is proprietary, so it ships only libraries under permissive
licences (MIT, BSD, Apache 2.0 and the like; the full list is
`scripts/licenses/allowed.txt`): for example FMPy, ONNX Runtime, MCAP and
mdflib. Tools under GPL, LGPL or AGPL licences (asammdf, OpenModelica,
OMSimulator) are used only in LightSim's own tests, or when you install
them yourself; they are never in the installer.
