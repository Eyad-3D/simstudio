# Turn update checks on or off

LightSim contacts nothing outside your computer until you allow it to
check for updates.

The first time you open LightSim after installing it, it asks *Check for
updates once a day?*

- **Yes**: once a day LightSim asks GitHub, where its downloads are,
  whether a newer version exists. It sends only its version number and
  platform (for example *LightSim/0.3.0 (win32-x64)*). GitHub also sees
  your computer's internet address, as with any download.
- **No**: LightSim never checks.
- **Ask later**: LightSim asks again the next time it opens.

To change your answer, choose **Help → Updates → Check for Updates Once a
Day** in the desktop app. **Help → Updates → Check for Updates Now** checks
once, whatever the setting.

## When a new version exists

LightSim shows what the new version changes in your results (the *Your
results will change* part of its [release notes](../../RELEASE-NOTES.md))
and offers:

- **Install on quit**: downloads it now and installs it when you close
  LightSim. Your projects stay where they are.
- **Release notes**: opens the version's page in your browser.
- **Skip this version**: no more reminders about this version.
- **Later**: asks again at the next check.

LightSim never installs an update without asking. A new version reaches
users in steps, so it can reach you a few days after it is published.

## Installs that cannot update themselves

- The Linux **.deb** package, unsigned Windows builds and the Windows
  **MSI** say that a new version exists and offer its download page
  instead (**Download page**).
- MSI installs check only when your organisation turns checks on. If
  your organisation manages the setting, the menu shows *(managed by your
  organisation)* and you cannot change it. See
  [Install LightSim for a lab or a company](deploy-for-it.md).
