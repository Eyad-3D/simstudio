## How it computes

The Script block runs outside the model's equations, on its own clock, as a
digital controller does: it reads its inputs, calls your `step()` function,
and holds the outputs it returns until its next call.

- **When it runs.** Once at the start of the run (t = 0) and then every
  period: the *Sample Time*, or 10 ms when the *Sample Time* is 0. Each call
  gets the time t in s and dt, the period.
- **Inputs.** The values of its input ports at that moment, by port id. Its
  ports have no unit: an input arrives as a number in the unit the app shows
  for the port it comes from (a Vehicle's speed in km/h, a battery's state
  of charge in %).
- **Outputs.** The dict `step()` returns, by output port id. An output it
  leaves out keeps its last value. An output leaves as a number in the unit
  of the input it feeds.
- **state** is a dict that keeps what you put in it from one call to the
  next; **params** holds the part's parameters.
- **Between calls** the outputs hold. When a call changes an output, the
  solver restarts from that moment, so the model reacts at once; a call that
  changes nothing leaves the run exactly as it was.
- **Errors.** A script that does not compile, or a call that raises an
  error, stops the run with the error's message.
