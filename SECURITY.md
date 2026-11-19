# Security

LightSim runs a local engine on your computer and runs the Python code in
a project's Script blocks. A flaw in either could let a project file or
another program on your computer do harm. Please report such flaws
privately, so they can be fixed before anyone else learns of them.

## How to report

- **Use GitHub's private vulnerability reporting:** the *Security* tab of
  <https://github.com/Eyad-3D/simstudio>, then **Report a vulnerability**
  (<https://github.com/Eyad-3D/simstudio/security/advisories/new>). Only
  the owner sees the report. (DRAFT, owner step before release: turn on
  *Private vulnerability reporting* in the repository's *Settings → Code
  security*; until then the button does not appear and the link does not
  work for anyone else. Owner decision D11 in
  [EULA-proposal.md](docs/licensing/EULA-proposal.md#owner-decisions).)
- If there is no **Report a vulnerability** button, write to [private
  address] (DRAFT: the owner's private contact, decision D1) instead.
- **Do not open a public issue** for a security problem.
- Say what an attacker could do, which version you tested (*Help →
  About*), your system, and the steps or a project file that shows it.

## What happens next

- You get a reply within **5 working days** (DRAFT: the owner sets this
  promise), saying whether the problem is confirmed and what will happen.
- Confirmed problems are fixed in a new release. The release notes name
  the problem once the fix is out, and credit you if you wish.
- Please give us 90 days to release a fix before you publish details.

## What counts

In scope: the desktop app, its local engine and the way it runs Script
blocks (see *Scripts run in a separate, locked-down process* in
[Known issues](docs/KNOWN-LIMITS.md#using-and-installing-the-app) for the
protection each system has today), the installer, and the help it serves.

Not security problems: wrong simulation results (report those as a bug),
and the known limits of the script sandbox that Known issues already
lists, unless you found a way past them.

## Supported versions

Only the latest release gets security fixes. LightSim is at version 0.3
and has no long-term support versions yet.
