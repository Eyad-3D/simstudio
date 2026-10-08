"""A run's results: status, summary numbers (by stable key), channels and
messages, with CSV, MAT and JSON export."""
from __future__ import annotations

import csv
import gzip
import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional

from . import matfile
from ._engine import engine


@dataclass(frozen=True)
class Message:
    """A line of the run's Messages: ``level`` is info, warning or error."""

    level: str
    text: str


@dataclass(frozen=True)
class Check:
    """A Data Check finding (the app's *Problems* list)."""

    level: str  # info, warning or error
    text: str
    fix: Optional[str] = None
    element_ids: tuple[str, ...] = ()
    #: the case it is about (it stops only that case's runs); None: the model
    case_id: Optional[str] = None

    def to_dict(self) -> dict:
        out = {"level": self.level, "text": self.text, "fix": self.fix,
               "elementIds": list(self.element_ids)}
        if self.case_id is not None:
            out["caseId"] = self.case_id
        return out


@dataclass(frozen=True)
class Channel:
    """One recorded signal: its values at :attr:`Result.time`; None where
    the channel has no data yet (a gap, not a zero)."""

    key: str  # "<elementId>:<portId>"
    label: str  # "<part label> · <port name>", as the app shows it
    unit: str
    values: tuple[Optional[float], ...]


@dataclass(frozen=True)
class Kpi:
    """A summary number. ``key`` is stable (docs/spec/results.md);
    ``label`` is what the app shows and may change between versions."""

    key: str
    label: str
    value: float
    unit: str
    not_valid: Optional[str] = None  # why the number is not valid, or None
    limit: Optional[float] = None
    passed: Optional[bool] = None


@dataclass
class Result:
    """What :func:`lightsim.run` returns, or :func:`lightsim.read_run` reads.

    ``status`` is the run's: ``success``, ``warning``, ``failed`` or
    ``cancelled``. :attr:`kpis` maps each summary number's stable key to its
    value; :attr:`summary` keeps the full rows (label, unit, why a number is
    not valid). :attr:`checks` holds the Data Checks when they stopped the
    run before it started.
    """

    case_id: str
    status: str
    case_name: str = ""
    project_name: str = ""
    messages: list[Message] = field(default_factory=list)
    summary: list[Kpi] = field(default_factory=list)
    time: list[float] = field(default_factory=list)
    channels: dict[str, Channel] = field(default_factory=dict)
    checks: list[Check] = field(default_factory=list)
    raw: Any = None  # the engine's SimResult, for anything not wrapped here

    # -- reading -----------------------------------------------------------
    @property
    def ok(self) -> bool:
        """True when the run is a *success* (finished, followed its cycle,
        stayed in its data, no warning)."""
        return self.status == "success"

    @property
    def valid(self) -> bool:
        """True when the run is a success and no summary number is marked
        *not valid*."""
        return self.ok and not any(k.not_valid for k in self.summary)

    @property
    def kpis(self) -> dict[str, float]:
        """Summary numbers by stable key, e.g. ``{"distance_km": 7.292, …}``."""
        return {k.key: k.value for k in self.summary if k.key}

    @property
    def units(self) -> dict[str, str]:
        """The unit of each summary number, by stable key."""
        return {k.key: k.unit for k in self.summary if k.key}

    @property
    def not_valid(self) -> dict[str, str]:
        """Why a summary number is not valid, for the ones that are not."""
        return {k.key: k.not_valid for k in self.summary if k.key and k.not_valid}

    def channel(self, ref: str) -> Channel:
        """A channel by its key (``"el-battery:sig_soc"``) or its label
        (``"HV Battery Pack · SOC"``)."""
        if ref in self.channels:
            return self.channels[ref]
        found = [c for c in self.channels.values() if c.label == ref]
        if len(found) == 1:
            return found[0]
        raise KeyError(f"No channel '{ref}'. Channels: {', '.join(sorted(self.channels))}")

    # -- export ------------------------------------------------------------
    def to_dict(self, channels: bool = True) -> dict:
        """The result as plain JSON-ready data (docs/spec/results.md);
        ``channels=False`` leaves the time series out."""
        out = {
            "format": "lightsim-result", "formatVersion": 1,
            "project": self.project_name, "caseId": self.case_id, "caseName": self.case_name,
            "status": self.status, "valid": self.valid,
            "kpis": self.kpis, "units": self.units, "notValid": self.not_valid,
            "summary": [_kpi_dict(k) for k in self.summary],
            "messages": [{"level": m.level, "text": m.text} for m in self.messages],
        }
        if self.checks:
            out["checks"] = [c.to_dict() for c in self.checks]
        if channels:
            out["time"] = self.time
            out["channels"] = {c.key: {"label": c.label, "unit": c.unit, "values": list(c.values)}
                               for c in self.channels.values()}
        return out

    def to_json(self, path: str | Path | None = None, channels: bool = True) -> str:
        """The result as JSON text, also written to ``path`` if given."""
        text = json.dumps(self.to_dict(channels), ensure_ascii=False, indent=1)
        if path is not None:
            Path(path).write_text(text, encoding="utf-8")
        return text

    def to_csv(self, path: str | Path) -> None:
        """Every channel against time, one column each. The header row holds
        "label [unit]" (the app's CSV export does the same), so the file
        opens in a spreadsheet as it is; an empty cell is a gap."""
        cols = list(self.channels.values())
        with open(path, "w", newline="", encoding="utf-8") as f:
            w = csv.writer(f)
            w.writerow(["Time [s]", *(f"{c.label} [{c.unit}]" for c in cols)])
            for i, t in enumerate(self.time):
                w.writerow([_num(t), *(_num(c.values[i]) if i < len(c.values) else ""
                                       for c in cols)])

    def to_mat(self, path: str | Path) -> None:
        """A MAT-file (Level 5, MATLAB ``save -v6``) for MATLAB, GNU Octave or
        ``scipy.io.loadmat``: ``time`` (s), one column per channel named
        after its label, and the structs ``units`` (each channel's unit),
        ``channel_keys`` (each channel's key), ``kpis`` (summary numbers by
        key, with ``.`` and other signs as ``_``), ``kpi_units`` and
        ``info`` (project, case, status)."""
        taken = {"time", "units", "channel_keys", "kpis", "kpi_units", "info"}
        variables: dict[str, Any] = {"time": self.time}
        units, keys = {}, {}
        for c in self.channels.values():
            name = matfile.valid_name(c.label, taken)
            variables[name] = list(c.values)
            units[name], keys[name] = c.unit, c.key
        kpi_names: set[str] = set()
        kpis, kpi_units = {}, {}
        for k in self.summary:
            if not k.key:
                continue
            name = matfile.valid_name(k.key, kpi_names)
            kpis[name], kpi_units[name] = k.value, k.unit
        variables.update(units=units, channel_keys=keys, kpis=kpis, kpi_units=kpi_units,
                         info={"project": self.project_name, "case": self.case_name,
                               "case_id": self.case_id, "status": self.status})
        matfile.write(path, variables, f"LightSim run of '{self.case_name or self.case_id}'")

    @property
    def df(self):
        """The channels as a pandas DataFrame indexed by time (s), one column
        per channel label; ``df.attrs["units"]`` maps each column to its unit.
        Needs pandas (``pip install pandas``)."""
        import pandas as pd  # optional: only this property needs it

        data = {c.label: [float("nan") if v is None else v for v in c.values]
                for c in self.channels.values()}
        frame = pd.DataFrame(data, index=pd.Index(self.time, name="Time [s]"))
        frame.attrs["units"] = {c.label: c.unit for c in self.channels.values()}
        frame.attrs["kpis"] = self.kpis
        return frame

    def __repr__(self) -> str:
        return (f"<lightsim.Result case={self.case_name or self.case_id!r} status={self.status} "
                f"kpis={len(self.kpis)} channels={len(self.channels)}>")


def _num(v: Optional[float]) -> str:
    return "" if v is None else repr(float(v))


def _kpi_dict(k: Kpi) -> dict:
    out = {"key": k.key, "label": k.label, "value": k.value, "unit": k.unit}
    for name, value in (("notValid", k.not_valid), ("limit", k.limit), ("passed", k.passed)):
        if value is not None:
            out[name] = value
    return out


def from_sim_result(sim, case_name: str = "", project_name: str = "",
                    checks: list[Check] | None = None) -> Result:
    """Wrap the engine's SimResult."""
    times: list[float] = []
    channels: dict[str, Channel] = {}
    for ch in sim.channels:
        if len(ch.timeSeries) > len(times):
            times = [float(p["t"]) for p in ch.timeSeries]
        key = f"{ch.elementId}:{ch.portId}"
        channels[key] = Channel(key, ch.label, ch.unit,
                                tuple(p.get("value") for p in ch.timeSeries))
    return Result(
        case_id=sim.caseId, status=sim.status, case_name=case_name, project_name=project_name,
        messages=[Message(m.level, m.text) for m in sim.messages],
        summary=[Kpi(s.key, s.label, s.value, s.unit, s.notValid, s.limit, s.passed)
                 for s in sim.summary],
        time=times, channels=channels, checks=list(checks or []), raw=sim,
    )


def _from_export(raw: dict) -> Result:
    """A Result back from :meth:`Result.to_dict` (``lightsim run -o x.json``)."""
    if raw.get("formatVersion") != 1:
        raise ValueError(f"lightsim-result format version {raw.get('formatVersion')} is newer "
                         f"than this LightSim reads (1).")
    return Result(
        case_id=raw["caseId"], status=raw["status"], case_name=raw.get("caseName", ""),
        project_name=raw.get("project", ""),
        messages=[Message(m["level"], m["text"]) for m in raw.get("messages", [])],
        summary=[Kpi(k["key"], k["label"], k["value"], k["unit"], k.get("notValid"),
                     k.get("limit"), k.get("passed")) for k in raw.get("summary", [])],
        time=list(raw.get("time", [])),
        channels={key: Channel(key, c["label"], c["unit"], tuple(c["values"]))
                  for key, c in raw.get("channels", {}).items()},
        checks=[Check(c["level"], c["text"], c.get("fix"), tuple(c.get("elementIds", [])),
                      c.get("caseId")) for c in raw.get("checks", [])],
    )


def read_run(path: str | Path) -> Result:
    """Read a run the app stored (``runs/<project id>/<run id>.json.gz`` in
    the projects folder), a SimResult saved as JSON, or the JSON that
    :meth:`Result.to_json` (``lightsim run -o run.json``) writes."""
    schemas = engine("schemas")
    data = Path(path).read_bytes()
    if data[:2] == b"\x1f\x8b":  # gzip
        data = gzip.decompress(data)
    raw = json.loads(data)
    if isinstance(raw, dict) and raw.get("format") == "lightsim-result":
        return _from_export(raw)
    if "result" in raw:  # a StoredRun, as the app keeps it
        run = schemas.StoredRun.model_validate(raw)
        project = run.snapshot.project.name if run.snapshot else ""
        return from_sim_result(run.result, run.caseName, project)
    return from_sim_result(schemas.SimResult.model_validate(raw))
