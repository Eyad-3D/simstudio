# Start from a template

A template is a pre-wired vehicle model: its parts are connected, its Data
Bus links and cases are set, and a short form asks only for the values that
make it your vehicle. LightSim comes with three: *Electric car, one motor*,
*P2 hybrid car* and *Formula Student electric*.

## Start a project from a template

1. On the **Home** tab, click **Templates**.
2. Pick a template. Its description and its *slots* (which part plays which
   role: Battery, E-Drive 1, Engine, Driveline, Chassis and so on) are
   listed with it.
3. Fill in the form: a project name and the values it asks for. A value
   you leave as it is keeps the template's.
4. Click **Create project**. The new project opens unsaved; **Save** keeps
   it. It records which template and version it came from.

## Make your own template

Any model can become a template, for example an exercise a lecturer hands
out or a team's standard vehicle.

1. Open the model and click **Templates**, then **Save this model as a
   template**.
2. Give it a name and a sentence on what it is for.
3. Under *Slots*, say which part plays which role (optional).
4. Under *Values the form asks*, tick the values a new project should ask
   for. Each keeps the model's value as its default and the part's limits.
5. Click **Save as template**. It is listed with the built-in ones.

Your templates are files in the projects folder, under `templates/`.
Saving a template under the same name again makes a new version of it. The
× next to one of your templates deletes it; the built-in ones stay.

## Good to know

- Swapping the part in a slot for another one while keeping its wiring is
  not built yet ([Known issues](../../KNOWN-LIMITS.md)).
