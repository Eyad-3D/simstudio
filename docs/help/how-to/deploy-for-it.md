# Install LightSim for a lab or a company

This page is for the person who installs LightSim on many computers: a lab
administrator or company IT. It covers installing without questions for all
users, the MSI package, the policy file that fixes settings for everyone,
and what LightSim does on the network.

## Check the download

Each file on the [Releases page](https://github.com/Eyad-3D/simstudio/releases)
has a `.sha256` file next to it with its SHA-256 checksum (a fingerprint of
the file). On Windows, compare it with:

```
certutil -hashfile LightSim-Setup-<version>.exe SHA256
```

Every install holds `sbom.cdx.json`, a list of every component inside it
(a software bill of materials in CycloneDX format), and
`THIRD-PARTY-NOTICES.txt`, in its `resources` folder.

When a release is signed, Windows shows its publisher in the file's
*Properties → Digital Signatures*, and you can allow LightSim by that
publisher name in your antivirus or application control. Allow the whole
install folder: the simulation engine is
`resources\backend\lightsim-backend.exe`, a program of its own that the app
starts. Releases are unsigned until the signing certificate is bought;
check the release notes of your version.

## Windows: the setup program, without questions

Run in an administrator command prompt:

```
LightSim-Setup-<version>.exe /S /allusers
```

`/S` installs silently; `/allusers` installs for every user of the PC into
`C:\Program Files\LightSim`, with shortcuts for everyone. Without
`/allusers` it installs for the current user only, into
`%LOCALAPPDATA%\Programs\LightSim`, and needs no administrator. To pick the
folder, add `/D=D:\Apps\LightSim` as the last option. Run interactively,
the setup program asks *for all users* or *only for me*.

To remove it without questions:

```
"C:\Program Files\LightSim\Uninstall LightSim.exe" /S /allusers
```

Run the same command on each PC from your deployment tool, or in a login
script, to install on 30 PCs at once. A newer setup program installs over
the old version.

## Windows: the MSI package

For Intune, Configuration Manager (SCCM) or Group Policy software
installation, use `LightSim-<version>-x64.msi`. It always installs for all
users, under Program Files.

```
msiexec /i LightSim-<version>-x64.msi /quiet /norestart
msiexec /x LightSim-<version>-x64.msi /quiet /norestart
```

A newer MSI installs over an older one and replaces it (an upgrade), so
deploy each new version the same way. Do not mix the two packages on one
PC: use the setup program or the MSI. An MSI install never updates itself
(see *Updates* below).

## Linux and macOS

- Linux: `sudo apt install ./LightSim-<version>-amd64.deb` installs for
  all users; the AppImage needs no install.
- macOS (Apple silicon): the `.dmg` holds the app; copy it to
  `/Applications`. A Mac build is published only once it is signed and
  notarised by Apple; until then there is none.

## Where LightSim keeps its files

Nothing is written to the install folder. Each user's projects, stored
runs, settings and logs are in their own folder:

| System | Folder |
|---|---|
| Windows | `%APPDATA%\LightSim` (projects in `projects\`) |
| macOS | `~/Library/Application Support/LightSim` |
| Linux | `~/.config/LightSim` |

`main.log` there records the app's events, including the policy file it
read and any keys it ignored.

## What LightSim does on the network

- The simulation engine listens on `127.0.0.1` only (this computer), on a
  port each install picks once, and refuses requests that do not carry a
  secret made afresh at each launch. Other computers cannot reach it.
- LightSim contacts nothing outside the computer unless a user agrees to
  update checks (or your policy file turns them on). It looks up no proxy
  until then.
- Update checks go to `github.com` and, for a download,
  `objects.githubusercontent.com`, once a day. They send only the app's
  version and platform. They use the computer's proxy settings.

## Fix settings for everyone: the policy file

Put a file named `policy.json` in:

| System | Path |
|---|---|
| Windows | `%ProgramData%\LightSim\policy.json` (usually `C:\ProgramData\LightSim\policy.json`) |
| macOS | `/Library/Application Support/LightSim/policy.json` |
| Linux | `/etc/lightsim/policy.json` |

Only administrators can write these folders, so users cannot change the
file. LightSim reads it at each start. A setting it fixes shows as
*managed by your organisation* and cannot be changed in the app. A key
that is missing leaves the setting to each user. A key LightSim does not
know, or a value it does not accept, is ignored and noted in `main.log`.

| Key | Values | What it does |
|---|---|---|
| `updates` | `"off"`, `"notify"`, `"auto"` | `off`: never check. `notify`: check once a day and tell users about a new version, with its download page. `auto`: check once a day and offer to install it when LightSim closes. Users are not asked whether to check. |
| `scriptTrust` | `"prompt"`, `"always-prompt"` | `prompt` (the default): ask once before running scripts from elsewhere and remember the answer. `always-prompt`: approvals last only until LightSim closes. See [Open a project that has scripts from someone else](open-a-project-with-scripts.md). |
| `examples` | `true`, `false` | `false`: the *Start* page and the Open menu offer no examples. |
| `projectsRoots` | a list of folders | The first folder is where projects are saved instead of the user's own folder, for example a network drive. `%USERNAME%`, `$USER` and `~` are filled in for each user. |
| `ai`, `aiProviders`, `licenceFile` | | Accepted and kept for later versions: LightSim has no AI features and no licence files yet, so they change nothing today. |

An example for a teaching lab:

```json
{
  "updates": "off",
  "scriptTrust": "always-prompt",
  "projectsRoots": ["H:\\LightSim\\%USERNAME%"]
}
```

## Updates

- Setup program, AppImage and macOS installs can update themselves once a
  user agrees to update checks, or when the policy says `auto`. LightSim
  always asks before it installs. See
  [Turn update checks on or off](check-for-updates.md).
- MSI installs cannot update themselves: update checks are off unless the
  policy says `notify` or `auto`, and then they only point to the
  download. Deploy each new MSI yourself.
- The `.deb` package only says that a new version exists.
- Each release lists what changes in the results in its
  [release notes](../../RELEASE-NOTES.md). Read it before you roll a new
  version out to a class mid-term.
