# Cite the data behind a run

Every run can list the data and methods its numbers rest on: the drive
cycle, the example car's values and maps, the library's default values and
the values typed into the project, each with its licence, the credit it
asks for and how far it can be trusted. A thesis or report can then cite
them, and LightSim itself, in one step.

## See a run's sources

1. Run a case, and click **Show results** in the notice it ends with.
2. Click the **i** button (*Run info*) next to the run list.
3. Click **Sources & credits** near the bottom of *Run info*.

Each line gives the source's row in LightSim's
[data register](../../DATA-REGISTER.md) (DR-nn), what it is, how far it can
be trusted and which parameters use it:

- **source unknown**: most of the library's default values; nobody has
  recorded where they come from yet. When a run uses any of these, a line
  in orange says so.
- **known source, not validated**: published data (FASTSim, EPA), values
  created for LightSim, or values you typed (*own*).
- **validated**: checked against measurements of the same thing. Nothing
  in the library is yet.

The methods and regulations behind the data follow, such as the regulation
that defines the drive cycle or the Formula Student rules a track is drawn
after. *Credits the data asks for* lists the sentences the data's licences
require you to repeat when you publish results made with it.

## Save the citations

Click **BibTeX** to save a `.bib` file for LaTeX, or **CSL-JSON** for
Zotero or Pandoc. The first entry cites LightSim with its version; the
others cite each source and method. A source's author is the organisation
behind it (EPA, the European Union, NREL for FASTSim's values, or the
LightSim authors for values made for LightSim); its full source text,
licence and credit are in the note.

## What it cannot tell

LightSim recognises an example's values by the part they belong to and by
their value: a value you copied from an example into a part of your own, or
changed and changed back, counts as your own. It lists what the model
uses, not how much each value moves the results.
