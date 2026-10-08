# Keep a project in a folder of your own

By default LightSim saves your projects in its own projects folder. You can
also keep a project as a `.lightsim` file in any folder: a git repository
your team shares, a course folder, a network drive. This needs the desktop
app: in a web browser, use **Export** and **Import** instead.

## Save a project as a file

1. Open the project.
2. On the **Home** tab, click **Save As…** (or press Ctrl+Shift+S, or choose
   **File → Save As…**).
3. Pick a folder and a name, and click **Save**.

The project is now that file. **Save** (Ctrl+S) writes to it from then on,
and the *Project* tab shows where it is under *Saved in*. If the project
was already saved somewhere else, the new file is a copy with an id of its
own, and the runs stay with the original.

## Open a project file

- Choose **File → Open…** (Ctrl+O), or **Home → Open → Open file…**, and
  pick the file.
- Or double-click the `.lightsim` file in your file manager (Windows, and
  Linux with the .deb package).
- Or drag the file onto the LightSim window.

**Home → Open** and the *Start* page list the files you opened lately under
*Recent files*. The cross next to a file takes it off the list; the file
itself stays where it is, and if the project is open, **Save** still
writes to it.

## What is next to the file

LightSim keeps a project's other files next to it, in folders named after
it:

| Folder | What is in it | Put it in git? |
|---|---|---|
| `car.lightsim-resources` | Files attached to the project (see below) | Yes |
| `car.lightsim-runs` | Runs and parameter studies | No: it holds its own `.gitignore` |
| `car.lightsim-backups` | Earlier versions, for **Project → Restore…** | No: it holds its own `.gitignore` |

Running a case or a sweep never changes the `.lightsim` file itself, so git
shows only the changes you made to the model.

## When the file changes on disk

LightSim looks at the open file every few seconds. If it changed (a
`git pull`, or a save from another window), LightSim asks whether to
reload it. With unsaved changes of your own, *Keep my changes* keeps them;
**Save** then asks before it replaces the version on disk.

## Attach files to a project

Some models need other files: a model from another tool (an FMU), an AI
model (an ONNX file) or measured data. On the **Project** tab, click
**Attached**, then **Attach a file…**. LightSim copies the file into the
project's resources folder and lists it with the project. **Export** then
saves a `.lightsim.zip` that holds the project and its attached files, and
**Import** opens such a zip on another computer.

The list marks a file that is missing from the folder or has changed since
you attached it, and Data Checks report it too.

## Good to know

- A project file from a newer LightSim opens read-only: install that
  version, or a newer one, to edit it or save a copy (**Save As…** is off
  for it, so nothing the newer version wrote is lost). A file from an older LightSim is
  upgraded when it opens, and the first save keeps the old file in
  **Project → Restore…**'s folder as `pre-migration-v1.json`.
- A project that carries code (Script blocks, attached FMUs or AI models)
  asks once, before its first run, whether you trust it. Run only projects
  from people you trust.
- See [Known issues](../../KNOWN-LIMITS.md) for what does not work yet.
