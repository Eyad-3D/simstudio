# Keyboard shortcuts

On a Mac, use Cmd where this page says Ctrl.

## Anywhere

| Keys | What they do |
|---|---|
| F1 | Open this help beside your work: the page of the parameter you are in, the selected part or the panel you are working in, or the front page. In a part's parameter window: that part's page, at the parameter you are in, in the Help window |
| Ctrl+Enter | Run the active case (not while you type in a script) |
| Ctrl+S | Save the project |
| Ctrl+Z | Undo (not while you type in a field) |
| Ctrl+Y or Ctrl+Shift+Z | Redo |
| Tab, Shift+Tab | Move between buttons, fields and lists |

## On the diagram

These keys work while the diagram has the keyboard: once you click it (a
part, a wire or the empty diagram), and while the pointer is over it. They
do nothing while you type in a field, and a click on another panel gives
the keyboard to that panel. **Delete** on the *Home* tab deletes the same
selection as the Delete key.

| Keys | What they do |
|---|---|
| Delete or Backspace | Delete the selected parts and wires; one Ctrl+Z brings them all back |
| Ctrl+A | Select every part on the diagram |
| Ctrl+C | Copy the selected parts |
| Ctrl+X | Cut the selected parts: copy them, then delete them |
| Ctrl+V | Paste them: where the pointer is, when it is over the diagram |
| Ctrl+D | Duplicate the selected parts |
| F2 | Rename the selected part |
| Enter | Go to the selected part's *Properties*, with the keyboard in its *Name* |
| . (full stop) | Zoom to the selected parts, or to the whole model when none is selected |
| Ctrl+click or Shift+click a part | Add it to the selection, or take it out |
| Shift+drag on empty diagram | Draw a box to select the parts in it |
| Shift+click a pin | Move the pin to the next side of its part |
| Shift+drag a pin | Slide the pin along the part's edges |
| Double-click a part | Open its parameters in a window of their own (a sub-system: go inside); again, bring that window to the front |
| Esc | Cancel click-to-place; close a menu, or a help card, unit chooser or import preview in a parameter window |
| Enter on a table's header (a map's top-left cell) | Choose the unit of that column (of the map's values) ([Work in other units](../how-to/work-in-other-units.md)) |

A part's parameter window is an ordinary window: resize it, minimise it or
move it to another screen, and keep several open. What you type there is in
the project at once (there is no Apply), and Ctrl+Z, Ctrl+Y and Ctrl+S work
there as in the main window. Close it with its own close button; it also
closes when its part is deleted or another project is opened. Its **Help**
button, and F1, open the part's help page in the Help window, a window of
its own that every part's Help reuses; Alt+Left and Alt+Right go back and
forward there in the desktop app.

A part with many parameters (the battery, the Vehicle, a Wheel) shows them
under headings; its rarely used ones sit in the last heading, folded until
you open it, and the window remembers what you opened or folded. Type in
**Search parameters** at the top to see only the parameters whose name or
help holds those words, and tick **Changed only** to see only the values
that are not the library's. A small dot beside a value marks one you
changed: click it to put the library's value back (Ctrl+Z undoes that). When
the active case runs with a value of its own, a line under the parameter
says so, in the unit shown; the field above it is still the part's own
value, and a click on the line shows the case's value in *Cases &
Parameters*.

A right-click on a part opens its menu: *Parameters*, *Rename…*,
*Signals…*, *Duplicate*, *Copy*, *Cut* and *Delete*. On the empty diagram it
offers *Paste*, *Select all* and *Fit view*. In the *Elements* list, Delete
and F2 work on the part you picked there. The **Zoom** list in the diagram's
toolbar sets the zoom (Fit, 50 %, 100 %, 200 %); with it focused, the up
and down arrows open its list.

## On a chart

The charts on the *Results* page and in the *Signal Plot* panel. Rest the
pointer on a chart to read the values there, in the legend under it (in
the *Signal Plot*, next to the channel list).

Each y axis fits the data in view, with 5 % to spare. Above the *Results*
chart, **Axes** starts a unit's axis at 0 or sets its ends (empty is
automatic; the button counts the axes set), and the **Time · auto** list
reads the time in s, min or h (automatic: s up to an hour, min up to 3
hours) or plots the run against the distance driven, in m below 1 km and
km above.

Each case keeps what you chose for its *Results* page: the ticked
channels, the view (chart, table, X-Y or sweep), the axes and the zoom of
the chart, the X-Y view's X channel, the sweep's figure and the baseline.
They stay for its next runs and when LightSim is opened again.

| Mouse or keys | What they do |
|---|---|
| Mouse wheel | Zoom in or out around the pointer (on the X-Y view, both axes) |
| Drag | Zoom in on the box you draw |
| Shift+drag | Move the view |
| Double-click | Show the whole run again |
| + and − | Zoom in and out (click the chart or move to it with Tab first) |
| Left and right arrows | Move the view by a tenth of its width |
| 0 | Show the whole run again |
| C (*Results* chart) | Measurement cursors A and B on or off (not while you type in a field) |
| Drag a cursor line (*Results* chart) | Move that cursor (the pointer shows ↔ near a line) |

In the A and B time fields under the chart, the up and down arrows move
the cursor to the next or the previous stored point, Page Up and Page Down
ten points; Enter or Tab puts it at the time you typed (the nearest stored
point). [How to measure between two times](../how-to/measure-between-two-times.md).

## In the component library

| Keys | What they do |
|---|---|
| Up and down arrows | Move between the parts in the list |
| Enter or Space | Add the part in the middle of the diagram |
| Double-click | Add the part in the middle of the diagram |
| Click, then click the diagram | Place the part where you click (Esc cancels) |

## In a table (maps and profiles)

| Keys | What they do |
|---|---|
| Arrows | Move between cells; with Shift, select a block of cells |
| Enter or F2 | Edit the cell; while editing, Enter keeps the value and moves down |
| Tab, Shift+Tab | Move right or left; while editing, Tab keeps the value and moves right |
| Type a character | Start editing the cell with it |
| Delete or Backspace | Clear the selected cells |
| Esc | Cancel the edit |
| Ctrl+Enter (while editing) | Put the typed value in every selected cell |
| Shift+F10 or the Menu key | Open the right-click menu of the selected cells, or of a row or column header ([Edit a table or a map](../how-to/edit-a-table.md#change-many-cells-at-once)) |
| Ctrl+A | Select every cell |
| Ctrl+C, Ctrl+V | Copy and paste cells, also from and to a spreadsheet; the **Copy** and **Paste** buttons above the grid do the same ([Edit a table or a map](../how-to/edit-a-table.md)) |

## In a source box of Data Bus Connections

| Keys | What they do |
|---|---|
| Type | Open the list of outputs and narrow it |
| Down arrow | Open the list; move down in it |
| Up arrow | Move up in the list |
| Enter | Link the highlighted output |
| Esc | Close the list |
