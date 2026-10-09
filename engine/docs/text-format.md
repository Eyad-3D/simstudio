# The text format for components

LightSim's engine builds every model from *components*: a resistor, an
inertia, a battery, a whole vehicle. You can write your own components as
text. The text says which *ports* a component has (the places where it
connects to other parts), its *parameters* (the numbers you set before a
run), its *variables* (the numbers the simulation works out) and the
*equations* that tie them together. The engine reads the text, checks it,
and solves the equations of every component in a model together.

The format uses Modelica's syntax. Modelica is an open language for
equation-based models; Base Modelica (the flat form of Modelica the
Modelica Association defines in MCP-0031) is the reference. A component in
this format reads like Modelica, and LightSim also imports models written in
Base Modelica by other tools ([Base Modelica import](#base-modelica-import)).

This page first walks through two components, then describes every part of
the format, then the error messages.

Contents

1. [An equation component: a resistor](#an-equation-component-a-resistor)
2. [A component made of components: a battery](#a-component-made-of-components-a-battery)
3. [Names](#names)
4. [Ports](#ports)
5. [Parameters](#parameters)
6. [Variables](#variables)
7. [Units](#units)
8. [Parts](#parts)
9. [Equations](#equations)
10. [Expressions](#expressions)
11. [Tables](#tables)
12. [Energy books](#energy-books)
13. [Connector types and enumeration types](#connector-types-and-enumeration-types)
14. [Error messages](#error-messages)
15. [Base Modelica import](#base-modelica-import)
16. [For programmers](#for-programmers)

## An equation component: a resistor

An *equation component* states its physics as equations. Here is the
library's resistor:

```modelica
model Electrical.Resistor "Ideal linear resistor."
  connector p: Pin "positive terminal (current flows in here)";
  connector n: Pin "negative terminal";
  parameter Real R(unit = "Ohm") = 1 "resistance";
  Real v(unit = "V") "voltage across it";
  Real i(unit = "A") "current from p to n";
equation
  v = p.v - n.v "the voltage across it is p.v - n.v";
  0 = p.i + n.i "the current into p leaves at n";
  i = p.i "its current is the current into p";
  v = R * i "Ohm's law";
  annotation(__LightSim_energy(loss = v * i));
end Electrical.Resistor;
```

Line by line:

* `model Electrical.Resistor "…"` starts the component and gives its name
  and a description. `end Electrical.Resistor;` closes it; the two names must
  be the same.
* `connector p: Pin` declares a port `p` of the connector type `Pin`. A
  `Pin` carries two quantities: the voltage `p.v` and the current `p.i`
  flowing *into* the component. Connecting two pins makes their voltages
  equal and their currents sum to zero.
* `parameter Real R(unit = "Ohm") = 1` declares a parameter `R` in ohms with
  the default value 1. Each part you place in a model can have its own
  value.
* `Real v(unit = "V")` declares a variable. The simulation finds its value
  at every moment.
* After `equation` come the equations. Each one may end with a short
  description in quotes, its *label*. When something is wrong with a model,
  the engine's messages quote these labels, so write what the equation
  states in plain words.
* `annotation(__LightSim_energy(loss = v * i))` tells the engine's energy
  books that this part turns `v * i` watts into heat.

The equations do not say which side is computed from which. `v = R * i`
serves to find the current from the voltage or the voltage from the
current, whichever the model needs. This is what *acausal* or
*equation-based* means.

A component with a state: an inertia whose speed `w` changes with the net
torque.

```modelica
model Rotational.Inertia "Rigid rotating mass; its speed is a state."
  connector a: Flange "one side";
  connector b: Flange "the other side";
  parameter Real J(unit = "kg.m2") = 1 "moment of inertia";
  Real w(unit = "rad/s", start = 0, fixed = true) "speed";
equation
  w = a.w "side a turns with it";
  w = b.w "side b turns with it";
  J * der(w) = a.tau + b.tau "J dw/dt is the net torque";
  annotation(__LightSim_energy(stored = 0.5 * J * w * w));
end Rotational.Inertia;
```

`der(w)` is the time derivative of `w`, in rad/s². `start = 0, fixed =
true` says the speed is 0 at the start of a run.

## A component made of components: a battery

You can also build a component from other components and wire their ports
together with `connect`. This battery is a constant voltage source in
series with a resistance and a resistor–capacitor pair:

```modelica
model Battery.SimpleCell "Equivalent-circuit battery: source, R0 and one RC pair."
  connector p: Pin "positive terminal";
  connector n: Pin "negative terminal";
  parameter Real ocv(unit = "V") = 400 "open-circuit voltage";
  parameter Real r_series(unit = "Ohm") = 0.05 "series resistance";
  parameter Real r_pair(unit = "Ohm") = 0.01 "RC pair resistance";
  parameter Real c_pair(unit = "F") = 0.01 "RC pair capacitance";
  Electrical.ConstantVoltage source(V = ocv) "the cells' open-circuit voltage";
  Electrical.Resistor r0(R = r_series);
  Electrical.Resistor r1(R = r_pair);
  Electrical.Capacitor c1(C = c_pair);
equation
  connect(n, source.n);
  connect(source.p, r0.p);
  connect(r0.n, r1.p);
  connect(r0.n, c1.p);
  connect(r1.n, c1.n);
  connect(r1.n, p);
end Battery.SimpleCell;
```

* `Electrical.Resistor r0(R = r_series)` places a part `r0` of the type
  `Electrical.Resistor` and gives its parameter `R` the value of this
  battery's parameter `r_series`. When you change `r_series` before a run,
  `r0` follows; nothing is rebuilt.
* `connect(r0.n, r1.p)` wires port `n` of `r0` to port `p` of `r1`.
  `connect(r1.n, p)` wires a part's port to the battery's own port `p`.
* A part and a parameter cannot share a name: write `r_series`, not `r0`,
  for the parameter (Modelica forbids the clash, so LightSim does too).

A component made of components and one made of equations are the same
thing to the engine: a composite may add equations of its own after the
`connect` lines.

## Names

* A name starts with a letter or `_` and holds letters, digits and `_`:
  `R`, `tau_max`, `c1`.
* Names are case-sensitive: `R` and `r` are different names.
* A name that is not a plain name, such as `'R1.v'` (from Base Modelica) or
  a reserved word such as `'end'`, goes in single quotes.
* Reserved words: the words of the format (`model`, `parameter`,
  `equation`, `when`, `connect`, `der`, `and`, `or`, `not`, `true`, `false`,
  `time`, `structural` …) cannot be names unless quoted.
* A dot reaches into a port or a part: `p.v` is the voltage of port `p`,
  `r0.i` the current of part `r0`.
* Text after `//` to the end of the line, and between `/*` and `*/`, is a
  comment.

## Ports

| declaration | what it is |
|---|---|
| `connector p: Pin "…";` | a physical port of the connector type `Pin`: brings `p.v` (across) and `p.i` (through) |
| `input Real u(unit = "N.m") "…";` | a signal input: its value comes from the output it is linked to |
| `output Real y(unit = "1") "…";` | a signal output: the component computes it, any number of inputs may read it |

The library's connector types:

| type | across quantity | through quantity (positive into the part) |
|---|---|---|
| `Pin` | `v`, voltage, V | `i`, current, A |
| `Flange` | `w`, speed, rad/s | `tau`, torque, N·m |
| `TFlange` | `v`, velocity, m/s | `f`, force, N |
| `HeatPort` | `T`, temperature, K | `Q`, heat flow, W |

At a connection the across quantities are equal and the through
quantities sum to zero. A physical port that nothing is connected to
carries no flow.

## Parameters

| declaration | value |
|---|---|
| `parameter Real R(unit = "Ohm") = 1 "…";` | a number in the declared unit |
| `parameter Real tau(unit = "s") = R * C;` | an expression of other parameters of the same component |
| `structural parameter Real n_cells(unit = "1") = 96;` | a parameter whose change rebuilds the model (it may change the equations, as today's *fixed* parameters) |
| `parameter Boolean use_heat_port = false;` | true or false |
| `parameter Mode mode = Mode.Auto;` | one option of an enumeration type (see below) |
| `parameter Real ocv(unit = "V") = table(…);` | a table ([Tables](#tables)) |

Every parameter needs a value. Attributes in parentheses: `unit`,
`displayUnit` (the unit shown to people, such as `"kW"`), `min` and `max`
(the allowed range). `parameter Real x … annotation(Evaluate = true)`
also makes a parameter structural, as other Modelica tools write it.

Every other parameter is a *runtime input*: a new value takes effect at the
next run without rebuilding the model.

An enumeration type lists named options. Declare it inside the component
(or at the top of the text for several components):

```modelica
model Gearbox.Selector "Picks the ratio by mode."
  type Mode = enumeration(Manual "the driver shifts", Auto "the controller shifts");
  parameter Mode mode = Mode.Auto "how gears are chosen";
  parameter Real manual_ratio(unit = "1") = 3.5;
  parameter Real auto_ratio(unit = "1") = 2.8;
  output Real ratio(unit = "1");
equation
  ratio = if mode == Mode.Manual then manual_ratio else auto_ratio;
end Gearbox.Selector;
```

In equations an option stands for its position, counting from 1:
`Mode.Manual` is 1, `Mode.Auto` is 2.

## Variables

| declaration | what it is |
|---|---|
| `Real v(unit = "V");` | a continuous variable |
| `Real w(unit = "rad/s", start = 0, fixed = true);` | a variable with a start value that must hold at the start (a state's initial condition) |
| `Real i(unit = "A", start = 160);` | a variable whose start value is only a first guess (for an algebraic loop) |
| `discrete Real gear(unit = "1", start = 1, fixed = true);` | a value that changes only at events (in `when`) |
| `Real p(unit = "W") = v * i;` | a variable with its equation written in place |

Attributes: `unit`, `displayUnit`, `start` (a number or an expression of
parameters), `fixed`, `nominal` (the variable's typical size, which the
solver uses to judge its accuracy). To bound a variable, write an `assert`
in the equations.

## Units

* Every declared `unit` must be a *coherent SI unit*: an SI unit without a
  prefix or offset, such as `"V"`, `"N.m"`, `"kg.m2"`, `"m/s2"`, `"W/K"`,
  `"1"`. Numbers inside the engine are SI, so a value of 1500 for a power
  means 1500 W.
* Put the unit people read in `displayUnit`: `unit = "W", displayUnit =
  "kW"`; `unit = "rad/s", displayUnit = "rpm"`; `unit = "K", displayUnit =
  "degC"`. The display unit must measure the same thing.
* Unit text follows Modelica: `.` multiplies, `/` divides, a number after a
  symbol is a power (`m2`, `s-1`). LightSim also reads `N·m`, `kg·m²`,
  `m/s^2`.
* Angles are plain numbers: `rad/s` and `1/s` are the same unit.
* A variable or parameter without a `unit` is dimensionless.

LightSim checks the units of every equation when it reads the text. Each
side of an equation, each term of a sum and both sides of a comparison
must be in the same unit; `exp`, `sin`, `log` and the like need
dimensionless arguments. A bare number takes the unit its place needs in a
sum or a comparison (`v - 1` with `v` in volts is fine) and is
dimensionless as a factor. A wrong equation is quoted as you wrote it:

```text
line 6, column 3: In 'Heater', the equation “v = R / i” does not balance its units:
the left side is in V, the right side in m2.kg.s-3.A-3.
```

## Parts

`Lib.Name part(parameter = value, …) "label";` places a part.

* The values are expressions of the enclosing component's parameters; a
  Boolean parameter takes `true` or `false`, an enumeration parameter an
  option, a table parameter a `table(…)` or a table parameter of the
  enclosing component.
* The label in quotes is the name people see in messages, such as `"HV
  Battery"`.
* A part placed from the app keeps its element id in
  `annotation(__LightSim(id = "el-battery"))`.

## Equations

| equation | meaning |
|---|---|
| `a = b "label";` | holds at every moment |
| `connect(a, b);` | joins two ports ([Ports](#ports)) |
| `when cond then … end when "label";` | at the moment `cond` becomes true, sets discrete variables or restarts states |
| `if cond then … else … end if;` | equations that depend on a condition |
| `assert(cond, "message");` | stops the run with the message when `cond` is false |
| `assert(cond, "message", AssertionLevel.warning);` | warns instead |

Equations that hold only at the start go after `initial equation`.

`when` sets discrete variables and restarts states:

```modelica
model Rotational.LatchBrake "A brake that clamps on once the speed reaches w_on."
  connector flange: Flange "the shaft";
  parameter Real tau_max(unit = "N.m") = 100 "torque once engaged";
  parameter Real w_on(unit = "rad/s") = 300 "the speed at which it engages";
  discrete Real engaged(unit = "1", start = 0, fixed = true) "1 once engaged";
equation
  flange.tau = tau_max * engaged "it takes its torque once engaged";
  when flange.w >= w_on then
    engaged = 1;
  end when "it engages when the speed reaches w_on";
  annotation(__LightSim_energy(loss = tau_max * engaged * flange.w));
end Rotational.LatchBrake;
```

* Inside `when`, `v = expression;` gives the discrete variable `v` a new
  value and `reinit(x, expression);` restarts the state `x` from a new
  value.
* The engine finds the moment the condition becomes true to the solver's
  precision and stops there; it does not wait for the next output step.
  A condition on time alone (`time >= t_shift`) is reached exactly: the
  event is at `t_shift`, and an output at that moment shows the values
  just after it.
* As in Modelica, a `when` acts when its condition *becomes* true while
  the model runs. A condition already true at the start does not act
  there (it acts once it has been false and becomes true again): what
  must hold from the start belongs in the start values.
* `when a then … elsewhen b then … end when;` handles two conditions; if
  both become true at the same moment, the first branch wins.

An `if` equation must have the same number of equations in each branch,
including the `else` branch:

```modelica
model Electrical.IdealDiode "Conducts forwards, blocks backwards."
  connector p: Pin "anode";
  connector n: Pin "cathode";
  parameter Real R_on(unit = "Ohm") = 1e-5 "resistance when conducting";
  parameter Real G_off(unit = "S") = 1e-8 "conductance when blocking";
  Real v(unit = "V");
  Real i(unit = "A");
equation
  v = p.v - n.v;
  0 = p.i + n.i;
  i = p.i;
  if noEvent(v > 0) then
    v = R_on * i;
  else
    i = G_off * v;
  end if;
end Electrical.IdealDiode;
```

## Expressions

| written | meaning |
|---|---|
| `+ - * / ^` | arithmetic; `^` is a power |
| `< <= > >= == <>` | comparisons; `<>` is "not equal" |
| `and or not` | logic |
| `if c then a elseif d then b else e` | a value that depends on conditions (always with `else`) |
| `true`, `false` | the logical values (1 and 0 in arithmetic) |
| `time` | the simulation time, s |
| `der(x)` | the time derivative of the variable `x` |
| `pre(x)` | the value of `x` just before the current event |
| `noEvent(e)` | `e` with its comparisons evaluated as they stand, without events |
| `limit(x, lo, hi)` | `x` held within `[lo, hi]`; in fast mode the value passes through and the moments outside the band are flagged |
| `sin cos tan asin acos atan atan2 sinh cosh tanh exp log log10 sqrt abs sign min max` | the usual functions (`log` is the natural logarithm) |
| `ocv(soc)` | the table parameter `ocv` read at `soc` |

Write signs and powers with parentheses where they meet another operator:
`a * (-b)`, `a - (-b)`, `x ^ (-2)`, `(a ^ b) ^ c`. A comparison in an
equation makes the solver stop exactly where it changes (an *event*);
wrap it in `noEvent(…)` when the expression is smooth across it.

## Tables

A table parameter holds data that the equations read by interpolation. A
table with one axis:

```modelica
model Battery.OcvCell "A cell whose open-circuit voltage depends on its state of charge."
  connector p: Pin "positive terminal";
  connector n: Pin "negative terminal";
  parameter Real ocv(unit = "V") = table(x = {0, 0.1, 0.5, 0.9, 1}, y = {3.0, 3.45, 3.65, 4.0, 4.2}, xUnit = "1") "open-circuit voltage over SOC";
  parameter Real capacity(unit = "C") = 180000 "charge capacity (50 A h)";
  Real soc(unit = "1", start = 0.9, fixed = true) "state of charge";
  Real v(unit = "V");
  Real i(unit = "A") "current out of p";
equation
  v = p.v - n.v;
  0 = p.i + n.i;
  i = -p.i;
  v = ocv(soc) "the terminal voltage is the open-circuit voltage";
  capacity * der(soc) = -i "discharge empties it";
end Battery.OcvCell;
```

* `x` lists the points of the axis, increasing; `y` the values there, in
  the parameter's unit; `xUnit` the axis' unit (coherent SI, as declared
  units are).
* A table with two axes: `table(x1 = {…}, x2 = {…}, values = [row 1; row 2;
  …], x1Unit = "…", x2Unit = "…")`, one row of `values` for each point of
  `x1`, one number in a row for each point of `x2`. Read it as `eff(w,
  tau)`.
* Between the points a table is a monotone cubic curve by default: its
  slope has no jumps, so the solver needs no events at the points, and it
  does not overshoot where the data rise or fall steadily. `interpolation =
  linear` joins the points with straight lines instead.
* `outside = {clamp}` (the default) holds the edge value beyond the data;
  `linear` extends the edge's slope; `error` stops the run. Give one rule
  for each axis, or one for all.
* Tables are runtime data: new data never rebuild the model.

## Energy books

`annotation(__LightSim_energy(stored = …, loss = …))`, at the end of the
component, gives the energy it stores (in J) and the power it turns into
heat (in W). With them the engine checks that each part's energy balance
closes: the energy in through its ports equals what it stores plus what it
loses. Other Modelica tools ignore the annotation.

## Rigid engagements

When an event changes a rigid coupling between moving parts (a gear
whose ratio changes at a shift), the speeds the coupling ties together
jump, as an instantaneous, rigid engagement makes them: the engine keeps
the momentum of everything the coupling ties together, with the masses
and inertias the parts declare in their stored energy, and books the
kinetic energy the engagement loses as lost at that moment, to the part
whose coupling changed. A gear-change model needs nothing more than its
equations; there is no `reinit` to write:

```modelica
model Rotational.ShiftingGear "A gear whose ratio is a signal: a turns ratio times as fast as b."
  connector a: Flange "input side";
  connector b: Flange "output side";
  input Real ratio(unit = "1") "the engaged ratio a.w / b.w";
equation
  a.w = ratio * b.w "a turns ratio times as fast as b";
  0 = ratio * a.tau + b.tau "the power through it is kept";
end Rotational.ShiftingGear;
```

For the two inertias it joins, `J_in` on its input and `J_out` on its
output, a shift to the ratio `r` gives `w_out = (J_out·w_out + r·J_in·w_in)
/ (J_out + r²·J_in)`, the speeds before the shift on the right.

A part with only bounded forces (a slipping clutch, a tyre at its grip
limit) passes no impulse: what is behind it keeps its speed. A part that
passes one on as if it were rigid for that moment says so with
`annotation(__LightSim_impulse(keep = …, active = …))`: the relative
velocity it keeps through an impulse, and while it does. A tyre that
grips keeps its slip velocity, so a gear shift's impulse reaches the
vehicle. What the impulse dissipates across the velocity it keeps (the
impulse through the tyre times its slip velocity) is booked to that part,
the rest of the engagement's loss to the part whose coupling changed:

```modelica
model Vehicle.GripTyre "A tyre whose force follows its slip, up to its grip."
  connector shaft: Flange "the wheel's shaft";
  connector road: TFlange "the vehicle";
  parameter Real r(unit = "m") = 0.3 "rolling radius";
  parameter Real k(unit = "N.s/m") = 5000 "force per slip velocity";
  parameter Real F_max(unit = "N") = 4000 "grip";
  Real F(unit = "N") "tyre force, driving positive";
equation
  F = min(max(k * (shaft.w * r - road.v), -F_max), F_max);
  shaft.tau = F * r;
  road.f = -F;
  annotation(__LightSim_energy(loss = F * (shaft.w * r - road.v)));
  annotation(__LightSim_impulse(keep = shaft.w * r - road.v, active = abs(F) < F_max));
end Vehicle.GripTyre;
```

## Connector types and enumeration types

A text may also define connector types and shared enumeration types, at
the top level:

```modelica
connector Pin "Electrical terminal: potential v and current i into the component."
  Real v(unit = "V");
  flow Real i(unit = "A");
end Pin;

connector HeatPort "Thermal port: temperature T and heat flow Q into the component."
  Real T(unit = "K");
  flow Real Q(unit = "W");
  annotation(__LightSim_power = "through");
end HeatPort;

type DriveMode = enumeration(Eco, Normal, Sport);
```

A connector has one across quantity and one `flow` (through) quantity.
`__LightSim_power = "through"` says the through quantity is itself a power
(heat flow), so a port's power is not across × through.

## Error messages

Every error says where it is (line and column) and what is wrong, in plain
words, with a short code that stays the same when the wording changes. A
few of them:

| code | example |
|---|---|
| `SYNTAX` | `line 4, column 8: a ';' is missing after the equation (found 'x')` |
| `END-NAME` | `line 2, column 5: the model 'M' is closed by 'end N;': the two names must be the same` |
| `UNKNOWN-NAME` | `line 4, column 7: 'y' is not a variable, parameter or port of 'M'` |
| `UNIT-MISMATCH` | `In 'M', the equation “v = 1 + v + i” does not balance its units: a sum mixes V and A.` |
| `UNIT-NOT-SI` | `the parameter 'P' is declared in 'kW', which is not an SI unit: … declare unit = "W" and displayUnit = "kW"` |
| `NAME-CLASH` | `'r' names both a parameter and a part of 'M': give the parameter another name (for example 'r_value') …` |
| `WHEN` | `“when x > 1 then x = 0; end when” assigns 'x' at an event, but 'x' is not discrete: declare it 'discrete Real', or restart a state with reinit(x, …)` |
| `TABLE` | `this table is not valid: the table has 3 grid points but 2 values` |
| `ARRAY` | `'x' has an array subscript '[': arrays are not supported (… scalars only)` |

Reading a text with the library at hand (as the app does) also checks the
parts: that each part's type exists, that the parameters given to it exist
and have the right units, and that connected ports carry the same
quantity.

## Base Modelica import

LightSim reads a Base Modelica file (written by a Modelica tool's
flattener) into one component. Such a file is a package that holds types,
records, functions and one model; every name is in single quotes:

```text
//! base 0.1.0
package 'RC'
  type 'Voltage' = Real(unit = "V");
  model 'RC' "a capacitor charged through a resistor"
    parameter Real 'R'(unit = "Ohm") = 50;
    parameter Real 'C'(unit = "F") = 1e-3;
    parameter 'Voltage' 'V' = 400;
    'Voltage' 'vC'(start = 0, fixed = true);
  equation
    'C' * der('vC') = ('V' - 'vC') / 'R';
  end 'RC';
end 'RC';
```

What the import reads:

* types that rename Real, Integer or Boolean with attributes (a unit, a
  display unit, a start value), and enumeration types;
* records of scalars: a record variable `'r'` becomes one variable per
  field (`'r'.'a'`); its values come from modifiers, a record constructor
  `'R'(1, 2)` or another record of the type;
* functions of scalars, which it writes into the equations where they are
  called (*inlining*): inputs with defaults, the output, protected local
  variables, and an algorithm of assignments (`:=`) and `if` statements;
* constants, which it replaces by their values;
* parameters, variables, inputs and outputs, `when`/`elsewhen`, `if`
  equations, `assert`, `initial equation`, `der`, `pre`, `noEvent` and the
  scalar functions; `min` and `max` of a variable become warnings when the
  value leaves the range;
* a variable without a unit gets the unit its equations imply, where they
  determine it.

It checks names and units as the text format does. It does not read
arrays, `for` loops, clocked (sampled) equations, external functions,
functions with records, `initial()`, `sample`, `delay` or rounding functions
(`floor`, `mod` …): each gives a message at its place.

Thirteen hand-written Base Modelica models of LightSim's reference problems
import and simulate to the problems' exact answers (the tests in
`crates/lsim-lang/tests/basemodelica.rs`).

## For programmers

The crate `lsim-lang` (`engine/crates/lsim-lang`):

```rust
// components; names, units and values checked against what the text declares
pub fn parse(text: &str) -> Result<Vec<ComponentDef>, Vec<LangError>>;
// also checks parts, ports and connector types against a library
pub fn parse_with(text: &str, lib: &Library) -> Result<Vec<ComponentDef>, Vec<LangError>>;
// connector types, enumeration types and components, as a library
pub fn parse_library(text: &str, base: Option<&Library>) -> Result<Library, Vec<LangError>>;
// the printer: parse(&to_text(&d)) == Ok(vec![d])
pub fn to_text(def: &ComponentDef) -> String;
pub fn connector_to_text(c: &ConnectorDef) -> String;
pub fn library_to_text(lib: &Library) -> String;
pub mod basemodelica { pub fn import(text: &str) -> Result<ComponentDef, Vec<LangError>>; }
```

A `LangError` has a `code`, a `span` (lines, columns and byte offsets of
the start and end) and a `message`; `render(text)` shows the line with a
caret under the place, and `to_diagnostic()` turns it into the engine's
`Diagnostic`.

How the format maps to the engine's IR (`lsim-ir`):

| text | IR |
|---|---|
| `connector p: Pin` | `PortDecl { kind: Physical { connector: "Pin" } }` |
| `input Real u(unit = …)` | `PortKind::Input { unit }` |
| `parameter Real …` | `ParamDecl { default: ParamValue::Real(expr) }` |
| `structural parameter` | `ParamDecl { structural: true }` |
| `parameter Boolean b = true` | `ParamValue::Bool(true)` |
| `parameter Mode m = Mode.Auto` | `ParamValue::Enum("Mode.Auto")`, the type in `ComponentDef::types` or `Library::types` |
| `= table(x = …, y = …, xUnit = …)` | `ParamValue::Table1D` |
| `= table(x1 = …, x2 = …, values = …)` | `ParamValue::Table2D` |
| a table with `interpolation` or `outside` | `ParamValue::Table(TableData)` |
| `ocv(soc)` | `Expr::Table { table: 0, args: [Name("ocv"), soc] }` (component scope) |
| `a == b`, `a <> b` | `a >= b and a <= b`, `a < b or a > b` |
| `true`, `false` | `Const(1.0)`, `Const(0.0)` |
| `when … end when "label"` | `Equation::When`, with the label on its `EquationDecl` |
| `assert(c, "m", AssertionLevel.warning)` | `Equation::Assert { error: false }` |
| `annotation(__LightSim_energy(…))` | `ComponentDef::energy` |
| `annotation(__LightSim_impulse(keep = …, active = …))` | `ComponentDef::impulse` (one `ImpulseDecl` each) |
| `annotation(__LightSim(id = "…"))` on a part | `SubDecl::ui_id` |

The checks at parse time follow the same rules as the engine's unit check
when it prepares a model, so a text that passes here passes there.
