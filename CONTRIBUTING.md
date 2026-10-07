# Helping LightSim

Thank you for wanting to help. LightSim is proprietary software (the owner
keeps all rights to it) that is free for non-commercial use. Its source
code is public **for reference only** ([`LICENSE`](LICENSE)): you may read
it, but the licence does not let you copy, change or redistribute it. That
shapes how you can help.

## Ways to help, and where they go

| You want to … | Do this |
|---|---|
| Report a bug | Open an issue with the **Bug report** form. Say what you did, what you expected and what you saw; attach the project file if you can share it. |
| Suggest an idea or an improvement | Open an issue with the **Idea** form. Say what you are trying to do; that matters more than the solution you have in mind. |
| Ask a question | Open an issue for now. A discussion forum is planned (roadmap BIZ-09). |
| Report a security problem | **Not in a public issue.** Follow [`SECURITY.md`](SECURITY.md). |
| Ask about a licence or buying one | See [How to ask about a licence](docs/licensing/licence-faq.md#how-to-ask-about-a-licence). Licence questions will get a private address. |
| Share a model or template | Attach the project file (`.json`) to an issue, or describe it in an Idea. Your models are yours (EULA §4). A gallery of shared models, under the Creative Commons Attribution licence CC BY 4.0 (free reuse with credit), is planned (roadmap BIZ-20). |
| Point out a wrong number or missing data | Open a Bug report; name the source of the right value (a datasheet, a regulation, a measurement). Say what licence the source is under: see [`docs/DATA-REGISTER.md`](docs/DATA-REGISTER.md) for the licences LightSim can ship. |
| Translate the app | Not yet: translations will need a short translator agreement first. |

## Code contributions

**Pull requests with code are not accepted unless you have signed
LightSim's Contributor Licence Agreement (CLA).** A CLA is a contract in
which you, the contributor, keep the copyright to your work but give the
owner the right to use it and to license it to others, including
commercially. Without that right, every outside line of code would weaken
the owner's ability to sell licences.

- The CLA is still a draft ([`docs/licensing/CLA-draft.md`](docs/licensing/CLA-draft.md));
  until it is final, outside code cannot be accepted.
- A Developer Certificate of Origin (DCO, a sign-off line in each commit)
  is not enough: it confirms you may submit the code under the project's
  licence, but grants the owner no right to license it commercially.
- A CI check (`.github/workflows/cla.yml`) marks pull requests from
  people who are not listed in `.github/CLA-SIGNATORIES.txt`, so none is
  merged by mistake.
- Small fixes are often quicker as an issue: describe the change and the
  owner can make it.

## Before you report

- Check [Known issues](docs/KNOWN-LIMITS.md): your problem may already be
  there, with what to do about it.
- Say which version you use (*Help → About*) and your system (Windows or
  Linux, and its version).
- Remove anything private from project files before you attach them.
