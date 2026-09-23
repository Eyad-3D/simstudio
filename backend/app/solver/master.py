"""Co-simulation master — steps slaves on a shared communication grid.

Master v1 semantics (locked decisions #1/#4):

- **Gauss–Seidel, non-iterative**: slaves are stepped in the fixed order
  given at construction. A slave reads the *freshest* pool values — outputs
  written by slaves earlier in the same micro-step propagate immediately;
  feedback across the order boundary arrives one micro-step late (exactly
  the pre-refactor coupling, so golden fixtures remain valid).
- **Zero-order hold**: between a slave's communication points its outputs
  stay constant in the pool; its inputs are sampled at its own points.
- **Multi-rate via integer divisors**: a slave with ``rate_divisor = N``
  steps every Nth master micro-step with ``h = N · dt`` (one global clock,
  no nested adaptivity).
- **Events**: ``StepResult.events`` from any slave are surfaced to the
  caller (and the optional ``on_event`` callback) — reconfigurations, not
  errors. An ``error`` status raises :class:`SlaveStepError` immediately.
- **Live parameters**: routed to the slave that declared the variable
  (causality "parameter"); otherwise broadcast in slave order until one
  claims it (needed while wholesale-wrapped slaves don't enumerate every
  parameter). ``get_state``/``set_state`` are deliberately unused here —
  they are the hook for a future iterative (rollback) master.

The master is deliberately dumb about *what* the variables mean: names are
opaque strings (see ``slave.var_name``), routes are input-name → source-name,
and time is owned by the caller (it passes ``t`` and ``dt`` per micro-step).
"""
from __future__ import annotations

from typing import Callable, Mapping, Optional

from .slave import ParamResult, Slave

OnEventFn = Callable[[str, str], None]  # (slave_id, event)


class SlaveStepError(RuntimeError):
    """A slave reported an error status from ``do_step``."""

    def __init__(self, slave_id: str, detail: str):
        super().__init__(f"slave '{slave_id}' failed: {detail}")
        self.slave_id = slave_id
        self.detail = detail


class Master:
    def __init__(
        self,
        slaves: list[Slave],
        routes: Mapping[str, str],
        on_event: Optional[OnEventFn] = None,
    ):
        ids = [s.slave_id for s in slaves]
        dupes = {i for i in ids if ids.count(i) > 1}
        if dupes:
            raise ValueError(f"duplicate slave ids: {sorted(dupes)}")
        self.slaves = list(slaves)
        self.routes = dict(routes)
        self.on_event = on_event
        self._pool: dict[str, float] = {}
        self._k = 0  # micro-step counter (drives rate divisors)

        # per-slave routed inputs [(input name, source name)] and the owner
        # of each declared parameter variable
        self._inputs_of: dict[str, list[tuple[str, str]]] = {}
        self._param_owner: dict[str, Slave] = {}
        for s in self.slaves:
            routed: list[tuple[str, str]] = []
            for v in s.variables():
                if v.causality == "input":
                    src = self.routes.get(v.name)
                    if src is not None:
                        routed.append((v.name, src))
                elif v.causality == "parameter":
                    self._param_owner[v.name] = s
            self._inputs_of[s.slave_id] = routed

    # -- lifecycle -------------------------------------------------------------

    def initialize(self, t0: float) -> None:
        """Set up every slave and seed the pool with their initial outputs,
        so the first micro-step's inputs see declared start conditions."""
        self._k = 0
        self._pool.clear()
        for s in self.slaves:
            s.setup(t0)
        for s in self.slaves:
            self._pool.update(s.get_outputs())

    def step(self, t: float, dt: float) -> list[str]:
        """Advance one master micro-step ``[t, t + dt]``; returns events."""
        events: list[str] = []
        for s in self.slaves:
            n = max(1, int(s.rate_divisor))
            if self._k % n:
                continue  # not this slave's communication point (ZOH)
            inputs = self._inputs_of[s.slave_id]
            if inputs:
                payload: dict[str, float] = {}
                for name, src in inputs:
                    val = self._pool.get(src)
                    if val is not None:
                        payload[name] = val
                if payload:
                    s.set_inputs(payload)
            result = s.do_step(t, dt if n == 1 else dt * n)
            if result.status != "ok":
                raise SlaveStepError(s.slave_id, result.detail or "do_step failed")
            if result.events:
                events.extend(result.events)
                if self.on_event:
                    for ev in result.events:
                        self.on_event(s.slave_id, ev)
            outputs = s.get_outputs()
            if outputs:
                self._pool.update(outputs)
        self._k += 1
        return events

    def reset(self) -> None:
        for s in self.slaves:
            s.reset()
        self._pool.clear()
        self._k = 0

    # -- variables & parameters -------------------------------------------------

    def value(self, name: str) -> Optional[float]:
        return self._pool.get(name)

    def values(self) -> Mapping[str, float]:
        """Read-only view of the exchanged-variable pool."""
        return self._pool

    def set_parameter(self, name: str, value: object) -> ParamResult:
        owner = self._param_owner.get(name)
        if owner is not None:
            return owner.set_parameter(name, value)
        for s in self.slaves:  # broadcast until claimed
            result = s.set_parameter(name, value)
            if result != "invalid":
                return result
        return "invalid"

    # -- diagnostics -------------------------------------------------------------

    def check(self) -> list[str]:
        """Route sanity: every route target must be a declared input and every
        route source must be produced (output/local) by some slave. Returns
        human-readable problems (empty = clean); wiring-level validation."""
        produced: set[str] = set()
        declared_inputs: set[str] = set()
        for s in self.slaves:
            for v in s.variables():
                if v.causality in ("output", "local"):
                    produced.add(v.name)
                elif v.causality == "input":
                    declared_inputs.add(v.name)
        problems: list[str] = []
        for target, src in sorted(self.routes.items()):
            if target not in declared_inputs:
                problems.append(f"route target '{target}' is not a declared input of any slave")
            if src not in produced:
                problems.append(f"route source '{src}' is not produced by any slave")
        return problems
