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
| LightSim project (JSON, [spec](spec/project.md)) | yes | yes | the app (**Save**, **Save As…** and **Open** for `.lightsim` files in any folder, **Export** and **Import**), the Python package, the command line | `backend/tests/test_project_roundtrip.py::test_nothing_in_a_file_is_dropped` |
| Project with its attached files (a zip of `project.json` and a `resources/` folder, `<id>.lightsim.zip`) | yes | yes | the app: **Export** writes it when the project has attached files; **Import** reads it | `backend/tests/test_attachments.py::test_a_project_with_an_fmu_and_an_onnx_file_runs_unchanged_on_another_machine` |
| Stored run (gzip JSON, [spec](spec/results.md#stored-run)) | yes | yes | the app's run history, `lightsim export`, `lightsim.read_run` | `backend/tests/test_runs.py::test_a_stored_run_comes_back_unchanged` |
| Results as CSV, units in the header | — | yes | `lightsim run -o x.csv`, `lightsim export`, `Result.to_csv` | `backend/tests/test_python_api.py::test_csv_has_units_in_the_header_and_a_row_per_point` |
| Results as CSV from the *Results* tab (the ticked channels, along the chart's x axis) | — | yes | the app (**CSV**) | `frontend/e2e/core-flow.spec.ts::BEV example: data checks, run, results channels and CSV export` |
| Results as a MATLAB MAT-file (Level 5, `save -v6`): the Python package's layout, a column for each channel | — | yes | `lightsim run -o x.mat`, `lightsim export`, `Result.to_mat` | `backend/tests/test_python_api.py::test_mat_file_round_trips_through_scipy` |
| Results as a MATLAB MAT-file: the app's layout, a struct for each part with its channels and a `meta` struct with the units and the run card | — | yes | the app (**MATLAB** on the *Results* tab), `lightsim-backend run --out x.mat`, `lightsim-backend export`, `lightsim_run.m` | `backend/tests/test_results_export.py::test_both_examples_round_trip_through_scipy_with_values_and_units` |
| Results as JSON (`lightsim-result`, [spec](spec/results.md#json-lightsim-result)) | yes | yes | `lightsim run -o x.json`, `lightsim.read_run` | `backend/tests/test_python_api.py::test_json_export_matches_its_schema_and_reads_back` |
| Run card (JSON, `lightsim-run-card`: what made the run, how it ended, the unit of every column) | — | yes | the engine's `export?format=json` route and `lightsim-backend export --format json` | `backend/tests/test_results_export.py::test_the_run_card_json_names_every_column` |
| JSON Schemas of every file ([spec](spec/README.md)) | — | yes | `docs/spec/schemas/`, `lightsim schema` | `backend/tests/test_spec.py::test_committed_schemas_are_up_to_date` |
| A chart as a PNG picture, at twice its size | — | yes | the app (**PNG** on the *Results* tab) | `frontend/e2e/res37-png-export.spec.ts::RES-37: the PNG export is the whole chart at twice its on-screen size` |
| The energy table and the bands of the energy-flow chart, as CSV | — | yes | the app (the *Energy* view of the *Results* tab) | `frontend/src/reports.test.ts::exports the table and the bands as CSV` |
| Sources and credits of a run, as BibTeX and CSL-JSON | — | yes | the app (*Run info → Sources & credits*) | `backend/tests/test_sources.py::test_bibtex_and_csl_json_cite_lightsim_and_every_source` |
| Parameter sheet (Excel `.xlsx` or CSV): every parameter of the model, with each table on a sheet of its own | yes | yes | the app (*Parameter Sheet* on the ribbon), `lightsim params`, `lightsim-backend params` | `backend/tests/test_param_sheet.py::test_the_examples_round_trip_with_no_differences` |
| A table, map or profile from a CSV, TSV or text file (`.csv`, `.tsv`, `.txt`) or an Excel workbook (`.xlsx`, `.xlsm`), units in the header or a row of units | yes | — | the app (**Import from file…** next to a table), `lightsim-backend import-table` | `backend/tests/test_table_import.py::test_a_20x30_motor_loss_map_from_excel_with_axes_and_units` |
| Drive cycle of your own from CSV or Excel (`.xlsx`): speed and grade against time or distance, units in the header | yes | — | the app (*Drive Cycle → Import a cycle from a file…*), kept in the project file ([spec](spec/project.md#drive-cycle-of-the-projects-own)) | `backend/tests/test_own_cycles.py::test_import_reads_time_speed_and_grade` |
| The 27 standard drive cycles (WLTC, NEDC, EPA's city, highway, US06, SC03, LA92 and New York cycles, the WMTC and EPA motorcycle cycles, a long-haul route), each with its source | yes | — | the app (a Driving Task's *Drive Cycle*) | `backend/tests/test_cycles.py::test_the_library_has_twenty_cycles_each_with_a_source_and_a_register_row` |
| Jupyter notebook (`.ipynb`) that runs a project and plots its results | — | yes | `lightsim notebook` | `backend/tests/test_cli.py::test_notebook_is_valid_json` |
| MCP settings of AI apps (JSON; TOML for Codex): one `lightsim` entry added and taken out again, the rest of the file left as it was | yes | yes | the app (**Connect AI**), `lightsim-backend mcp install` | `backend/tests/test_ai_install.py::test_install_then_uninstall_leaves_other_settings_alone` |

## Partly there

| Format | What works | What is missing |
|---|---|---|
| FMU (`.fmu`): Co-Simulation FMUs of FMI 2.0 and 3.0, read; no FMU is written | The *FMU* block runs them with the rest of the model; CI runs the Modelica Association's Reference FMUs against FMPy's own results (`backend/tests/test_fmu.py::test_reference_fmu_matches_fmpy`) | The FMU pack (FMPy) is not in the installer yet, because it needs a licence decision ([Known issues](KNOWN-LIMITS.md)); Model Exchange FMUs do not run |
| A lap from a data logger or a lap simulator (CSV, read by *Import lap*, and by *Calibrate lap* with lateral acceleration), in the layouts Generic, GPS logger, MoTeC i2, AiM Race Studio, OpenLAP and TUM laptime-simulation | The Generic, MoTeC i2, OpenLAP and TUM layouts, read from files made up in those layouts (`backend/tests/test_laplog.py`; none from the tools themselves is bundled) | A test of the GPS logger and AiM Race Studio layouts; any file written by those tools: all the layouts are LightSim's reading of them, not checked against their files |
| Paste a table from a spreadsheet | The table editor takes a pasted block of cells | A test of the paste |
| The energy-flow chart as an SVG picture, the duty table as CSV, a study's table as CSV | The *Results* and *Studies* views save them | A test that reads the files back |
| Results as Apache Parquet | `Result.to_parquet` and `lightsim run -o x.parquet` in the Python package, when pyarrow is installed (`backend/tests/test_python_api.py::test_parquet_round_trips_through_pyarrow`) | pyarrow is not in LightSim's installer or its CI, so the app cannot write it and the test runs only where pyarrow is installed |

## Planned

For students and small teams:

- Measured signals from a CSV file, plotted beside a run's results (the
  imports above read tables, cycles and laps, but the *Results* tab takes
  no file)
- Charts as SVG (the *Results* tab saves a chart as PNG; only the
  energy-flow chart is saved as SVG)
- GPX tracks (GPS recordings) as drive cycles and road profiles

For carmaker and supplier engineers:

- MDF4 import and export (ASAM measurement data files; a project can carry
  an `.mf4` file as an attachment, but LightSim does not read it)
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
`scripts/licenses/allowed.txt`). FMPy, which runs FMUs, is one (the FMU pack
is not in the installer yet, see above); ONNX Runtime, MCAP and mdflib are
the kind the planned formats would use. Tools under GPL, LGPL or AGPL
licences (asammdf, OpenModelica, OMSimulator) are used only in LightSim's
own tests, or when you install them yourself; they are never in the
installer.
