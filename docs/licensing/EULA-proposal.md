# EULA 1.1: proposed changes (draft)

> **Draft for the owner. Not in force.** `EULA.txt` (version 1.0, shipped
> with 0.2.0 and 0.3.0) is the licence that applies today. This page
> proposes a version 1.1 and explains every change. It is **not legal
> advice**: it was written by the roadmap team, not by a lawyer. Have a
> lawyer review the final text before it ships. Items with
> **[Decision D-n]** need the owner's choice; they are collected in
> [Owner decisions](#owner-decisions).

Roadmap items: BIZ-29 (define "non-commercial") and BIZ-30 (close the gaps
that block labs, company IT, trials, automation and buying). The
plain-language [licence FAQ](licence-faq.md) answers everyday cases under
both versions.

## Why change it

EULA 1.0 is short and clear about its main idea: free for non-commercial
use, a paid licence for business use. But it leaves out the people LightSim
wants most:

| Who | What stops them in EULA 1.0 |
|---|---|
| A lecturer with a computer lab | §1 allows "your own computers" only. Lab PCs belong to the university. §3 bans handing out the installer to a class. |
| A Formula Student team with sponsors | "Non-commercial" is not defined. A sponsor's money can look like business. |
| A student writing a thesis at a company | Is a thesis at a company "paid work" (§2)? It does not say. |
| A PhD student on an industry-funded project | "Research" is free, but contract research for a company is a grey zone. |
| Company IT | §3 bans all redistribution, so IT cannot put the installer on an internal software portal. |
| An engineer who wants to try it | There is no trial. Any use at work is commercial use from the first minute. |
| Anyone who automates | Scripts, CI jobs and AI agents are not mentioned. |
| An engineer sent a LightSim model | Opening it at work is commercial use, so they cannot even look. |
| A purchasing department | §2 says "open an issue" on GitHub: a public page that shows who is buying. |
| Anyone who installed a version | §9 says future versions may have other terms, but not that the version you have keeps its terms. |

The model for the new wording is the
[PolyForm Noncommercial License 1.0.0](https://github.com/polyformproject/polyform-licenses/blob/1.0.0/PolyForm-Noncommercial-1.0.0.md)
(PolyForm: a family of standard source-available licences written by
lawyers) and, for the trial, the
[PolyForm Free Trial License 1.0.0](https://github.com/polyformproject/polyform-licenses/blob/1.0.0/PolyForm-Free-Trial-1.0.0.md).
Quotes from them are marked.

## The changes at a glance

| # | Section | EULA 1.0 says | 1.1 proposes | Why | Item |
|---|---|---|---|---|---|
| 1 | §1 Definitions (new) | "evaluation, learning, research and other non-commercial purposes", undefined | Defines non-commercial use: personal study and research, use by schools, universities, public research bodies, charities and government "regardless of the source of funding", student competition teams (sponsored or not), and students' theses | Students and universities can be sure; companies cannot call paid work "research" | BIZ-29 |
| 2 | §1 Definitions (new) | "in the course of a business or for paid work" | Lists commercial use: work for or inside a company (start-ups before revenue too), paid or contract work, consulting, paid training courses | Companies know when they must pay | BIZ-29 |
| 3 | §2 Free licence | "on any number of your own computers" | Also on computers an institution manages (labs, pools), and copies of the unchanged installer for students and colleagues | Lab PCs and class hand-outs | BIZ-30 (1) |
| 4 | §7 Copies | "You may not … redistribute the Software" | Unchanged copies inside one organisation (mirrors, software portals, managed installs) and package-manager entries that fetch the official installer | Company IT and winget/Homebrew | BIZ-30 (2) |
| 5 | §6 Automation (new) | nothing | Non-commercial use includes scripts, command line, Python, MCP (a protocol that lets AI assistants use tools), CI. A commercial licence covers one named person plus the scripts, CI jobs and AI agents acting for that person; a shared server needs a server licence | Automation is covered either way | BIZ-30 (3) |
| 6 | §4 Evaluation (new) | nothing; "evaluation" is listed as free with no limit | A company team may evaluate for up to 30 consecutive days, not for production work; longer by written agreement | Engineers can try before buying; "evaluation" cannot run for ever | BIZ-30 (4) |
| 7 | §5 Viewing (new) | nothing | Anyone may open, inspect and export results from models they receive, free | Reviewers, customers and suppliers can look at shared work | BIZ-30 (5), BIZ-33 |
| 8 | §13 Your version (was §9) | "Future versions … may come with different terms" | Each version keeps the terms it shipped with; new terms apply only to a version you choose to install | No one loses rights by a change of terms | BIZ-30 (6) |
| 9 | §8 Your work (was §4) | Your models and results belong to you | Adds: any LightSim code or data inside an exported file may be passed on with that file, royalty-free | Exports can be shared without a licence question | BIZ-30 (7), BIZ-05 |
| 10 | §3 Commercial licence (was §2) | "open an issue at https://github.com/Eyad-3D/simstudio" | A private e-mail address or form | Buyers need not ask in public | BIZ-30 (8) |
| 11 | §16 Law and contact (new) | nothing | Governing law, courts and a contact address | Purchasing and legal departments ask for both | BIZ-30 (9) |
| 12 | §14 Breaking the terms (was §9) | "This licence ends automatically if you break these terms" | First breach: 32 days to put it right before the licence ends (PolyForm wording) | A fair chance to fix an honest mistake, which institutions' lawyers look for | BIZ-29 |
| 13 | §15 Rights the law gives (new) | nothing | "These terms do not limit" fair use and other rights the law gives (PolyForm wording) | Replaces "except where the law allows it" with a clear statement | BIZ-29 |

Sections 9 to 12 of the new text (results, warranty, liability,
third-party components) keep EULA 1.0's wording, with small edits.

## Proposed text: EULA 1.1

The text below is meant to replace `EULA.txt` once the owner and a lawyer
have approved it. Square brackets mark the owner's decisions.

```text
LightSim End-User Licence Agreement, version 1.1

Copyright (c) 2026 Eyad Abualkhair. All rights reserved.

By installing or using LightSim ("the Software") you agree to these terms.
"We" and "us" means the copyright holder. "You" means the person who
installs or uses the Software and, where you act for an organisation, that
organisation. The Software includes the desktop application, its
simulation engine and any command-line, scripting or automation interface
we release with it.

1. Definitions.
   (a) Non-commercial use is any of these:
       (i)   personal use for study, research, experiment and testing for
             the benefit of public knowledge, personal learning and hobby
             projects, without any anticipated commercial application;
       (ii)  use by a school, university or other educational institution,
             a public research organisation, a charity or a government
             body, for its teaching, research or public mission,
             regardless of the source of funding or obligations resulting
             from the funding;
       (iii) use by a team of students taking part in a student
             engineering competition, such as Formula Student or Formula
             SAE, including teams that have sponsors;
       (iv)  a student's own coursework, project or thesis that an
             educational institution supervises and assesses, [including
             a thesis written at a company, for the thesis work itself]
             [Decision D4].
   (b) Commercial use is any use that is not non-commercial use. It
       includes use by or for a company or other business (also before it
       has revenue), work you are paid for or do under contract,
       consulting, and teaching a course that participants pay for.
   (c) An organisation is a company, institution or other legal entity,
       together with the people working for it.

2. Free licence for non-commercial use. You may install and use the
   Software free of charge for non-commercial use, on any number of
   computers that you own or manage. An institution named in 1(a)(ii) may
   install the Software on computers it manages, such as teaching labs and
   shared workstations, for its students and staff. You may give copies of
   the unchanged official installer, with its published checksum, to
   students and colleagues for their non-commercial use.

3. Commercial licence. Commercial use needs a commercial licence from us,
   except as section 4 (evaluation) and section 5 (viewing) allow. Unless
   it says otherwise, a commercial licence covers one named person, and
   the scripts, scheduled jobs, continuous-integration jobs and AI agents
   that run the Software on that person's behalf. Running the Software as
   a service that several people use, such as a shared server, needs a
   server licence. To ask for a licence, write to [private address]
   [Decision D1].

4. Evaluation. An organisation may use the Software for up to [30]
   [Decision D2] consecutive days per team to decide whether it suits its
   work. It may not use results from the evaluation in its products,
   deliverables or paid work. We may agree a longer evaluation in writing.

5. Viewing. Anyone may, free of charge, use the Software to open and
   inspect models and projects that someone else made, view and export
   their stored results, [and run them unchanged to check them]
   [Decision D5]. Creating or changing models for commercial use needs a
   commercial licence.

6. Automation. You may run the Software through its command-line,
   scripting and automation interfaces, from your own scripts, from
   continuous-integration jobs and from AI agents. Such use is
   non-commercial or commercial use by the same rules as any other use.

7. Copies. You may make unchanged copies of the official installer:
   (a) as section 2 allows;
   (b) inside one organisation, such as on an internal mirror, a software
       portal or a managed deployment, for people in that organisation
       whom these terms or the organisation's licence cover; and
   (c) in public package-manager entries that download the official
       installer, unchanged, from our release page.
   Otherwise you may not sell, rent, sublicense or distribute the
   Software, or remove its copyright notices. Except where the law allows
   it, you may not modify, decompile or make derivative works of the
   Software.

8. Your work. Models, projects, results and exports you create with the
   Software belong to you. You may publish them, including results made
   under the free licence. Any part of the Software that an exported file
   contains may be copied and used together with that file, free of
   charge.

9. Simulation results. The Software's physical models are simplified and
   have not been validated against measured data. Do not rely on its
   results for safety-critical, regulatory or financial decisions without
   your own independent verification.

10. No warranty. As far as the law allows, the Software comes as is,
    without any warranty or condition of any kind, express or implied,
    including fitness for a particular purpose and non-infringement.

11. Limitation of liability. As far as the law allows, we are not liable
    for any damages arising from these terms or from the use of, or the
    inability to use, the Software, under any kind of legal claim.

12. Third-party components. Components listed in the third-party notices
    shipped with the Software remain under their own licences, which
    govern those components.

13. Your version keeps its terms. Each version of the Software comes with
    the terms shipped with it. You may keep using a version you installed
    under those terms. A later version may come with different terms;
    they apply to that version only if you choose to install it.

14. Breaking these terms. The first time we tell you in writing that you
    have broken these terms, your licence continues if, within 32 days of
    our notice, you comply with these terms and take practical steps to
    put right any past breach. Otherwise your licence ends.

15. Rights the law gives you. You may have rights under the law, such as
    fair use or the right to make a back-up copy. These terms do not limit
    them.

16. Law and contact. These terms are governed by the law of [country]
    [Decision D6], and the courts of [place] have jurisdiction. Questions
    about these terms: [private address] [Decision D1].
```

### Where the wording comes from

| 1.1 wording | Source |
|---|---|
| 1(a)(i) "for study, research, experiment and testing for the benefit of public knowledge … without any anticipated commercial application" | PolyForm Noncommercial, *Personal Uses*: "Personal use for research, experiment, and testing for the benefit of public knowledge, personal study, private entertainment, hobby projects, amateur pursuits, or religious observance, without any anticipated commercial application" |
| 1(a)(ii) "regardless of the source of funding or obligations resulting from the funding" | PolyForm Noncommercial, *Noncommercial Organizations* (verbatim) |
| 1(a)(iii), (iv) | New, for LightSim's users (voices research §4: FS teams and theses are everyday cases) |
| 4 "decide whether it suits its work" | PolyForm Free Trial: "Use to evaluate whether the software suits a particular application for less than 32 consecutive calendar days" |
| 3 named person plus agents; server licence | New; compare MathWorks' MCP server, which "must not be shared by multiple users" |
| 10, 11 "As far as the law allows … under any kind of legal claim" | PolyForm, *No Liability* |
| 14 the 32-day cure | PolyForm, *Violations* |
| 15 | PolyForm, *Fair Use*: "These terms do not limit them" |

### Things this draft deliberately does not change

- **The main idea.** Free for non-commercial use, paid for business use.
- **The ban on modifying and decompiling** (now in §7): LICENSE keeps the
  source code "for reference only".
- **No key, registration or usage statistics for free use.** EULA 1.0 asks
  for none, and 1.1 adds none. If the owner wants a licence file for
  commercial users (BIZ-05), that is a separate change.
- **Prices and the start-up licence** are not terms of the EULA. They
  belong on a price page (BIZ-24) and in the commercial licence (BIZ-31).
  §1(b) makes pre-revenue start-ups commercial; a start-up licence would
  then be their free or cheap path.

## Plain-words summary for the installer (draft)

The installer's licence page (`desktop/electron-builder.yml`,
`nsis.license`) shows the whole EULA. Proposed short text above it, or as
the first lines of `EULA.txt`:

```text
In plain words (the full terms below decide):
- Free for non-commercial use: students, Formula Student and FSAE teams
  (sponsored too), schools, universities and public research, whoever
  funds them, and hobby projects.
- Universities may install it on lab PCs and give the installer to
  students. Company IT may copy it to internal software portals.
- Use at or for a company needs a commercial licence, after a free
  evaluation of up to 30 days. Anyone may open and view models sent to
  them.
- Your models and results are yours. The version you install keeps its
  terms.
- Licence questions: [private address].
```

## Owner decisions

| # | Decision | Recommendation | Where it appears |
|---|---|---|---|
| D1 | A private contact for licence enquiries (an e-mail address or a form) | A dedicated address such as licensing@ on LightSim's own domain, so it can move to a team later | EULA §3 and §16; FAQ; SECURITY.md and the issue templates point to it |
| D2 | Evaluation length | 30 days per team, as BIZ-30 proposes (PolyForm Free Trial uses "less than 32") | EULA §4 |
| D3 | Whether to adopt the definitions in §1 as written | Yes, after a lawyer's review | EULA §1, FAQ |
| D4 | A student's thesis written at a company: non-commercial for the thesis work itself? | Yes for the thesis, if a university supervises and assesses it; the company using the results in its products needs a licence | EULA §1(a)(iv), FAQ |
| D5 | May the free Viewer re-run a received model unchanged? | Yes (BIZ-33): reviewers need to check results | EULA §5 |
| D6 | Governing law and courts | The owner's home country | EULA §16 |
| D7 | Start-up path: a start-up licence (BIZ-31), its limits (people, revenue, age) and price | Free or token-priced for up to 10 people, under US$1 million revenue and 3 years old | Commercial licence, price page; FAQ |
| D8 | Prices: Professional, Team, Site, Server (BIZ-24) | Publish them before 1.0 | Price page; FAQ |
| D9 | Whether EULA 1.1 ships with 0.3.0 or a later release | With the first public release, so the public never sees 1.0's gaps | Release notes, installer |
| D10 | The "facts" basis for shipped data (`LicenseRef-Facts` in `scripts/licenses/data-allowed.txt`) | Keep: rule values and published figures are quoted as single facts with their source | Data licence gate (BIZ-34) |
| D11 | Turn on GitHub's private vulnerability reporting for the repository (*Settings → Code security*). It is off today, so SECURITY.md's only private route does not work for outside reporters | Turn it on before the first public release, and name the D1 address in SECURITY.md as the fallback | `SECURITY.md`, `.github/ISSUE_TEMPLATE/config.yml` |

## When the owner approves

1. Replace `EULA.txt` with the approved text; add the plain-words summary
   at its top.
2. Move the [licence FAQ](licence-faq.md) from draft to published: drop
   its "EULA 1.0 today" column, add it to the help (one line in
   `frontend/scripts/build-docs.mjs`), and link it from the README's
   *License* section, the About box (`desktop/src/main.js`) and the
   installer's summary.
3. Put the private address (D1) into `CONTRIBUTING.md`, `SECURITY.md`,
   `.github/ISSUE_TEMPLATE/config.yml` and the FAQ, replacing the
   `[private address]` placeholders.
4. Update the licence lines in the README, `docs/KNOWN-LIMITS.md` and
   `docs/RELEASE-NOTES.md`, and say in the release notes that the terms
   changed and how.
