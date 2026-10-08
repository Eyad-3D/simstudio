# Use an AI assistant with LightSim

There are two ways to get help from an AI assistant (a chatbot such as
Claude, ChatGPT, Copilot or Gemini) with your model. Neither sends anything
from LightSim by itself.

## Paste a summary into any chatbot

1. Open your project and run the case you want to ask about.
2. On the *Project* tab, click **Copy for AI**. LightSim puts a short
   summary on the clipboard (under 8 KB): the parts and how they are
   wired, every value you changed from the library defaults, the cases,
   the last run's key results with any *not valid* notes, the Data Check
   messages, and a line saying the results are not validated.
3. Paste it into the chatbot, then ask your question.

To keep your numbers private, tick **Hide values** before you click:
every number in values and results becomes `[hidden]`.

*Messages* says what was copied. The button works without any setup and
with no internet connection: you decide what you paste, and where.

## Connect an assistant on this computer

An AI app installed on this computer (Claude Desktop, Claude Code, VS Code
with GitHub Copilot, the GitHub Copilot CLI, OpenAI Codex, Gemini CLI or
Cursor) can work with LightSim directly: list your projects, outline a
model, run its Data Checks and cases, and read and compare the results. It
uses the Model Context Protocol (MCP), the standard way AI apps connect to
tools.

1. On the *Project* tab, click **Connect AI**.
2. Click **Add** next to your AI app. LightSim adds itself to that app's
   settings and keeps a copy of the file as it was
   (`<file>.lightsim-backup`).
3. Restart the AI app. Ask it, for example: "Open the Battery Electric Car
   example in LightSim, run the City Cycle and tell me the final state of
   charge and the consumption."

**Remove** takes LightSim out of the app's settings again. The window also
says when an assistant last used LightSim.

You can do the same in a terminal, with the command the window shows:

```
lightsim-backend mcp install --client claude
lightsim-backend mcp uninstall --client claude
lightsim-backend mcp status
```

`--client` is one of `claude`, `claude-code`, `vscode`, `copilot`,
`codex`, `gemini` or `cursor`. Add `--allow-folder <folder>` to let the
assistant also open project files in that folder, or `--read-only` to
refuse every change.

### What the assistant may do

- It starts LightSim's engine itself and talks to it directly. Nothing
  opens a network port.
- It sees your saved projects and the examples, and the folders you
  allowed. A project file that contains `"noAi": true` stays hidden, and
  so does a link in an allowed folder to a file outside them.
- It can read, check and run models. It can propose changes as a dry run;
  saving one needs your **Allow this** in the AI app. An example is never
  changed: the edit is saved as a new project. LightSim keeps the previous
  version of a project as a backup, as for every save. For a file in a
  folder you allowed, the backups are kept beside it, in a folder named
  after it (`car.json-backups`, which git ignores).
- A project with Script blocks (Python code) runs only after you allow
  it. On Windows, where LightSim's script sandbox is weak, it does not run
  at all unless you added `--trust-scripts` to the connection.
- A run started by an assistant stops after 5 minutes. Its runs are kept
  apart from yours (the newest 20 per project).
- Every request is written to a log on this computer,
  `.ai/audit.jsonl` in your projects folder. It is never uploaded.
- On a computer your organisation manages, its policy file can turn AI
  access off (`"ai": "off"`, see [Install LightSim for a lab or
  a company](deploy-for-it.md)). The window then says so and cannot add
  LightSim to an AI app, and an assistant already connected gets nothing.

An assistant can still misread a result. Check the numbers it reports
against the run in LightSim, and read [Known issues](../../KNOWN-LIMITS.md).

## Teach the assistant LightSim

LightSim comes with a skill pack: short guides, in the open Agent Skills
format, on building a car, wiring rules, units, Script blocks, checking a
model and reading *not valid* results. A connected assistant reads them
from LightSim when it needs them. To use them in an app that loads skills
from a folder, copy them from `resources/backend/_internal/app/ai/skills/`
in LightSim's install folder.
