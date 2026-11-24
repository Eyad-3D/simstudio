## How it computes

Only a case of kind *Lap* uses the Race Track. In any other case (a drive
cycle, a performance or an acceleration test) the track has no physics and
every one of its outputs reads 0 for the whole run: anything wired to them
gets 0.

In a *Lap* case each lap runs in two steps, on the engine's model of the
car.

1. **The speed profile.** It is solved one instant at a time at each track
   point (about every metre), from the car's state at that lap's start
   (the battery's charge): the speed each corner allows, then a forward pass
   at full acceleration and a backward pass at full braking. Live edits
   made during the run are taken at the next lap's start: that lap's
   profile and energy pass use them.
2. **The energy pass.** It drives that profile through the whole model in
   fast mode (speed, curvature and grade are given, and the pedal is
   worked out back from them) in 0.01 s steps. Its energy books close to
   about 1e-7 of the energy through the parts, so the *Lap energy balance
   error* reads about 0.00001 %. The laps are joined into one run.

The car is the one the engine models, so some things differ from an ideal
lap solver:

- **Braking.** The brakes share the braking by their *Max Torque* (the FS
  example: 600/600 N·m at the front, 250/250 at the rear, so 70.6 % at the
  front, about what its weight transfer under braking asks for), not by an
  ideal balance, and the regeneration comes first, up to the Driver's
  *Recuperation Weight*, on the driven wheels only. Regeneration adds to
  the driven axle's braking, so where it is strong those tyres reach their
  grip first and the car brakes less hard than its four tyres could: the
  lap brakes earlier and takes longer. The FS example uses 40 %; at 100 %
  its 30 kW of regeneration would hold its rear tyres at their grip below
  about 33 m/s, and its autocross lap would take 60.2 s instead of 58.5 s.
- **The differential.** The car drives through its differential as its
  *Type* says. Open: in a corner the unloaded inner driven wheel's grip can
  hold the car back (the FS autocross takes 58.48 s with the example's
  limited-slip one, 58.98 s with an open one). Locked and Limited-slip: the
  two wheels of an axle turn at one speed in a lap, so the torque splits
  by the wheels' grip, a limited-slip one's as far as its locking torque
  reaches; its clutch's heat in a corner and a locked axle's resistance to
  turning are not counted (the Differential's page says more).
- **Tyre slip.** The tyres slip, and the lap counts the slip loss (force ×
  slip × speed). Where the motors at full torque hold the car (below their
  base speed), the lap takes that loss out of the drive force instead of
  turning the wheel faster, so the acceleration there is short by about the
  slip: over 75 m at μ 2.5 (10 % slip) the lap is 3.5 % slower than the
  time-domain acceleration test.
- **Top speed.** The top speed is the motors' in their gear: the highest
  speed the model solves at on the level (for the FS example 6,500 1/min
  through 4.4 on 0.228 m wheels, 35.3 m/s). The car settles within the
  motors' last 2 %, where their torque fades out.

The *Limit* channel says what held the car back: 1 cornering grip, 2
traction grip, 3 motor, 4 battery, 5 power cap, 6 braking, 7
lift-and-coast, 8 top speed. When the car reaches its top speed the
summary has a *Time limited by top speed* row.

The lap is solved per stretch of track (about 1 m), and that is its
resolution: in a lap case the battery's peak terminal power, its current,
its pack voltages, its time at its power limit and the E-Motor's time
limited by its supply are given per stretch, from the stretch means. Inside
a stretch the constant acceleration runs the power a few per cent over the
power cap at the stretch's end and under it at its start (a peak up to
6.5 % over the endurance's 30 kW cap, 1.6 % over 80 kW), while the
stretch's mean is on the cap; the channels' minimum and maximum show the
values inside the stretches. On stretches held by the motors at full load
or by the battery's own limits the profile takes four sub-steps, so the
energy pass never asks for more than they give. Where a battery gives less
than the lap's start assumed (its charge falls during the lap), the rest of
the lap is solved again from the car's state there, at most 3 times a lap,
and the run says so. A battery that starts a lap exactly full is taken
0.1 % below full for that lap's profile: at exactly full it takes no charge
back, and the profile would brake without regeneration while the car
regenerates from the first braking on.first braking on.

The Race Track's channels are recorded in lap cases only.

A lap stops, and the run fails with the engine's message and the time, on a
few cars: a battery held at its maximum-power point, and cars where the
pedal comes out undetermined. [Known limits](../../../KNOWN-LIMITS.md) lists
them.
