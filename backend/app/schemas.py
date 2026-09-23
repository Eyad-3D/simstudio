"""Pydantic models mirroring the SimStudio project data model (spec §4).

Field names use camelCase to match the frontend/JSON representation 1:1.
The models a project file is made of accept fields they do not know and keep
them, so a save through the engine never drops what a (newer) UI stored.
"""
from __future__ import annotations

from typing import Literal, Optional, Union

from pydantic import BaseModel, ConfigDict, Field

ScalarValue = Union[bool, int, float, str]
# Tabular parameter data is a dict keyed by the independent variable
# (JSON object keys are strings; they hold numeric text, e.g. "1500").
# table1d: {x: value}; table2d: {x_outer: {x_inner: value}}.
Table1D = dict[str, float]
Table2D = dict[str, dict[str, float]]
ParamValue = Union[ScalarValue, Table2D, Table1D]

# Project-file models keep unknown fields (canvas layout, newer UI data) on a
# load → save round trip instead of silently dropping them.
PERSISTED = ConfigDict(extra="allow")


class PortDef(BaseModel):
    id: str
    name: str
    direction: Literal["input", "output", "bidirectional"]
    kind: Literal["power", "signal", "mechanical", "electrical", "thermal", "fluid"]
    unitGroup: Optional[str] = None
    side: Optional[Literal["left", "right"]] = None  # canvas layout hint
    # Electrical polarity: positive (supply/+) or negative (return/−).
    # Neutral junctions (node, ground) leave this unset.
    polarity: Optional[Literal["positive", "negative"]] = None


class AxisDef(BaseModel):
    """Independent-variable axis of a tabular parameter (fixed per component)."""

    name: str
    unit: str


class ParameterDef(BaseModel):
    key: str
    label: str
    # Required for every parameter; "-" marks dimensionless quantities.
    # For table parameters this is the unit of the dependent value.
    unit: str
    default: ParamValue
    type: Literal["number", "enum", "boolean", "string", "code", "table1d", "table2d"]
    options: Optional[list[str]] = None
    # table1d: exactly one axis; table2d: [outer, inner] axes.
    axes: Optional[list[AxisDef]] = None
    # FMI-style variability: "fixed" parameters are baked in at model build
    # (a live edit takes effect on the next run); "tunable" ones apply live.
    variability: Literal["fixed", "tunable"] = "tunable"


class ComponentDef(BaseModel):
    id: str
    category: str
    name: str
    icon: str
    domain: Literal["mechanical", "electrical", "thermal", "signal", "fluid"]
    description: Optional[str] = None
    ports: list[PortDef]
    parameters: list[ParameterDef]
    # Elements of this type may carry per-instance signal ports (Script, Monitor).
    allowDynamicPorts: bool = False


class ElementInstance(BaseModel):
    model_config = ConfigDict(populate_by_name=True, extra="allow")

    id: str
    componentDefId: str
    label: str
    position: dict[str, float]
    parameterOverrides: dict[str, ParamValue] = Field(default_factory=dict)
    # Per-instance signal ports; only honored when the component definition
    # sets allowDynamicPorts (validated in data checks).
    dynamicPorts: list[PortDef] = Field(default_factory=list)
    # Per-instance canvas pin placement overrides: port id → left/right/top/bottom.
    portSides: dict[str, Literal["left", "right", "top", "bottom"]] = Field(default_factory=dict)
    # Per-instance pin offset along its side, 0..1 (Shift+drag a pin).
    portOffsets: dict[str, float] = Field(default_factory=dict)
    # Canvas node size in flow units ({width, height}); None = default size.
    size: Optional[dict[str, float]] = None
    isSubSystem: bool = False
    subSystemId: Optional[str] = None  # SystemNode this element drills into


class Connection(BaseModel):
    model_config = PERSISTED

    id: str
    sourceElementId: str
    sourcePortId: str
    targetElementId: str
    targetPortId: str


class DataBusConnection(BaseModel):
    model_config = PERSISTED

    id: str
    element1Id: str
    port1Id: str
    element2Id: str
    port2Id: str


class SystemNode(BaseModel):
    model_config = PERSISTED

    id: str
    name: str
    parentId: Optional[str] = None
    elements: list[ElementInstance] = Field(default_factory=list)
    connections: list[Connection] = Field(default_factory=list)


class SimCase(BaseModel):
    model_config = PERSISTED

    id: str
    name: str
    duration: float = 600.0
    # output step: results are stored and live edits applied at this interval;
    # controllers, signal blocks and physics all run at the solver step (≤ 10 ms)
    timeStep: float = 1.0
    # record a data point every N output steps (further decimation); 1 = every step
    outputEvery: int = 1
    # 0 = run as fast as possible; N > 0 = pace at N× real time (for live tuning)
    realtimeFactor: float = 0.0
    # Per-case parameter overrides: {elementId: {paramKey: value}}. Layered on
    # top of each element's own parameterOverrides at model-build time, so a
    # case can tweak values — and a parameter sweep can vary one — without
    # editing the shared topology.
    parameterOverrides: dict[str, dict[str, ParamValue]] = Field(default_factory=dict)


class Project(BaseModel):
    model_config = PERSISTED

    id: str
    name: str
    # Project-file format version; bump when the shape changes so loaders can
    # migrate. Files written before versioning load as version 1.
    schemaVersion: int = 1
    # Short human-readable summary, shown in the Open menu so example projects
    # are self-describing. Optional — user-created projects usually omit it.
    description: str | None = None
    systems: list[SystemNode]
    dataBusConnections: list[DataBusConnection] = Field(default_factory=list)
    cases: list[SimCase] = Field(default_factory=list)


class SimMessage(BaseModel):
    level: Literal["info", "warning", "error"]
    text: str


class Channel(BaseModel):
    elementId: str
    portId: str
    label: str
    unit: str
    # {t, value}; value is null where a channel has no data yet (gap, not zero)
    timeSeries: list[dict[str, Optional[float]]]


class SummaryValue(BaseModel):
    label: str
    value: float
    unit: str
    # why this number is not valid (the run verdict), e.g. "cycle not followed"
    notValid: Optional[str] = None


class SimResult(BaseModel):
    caseId: str
    status: Literal["success", "failed", "warning"]
    messages: list[SimMessage]
    channels: list[Channel]
    summary: list[SummaryValue] = Field(default_factory=list)


class DataCheck(BaseModel):
    level: Literal["info", "warning", "error"]
    elementId: Optional[str] = None
    elementLabel: Optional[str] = None
    text: str


class SimulateRequest(BaseModel):
    project: Project
    caseId: str


class ValidateRequest(BaseModel):
    project: Project
