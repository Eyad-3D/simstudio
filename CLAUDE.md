# Notes for Claude Code

## Pick a model for each piece of delegated work

When you hand work to a subagent, a workflow agent or a new session, choose
its model for that task instead of letting it inherit yours. The Agent tool
and a workflow's `agent()` take a model option per call; a new session takes
a model when it is created.

Before each hand-off, judge whether the mid-tier model (one step below the
most capable one, not the smallest) can do the task well:

- **Mid-tier model**: well-defined work with a clear test of success.
  - Running tests, lint and type checks, and fixing failures whose cause is
    plain.
  - Dependency bumps and lockfile regeneration.
  - Mechanical edits across files: renames, updated counts or quoted
    numbers, regenerated files.
  - Searching and summarising code, logs or docs.
  - Docs and release-note text written from facts you hand it.
- **Most capable model**: work where a mistake is costly or that needs
  judgement.
  - Design decisions and features that span several parts of the app.
  - The solver's physics, numerics and energy accounting, where results
    must stay correct to the last digit.
  - Security-sensitive code: the script sandbox, AI access, file trust and
    the IT policy file.
  - Root-causing failures that are not obvious.
  - Reviewing other agents' work before it is merged, and resolving merge
    conflicts in logic.

When unsure, give the task to the mid-tier model only if its result will be
checked before it lands (by tests, or by a review on the most capable
model); otherwise use the most capable model. If a mid-tier agent comes back
stuck, or its work fails review, hand the task to the most capable model
rather than retrying on the same one.
