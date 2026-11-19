// The gear change of the reference problem mech_gear_change, written in the
// text format (engine/docs/text-format.md): a motor at constant torque
// accelerates a load through a gear that shifts from 12:1 to 7:1 at 4 s.
//
// Rotational.ShiftingGear is the library's shifting gear
// (lsim_lib::rotational::lossy_gear with a signal ratio) as text: a change
// of its ratio is a rigid engagement, so the engine keeps the momentum of
// everything the gear ties together and books the kinetic energy the shift
// loses to the gear. Rotational.ReinitGear writes the same shift out by
// hand with reinit, for two inertias it is told about.

model Rotational.ShiftingGear "Gear with a ratio and an efficiency on the power through it, in the direction it flows: a turns ratio times as fast as b; driving (power into a), b gets eta of it; flowing back, a gets eta of what b gives."
  connector a: Flange "input side";
  connector b: Flange "output side";
  input Real ratio(unit = "1") "a's speed / b's speed";
  parameter Real eta(unit = "1") = 1 "efficiency, in the direction the power flows";
  Real eta_dir(unit = "1", start = eta) "the torque factor for the present power direction";
equation
  a.w = ratio * b.w "a turns ratio times as fast as b";
  eta_dir = if a.tau * a.w >= 0 then eta else 1 / eta "the loss follows the power";
  b.tau = -ratio * eta_dir * a.tau "the torque is multiplied by the ratio, less the loss";
  annotation(__LightSim_energy(loss = a.tau * a.w + b.tau * b.w));
  annotation(__LightSim_engagement(changes = ratio));
end Rotational.ShiftingGear;

model Rotational.ReinitGear "Ideal gear that shifts from i1 to i2 at t_shift between an input inertia J_in and an output inertia J_out, restarting the output speed so that their angular momentum is kept."
  connector a: Flange "input side";
  connector b: Flange "output side";
  parameter Real i1(unit = "1") = 12 "the ratio a.w / b.w before the shift";
  parameter Real i2(unit = "1") = 7 "the ratio after it";
  parameter Real t_shift(unit = "s") = 4 "when it shifts";
  parameter Real J_in(unit = "kg.m2") = 0.05 "the inertia on side a";
  parameter Real J_out(unit = "kg.m2") = 135 "the inertia on side b";
  discrete Real ratio(unit = "1", start = i1, fixed = true) "the engaged ratio a.w / b.w";
equation
  a.w = ratio * b.w "a turns ratio times as fast as b";
  0 = ratio * a.tau + b.tau "the power through it is kept";
  when time >= t_shift then
    ratio = i2;
    reinit(b.w, (J_out + pre(ratio) * i2 * J_in) / (J_out + i2 * i2 * J_in) * b.w);
  end when "it shifts, keeping J_out * b.w + i2 * J_in * a.w";
  annotation(__LightSim_energy(loss = a.tau * a.w + b.tau * b.w));
end Rotational.ReinitGear;

model Examples.GearChange "A motor at 200 N.m drives a 135 kg.m2 load through a gear that shifts from 12:1 to 7:1 at 4 s."
  Rotational.Inertia motor(J = 0.05) "Motor rotor";
  Rotational.Inertia load(J = 135) "Load";
  Rotational.ConstantTorque drive(tau = 200) "Motor torque";
  Rotational.ShiftingGear gear(eta = 1) "Gearbox";
  Signal.Step select(y0 = 12, y1 = 7, t_step = 4) "Selected ratio";
equation
  connect(drive.flange, motor.a);
  connect(motor.b, gear.a);
  connect(gear.b, load.a);
  connect(select.y, gear.ratio);
end Examples.GearChange;

model Examples.GearChangeReinit "The same gear change with the shift written out by hand (reinit)."
  Rotational.Inertia motor(J = 0.05) "Motor rotor";
  Rotational.Inertia load(J = 135) "Load";
  Rotational.ConstantTorque drive(tau = 200) "Motor torque";
  Rotational.ReinitGear gear(i1 = 12, i2 = 7, t_shift = 4, J_in = 0.05, J_out = 135) "Gearbox";
equation
  connect(drive.flange, motor.a);
  connect(motor.b, gear.a);
  connect(gear.b, load.a);
end Examples.GearChangeReinit;
