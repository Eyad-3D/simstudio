# Build an electric car from scratch

In this tutorial you build the smallest electric car LightSim can drive:
seven parts, four wires and three signals. You learn how parts, ports and
signals fit together. It takes about fifteen minutes.

Do [Your first run](first-run.md) first if you have not used LightSim yet.

## 1. Start a blank project

On the *Home* tab, click **New**: the *Start* page opens. Click **Blank
project**. If LightSim asks whether to save the open project, choose
**Save** or **Don't save**. The diagram is now empty; its **Add a part**
button takes you to the library search.

## 2. Add the parts

The *Components* panel on the left is the library. To add a part, type its
name in **Search components…**, then double-click it in the list, or move
to it with the arrow keys and press Enter: it lands on the diagram. You can
also drag it to where you want it. Add these seven:

| Part | What it is here |
|---|---|
| **Vehicle** | The car's body: its mass and its air and rolling resistance |
| **Driving Task** | The speed the car must follow over time |
| **Driver** | Presses the accelerator and brake to follow that speed |
| **HV Battery Pack** | The battery |
| **E-Motor** | The electric motor and its inverter |
| **Final Drive** | A fixed gear between the motor and the wheel |
| **Wheel** | One driven wheel that stands for the axle |

Each new part is named after its type and a number, such as *E-Motor 1*.
Drag the parts so that the battery, motor, final drive and wheel sit in a
row: it makes the wiring easier.

## 3. Wire the power path

Each part has ports, the small dots on its edges. A wire joins two ports of
the same domain: electrical to electrical, mechanical to mechanical. Drag
from one port to the other to wire them; hover over a port to see its name.

1. *HV Battery Pack 1*'s **Positive Terminal (+)** to *E-Motor 1*'s
   **Positive Terminal (+)**.
2. The battery's **Negative Terminal (−)** to the motor's **Negative
   Terminal (−)**.
3. The motor's **Mechanical Shaft** to *Final Drive 1*'s **Flange In**.
4. The final drive's **Flange Out** to *Wheel 1*'s **Mechanical Shaft**.

The Vehicle needs no wire: the wheels push it along.

## 4. Link the signals

Signals are numbers that parts pass to each other, such as a speed or a
command. They are linked in *Data Bus Connections*, a tab in the panel under
the diagram (click the tab to open the panel). The panel lists every signal
input, one row each, with a box for its source.

For each of these three inputs, click the box in its row and pick the
source from the list (type a few letters to narrow it):

| Input | Source |
|---|---|
| Driver 1 · Target Speed | Driving Task 1 · Target Speed |
| Driver 1 · Actual Speed | Vehicle 1 · Vehicle Speed |
| E-Motor 1 · Traction Command | Driver 1 · Traction Command |

The Driver now compares the target speed with the car's speed and tells the
motor how hard to push, or to brake by running as a generator
(recuperation). Other inputs stay empty; *Unconnected inputs* lists them.

## 5. Fix what Problems reports

*Problems*, another tab under the diagram, lists what LightSim finds wrong
with the model, a moment after each change. Here it gives one warning: the
wheel load shares add up to 25 %, not 100 %. A wheel's *Vehicle Load Share*
is the part of the car's weight it carries; the library's wheel carries a
quarter, for a car with four.

Click the row: LightSim selects *Wheel 1*. In *Properties*, set *Vehicle
Load Share* to 100. The warning goes away.

## 6. Run it

The project has one case, *Case 1*, 600 s long. The Driving Task's default
profile is a 540 s drive in town at up to 80 km/h. Press **Run**.

The run ends as *success*: 7.292 km driven, about 11.0 kWh/100 km, close to
the Battery Electric Car on its City Cycle. Your car has the library's
default values; set them to a real car's to model it (each part's page in
the [component reference](../reference/components/index.md) lists its
parameters with their units and defaults).

## Next

- [Pick a drive cycle](../how-to/pick-a-drive-cycle.md) instead of the
  default profile.
- [Wire control signals](../how-to/wire-control-signals.md) says more about
  the *Data Bus Connections* panel.
- **Save** keeps the project; [restore a version](../how-to/restore-a-version.md)
  if an edit goes wrong.
