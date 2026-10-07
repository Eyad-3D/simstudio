# Import a lap from a data logger or lap simulator

If your team already has a lap's speed, from the car's data logger or from
your own lap simulator, LightSim can drive that speed as a drive cycle and
tell you the energy, battery and motor loads it takes. LightSim does not
work out the cornering speed here: the speed comes from your file. (A
*Lap* case works the speed out itself, see
[Score the Formula Student events](score-fs-events.md).)

## Import the lap

1. Open your car. On the *Simulations* tab of the ribbon, click
   **Import lap**.
2. In **Layout**, pick the tool that wrote the file: *Generic*, *GPS
   logger*, *MoTeC i2 CSV*, *AiM Race Studio CSV*, *OpenLAP* or *TUM
   laptime-simulation*. The layouts of these tools are LightSim's reading
   of them and have not yet been checked against teams' files.
3. Click **Choose file** and pick the CSV file. The file stays on your
   computer.
4. Check the columns LightSim found for **Time**, **Distance**, **Speed**
   and **Lap number**, and the **Speed unit**. Pick another column if one
   is wrong.
5. If the file holds a session of several laps, **Lap** takes the fastest
   full lap (not the first and last, the out and in laps); pick another
   if you like.
6. Look at the speed chart and the time and distance under it, and read
   any warnings (gaps, spikes, rows left out).
7. Optionally tick **Repeat to a 22 km endurance**: the lap is repeated
   to about 22 km, with a stop for the driver change at half distance
   (180 s unless you change it).
8. Name the case and click **Add as case**.

LightSim adds a *Cycle* case that drives the lap with a step of 0.1 s.
If your car has no *Driving Task*, one named *Imported lap* is added and
wired to the Driver. **Undo** takes the import back. The *Messages* panel
records the source file's SHA-256 fingerprint, so you can tell later which
file a case came from.

## Good to know

- A trace against distance (most lap simulators write one) is turned into
  time from its speed: t = ∫ ds / v. Its distance then matches the
  file's own within 0.5 %.
- The file is read once; the case keeps the speed trace, not the file.
- A logged speed is often noisy: wheel spin or a lost GPS fix shows as a
  spike. LightSim warns about a speed change faster than 2.5 g, but does
  not smooth the trace.
- See [Known issues](../../KNOWN-LIMITS.md) for what the import does not do
  yet.
