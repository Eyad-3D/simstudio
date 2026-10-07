# Engine API

LightSim's window talks to its engine, a local program on your computer,
over HTTP and one WebSocket. You can read this API, but the engine is not
a server for other programs: it answers only on 127.0.0.1, only requests
from its own window, and in the desktop app every `/api` call needs a
token that changes each time the app starts. To run models from your own
scripts, use the `lightsim` [Python package](../help/reference/python-api.md)
or [command-line tool](../help/reference/command-line.md) instead: they run
the same engine inside your own process.

## The API reference page

The engine serves a reference of every route and data model at `/docs`
on its own address: press **F1** in LightSim, and in the browser page
that opens replace everything after the port number (`/help/…`) with
`/docs`. It is built from the engine's
OpenAPI description and needs no internet connection. The
machine-readable description is at `/openapi.json`.

## Routes

| Method & path | Purpose |
|---|---|
| `GET /api/health` | The engine's version; answered without the token |
| `GET /api/library` | The part library: `components` ([schema](schemas/library.schema.json)) and `unitGroups` (quantity → unit) |
| `GET /api/cycles`, `GET /api/cycles/{id}` | The bundled drive cycles / one of them with its trace (`t` in s, `v` in km/h) |
| `GET /api/projects` | Your saved projects: id, name, description, when saved, number of parts |
| `GET /api/projects/{id}` | A [project](project.md), plus `revision` (also the `ETag`) |
| `PUT /api/projects/{id}` | Save a project. `If-Match: "<revision>"` refuses (409) if the file changed since; `If-None-Match: *` refuses to replace one |
| `DELETE /api/projects/{id}` | Delete a project and its runs |
| `GET /api/projects/{id}/backups`, `…/backups/{backup}` | Earlier versions kept by saves, newest first / one of them |
| `GET /api/projects/{id}/runs` | The stored runs (the [index](results.md#the-runs-folder) entries), newest first |
| `GET/PUT/DELETE /api/projects/{id}/runs/{run}` | One [stored run](results.md#stored-run) (sent gzip-encoded when the client accepts it) |
| `DELETE /api/projects/{id}/runs` | Delete all of a project's runs |
| `GET /api/examples`, `GET /api/examples/{id}` | The examples (with `hidden`) / one example, read-only |
| `POST /api/examples/{id}/hide`, `POST /api/examples/restore` | Hide an example from the Open menu / show all again |
| `POST /api/validate` | Body `{project}`: the Data Checks' findings ([schema](schemas/data-checks.schema.json)) |
| `POST /api/simulate` | Body `{project, caseId}`: check, then run the case and return its [result](results.md#result) |
| `WS /api/simulate/run` | A live run, below |

## The live run (WebSocket)

`/api/simulate/run` runs a case while streaming its progress, and takes
changes to values while it runs. Every message is a JSON object with a
`type`.

**From the window to the engine**

| `type` | Fields | When |
|---|---|---|
| `start` | `project` (a [project](project.md)), `caseId` | First, once |
| `set_param` | `elementId`, `key`, `value` (a number, true/false or text) | Any time during the run: change a part's value from the next solver step. Values marked `"variability": "fixed"` in the library apply only to the next run |
| `cancel` | — | Stop the run; it ends *cancelled* |

Closing the connection also stops the run.

**From the engine to the window**

| `type` | Fields | When |
|---|---|---|
| `step` | `t` (s), `pct` (0 to 100, how far the run is), `values` (`"elementId:portId"` → value, for the channels that have data at this point) | Each stored point |
| `message` | `level` (`info`, `warning`, `error`), `text` | When the run reports something |
| `done` | `result` (a [result](results.md#result)) | Last; the engine then closes the connection. If the Data Checks find an error, `done` comes at once with a *failed* result whose messages name the errors |
| `error` | `detail` | The first message was not a valid `start`; the engine closes the connection |

Example:

```
→ {"type": "start", "project": {…}, "caseId": "case-city"}
← {"type": "step", "t": 0.0, "pct": 0.0, "values": {"el-battery:sig_soc": 90.0, …}}
→ {"type": "set_param", "elementId": "el-driver", "key": "driver_kp", "value": 0.8}
← {"type": "message", "level": "info", "text": "…"}
← {"type": "done", "result": {"caseId": "case-city", "status": "success", …}}
```
