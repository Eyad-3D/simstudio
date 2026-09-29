# How LightSim's help is written

One page of rules for the help pages in `docs/help/` and for the other pages
the help is built from (README sections, KNOWN-LIMITS, VALIDATION-STATUS,
RELEASE-NOTES, DATA-REGISTER). `frontend/scripts/build-docs.mjs` turns them
into the help that LightSim serves at `/help/`.

## Plain words

- Write for an engineering student who has never used LightSim. Say what
  the reader does and sees, in the order they do it.
- Short sentences, active voice, "you". One idea a sentence.
- Use the words on the screen, spelled and capitalised as the app shows
  them: **Run**, *Cases*, *Drive Cycle*. Buttons and menus in bold, panel
  and tab names in italics.
- Say where a thing is: "the *Cases* tab on the right", "Home → Open".
- No marketing words (powerful, seamless, simply), no jokes, no "please".
- Numbers with their units and thousands separators: 1,800 s, 23.27 km.

## Explain every term on first use

- The first time a page uses a term that is not everyday English, say what
  it means in a few words, then link it to the [Glossary](help/glossary.md)
  if it is there: "the state of charge (SOC, how full the battery is)".
- Add a term to the glossary when two pages need it.
- Spell out an abbreviation the first time on each page: "Worldwide
  harmonised Light vehicles Test Cycle (WLTC)".

## Units

- SI units first: m, kg, s, N, N·m, W, kW, V, A, Ω, °C, Pa.
- Add the common alternative where readers think in it: km/h (with m/s),
  kWh/100 km, l/100 km, 1/min (rpm), kPa.
- The unit a field shows in the app is the one to write.

## One job a page

- A **tutorial** teaches, start to end, with a result the reader can see.
  It never offers choices.
- A **how-to guide** answers one question ("How do I pick a drive cycle?").
  Title it as the task, number its steps, one action a step.
- **Reference** states facts: what each field is, its unit and default.
  No steps.
- **Theory** explains why the app behaves as it does.

## Check it in the app

- Walk every click path in the built app before the page ships, and again
  when the screens it names change.
- Every number a page quotes (a result, a count, a duration) must come from
  the app or the files, not from memory.
- Link other pages by their file in the repo (`../KNOWN-LIMITS.md`,
  `../how-to/pick-a-drive-cycle.md`); the build turns the links into links
  between help pages and stops on one that goes nowhere.
- Pages that describe what can be wrong say so and link *Known issues*.
