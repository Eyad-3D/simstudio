# Data licence register

Drive cycles, vehicle parameters and component maps can carry their own
licences, separate from LightSim's code. One wrongly licensed file could force
a takedown of a release. [`data-register.csv`](data-register.csv) records,
for every dataset in the repository, where it came from, its licence, the
credit it requires, whether the installer ships it and whether the owner has
cleared it for shipping.

## Columns

| Column | Meaning |
| --- | --- |
| `id` | Stable row id (`DR-nn`). Other rows refer to it. |
| `file` | Repository path of the file that holds the data. |
| `dataset` | `*` for the file as a whole. Inside the component catalogue it is `<component>.<parameter>`; inside an example project it is `<element>.<parameter>`, or `<case>/<element>.<parameter>` for a value one case sets (such as a case's own typed drive cycle). Every map, curve and drive or grade profile has its own row; a bundled drive cycle is a file of its own (`backend/app/cycles/`), so its row uses `*`. |
| `kind` | `example-project`, `component-defaults`, `drive-cycle`, `grade-profile`, `map`, `curve`, `table`, `track-layouts`, `template`, `generated-copy` or `test-fixture`. |
| `source` | Where the numbers come from: a URL or document, `Synthetic / created for LightSim` (only when the history shows it), or `Provenance unknown`. |
| `history` | What git history and the research notes say about the source. |
| `licence`, `credit` | The licence the data is under and the credit text it requires. |
| `reuse_basis` | Why LightSim, a paid app, may ship the data, from the list below, one or more separated by `; `: `EU-2011/833` (EU legal texts on EUR-Lex), `US-17USC105` (US Government works), `JP-Art13` (Japanese laws and official notices), `Apache-2.0`, `MIT`, `BSD-3-Clause`, `CC-BY-4.0`, `CDLA-Permissive-2.0` or `OGL-Canada-2.0` (permissive licences), `LightSim-own` (created for LightSim), `Facts` (figures quoted from a public document), `Derived` (made by LightSim from other rows) or `Unknown`. |
| `ships_in_installer` | `yes` or `no`. The engine bundle carries `backend/projects/`, the component catalogue and the drive cycles in `backend/app/cycles/` (`backend/lightsim-backend.spec`). The UI bundle inlines `frontend/src/data/` as its offline fallback. |
| `cleared` | The owner's sign-off that LightSim may ship the data: `yes` (the licence is known and allows it), `no` (it must not ship) or `pending` (not yet confirmed). |
| `notes` | Caveats and open actions. |

## Status (2026-10-07)

- The register has 94 rows.
- **The drive-cycle library has 27 cycles** (CON-04), built by
  `scripts/cycles/build_cycles.py` from official texts where their terms
  allow reuse (CON-31): the WLTC classes 1, 2, 3a and 3b, their city cycles
  and phases from the EU's Regulation 2017/1151 on EUR-Lex, the NEDC from
  UN Regulation No 83 as the EU published it, and the EPA cycles (FTP-75,
  US06, SC03, LA92, New York City, motorcycle FTP) from EPA's schedule
  files. The WMTC motorcycle cycles and the long-haul truck route are
  FASTSim's Apache-2.0 copies. Every cycle has a fingerprint (the sum of its
  1 Hz speeds and the SHA-256 of its file) that `test_cycles.py` recomputes.
- **WLTC class 3b, UDDS and HWFET are now checked against the official
  tables.** The files, first converted from FASTSim's copies, equal the EU
  regulation's tables and EPA's files value for value, so rule 2 holds for
  them and DR-25 now cites the EU text.
- **Third-party data is now bundled.** The example rebuild (CON-02, CON-03)
  took the Battery Electric Car's vehicle values from FASTSim's
  2021_Cupra_Born.csv, calibrated its motor loss map to FASTSim's default
  motor efficiency curve, and took the WLTC class 3b, UDDS and HWFET drive
  cycles from FASTSim's cycle files (all Apache-2.0, rules 2 and 3). The P2
  Hybrid Car's test mass, road load and gearing come from the EPA 2022 Test
  Car List (a US Government work; EPA's own terms were not re-checked, rule
  5). FASTSim's credit and NOTICE are in THIRD-PARTY-NOTICES.txt (Help >
  Third-Party Notices), listed in `scripts/licenses/bundled-data.json`.
- The cycles live in `backend/app/cycles/` (CON-16): one CSV each and
  `cycles.json` for their names, sources, phases and fingerprints (DR-46).
  The examples, the Driving Task's *Drive Cycle* and the Road Profile's
  *Grade From Cycle* name them by id.
- The examples' new engine, motor and battery maps are synthetic, created
  for LightSim in that change; their rows say what they are calibrated to.
- The Race Track's layouts (`backend/app/library/tracks.json`, DR-38) are
  drawn for LightSim after the Formula Student rules' track guidance; no
  official layout, TUM racetrack-database (LGPL-3.0) or OpenStreetMap data
  ships.
- The Formula Student example (`backend/projects/fs-electric.json`, DR-41 to
  DR-45) is synthetic: typical values and maps created for LightSim, not a
  real car's or a product's. Its rule values are facts cited from FS Rules
  2026 v1.1 (FSG) and its reference results facts from the FS Czech Republic
  2025 results; it ships no tyre test data and no track of its own (its lap
  cases use the Race Track's layouts, DR-38).
- The reference suite (`backend/validation/`, DR-61 to DR-66, VAL-05)
  copies individual facts from EPA's 2022 Test Car List, fueleconomy.gov and
  FASTSim's Apache-2.0 vehicle files into small case files. It is test data
  and does not ship. EPA's disclaimers page (re-checked 2026-10-07) allows
  free use "for non-commercial, scientific and educational purposes" and
  says commercial use may be protected; the owner should confirm that this
  fits LightSim's own test suite.
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
  LightSim may ship it.
- **Open actions:**
  - The owner should confirm that the unknown-provenance values were written
    for LightSim. Each row can then say `Synthetic / created for LightSim`,
    name the LightSim LICENSE and become `cleared = yes`. Any value that was
    taken from somewhere else needs its source recorded instead, or it should
    be replaced.
  - The owner should sign off the FASTSim (Apache-2.0), EU and EPA rows.
    Two questions are the owner's: EPA's website disclaimer says that
    "commercial use of the documents available from the EPA websites may be
    protected", although the schedules are US Government works in the Code
    of Federal Regulations (rule 5); and EUR-Lex asks for its source to be
    acknowledged, which the app does with each cycle, but Help >
    Third-Party Notices cannot list it until the licence gate
    (`scripts/licenses/allowed.txt`, BIZ-34) accepts a non-SPDX term for
    Decision 2011/833/EU.
  - Once every shipped row is cleared, make `pending` fail for shipped rows
    in the check (see below), so that later data cannot ship unconfirmed.

## Adding or changing data

1. Add or update the row in the same change as the data. CI enforces this
   (see below).
   If the row ships, run `python -m app.sources` from `backend/`: it copies
   the shipped rows to `backend/app/library/sources.json`, which each run's
   *Sources & credits* reads (VAL-37); `test_sources.py` fails until you do.
2. Take data only from a source whose terms allow reuse inside a paid app,
   and record that basis in `reuse_basis`. For drive cycles, in this order
   (CON-31):
   1. the EU's copy of the regulation on EUR-Lex: Commission Regulation (EU)
      2017/1151, Annex XXI, Sub-Annex 1 for the WLTC, and the UN Regulations
      the EU publishes in its Official Journal, such as Regulation No 83 for
      the NEDC (OJ L 42, 15.2.2012). EUR-Lex allows reuse "for commercial or
      non-commercial purposes" under Decision 2011/833/EU if the source is
      acknowledged and changes are noted: credit "Source: EUR-Lex, ©
      European Union" and say what was changed ("converted to a CSV in
      km/h");
   2. US federal texts and EPA's schedule files, US Government works (cite
      the CFR, which EPA calls the official source; see rule 5);
   3. official notices the law leaves free of copyright, such as Japan's
      (Copyright Act Article 13: JC08 and the Japanese WLTC);
   4. Apache-2.0 or MIT copies, such as FASTSim's, as the shipped file or as
      a cross-check.

   Re-type a cycle from its table, or check a copy against the table value
   for value (`scripts/cycles/build_cycles.py` does both), and cite the
   regulation and the table. Vehicle data may come from OpenEV Data
   (CDLA-Permissive-2.0), the EEA's CO₂ monitoring data (CC-BY-4.0),
   fueleconomy.gov and NRCan's ratings (Open Government Licence – Canada),
   with credit.
3. Never take data from these, not even to check a value by hand:
   - the UNECE website (its terms forbid reuse without written permission;
     use the EU's copy of the same regulation instead);
   - ev-database.org (prior permission and manual copying only) and
     evspecifications.com (scraping forbidden);
   - the gaia-charge/evdb database (CC BY-SA 4.0, share-alike) and the
     fastsim-vehicles database (no licence; rule 4);
   - EUPL, GPL or LGPL files: VECTO's missions, JRC `wltp`, the TUM
     racetrack database, OpenLAP;
   - standards sold by their publisher, such as China's CLTC (GB/T
     38146.1-2019), and traces without clear terms, such as Artemis. Users
     may import these themselves (CON-34); LightSim does not ship them.

   `test_data_register.py` fails when a row's source names one of these.
   FASTSim: read its licence from its LICENSE file, not the PyPI classifier
   (fastsim 3.1.0 says "Other/Proprietary" while its LICENSE is
   Apache-2.0), keep the 2.1.5 NOTICE holder (Alliance for Sustainable
   Energy, LLC) for data from 2.1.5 and use the new holder (Alliance for
   Energy Innovation, LLC) for data from 3.x. FASTSim cycles and vehicles
   are Apache-2.0: ship their licence and NOTICE text and give the credit it
   asks for.
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
7. Never change a shipped drive-cycle file in place. A stored run and a
   project record only the cycle's id, so a corrected or resampled cycle gets
   a new id, file and row, and the old one stays.

8. Data may ship only under a licence in
   [`scripts/licenses/data-allowed.txt`](../scripts/licenses/data-allowed.txt):
   public-domain dedications and open data licences that ask only for credit
   (CC0, CC BY 4.0, CDLA-Permissive, the UK and Canadian Open Government
   Licences), permissive code licences (Apache-2.0, MIT, BSD-3-Clause),
   reuse rights written into law (US federal works, EU reuse under
   Decision 2011/833/EU, Japanese official texts), LightSim's own data, and
   single facts quoted with their source. Never bundle data under a
   non-commercial (NC) or no-derivatives (ND) licence, a share-alike licence
   (CC BY-SA, ODbL), GPL, LGPL, AGPL or EUPL, with no licence, or with only
   a permission on request: such sources may be offered for users to fetch
   or import themselves. List the row's licence terms in
   [`scripts/licenses/data-licences.json`](../scripts/licenses/data-licences.json).
9. Before data from a new outside source reaches a pull request, add the
   source to
   [`scripts/licenses/model-sources.json`](../scripts/licenses/model-sources.json)
   with its licence and one of three classes: **BUNDLE** (may ship, with
   credit), **USER-IMPORT** (never bundled; the user fetches or imports it
   on their own terms, such as VECTO missions under EUPL or BPX cell sets
   under CC BY-SA) or **LEARN** (read for the method, copy nothing, such as
   OpenLAP under GPL).

Rules 2–5 come from the roadmap research of September 2026 (`content` and
`open-source-repos` §3.1, not kept in this repository); the EUR-Lex terms
were read on its legal notice page on 2026-10-07. Check them again when the
data is added, and before each release check whether the fastsim-vehicles
database has gained a licence file.

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
- a shipped row has no licence terms in `scripts/licenses/data-licences.json`,
  or a term that `scripts/licenses/data-allowed.txt` does not list (rule 8).
  Non-commercial, no-derivatives, share-alike and copyleft terms, no licence
  and permission-on-request fail even if someone adds them to that list, and
  so does a `licence` column that names one in words. `LicenseRef-Unknown`
  (provenance not recorded) passes only for the rows recorded before this
  check existed (`GRANDFATHERED_UNKNOWN` in the test, a list that may only
  shrink) and only while they are `pending`; the owner's sign-off replaces
  it with the real term. A new row cannot ship with an unknown licence.
- a shipped row whose licence asks for credit (CC BY, OGL, Apache-2.0, MIT,
  BSD, EU reuse) has no credit text.
- a model source in `scripts/licenses/model-sources.json` has no class, a
  BUNDLE source has a licence outside the allow-list, or a shipped row's
  `source` names a USER-IMPORT or LEARN source (rule 9).

The check reads the files git tracks, so `git add` a new data file before you
run it. Projects you save while developing go to `backend/dev-projects/`,
which is not shipped and not checked.

Data written into code, such as a cycle typed as a Python or TypeScript array,
is not detected. Add its row by hand, with the source file as `file`.
