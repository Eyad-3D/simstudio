# Restore an earlier version of a project

Every time you save a project over an earlier save, LightSim keeps the
version it replaces: the last 20 of each project. You can open any of them
again.

1. Open the project and click the **Project** tab at the top.
2. Click **Restore…**. The list shows the earlier versions, newest first,
   with the time each was saved, its size and its number of parts.
3. Click the version you want. If the open project has unsaved changes,
   LightSim asks whether to save them first.

The version opens as an unsaved copy named after it, such as *Battery
Electric Car (version of 28/09/2026, 14:05:10)*. The project on disk is not
changed. To keep the old version, **Save** the copy: it becomes a new
project of its own, next to the one it came from. Rename it on the
*Project* tab, in **Project name**, if you like.

## Good to know

- An example has no earlier versions until you save your copy of it.
- **Restore…** needs the engine: it is greyed out when Messages says the
  backend is not reachable.
- Versions are files in the hidden `.backups` folder of your projects
  folder (**File → Open Projects Folder** in the desktop app).
- To undo an edit you just made, use **Undo** (Ctrl+Z) instead.
