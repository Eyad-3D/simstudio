# Record where a value comes from

Every number and table of a part can say where it comes from, what kind of
source that is and how sure you are of it. The examples come with their
values' sources filled in, taken from the [data register](../../DATA-REGISTER.md).

## See the sources

1. Click a part on the diagram.
2. In *Properties*, a value with a recorded source has a small tag after
   its name, such as *datasheet 1*: the kind of source and the confidence.
   Point at the tag to read the source.
3. Under the parameters table, **Value sources** says how many of the
   part's values have a source and how many still hold the library's
   default. Click it to list them.

## Record or change one

1. Open **Value sources** under the part's parameters.
2. Choose the value in **Record a source for…**.
3. Type the source: a document, a web address or a test, with its date.
4. Choose its kind and the confidence, then click **Save source**.

To forget a source, click the × after it. **Undo** takes back either step.

## Kinds and confidence

| Kind | Meaning |
|---|---|
| measured | From your own test or log. |
| datasheet | From a published document: a datasheet, a certificate, a regulation, a public database such as EPA's. |
| estimated | Worked out, fitted or chosen by someone. |
| generated | Made by a model or a tool, such as a synthetic map. |
| library default | The value the part had when it was added. |

The confidence follows ADVISOR's scale: 0 not checked, 1 agrees with its
source, 2 source and method checked.

## Good to know

- The sources are saved in the project file, so they travel with it.
- After a run with nothing to report, *Data Checks* says how many of the
  model's values still hold their library default with no source recorded.
- An uncertainty (± or a spread) for each value is not there yet
  ([Known issues](../../KNOWN-LIMITS.md)).
