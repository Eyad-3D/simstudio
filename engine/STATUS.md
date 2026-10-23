# New engine: status of the work packages (15 October 2026)

The six packages of DESIGN.md §16 are built on their own branches and
merged as they finish. On 15 October 2026 all six were merged together
(each package's latest round): every engine test passes (386), the
golden comparison is unchanged (199 of 201 figures, 984 of 1038 channels,
books closed to 1.5e-9), and the packages' meeting points were fixed
(a table's derivative from WP2 declined by WP3's compiled books, and
forwarded by WP6's `Started`). CI is green on the merged tree, Windows
included, from dd8feb0 (run 38840409543). A review of the merge (16
October) confirmed 38 findings; fixed so far: the prepared-model cache
(it reused a state choice made at other parameter values; it now keys on
every input), the start check of table derivatives, the interpolant's
first derivatives at a NaN argument, and the stale statements in this
file and DESIGN.md; then the native Script runner and the Python module
(the app's own runner as the reference, as the app calls it; every call
bounded in time, memory and stack depth; what cannot match is left to
Python; three rounds of adversarial checks) and fast mode, which refuses
a model whose sampled blocks have no hosts; then fast mode's mode switches
at a trace's samples and a block's ticks (input events, with the solver
work, 20 October), with the independent check's fixes (every bundled
cycle on both battery-electric cars, strict `when`s, a mode set from zero
settling those on it, the standing inverter, an emptied battery flagged).
Still open: the app's new-engine results (today's run verdict). The
shared types in `lsim-ir` are unified (`engine/ir-unify`, DESIGN.md
§5.8); every package builds on them.

Build setup for every package: its own target directory, and
`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=line-tables-only
CARGO_PROFILE_TEST_DEBUG=line-tables-only`, `cargo -j 2`. A target
directory shared between worktrees mixes their workspace crates silently.

Target platform: Windows x86-64 only for now (the owner's decision, 10
October 2026); macOS and Linux may come later. Development and most tests
also run on Linux, but only Windows CI proves what ships.

## Package by package

| Package | Branch | State | Next |
|---|---|---|---|
| WP1 language, units | `wp1/language` (merged) | Done: parser and printer with plain errors and units checked at parse time; Base Modelica import (13 models match exact answers); 70 malformed inputs; round trip of the library, of every vehicle block in every combination of its on/off switches and in fast mode's inverse variants (376 builds, 248 distinct definitions) and of every definition the example projects import; `engine/docs/text-format.md`, every example of it run and checked against the numbers it states; unified shared types. The gear change in the text format (`lsim-lib/text/gear_change.mo`): the library's shifting gear as text runs bit for bit as the model built in Rust (momentum through the shift kept exactly, its 2797.77 J loss to 4e-14, booked to the gearbox); the same shift written with `reinit` matches to 8e-16, its loss the event's, not the gearbox's; `reinit` of a port's across quantity or a part's variable now reads; `stateSelect` read from the text format and Base Modelica (`StateSelect.never` … `.always`, anything else refused with a span) and printed back (the solver work, 21 October) | The app and Python do not read text components yet (WP6); a model with no state runs since WP4's eleventh round (`Examples.Divider` in the guide) |
| WP2 preparation | `wp2/prep` (merged) | Done: index-2/3 models match exact answers to 1e-9 or better; tearing matches the brute-force minimum on 8 loops; 10⁵-equation networks prepare in 0.49–0.69 s; a test model per fault code; inverse models of stand-ins of the example cars; `reinit` as a when-assignment to a state's jump part (gear change exact to 2e-14, its loss booked to 4e-14); a start value carried from an alias to its representative (`wp4/solver`); index reduction through tables (1-D and 2-D, monotone cubic, bicubic and linear): a table read differentiates into reads of its partial derivatives, the interpolant's own (one implementation, moved to `lsim-ir`, for the interpreter, preparation, the generated code and the solver; the compiled code bitwise the interpreter), where a derivative jumps the cell the argument lies in, a table differentiated more often than it is smooth refused (`INDEX-TABLE-NOT-SMOOTH`), a table read along time whose read derivative jumps at its breakpoints stopped just before each and the integrator restarted there; index-2 models through tables match closed forms or quadrature to 1e-10 or better (a capacitor's current to 4e-16), index-3 pendulums whose length follows a 1-D or 2-D table match an independent integration to 1e-8 (their length held to 2e-10), the library's and the vehicle library's table blocks in constraints to 5e-11; the choice of states carried with the prepared model (`state_sets`, for the run to watch), `stateSelect` leading its pivots' preference (the solver work, 21 October; WP4's thirteenth round); the choice made again at a point of a run, by the same pivoting there (`prepare_choosing`, 23 October; WP4's fourteenth round) | Events at the breakpoints of a table read at a state whose read derivative jumps there (stepped over by error control today: 7e-11 on the test) |
| WP3 code generation | `wp3/codegen` (merged) | Done: one lowering into machine code or a tape (bitwise the same); coloured sparse Jacobians; Steffen 1-D and monotone bicubic 2-D tables (C¹, monotone where the data are); compiled modes, guards, `when`; the initialisation on tapes; condition kernels bitwise the interpreter (the golden comparison byte-identical with them on and off); 10 000 random expressions against the interpreter, every Jacobian against differences, the `when` semantics pinned; the cars compile in 23–32 ms of one thread, 10⁴ equations ready in 42–48 ms (tiered: tapes while the machine code compiles); the ISA's own calling convention everywhere (Windows x64 checked by cross-compiling); machine code not cached (measured: saves 9–16 ms a build); the per-call costs below those at the start (residual 513–560 ns against 565); after the review: machine code freed on drop, background compiles cancelled with their models and reporting how they end, tapes where executable memory is refused, the default floating-point environment for compiling and running, the same bits with and without FMA; one mathematical library, the engine's own (`lsim-math`: CORE-MATH's correctly rounded functions, vendored and built for CPUs with and without FMA, clang-cl on Windows), for every library call of the interpreter, the generated code, the tapes, the kernels, the books and SUNDIALS' own C code, so results are the same on every platform and CPU and the interval code's widening of library results (now one ulp) is a proof; clippy and a source test keep the platform's maths out; Windows CI green with it on the branch (DESIGN.md 7.1; the merged tree from dd8feb0); after the review: the vendored CORE-MATH patched (no `exit` on an unexpected case, no 32-bit `long` on Windows: every reference correctly rounded compiled with clang-cl's semantics) and its files hashed, the environment's C flags ignored and options that break correct rounding refused, the library checked on hard cases at its first call; the energy books' integrands compiled (tape at once, machine code on another thread, kept per spec), bitwise the interpreter (every example project's books, random books, edge cases with signed zeros, infinities and NaNs; golden comparison byte-identical with them on and off and with the code before): the BEV WLTC run 22.41 → 18.76 billion instructions, 4.4 → 3.4 s (about 410× → 525× real time, same machine); a step on which every condition's certificate holds cleared in one pass (the check 773 → 674 million instructions, the same counters) | The check is 3.6 % of the (now cheaper) BEV WLTC run's instructions, not 3 %: its floor without changing what it checks is about 3.5 % (DESIGN.md, WP3); diffsol (the cross-check backend) still calls the C runtime's `pow` and `cbrt` in its step control, so diffsol runs remain CPU- and platform-dependent (patched copies of diffsol, diffsol-nl and diffsol-la would fix it, DESIGN.md 7.1); the Script blocks' Python still computes with the C runtime's maths |
| WP4 solver | `wp4/solver` (merged) | Done: SUNDIALS built with only a C compiler (Linux); SUNDIALS and diffsol backends agree within 4.2·rtol; faer sparse LU; events, initialisation, energy books, accuracy check, sweeps (3.44–4.09× on 4 cores). Second round: the run-loop fixes from WP5's golden comparison, exact time events, cheap restarts, the impulse projection that keeps the momentum at a gear shift (DESIGN.md 8.2). Third round, from the review: the projection only at declared engagements, in two physical stages with exact derivatives; event iteration after it; light restarts opt-in; time crossings re-armed, set to now or at a root's instant; strict `when` conditions; alias start conflicts told. Fourth round: tyres pass no impulse (their slip after a shift integrated, checked against a fully resolved two-axle car); links judged after the rigid stage; cascades of engagements projected to their end or stopped with a named event storm. Fifth round: the energy books' stored-energy rates exact (closure back to round-off), tables read along time stop the integrator at their breakpoints, both sides of an event at an output time recorded. Sixth round: blocks ticking at one instant tick in one event; the books read the model's tables; conditions on explicit functions of time found ahead and reached exactly (the rest warned about); left limits of the changed channels only. Seventh round: iteration variables solved again between the ticks of one instant; mixed conditions checked along every step's dense output; integer powers enclosed with a proven bound, the platform's libm tested; no run without the tables it reads; books without error control flagged. Eighth round: the driving cycle's conditions checked too, most steps cleared by a certificate (a window and a box of the states) and the rest along the dense output; 2-D tables enclosed by their cells; conditions moved only by `noEvent` comparisons left to root finding. Ninth round: Adams' order capped at 7; the mixed conditions checked on the integrator's own dense-output polynomial, bounded rigorously over each step on SUNDIALS (on diffsol up to an estimated round-off; no fitted polynomial, no sampled certificate check); the tables' breakpoints taken from the data the model interpolates (a compiled model's tables swapped after preparation); every table the run reads outside the compiled code checked at its start, and a warning where a model gives a table's values but not its breakpoints; a benchmark of the check's share of the run (`scan_share`); an automatic switch from Adams to BDF made before the next step, so the step that asked for it keeps its dense output (outputs, block ticks, books and the check inside it read it). Tenth round: an idle 10 ms block's tick made cheap (inputs read ahead across the step, bit for bit: the dense output across the ticks, a computed input's chain as a flat tape; idle ticks in a row, nothing allocated): 2.0 / 3.9 / 4.4 % of the 21-state run (discrete / state / computed input; from 13.0 / 16.9 / 25.0 %, and 19.5 % in the earlier measurement), under 3 % of the 101-state one; the recorder's channels each given their own storage (fewer page faults in sweeps); the Windows build and every engine test on MSVC proven by CI; the golden comparison the same to the last digit. Eleventh round (solver fundamentals, 16 October): a model with no state and no equation solved by iteration runs (an integrator of nothing but the books: outputs exact, its events where they are, its books to 1e-15; it stopped at its start before; one with such an equation runs on IDA, its outputs to the tolerance); IDAS decides an order decrease on the states' and the books' errors together (the vendored copy patched: a spin-up whose states are exactly linear took 6 406 / 63 715 / 636 288 steps at 1e-6 / 1e-8 / 1e-10 at order 1, the books 0.26·√rtol off; now 96 / 116 / 184 steps, the books 2e-10 / 1e-12 / 2e-14 off); where the error test fails ten times at a kink the history cannot cross, the integrator restarts there and goes on (a gear whose torque starts a ramp at a table's breakpoint stopped with IDA's flag -3, now exact to 1e-15; the BEV's city cycle at 1e-9 likewise), and ten steps in a row that no longer move the time stop the run, naming the variable with the largest local error (at a jump without an event they went on without end); with the order fix the golden comparison moves by about the tolerance, every figure and channel on its side of its band (199 of 201, 984 of 1038; at most 0.16 × band), the books closed to 6.6e-10, no case slower. The outside judge (`engine/judge`, DESIGN.md §15: the Bari IVP Test Set's Andrews mechanism, car axis and Akzo Nobel problem against their published references, the IFToMM planar pendulum and multiple four-bar, the pendulum's closed form swinging and looping, the Reference FMUs' Stair and BouncingBall, SUNDIALS' own IDAS examples with quadratures, OpenModelica on the same models; rtol 1e-4 … 1e-10) judged the round: a model with no state (Stair) to the last digit (it failed at its start before); the IDAS patch reproduces none of LLNL's example outputs, which upstream 7.9.0 reproduces to the last digit (7.1.1 too, but for the slider-crank's quadrature in its 12th digit), because it changes more than the zero case: the quadratures come out closer to independent references (7.8e-7 off instead of 2.8e-6, 1.0e-10 instead of 2.5e-9), the steps 22 % fewer to 45 % more, as accurate for the same work or less, and a patch for the zero case alone does not mend the engine's (DESIGN.md 8.1), so it stays; the run stopped where no step passes now says the model may be singular as well as not smooth (the judge's pendulums stop there at a singular state choice, not at a jump; before the round, a bare SUNDIALS flag −9 or no answer at all); `golden/results.json` recorded again with the order fix (its test failed on the stale record). Twelfth round (the outside judge's checks, 16 October): an independent check of the round found a `when` on a variable solved from an implicit equation firing two or three times for one crossing (8 of the judge's 28 such runs; OpenModelica once in all) and a mode on one staying off until the next step end (its integral 1.6e-2 short at rtol 1e-5): the condition is now held on its side, the mode met again (DESIGN.md 8.2; `lsim-solve/tests/implicit_conditions.rs`); the judge now scores by time, not by row (it had taken OpenModelica's event rows for grid points and scored its Stair 2.0 off, not the 1 its late events give), counts crossings against exact answers (`count-…`), and judges a closed-form run right only within 100 rtol of its reference over the run, fixed before scoring; the SUNDIALS, BouncingBall and OpenModelica figures above and in DESIGN.md corrected to what was measured; the golden comparison the same but for one mode of the formula student car, met 1.7e-7 s earlier (at most 6e-4 of a band). Thirteenth round (the solver work's state selection, detection, 21 October): index reduction's choice of states is carried with the prepared model and watched along every run, at its start (a parameter set on a built model, a cached model) and on `set_param`, which refuses values at whose start it does not work; where it degrades (a decided variable moving a hundred times as much as a state, or the chosen block's determinant changing sign inside a step) the run stops at the point the step's dense output shows, naming the constraint, the variables, the parts and what to do (`RUN-STATE-SELECT`), and any other failure of SUNDIALS is told with the equation and variable the run sees there, its words beside it (DESIGN.md §6 step 5); Modelica's `stateSelect`, read and ignored before, leads the pivots' preference (`STATE-SELECT-ALWAYS`, `-NEVER`, `-CONFLICT` told). The outside judge on it: of the pendulum's 2 198 runs released at every 0.01 rad, the 432 that ended wrong without a message and the 840 that stopped on a bare SUNDIALS flag (175) or with no step passing (665) are now 103 wrong and 1 169 stopped, naming the rod, where the states degrade (2.26 ms before the bottom: within 100 rtol seconds of the closed form's time at every tolerance); the IFToMM pendulum and the looping ones stop so at every tolerance (3 and 6 of their runs were wrong, 1 to 2 m off); the 103 still wrong are runs whose choice holds, over five periods, where the same pendulum in its angle (an ODE, no choice of states) is wrong in 1 096 of its 2 198 runs by the same criterion; with x marked `prefer` the pendulum released at 1.50 and 1.55 rad runs to its end (8.4e-8 and 2.7e-7 m off at 1e-10; OpenModelica's dynamic selection 9.8e-8 and 6.6e-8; before, every run stopped or ended 1 m off); no run of the judge ends on a bare SUNDIALS flag (182 before), and the long-haul cycle's full dynamic runs, which empty the battery (both battery-electric cars, the only two of fast mode's judge that stopped on one), name the E-Motor's equation that fits its torque to its supply; every other judge run is bitwise as before; MSL's PlanarFourbar (revolute1 `always`, met) to 3.1e-9 rad at 1e-10 (OpenModelica 1.9e-7); the golden comparison the same to the last digit, the books closed to 6.6e-10, the cars' CPU time the same within this machine's noise (89.9 → 90.5 s for the whole comparison, ±3 % from run to run; instructions +0.04 % on the BEV's WLTC, +0.45 % on the hybrid's mixed cycle), a mechanism paying more (+8.5 % on Andrews', +14 % on PlanarFourbar: the sines and cosines of its blocks' entries, evaluated at every step); the independent check of the round, fixed: the step after any event, and every step with a block's tick inside, went unwatched (released at 2.8 rad, a `when` that changed nothing 2.3 ms before the states degenerate, or a display block ticking every 3 to 10 ms, let the run pass the horizontal on the wrong branch and end 2.45e-2 m off without a word at rtol 1e-4, or stop up to 72 100 rtol seconds late): the points before and after every event are now checked as a step's end, and such runs stop where they stop without the event (the judge's new `pendulum-event-…` within 10 rtol seconds of the closed form at every tolerance; the check's sweep with an event 1 to 4 ms before each stop: 8 of its 882 runs passed the point, now all stop, within 206 rtol seconds of the run without the event); a choice `stateSelect` asks for is taken only where it starts within half the run's limit, at a factor of 50 (at the limit itself, atan 99, x marked `prefer` stopped at its far turning point at one tolerance and ended 297 to 3 664 rtol off without a word at the six others; now the mark is refused, and told, `STATE-SELECT-PREFER`, and the run stops near the bottom within 6 rtol seconds); every other run of the judge bitwise as before (4 557); the golden comparison the same to the last digit, the books closed to 6.6e-10, its CPU time the same (91.7 against 92.2 s, back to back), instructions +0.47 % on the hybrid's mixed cycle (its 56 218 events each checked on both sides), −0.14 % on the BEV's WLTC. Fourteenth round (the solver work's state selection, dynamic, 23 October): where the watched choice degrades past a threshold (3 times its factor where its fixed entries were last read, chosen on the judge's sweep against 1.125, 1.5, 2, 4 and 10; 50 while it keeps a column the model marks to be kept), the run chooses the states again there by preparation's own pivoting at that point (`lsim_prep::prepare_choosing`, `stateSelect` leading as at the start, `never` and `always` respected) and goes on: a new integrator on the new states from the solution at the step's end (or, where the step went past the point, where its dense output shows it), the energy integrals carried and the stored energy's change booked as an event's (the pendulum's books within 22 rtol across 17 switches), the recorded channels kept by name, an event recorded (`EventKind::States`); each choice compiled once and kept with the model; where no other choice works there, or the model was given none (diffsol, fast mode's inverse model, a model whose tables were swapped after it was built), the run stops naming the constraint, as before (DESIGN.md §6 step 5, *the choice made again*). The pendulum released exactly level, refused at preparation before, prepares (an initialisation's explicit solution 0 / 0 keeps its guess; its Newton takes an exact root at a singular Jacobian) and runs. The outside judge on it: every one of its 4 592 runs ends; the release-angle sweep's 1 169 that stopped (from 1.48 rad) go to their end through 8 to 19 switches each, 146 of them right within 100 rtol and 1 023 not, where the same pendulum in its angle (an ODE, no choice of states) is right in 121 of the same runs and the x-y form's error is 1.21 times the angle form's (geometric mean): the problem's own sensitivity over its five periods, so the target of every run right is not met, by either form; at rtol 1e-10 3.2e-8 m released at 1.50 rad, 6.2e-9 at 1.55, 3.6e-8 the IFToMM pendulum, 2.6e-8 exactly level, 5.2e-7 at 3.00, 1.9e-5 at 3.13, 1.8e-7 … 6.7e-6 looping (OpenModelica's dynamic selection, the first verdict's runs: 9.8e-8, 6.6e-8, 8.6e-8, not run, 1.3e-5, 6.4e-4, 6.4e-7 … 2.0e-5; `omc` not run again); per tolerance, rtol 1e-4 … 1e-10, the sweep's runs right / wrong / stopped from 146/1/167 … 112/35/167 to 176/138/0 … 120/194/0 (DESIGN.md §15, the fifth verdict); every run that does not switch bitwise as before (3 360 of the judge's runs; Andrews', PlanarFourbar, the marked pendulums at 1.50 and 1.55 rad); the golden comparison the same to the last digit, and every other field of it, its CPU time the same (100.9 and 109.0 s before, 98.8 and 101.5 s after, back to back), instructions +0.3 % on the hybrid's mixed cycle (the look at each of its 571 511 step ends), none on the BEV's WLTC (no set moves), about 1 % of Andrews' and PlanarFourbar's runs (650 and 400 instructions a step); a switch costs about 0.25 ms on the pendulum (the model prepared again at the point), the first to a choice 4 to 8 ms more (its compilation) | The smallest model with a computed input is at the 5 % target, not clearly under it when run at 10 ms (4.9 % median, noise ±2 %): about 160 instructions of bookkeeping a tick remain; sweeps 3.2–3.5× on 16 short runs (target 3.5×; this VM's noise ±15 %), the rest is fresh memory for each run's results (a change to `SimResult`'s layout, WP6); diffsol's events at 1e-10 within 34·rtol (its step error control; the cross-check only). Dynamic state selection (risk R3) runs every pendulum of the judge to its end, but from 1.48 rad most of its runs (1 023 of the 1 169) are more than 100 rtol off the closed form, as the same pendulum in its angle is (1 048): the problem's own sensitivity over five periods, which no choice of states mends; it switches on SUNDIALS only (on diffsol, the cross-check, the run stops where the choice degrades, as before); each switch prepares the model again (0.25 ms on the pendulum; keeping the structure and pivoting alone at the point would be cheaper); the IFToMM four-bar does not prepare (`SINGULAR-LOOP` at its start values, the accelerations' and rod forces' block: not a choice of states; neither of OpenModelica's selections runs it). The runs still wrong below 1.48 rad, where the choice holds and nothing switches (103 of the 2 198, released at 0.72 to 1.47 rad, the error growing with the angle: 17 to 69 rtol up to 0.70 rad, 868 at 1.45 rad, rtol 1e-10) are the x-y form's own larger error under a choice that stays inside its window (the factor at the release at most 9.9): in its angle the pendulum is wrong in only 48 of its 1 029 runs up to 1.47 rad, and right in 66 of these 103 at the same angle and tolerance; per run the x-y form is up to 28.6 times the angle form's error (1.47 rad, rtol 1e-6), up to 6.0 times at 1e-10 from 1.31 rad; with `prefer` at 1.55 rad (a factor of 48 at the release, inside the 50 a marked choice may start at) 22 times the angle form's. Andrews' mechanism gets more significant correct digits than the test set's five solvers at the same tolerance (6.6 at 1e-7 against 3.5–5.4) in 8 to 20 times their steps. A known limit: a condition on the states alone, not on time, is left to root finding, which misses two crossings inside one step without a warning (the judge's `count-explicit-pulse` at four tolerances of seven, an oscillator's peaks at six; OpenModelica misses the peaks at five); checking such conditions along the step as the mixed ones are checked finds them all, but stops the hybrid's HWFET run as an event storm of its clutch's stick conditions and takes 12 to 13 times as long without compiled kernels for them (DESIGN.md 8.2): it waits for the clutch's switching and those kernels |
| WP5 library, import | `wp5/library` (merged) | Done: physical library and the 36 vehicle blocks; project importer; 199 of 201 golden figures inside their bands, 984 of 1038 channels; energy books closed to 1e-6 or better (1.5e-9 or better on every case). Triage of every difference outside (October 2026): all 2 figures and 54 channels are today's engine's error, one cause (it applies a gear's efficiency to, and reports as the gear's, the torque of the sources above it, the rotor's and gears' inertias not taken out; the new engine's gears lose it on, and report, the torque their teeth carry); shown by today's rule rebuilt from the new engine's run (`golden/definitions.py`: every channel to 0.17 × band or less), the same car without those inertias (every figure and channel inside), the inertias moved past the gear (the figure inside), an exact answer (`tests/gear_loss.rs`) and the wheels' momentum balance; none is the new engine's or a band's | The 2 figures (the BEV's city recuperation, 1.0039 × since IDAS's order fix, 1.0013 × before, converged 1.0044 ×) and 54 channels stay outside as compared: today's engine is not changed here (`lsim-project/golden/README.md`). Found on the way, for WP4, and fixed there (eleventh round): on IDA a model whose states are exactly polynomial in time stayed at order 1, so its books' quadratures set the step (9 336 steps for a 2 s spin-up at 1e-6, steps growing as 1/√rtol, the books off by about 0.26·√rtol); IDAS's order decrease is now decided on the books' errors too (96 steps, the books 2e-10 off) |
| WP6 fast mode, Python | `wp6/fast-engine-python` (merged) | Rosenbrock-W fast stepper; limit flags match the forward run's limit hits; the example projects' drive-cycle cases in fast mode (`ImportOptions::inverse`, DESIGN.md §10: the cycle prescribed, the Driver's pedal freed, inverse variants of the Driver, body, wheels, brakes and e-motor; an acceleration test and the hybrid, whose Script reads the pedal, refused with their reasons; judged by `engine/judge/fast.py`); input events (FMI 3.0) at a trace's breakpoints and a block's ticks, every mode checked against its crossing after each step, strict `when`s fired as the forward run loop fires them, the modes and clauses on a mode set where its crossing leaves zero following at once (DESIGN.md §10); every bundled cycle runs on both battery-electric cars (54; 9 stopped on a singular Jacobian that named nothing before, 2 raised flags the car never reached): the inverse variants follow the pedal beyond full throttle and brake and have one root (no saturation leaves the pedal free, the tyre's slip held to its grip, a supply window flagged, not cut), the inverter draws nothing standing still, an emptied battery is flagged, every solve kept to the root that continues from its consistent point, a singular Jacobian names the variables it leaves free; fast mode's charge used within 1 % of the full dynamic run's on all 61 cases with a reference (−0.76 … +0.02 %; New York's city cycle was 1.43 % over without a flag, and on 7 of the 9 example cases 25 to 48 % short before the input events), −0.05 … +0.07 % on the speed the full dynamic run drove, a cycle changed at round-off level changing it by 1.5e-7 at most and no flag, its energy on EPA's four reference cars within 1.3 % of FASTSim 3.1.0's given the same cars (mean 0.92 %), samples added on a trace's lines changing nothing (a property test); engine facade, whose `Started` wrapper forwards every `ModelFunctions` method (a test fails if one is missed); the Python module `lightsim_engine` (build, simulate, set_params, sweep, diagnose, NumPy arrays, interpreter lock released, plain errors; bit for bit the golden comparison's run on the BEV and hybrid cases, DESIGN.md §13); Script blocks without Python (`lsim-script`: a strict subset of Python whose results are the app's Script runner's bit for bit, checked against that runner as the app calls it; anything outside the subset runs on Python, a case that meets it mid-run is run again on Python; each call held to the worker's 2 s and 512 MB (every heap block its values and code hold counted) and to half a MiB of stack; DESIGN.md §9); numbers in JSON read exactly (before, about one in eight came back off by a unit in the last place); the app's backend runs drive-cycle and acceleration cases on the new engine when `LIGHTSIM_ENGINE=new` is set (today's engine is the default and the fall-back; Script blocks still run in today's locked-down worker there; `backend/app/solver/new_engine.py`); measured on one machine, release builds, against the start of this round: fast mode on the test car 16.6 → 6.0 ms (1.1·10⁵ → 3.0·10⁵× real time), its full dynamic run 1008 → 119 ms (the cycle read as a time table), the hybrid's UDDS 93 → 56 s, its mixed cycle 26 → 12 s (Script blocks without Python), the BEV's WLTC unchanged (2.7–2.8 s, about 650×); the golden comparison unchanged (199 of 201 figures, 984 of 1038 channels, books closed to 1.5e-9) | Speed against the average of comparable tools (the owner's goal since 15 October 2026, DESIGN.md §2; the fixed 10⁶× and 1000× targets are dropped): to measure. Today fast mode 3.0·10⁵× real time; full dynamic BEV WLTC 2.2 s on a 4-core VM after the merge with WP3's compiled books (about 820×), the hybrid's UDDS 42 s; the hybrid (33× real time on the UDDS since the merge; 25× before it) restarts the integrator at every controller output change (about 22 steps per 10 ms tick; going on without a restart is off by default for accuracy, WP4); Python 3.13 fails two of the backend's Script-safety tests (they pass on 3.11, CI's version, and failed before this work too); whether the app may run Script blocks on the native runner instead of the worker is the owner's call; the new engine's results in the app lack the energy report, part energies, duty, limits and references; Python API: fast mode for project files, FMU parts, progress and cancel; wheels; fast mode for a car whose sampled controller reads the pedal (the hybrid: its supervisory controller would have to be inverted, or its outputs freed instead of the pedal); the full dynamic run of the 805 km long-haul cycle, which empties the battery, stops where the battery empties (14 881 s on the aerodynamic BEV, 14 541 s on the BEV), naming the E-Motor's equation that cuts its torque to what its supply gives (before, on a bare SUNDIALS flag −4; fast mode runs it and flags the battery from the second it is empty); the inverse model has no stiction and no slip below 0.5 m/s (on the driven speed fast mode is within 0.07 % of the full dynamic run) |

## Run-loop issues from the golden comparison: resolved (`wp4/solver`)

- A `when` condition made true by a sample tick's new outputs fires at the
  tick (its value before the event is the one before the tick).
- Mode changes a tick or a time event makes at its instant are scheduled,
  not an event storm; the golden harness's allowance is gone.
- A `when` condition already true at the start: decided as Modelica does
  (nothing fires at the start; it fires once it has been false and becomes
  true again), documented and tested.
- Event iteration re-checks the conditions with the iteration variables
  solved for the new discrete values.
- A tick a few ulps before the end time is that instant (no "tout too
  close to t0"); the output grid ends at the end time exactly.
- A restart hands IDA y' in full (z' too), skips the second consistency
  solve and sizes its first step from x''. Going on with the history after
  a slight change (a light restart) is opt-in: its error accumulates over
  a long run (the review: 57 tolerance units on a 10 000-tick ramp); a tick
  whose outputs reach nothing integrated goes on exactly.
  `suppress_algebraic_error` was measured and stays off (its errors grow up
  to 8× on the exact-answer suite's DAEs).
- A condition on time alone (`time >= c`) is an exact time event at c; its
  right limit holds only while its time is still now (a timer re-armed at
  its own instant, `t_last := time`), and a root SUNDIALS reports at a
  scheduled time joins it.
- A strict `when x > 0` from x = 0 fires as x leaves zero (Modelica).
- A gear shift, declared by the gearbox as a rigid engagement, keeps the
  momentum of everything the gears tie together, and that loss is the
  gearbox's. A tyre passes no impulse (its force is bounded by its grip):
  its slip relaxes after the shift through its own law, inside its grip
  or sliding at it, integrated, passing the momentum on to the vehicle
  over that time, its loss its own (today's engine relaxes a gripping
  tyre's slip at once; a fully resolved two-axle car agrees with the new
  engine, not with that). Couplings a model declares stiff and unbounded
  still pass an impulse on, judged after the rigid stage. Nothing else
  starts a projection, what a `reinit` set stays, and a cascade of
  engagements at one instant is projected to its end or stops the run
  naming it.
- `Integrator::consistent_z` fails by default (a DAE backend cannot skip
  it silently); the run report counts inert ticks apart from light
  restarts.
- The energy books take each stored energy's rate exactly (forward-mode
  differentiation through the assignments; the model's x' on IDA): the
  finite difference before it lost precision whenever an entry of the
  state passed zero while moving, and the hybrid's books closed to only
  1.6e-6; now 1.5e-9 or better on every golden case.
- Tables read along time (a driving cycle) stop the integrator at their
  breakpoints, so a condition they drive is not stepped over while
  nothing integrated moves.
- An output time at an event records both sides (`SimResult::left_limits`
  beside the values after it, only the channels the event changed); the
  golden comparison takes the side today's engine records.
- Every sampled block due at an instant ticks in one event: an output
  point there shows the values after all of them.
- A declared stored energy or loss may read a table: the books read the
  model's own interpolants (they were NaN, and the default run stopped at
  its start), the rates through tables and time exact; a zero direction
  has a zero rate (no `inf · 0`).
- A condition on an explicit function of time (`sin(2π time / T) >
  0.95`) is not stepped over while nothing integrated moves: its sign
  changes are found ahead and reached exactly; one that also reads
  states (a driving cycle's target against a speed, a motor's limits
  from its 2-D maps) is checked on every step, by a certificate where
  one holds and along the dense output elsewhere (the polynomial the
  integrator interpolates with, bounded rigorously over the step on
  SUNDIALS; on diffsol up to an estimated round-off), and
  the step ends at a pulse root finding did not see; one moved only by `noEvent`
  comparisons is left to root finding, as `noEvent` asks; what cannot be
  searched is named in a warning.
- Blocks ticking at one instant read the iteration variables as the
  blocks before them left them (solved again when needed), and a
  computed input at the instant it is read.
- A model that does not give the tables its books, conditions, time
  events or impulse projection read does not start; books computed
  without error control are flagged.
- The breakpoints of a table whose position moves are recomputed after
  a discrete change (no stale stops).
- A `when` on a variable solved from an implicit equation (`x > 0.5` with
  `x³ + x = z`) fires once per crossing: where solving the iteration
  variables again at the event put the condition back a little short of
  zero, the restarted integrator saw it cross again and the `when` fired
  two or three times (43 of 140 runs; OpenModelica: one at every
  tolerance); the condition is now held on its side until it gets there.
  A mode on one, which its relation set back at the event, stayed off
  until the integrator's next step end (its integral 1.6e-2 short at
  rtol 1e-5); the integrator now restarts there and meets the crossing
  again (DESIGN.md 8.2, *Conditions on iteration variables*).

## The vendored IDAS against SUNDIALS 7.9.0 (20 October 2026)

The engine's IDAS is 7.1.1 with `sundials/lsim.patch` (an order decrease is
decided on the states' and the books' errors together, DESIGN.md 8.1). It
was checked against the 7.9.0 release; the vendored copy stays at 7.1.1
(moving it moves every result, and is a separate decision).

- The bug is still there: `IDAQuadTestError` is the same text in 7.1.1 and
  7.9.0, its guard (`knew != kk`) included, and the changelog from 7.2.0 to
  7.9.0 does not mention it. The patch applies to the 7.9.0 release as it
  is (160 lines further down).
- A short C program (IDAS, `y' = 1`, the quadrature `z' = y²` under error
  control) takes 1 422 steps at rtol 1e-4 and 361 976 at 1e-10 on both
  releases, never above order 2, with z off by 1.0e-3 and 1.1e-6; with the
  patch it takes 42 or 43 steps at every tolerance and z is off by 9e-15 at
  most. The program and a draft issue for LLNL are with the owner; nothing
  is filed.
- SUNDIALS' own `idasSlCrank_dns` and `idasAkzoNob_dns`, built from each
  release: as released they reproduce the shipped outputs to the last
  digit, but for 7.1.1's slider-crank G in its 12th digit (the build prints
  3.3366160662909388, the value 7.9.0's `.out` has, against the shipped
  3.3366160662934146). With the patch, on either release, they take other
  steps (312 instead of 251, 473 instead of 500); every printed solution
  digit of Akzo Nobel is the shipped one, 70 of the slider-crank's 78 are
  (the rest one unit of the last digit away), and G is closer to its
  independent value in both. A patch for the zero case alone (the decrease
  is revisited only where the states' errors are all exactly zero) leaves
  both outputs byte for byte and mends the program above, but not the
  engine's cases whose states' errors are round-off rather than zero (8.1,
  measured earlier), nor the same program with `y = t²`, whose states'
  errors are round-off: as released and with that patch it never goes
  above order 4 and takes 1 140 steps at rtol 1e-10, with the patch 93.
- From 7.1.1 to 7.9.0 the order and error-test code of IDAS and CVODES is
  unchanged, and the two releases agree on the steps and solutions of the
  eight of SUNDIALS' examples we ran. Three changes can still move a
  number: the quadrature's first derivative is set after `IDACalcIC` (7.8.0),
  `TOO_CLOSE` is returned at more first calls (7.6.0), and the linear
  solver's counters are reset at a restart (7.6.0; statistics only).

## The library Driver's anti-windup (20 October 2026)

The solver work's item D (the merge review's finding 37), judged from
outside (`engine/judge`: `msl-pid-controller` and `driver.py`).

- **The cause.** The Driver froze its integral while its command was held
  at a limit it pushed against (conditional integration, as today's
  engine does), written as a mode. A car held at full throttle that fell
  behind and caught up met the mode's boundary with both sides driving it
  back: frozen, the command falls below its limit; integrating, the
  integral pushes it above. It slid along the boundary (Filippov sliding),
  and the run loop, which switches a mode only at an event, stopped it as
  an event storm (DESIGN.md 8.2): the Battery Electric Car at 6000 kg on
  the WLTC at 961.83 s, at 19 270 kg on its city cycle at 2.64 s.
- **The fix.** The anti-windup is Modelica `LimPID`'s back-calculation,
  which has no mode: der(I) = err + (cmd − cmd_u)/(ki·Tt), Tt = Ni·Ti,
  Ti = kp/ki, Ni = 0.9 (`blocks/vehicle.rs`). Inside the limits it is the
  old equation. `lsim-project/tests/driver.rs`: a point car held at full
  throttle follows back-calculation's closed form (speed and integral to
  5e-9 and 8e-8 at rtol 1e-8; it stopped as a storm at 92 s before); the
  review's two cars run to their ends, inside the speed-trace rule's band
  wherever no pedal is fully down (both stopped as storms before).
- **MSL's `PID_Controller`** with the Driver as its `LimPID`, against the
  library's reference result (six signals, each relative to its largest
  value): with the frozen integral 1.6 off at every tolerance, the limit
  left 6.5e-2 s late; now 5.1e-3 at rtol 1e-4, 8.9e-5 at 1e-6 and 1.2e-4
  at 1e-7 … 1e-10, where OpenModelica's run of the original example is
  too: the reference's own accuracy (Dymola at tolerance 1e-6; its limit's
  instants are 1.2e-6 s off both). Beside OpenModelica's run at the same
  tolerance the engine's differs by 4.0e-3 at 1e-5, 1.3e-5 at 1e-8 and
  1.6e-7 at 1e-10 (the spring's relative speed, the most sensitive signal;
  the controller's integral by 5.3e-9), the limit's instants by 2.9e-10 s.
- **The map** (`driver.py`): the Battery Electric Car at 1 to 25 t on the
  27 bundled cycles and its own city cycle (19 270 kg too), 701 runs, each
  driven trace judged by 40 CFR 1066.425(b) with its allowances. Before:
  191 inside, 1 outside only where the car was too fast with its brakes
  fully applied, 504 stopped as event storms of the frozen integral (the
  first at 4 t, on the LA92 and the US06), 5 with a bare SUNDIALS flag
  (long-haul at 1 to 5 t). After: 588 inside; 75 outside only at full
  brake, the car unable to brake as the schedule asks (on 11 of the 28
  columns, from 10 t on the LA92, 15 t on the US06, 18 to 23 t on the
  other nine); 37 inside only by the rule's allowance for a car at its
  maximum available power, standing at its empty battery on the two
  long-haul cycles; 1 stopped with an event storm the run loop names (the
  battery's minimum-SOC mode, 93 switches in 2.1 s as the car, coasting at
  its empty battery, began to recuperate: long-haul at 11 t). None is
  outside where the Driver could have helped, and none ends on a bare
  flag. The 192 runs that ran before and after: 22 437 127 → 22 484 196
  steps, 2 842 885 → 2 867 306 error-test failures, 20 927 → 20 033
  events, 285 → 281 s of CPU.
- **Found on the way and fixed**: the E-Motor at an empty battery. Its
  supply's share was still solved while the inverter was off, and with
  no room for the spin losses the share whose power fits is a generator
  torque that grows as the rotor slows; below about 4 rad/s there is
  none, and IDA stopped with a bare flag -4 (corrector convergence). With
  the Driver's fix alone, 37 runs stopped so (long-haul at 24 of its 25
  masses, long-haul-100km from 13 t) and one with the run loop's "no
  progress" naming the share (long-haul at 11 t). The share is not solved
  while the inverter is off now (DESIGN.md 12.2,
  `lsim-project/tests/motor.rs`), and those 38 runs are the 37 and the 1
  above.
- **Today's engine beside the new one** on the heavy cars (6, 12 and 25 t
  on the 26 bundled cycles but long-haul, and 6, 12, 19.27 and 25 t on the
  city cycle; 82 runs, today at 10 ms): the same verdict in 78; in 4 (the
  FTP-75, its motorcycle version, the UDDS and the US06 at 25 t) today's
  engine has occasions the rule counts against its Driver where the new
  engine's are all at full brake: on the same occasions its Driver lets
  the brake up (to 0.94 … 0.99) for the last 0.1 to 0.3 s, the car still
  0.2 to 0.7 km/h too fast. A frozen integral lets the command off its
  limit while the error is still large; back-calculation pulls the
  integral toward cmd_u = −1 + Ni·kp·err, beyond the limit until the
  error is nearly gone. The consumptions differ by up to 1.4 % at 6 t,
  1.6 % at 12 t (both the NYCC) and 2.0 % at 25 t. Some of that is
  today's 10 ms step (at 1 ms today's engine moves by 0.03 to 0.8 %, most
  on the NYCC); the rest is the anti-windup. Today's engine at 1 ms with
  back-calculation in place of its frozen integral (`driver.py today
  --back-calculation`: a check in one process, today's engine itself not
  changed) agrees with the new engine in the five cases checked (the NYCC
  at 6 and 12 t; the WLTC's medium phase, its city part and the US06 at
  25 t) to 0.05 % in the energy its battery delivers, 0.13 % in
  consumption and 0.03 % in distance, the recuperated energy to 0.54 %;
  at 25 t its speed to 0.11 km/h and its pedal to 0.018, where the frozen
  integral's pedal is up to 0.76 away; and its US06 at 25 t is then
  outside only at full brake too. On the review's WLTC car at 6 t the new
  engine's consumption is 0.02 % from today's at 1 ms.
- **The golden comparison** (14 cases): one moved, the hybrid's UDDS, by
  at most 1.6e-5 of a band (12 figures, 73 channels, none across its
  band; steps 3 022 879 → 3 022 291, events 118 074 → 118 072). No
  golden car is held at its Driver's limit (a build with the mode taken
  out and nothing in its place reproduces every case to the last digit);
  the integral's equation now reads the command before and after its
  limits, which is the same to the bit inside them, but the Newton
  matrix is not (the integral reads itself through the command), and in
  the hybrid's 118 000 events the rounding of its solves moves a clutch
  event 2.7e-10 s at 163.05 s. Every other case is the same to the last
  digit, the books closed to 6.6e-10; the E-Motor's fix moves none. Not
  slower: the whole comparison took 101.0 and 102.7 s of CPU against
  103.0 and 102.7 s for cb76842's build, the two alternating
  (`lsim-project/golden/README.md`).
- **Left**: the battery's minimum-SOC storm above (the battery's, not the
  Driver's); the 75 runs at full brake are the car's (its brakes sized
  for 1.9 t), not the Driver's. The library's PID block (`control.pid`)
  keeps today's conditional integration as a mode (the app documents
  that its sum stops growing at a limit), so a loop that slides along
  its limit stops there as a named storm. Only the Battery Electric Car
  was mapped. The rule is read on a 0.1 s output grid (an occasion's
  length known to a step; 1 s on the 83 042 s long-haul cycle). SAE
  J2951's drive-quality metrics, which 1066.425(j) asks for, are not
  computed: the standard is paid for.
