# Open a project that has scripts from someone else

A Script block holds Python code (a short program) that runs while the
model simulates. Code from another person can do things you did not
expect, so LightSim shows it to you and asks before it runs.

1. Open or import the project (**Open**, or **Import** on the Home tab).
   If it has scripts you have not approved on this computer, LightSim shows
   *This project contains … you have not approved*, with each script's
   name and code.
2. Read the code. Choose **Run scripts** if you trust where the project
   came from. Choose **Open without running scripts** to look at the
   project first: you can read and change everything, but it does not run.
3. When you press **Run** on a project whose scripts are not approved yet,
   LightSim asks again (**Run scripts** or **Don't run**).

## What counts as approved

- The scripts of the examples that come with LightSim, and the code a new
  Script block starts with.
- Code you type or paste into a Script block's code yourself.
- Code you approved with **Run scripts**. LightSim remembers that exact
  code: change one character, or open a project where someone changed it,
  and it asks again.
- The scripts in the projects you had saved before your first start of
  LightSim 0.3.0: those you wrote yourself. This is done once, and only
  for your own projects folder, not one your organisation put on a shared
  drive.

Approvals are kept in the hidden file `.script-trust.json` in your own
LightSim folder, not in the projects folder: `%APPDATA%\LightSim` on
Windows, `~/Library/Application Support/LightSim` on macOS and
`~/.config/LightSim` on Linux. Delete it to be asked again about every
script, including those in the projects you had saved before 0.3.0.

## Good to know

- Your organisation can make LightSim ask every time it opens a project
  (the dialog then says *Managed by your organisation*); approvals then
  last until LightSim closes. See [Install LightSim for a lab or a company](deploy-for-it.md).
- Data Checks never run a script: they only check that it is valid.
- Approving a script does not unlock it: it still runs in a locked-down
  process. What that process may do depends on your system, see the
  [Script API](../reference/script-api.md) and
  [Known issues](../../KNOWN-LIMITS.md).
