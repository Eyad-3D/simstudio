# Calibrate lap mode on a logged lap

Lap mode's lap times are usually optimistic: it drives an ideal line at
the tyres' limit. If your car has driven a lap with a data logger, you can
fit lap mode's grip and downforce to that lap, then check the result on a
second lap the fit has not seen.

## What the log needs

A CSV file of one lap, or of a session with a lap number column, with:

- time (s) or distance along the lap (m);
- speed (km/h, m/s or mph);
- lateral acceleration (in g, or m/s² when the values go past 5);
- optional: the accumulator's power (kW), for the energy check.

Column names such as *Time*, *Lap Distance*, *Ground Speed*, *G Lat* and
*Pack Power* are found on their own. The files stay on your computer.

## Calibrate and check

1. Open your car. It needs a *Race Track*, which LightSim sets to the
   logged lap's line for the fit (your track's own settings do not
   change).
2. On the *Simulations* tab, click **Calibrate lap**.
3. Pick the lap to fit in **Lap to calibrate on**, and a lap from another
   session in **Lap to check**.
4. Check the **Speed unit** and click **Calibrate**. It takes about 30 s.
5. Read the results:
   - **Best fit**: the factor on every tyre's grip (μ, lateral μ and load
     sensitivity together) and the downforce area CzA (m²);
   - for each lap, the logged and the predicted lap time, the time error,
     the speed trace's RMS error (km/h, the typical gap between the two
     speed traces) and the energy error when the log has the power.
6. Click **Apply to the model** to put the fitted grip and CzA in your
   car (one undo), or **Close** to leave it as it is.

## How it works

LightSim builds the track from the logged lap: the curvature at each
metre is the lateral acceleration divided by the speed squared, smoothed
over 3 m. It then tries grip factors from 0.6 to 1.5 and CzA from 0 to
5 m², and narrows the search twice around the best, keeping the pair whose
lap mode speed is closest to the logged speed (the least root mean square
of the difference). The check lap is driven with that pair in a full lap
case, so its energy comes from the motors and the battery.

## Good to know

- Grip and downforce both raise the cornering speed. On one lap they can
  trade off, so two quite different pairs may fit almost equally well:
  judge the fit by the check lap's errors, not by the two values.
- Only grip and downforce are fitted. Mass, drag, the motors and the
  battery are your model's own: fix those first.
- A noisy lateral acceleration makes a noisy track. Lap mode refuses a
  curvature tighter than a 2 m radius, so very low speeds are left out.
- LightSim's own check of this method used its own laps with noise, not a
  real car's: see [Validation](../../VALIDATION-STATUS.md) and
  [Known issues](../../KNOWN-LIMITS.md).
