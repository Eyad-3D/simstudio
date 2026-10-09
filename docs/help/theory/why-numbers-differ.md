# Why a result differs from the official figure

You model a car you know, run the WLTC (the Worldwide harmonised Light
vehicles Test Cycle) and get a number that is not the one in the
brochure. Most of the gap usually has one of five causes below, and
none of them means the model is wrong. Check them before you change the
model's values to match.

## 1. Battery or socket

LightSim's *Consumption* is the energy at the battery's terminals: what
left the battery, minus what braking put back. An official figure is
measured at the charging socket, so it also holds what the charger and
the battery lose while charging.

The Battery Electric Car example shows this: it uses 14.05 kWh/100 km
on the WLTC at its battery, and a car of its class is rated about 15 to
16 kWh/100 km at the socket. To compare with a socket figure, divide
LightSim's by your charger's efficiency (for example 0.9 for 90 %).

## 2. The test mass

Official tests drive the car at a test mass: the empty car plus a driver,
a share of the load and fluids, set by the test's rules. A brochure's
mass is often the empty car. Use the test mass for *Vehicle Mass* when
you compare with a test result. The example's
[exercise](../examples/bev-car.md#exercises) shows how much 200 kg moves
its consumption: 14.05 to 14.79 kWh/100 km on the WLTC.

## 3. Test figure or label figure

A consumption test gives one figure; what is printed for buyers can be
another. In the United States, for example, the label figures are
adjusted from the test results to come closer to everyday driving.
LightSim gives the test-cycle figure: compare it with the test result,
not the label (background knowledge, unverified: check the rules of the
figure you compare with).

## 4. Heating and air-con

Official consumption tests run with heating and air-con off. In everyday
driving they can draw kilowatts, and at low speed that is a large share
of the energy per kilometre. The example's *WLTC, heating/air-con on*
case adds a 2.5 kW load and uses 18.88 kWh/100 km instead of 14.05. Add
a *Power Consumer* with your car's load to see its effect.

## 5. Cold start

An engine that starts cold uses more fuel until it is warm, and a cold
battery has a higher resistance. LightSim's parts have no temperature:
an engine is warm and a battery at its datasheet values from the first
second. The P2 Hybrid Car example uses 2.83 l/100 km on the EPA city
cycle, below EPA's own 2.91 l/100 km for the car it is sized after, whose
test starts cold.

## Still different?

- Check that the run is a *success*, and that no figure is marked *not
  valid* ([what that means](../reference/results.md#not-valid-and-the-run-status)).
- Check the values that decide most of the energy: mass, drag
  coefficient and frontal area (or the road-load coefficients A, B and
  C), the motor's loss map and the auxiliary loads.
- [Known issues](../../KNOWN-LIMITS.md) lists what the model leaves out
  or gets wrong today, and [Validation](../../VALIDATION-STATUS.md) what
  has been checked against measured data.
