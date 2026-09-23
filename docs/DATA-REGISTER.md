# Data licence register

Drive cycles, vehicle parameters and component maps can carry their own
licences, separate from SimStudio's code. One wrongly licensed file could force
a takedown of a release. [`data-register.csv`](data-register.csv) records,
for every dataset in the repository, where it came from, its licence, the
credit it requires, whether the installer ships it and whether the owner has
cleared it for shipping.

## Columns

| Column | Meaning |
| --- | --- |
| `id` | Stable row id (`DR-nn`). Other rows refer to it. |
| `file` | Repository path of the file that holds the data. |
| `dataset` | `*` for the file as a whole. Inside the component catalogue it is `<component>.<parameter>`; inside an example project it is `<element>.<parameter>`, or `<case>/<element>.<parameter>` for a value one case sets (such as a case's own drive cycle). Every map, curve and drive or grade profile has its own row. |
| `kind` | `example-project`, `component-defaults`, `drive-cycle`, `grade-profile`, `map`, `curve`, `table`, `generated-copy` or `test-fixture`. |
| `source` | Where the numbers come from: a URL or document, `Synthetic / created for SimStudio` (only when the history shows it), or `Provenance unknown`. |
| `history` | What git history and the research notes say about the source. |
| `licence`, `credit` | The licence the data is under and the credit text it requires. |
| `ships_in_installer` | `yes` or `no`. The engine bundle carries `backend/projects/` and the component catalogue (`backend/simstudio-backend.spec`). The UI bundle inlines `frontend/src/data/` as its offline fallback. |
| `cleared` | The owner's sign-off that SimStudio may ship the data: `yes` (the licence is known and allows it), `no` (it must not ship) or `pending` (not yet confirmed). |
| `notes` | Caveats and open actions. |

## Status (2026-09-23)

- The register has 26 rows.
- **Third-party data is now bundled.** The Battery Electric Car rebuild
  (CON-03) took its vehicle values from FASTSim's 2021_Cupra_Born.csv,
  calibrated its motor loss map to FASTSim's default motor efficiency curve,
  and took the WLTC class 3b drive cycle from FASTSim's cycle files (all
  Apache-2.0, rules 2 and 3). FASTSim's credit and NOTICE are in
  THIRD-PARTY-NOTICES.txt (Help > Third-Party Notices), listed in
  `scripts/licenses/bundled-data.json`.
- The WLTC trace is FASTSim's copy, converted to km/h and checked against the
  regulation's own figures (duration, distance, top speed, 0.1 km/h grid,
  phase distances). It was not re-typed from the regulation table as rule 2
  asks, because unece.org could not be reached when it was added; re-check it
  against the table when it can.
- The example's new motor maps are synthetic, created for SimStudio in that
  change; their rows say what they are calibrated to.
- **Every other shipped map, curve, profile and default value still has
  unknown provenance.** All of them except the fuel density (a textbook
  value added in `4af7f9c`) first appear in the root commit of the main
  history (`064301a` "Second version", 2026-07-07), which imported the whole
  app with no notes on where the numbers came from. Their licence and credit
  are therefore recorded as unknown. The "City Cycle" and "Mixed Cycle" are
  hand-typed trapezoids of 9 round-number points, not regulatory cycles.
- The generated files have a known origin: the golden test fixtures (the
  solver's own output) and the UI's synced copies of the catalogue and the BEV
  example.
- Every shipped row is `cleared = pending`: the register records that the
  data exists and what its licence is, not yet the owner's sign-off that
  SimStudio may ship it.
- **Open actions:**
  - The owner should confirm that the unknown-provenance values were written
    for SimStudio. Each row can then say `Synthetic / created for SimStudio`,
    name the SimStudio LICENSE and become `cleared = yes`. Any value that was
    taken from somewhere else needs its source recorded instead, or it should
    be replaced.
  - The owner should sign off the FASTSim (Apache-2.0) rows.
  - Once every shipped row is cleared, make `pending` fail for shipped rows
    in the check (see below), so that later data cannot ship unconfirmed.

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
6. Any row with a credit text must also appear on the app's credits screen,
   Help > Third-Party Notices: list its id under its source in
   `scripts/licenses/bundled-data.json`, with the source's NOTICE text, and
   `scripts/third-party-notices.py` writes the credit into
   THIRD-PARTY-NOTICES.txt and the SBOM.

Rules 2–5 come from the roadmap research of September 2026 (`open-source-repos`
§3.1, not kept in this repository). Check them again when the data is added.

## The check

`backend/tests/test_data_register.py` runs with the backend tests in CI. It
fails when:

- a tracked data file (`.json`, `.csv`, `.tsv`, `.txt`, `.yaml`, `.mat`,
  `.xlsx`, `.parquet` and similar; `.txt` because EPA publishes its driving
  schedules as text tables) under `backend/`, `frontend/src/`,
  `frontend/public/` or `desktop/src/` has no row. These are the trees the app
  is built from. Tooling elsewhere in the repository, such as the SBOM, the
  licence lists in `scripts/licenses/` or the CI workflows, is not data and is
  not scanned. Inside those trees, `package.json`, `tsconfig.json` and
  `requirements*.txt` files are exempt. If data ever ships from another
  folder, add it to `DATA_ROOTS` in the test.
- a map, curve or profile in the component catalogue's defaults or in an
  example project's parameters, its elements' or its cases', has no row.
- a shipped row with a credit text of its own (not "None ..." or "Same as
  DR-nn") is not listed in `scripts/licenses/bundled-data.json`, or that file
  lists a row that does not exist or credits nothing.
- a row is missing its source, licence or credit, or points at a file or
  dataset that no longer exists.
- `ships_in_installer` does not match what the packaging bundles.
- a shipped row is `cleared = no`, or a `cleared = yes` row has an unknown
  licence. Shipped rows that are still `pending` are listed as a warning in
  every test run.

The check reads the files git tracks, so `git add` a new data file before you
run it. Projects you save while developing (the engine saves into
`backend/projects/` in development) are not checked until you add them.

Data written into code, such as a cycle typed as a Python or TypeScript array,
is not detected. Add its row by hand, with the source file as `file`.
