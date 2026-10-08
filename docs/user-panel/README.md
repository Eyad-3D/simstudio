# LightSim user panel: plan (draft)

> **Draft for the owner (BIZ-35).** A written plan and templates. Nobody
> has been contacted. The owner decides whether to run the panel, who
> runs the sessions, and what participants get in return.

## Why

Review sites, forums and the places where industry engineers talk about
tools could not be reached during the roadmap research (September 2026),
and engineers rarely post in public. A small standing panel of real users
who try every release on their own tasks is the cheapest way to hear the
people who decide whether LightSim is adopted.

## Who: nine seats

| Seats | Who | Why them | Where to find them |
|---|---|---|---|
| 5 | Electric Formula Student teams: two at FSG events, one FSUK, one FSAE, one other (FS Czech Republic, FS East, FS Spain …) | LightSim's first audience; they fix their car's concept in autumn | Teams' public contact pages; competition team lists; university vehicle-engineering chairs |
| 3 | Engineers at a carmaker or supplier doing concept, energy or powertrain work | The paying audience; they rarely speak in public | Alumni of FS teams now in industry; conference contacts; the owner's network |
| 1 | A CAE or IT gatekeeper who approves engineering tools at a company | Decides whether LightSim can be installed at all | Through one of the industry engineers |

Each seat is one person who stays for at least three releases. Replace
anyone who leaves. Aim for at least eight seats filled within two months
of starting.

## What happens each release

1. **Invite** each panellist to a 45-minute video session within two weeks
   of a release (the [session guide](session-guide.md)).
2. **Three tasks**, chosen from their own work and timed: for example, a
   75 m acceleration test on their car, reading net kWh for a lap trace,
   installing on a managed PC. The guide lists tasks per seat type.
3. **Score**: time to finish each task, whether they finished without
   help, and the System Usability Scale (SUS: ten standard questions,
   scored 0 to 100), as UX-35 asks.
4. **The gatekeeper** goes through the [IT approval checklist](it-gatekeeper-checklist.md)
   once, then re-checks what changed; ask for their company's real
   questionnaire, with names removed.
5. **Log** every finding in [`findings-log.csv`](findings-log.csv) against
   a roadmap id (a new id if none fits), the same week.
6. **Report**: a one-page summary per release (template at the end of the
   session guide) that the owner reads before choosing the next release's
   work.

Target: at least five sessions per release.

## Between releases

- **Help → What Stopped You?…** in the desktop app opens the public Idea
  form with the version filled in. Nothing is sent unless the person
  submits it. Panellists and anyone else can use it at any time.
- When GitHub Discussions is turned on (BIZ-09), point the menu item at a
  feedback category there instead.

## Rules

- **Consent first.** Every panellist signs the [consent form](consent.md)
  before the first session. No recording without it.
- **Their data stays theirs.** Panellists may use their own cars and
  data. Nothing they show leaves the session notes without their written
  permission; the log names seat types (FS-1, IND-2), never people or
  companies.
- **Licence.** FS and university panellists use the free licence. For
  industry panellists, panel work is evaluation; give them a written
  evaluation permission until EULA 1.1's evaluation clause (proposal §4)
  is in force.
- **No promises.** Say that a finding is logged, not that it will be
  built.

## Timeline (proposed)

| When | What |
|---|---|
| Week 0 | Owner approves this plan, the thank-you and the consent form |
| Weeks 1–4 | Invite FS teams first (they decide concepts in autumn), then industry |
| Weeks 4–8 | First sessions on the current release; first summary |
| Every release | Sessions within two weeks; summary; log reviewed in planning |

## Owner decisions

- Run the panel or not, and who runs the sessions (the owner, or someone
  the owner names).
- The thank-you for panellists: for example a free commercial licence
  for a year for industry seats, a credit in the release notes, or a
  small sponsorship for FS teams.
- The contact address invitations are sent from (EULA proposal, D1).
- Whether sessions may be recorded at all.

## Files

- [`recruiting.md`](recruiting.md): invitations for each seat type and the
  screening questions.
- [`consent.md`](consent.md): the consent form.
- [`session-guide.md`](session-guide.md): the 45-minute script, the tasks,
  the timing sheet, SUS and the release summary.
- [`it-gatekeeper-checklist.md`](it-gatekeeper-checklist.md): what the
  gatekeeper checks.
- [`findings-log.csv`](findings-log.csv): one row per finding.
