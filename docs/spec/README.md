# LightSim file formats and API

This specification describes the files LightSim reads and writes and the
engine's API, so that you can read LightSim projects and results with your
own scripts, convert them to other tools, and keep using your data if
LightSim ever goes away. Scripts, the `lightsim` Python package, the
command-line tool and AI tools rely on these contracts.

> **Draft licence, waiting for the owner's decision.** This specification
> and the JSON Schemas in [`schemas/`](schemas/project.schema.json) are
> intended to be published under the Creative Commons Attribution 4.0
> International licence (CC BY 4.0,
> https://creativecommons.org/licenses/by/4.0/). Until the owner confirms
> it, the notice below is a draft. Only the formats are opened: the
> LightSim app itself stays proprietary under its [EULA](../../EULA.txt).

**Draft notice.** You may read, write and convert files in the formats
described here with any software of your own or of others, including
software you sell, without asking. Writing such a reader or converter is
not a derivative work of LightSim for the purpose of its EULA. Attribute
this specification as "LightSim file formats and API, version 1, by Eyad
Abualkhair, CC BY 4.0".

## What is in it

| Page | Covers |
|---|---|
| [Project file](project.md) | A project: its parts, wires, signal links, cases and studies |
| [Runs and results](results.md) | The runs folder, a stored run, a result, the summary keys, and the CSV, MAT and JSON exports |
| [Engine API](api.md) | The HTTP API, the live-run WebSocket messages and the offline API page |
| [Format changelog](CHANGELOG.md) | What changed in each version of each format |

The machine-readable [JSON Schemas](schemas/project.schema.json)
(draft 2020-12) are generated from the engine's own data models, so they
cannot drift from what the app reads and writes:

| Schema | Describes |
|---|---|
| `project.schema.json` | a project file |
| `run.schema.json` | a stored run (`runs/<project id>/<run id>.json.gz`, after unzipping) |
| `result.schema.json` | a simulation result (`SimResult`) |
| `study.schema.json` | one parameter study inside a project |
| `library.schema.json` | the part library (`GET /api/library`, `components`) |
| `data-checks.schema.json` | the Data Checks' findings (`POST /api/validate`) |
| `lightsim-result.schema.json` | the JSON that `lightsim run -o result.json` writes |

`python -m lightsim schema --out docs/spec/schemas` writes them again; a
test fails when the committed copies are out of date, and another checks
the example projects and their results against them.

## Versions

Every format has a version number, in each schema's `$id`
(`urn:lightsim:schema:project:1`) and, for a project, in its
`schemaVersion` field. The rules:

- **Adding** an optional field keeps the version. Readers must ignore
  fields they do not know, and LightSim keeps unknown fields of a project
  when it saves it.
- **Changing or removing** a field, or changing its unit or meaning, raises
  the version. LightSim then reads the older version and converts it (its
  migrations); the [changelog](CHANGELOG.md) says what changed.
- A **summary key** ([results](results.md#summary-keys)) never changes its
  meaning. A figure that is computed differently gets a new key.

The project file is about to change: work on the file format (named
`.lightsim` files, versioned migrations) is under way. Until it lands, a
project is the JSON file described in [Project file](project.md), and
`schemaVersion` is 1.

## Conventions

- Files are UTF-8 JSON. Names of fields are camelCase.
- Numbers are in the unit the library gives the parameter (shown in the
  app next to the field, and in `library.schema.json`'s `unit`). Units
  are SI with a few everyday ones: kW, kWh, km/h, 1/min (rpm), %, °C.
- Ids (of parts, wires, cases, runs) are opaque text: compare them, do not
  parse them. Labels are what the user typed; they are not unique and may
  change.
- Time is in seconds from the start of the run; a time stamp in a file
  (`startedAt`) is milliseconds since 1 January 1970, UTC.
