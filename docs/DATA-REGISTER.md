# Data licence register

Drive cycles, vehicle parameters and component maps can carry their own
licences, separate from SimStudio's code. One wrongly licensed file could force
a takedown of a release. [`data-register.csv`](data-register.csv) records,
for every dataset in the repository, where it came from, its licence, the
credit it requires and whether the installer ships it.

## Columns

| Column | Meaning |
| --- | --- |
| `id` | Stable row id (`DR-nn`). Other rows refer to it. |
| `file` | Repository path of the file that holds the data. |
| `dataset` | `*` for the file as a whole. Inside the component catalogue it is `<component>.<parameter>`; inside an example project it is `<element>.<parameter>`. Every map, curve and drive or grade profile has its own row. |
| `kind` | `example-project`, `component-defaults`, `drive-cycle`, `grade-profile`, `map`, `curve`, `table`, `generated-copy` or `test-fixture`. |
| `source` | Where the numbers come from: a URL or document, `Synthetic / created for SimStudio` (only when the history shows it), or `Provenance unknown`. |
| `history` | What git history and the research notes say about the source. |
| `licence`, `credit` | The licence the data is under and the credit text it requires. |
| `ships_in_installer` | `yes` or `no`. The engine bundle carries `backend/projects/` and the component catalogue (`backend/simstudio-backend.spec`). The UI bundle inlines `frontend/src/data/` as its offline fallback. |
| `notes` | Caveats and open actions. |

## Status (2026-09-23)

- The register has 21 rows. No dataset from a third party is known to be
  bundled.
- **Every shipped map, curve, profile and default value has unknown
  provenance.** All of them except the fuel density (a textbook value added
  in `4af7f9c`) first appear in the root commit of the main history
  (`064301a` "Second version", 2026-07-07), which imported the whole app with
  no notes on where the numbers came from. Their licence and credit are
  therefore recorded as unknown.
- The two drive cycles ("City Cycle" and "Mixed Cycle") are hand-typed
  trapezoids of 9 round-number points. They are not regulatory cycles, so
  there is no WLTC or EPA licence question yet.
- Only the generated files have a known origin: the golden test fixtures (the
  solver's own output) and the UI's synced copies of the catalogue and the BEV
  example.
- **Open action:** the owner should confirm that these values were written for
  SimStudio. Each row can then say `Synthetic / created for SimStudio` and
  name the SimStudio LICENSE. Any value that was taken from somewhere else
  needs its source recorded instead, or it should be replaced (see CON-03).

## Adding or changing data

1. Add or update the row in the same change as the data. CI enforces this
   (see below).
2. Regulatory cycles, such as UNECE GTR 15 WLTC or the EPA schedules: re-type
   them from the regulation's published tables and cite the regulation and
   the table. Do not copy them from EUPL-licensed code such as JRC `wltp`.
3. FASTSim cycles and vehicles are Apache-2.0: ship their licence and NOTICE
   text and give the credit it asks for.
4. The FASTSim vehicle database has no licence file. Do not bundle it. Fetch
   it only when the user asks, with credit, until NREL (now NLR) confirms the
   terms.
5. US government data, such as fueleconomy.gov, counts as public domain only
   after the terms of that specific source have been checked.
6. Any row with a credit text must also appear on the app's About or credits
   screen. There is no such screen yet, because nothing needs credit today.

Rules 2–5 come from the roadmap research of September 2026 (`open-source-repos`
§3.1, not kept in this repository). Check them again when the data is added.

## The check

`backend/tests/test_data_register.py` runs with the backend tests in CI. It
fails when:

- a tracked data file (`.json`, `.csv`, `.yaml`, `.mat`, `.xlsx`, `.parquet`
  and similar) under `backend/`, `frontend/src/`, `frontend/public/` or
  `desktop/src/` has no row. These are the trees the app is built from.
  Tooling elsewhere in the repository, such as the SBOM, the licence lists in
  `scripts/licenses/` or the CI workflows, is not data and is not scanned.
  Inside those trees, `package.json` and `tsconfig.json` files are exempt. If
  data ever ships from another folder, add it to `DATA_ROOTS` in the test.
- a map, curve or profile in the component catalogue's defaults or in an
  example project's parameters has no row.
- a row is missing its source, licence or credit, or points at a file or
  dataset that no longer exists.
- `ships_in_installer` does not match what the packaging bundles.

The check reads the files git tracks, so `git add` a new data file before you
run it. Projects you save while developing (the engine saves into
`backend/projects/` in development) are not checked until you add them.
