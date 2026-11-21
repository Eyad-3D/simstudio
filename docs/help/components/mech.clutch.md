## How it computes

The clutch is dry friction with stiction (stiction: it holds its two sides
together until the torque between them exceeds its capacity). It has no
inertia of its own.

Its capacity is

`T_c = e·max(T_max, 0)`

with e the *Engagement* input held to 0–1 (1 with nothing wired) and
T_max the *Maximum Torque* (N·m). The slip speed is `s = ω_A − ω_B`.

**Slipping** (s ≠ 0): it passes `T_c` from the faster side to the slower,
against the slip: `τ = T_c·sign(s)`.

**Stuck** (s = 0): both sides turn together and it passes whatever torque
keeps them together, as long as that is within `T_c`.

The changes between the two are located exactly in time:

- it sticks at the instant the slip reaches 0, if it has a capacity
  (`T_c` above 0);
- it breaks away, and slips, at the instant the torque it must pass
  exceeds `T_c`;
- a gear shift that pushes its two sides apart by more than 0.001 rad/s
  makes it slip until they meet again.

At an Engagement of 0, or a Maximum Torque of 0, the clutch is open: it
passes no torque and never sticks.

### Energy

It books the slip loss `τ·s` (*Slip Losses*): never negative, and 0 while
stuck. It stores nothing.

*Transmitted Torque* is the torque into Flange A (positive when A drives
B) and *Slip Speed* is s, A's speed less B's.

### At the start

In a run that starts moving, the clutch starts closed, both sides at the
same speed, when nothing is wired to Engagement, or when a Constant or a
Driving Task engages it at t = 0. If another block (a Script, a PID…)
commands it, or its Maximum Torque is 0, it counts as open: what only the
clutch ties to the wheels starts at rest, and the clutch starts slipping.
