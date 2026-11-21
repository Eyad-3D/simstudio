## How it computes

In this version of LightSim the Race Track computes nothing. A case of kind
*Lap* is refused before it runs (*Not run [NEW-LAP]: lap cases do not run in
this version of LightSim yet*), so the lap simulation the description above
explains does not run yet. In any other case (a drive cycle, a performance
or an acceleration test) the track has no physics and every one of its
outputs reads 0 for the whole run: anything wired to them gets 0.
