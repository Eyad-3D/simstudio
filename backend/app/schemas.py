"""Pydantic models mirroring the LightSim project data model (spec §4).

Field names use camelCase to match the frontend/JSON representation 1:1.
The models a project file is made of accept fields they do not know and keep
them, so a save through the engine never drops what a (newer) UI stored.
"""
from __future__ import annotations

from typing import Any, Literal, Optional, Union

from pydantic import BaseModel, ConfigDict, Field

from .migrations import CURRENT_VERSION

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


# What a table does outside its data on one axis: stop the run, hold the edge
# value, or extend the edge segment's slope (MOD-18).
OutsidePolicy = Literal["error", "clamp", "linear"]


class AxisDef(BaseModel):
    """Independent-variable axis of a tabular parameter (fixed per component)."""

    name: str
    unit: str
    # The library's "outside the data" setting for this axis; None for a
    # table that is not interpolated (gear ratios), which behaves as clamp.
    outside: Optional[OutsidePolicy] = None


class ShowIf(BaseModel):
    key: str
    values: list[ScalarValue]


class ParameterDef(BaseModel):
    key: str
    label: str
    # Required for every parameter; "-" marks dimensionless quantities.
    # For table parameters this is the unit of the dependent value.
    unit: str
    default: ParamValue
    # "file": the value names a file attached to the project
    # ("resources/<name>", STD-02); "" while none is chosen
    type: Literal["number", "enum", "boolean", "string", "code", "table1d", "table2d", "file"]
    options: Optional[list[str]] = None
    # file parameters: the file extensions it takes, e.g. [".fmu"]
    accept: Optional[list[str]] = None
    # table1d: exactly one axis; table2d: [outer, inner] axes.
    axes: Optional[list[AxisDef]] = None
    # FMI-style variability: "fixed" parameters are baked in at model build
    # (a live edit takes effect on the next run); "tunable" ones apply live.
    variability: Literal["fixed", "tunable"] = "tunable"
    # Help (LRN-05): what it is in plain words, the usual values (per kind of
    # vehicle where that matters) and where to find the real number.
    description: Optional[str] = None
    typical: Optional[str] = None
    whereToFind: Optional[str] = None
    # Hard limits of a number, named as in JSON Schema: a value outside them
    # is a Data Check error and turns the field red as it is typed (UX-10).
    minimum: Optional[float] = None
    exclusiveMinimum: Optional[float] = None
    maximum: Optional[float] = None
    # Shown only while another parameter of the part has one of these values
    # ({"key": "pack_model", "values": ["Cells"]}); the UI hides it otherwise.
    showIf: Optional[ShowIf] = None

    def range_problem(self, value: float) -> Optional[str]:
        """Why `value` breaks the limits ("must be above 0 and at most 100 %"),
        or None. frontend/src/paramRules.ts words it the same way."""
        low, above, high = self.minimum, self.exclusiveMinimum, self.maximum
        if ((low is None or value >= low) and (above is None or value > above)
                and (high is None or value <= high)):
            return None
        parts = ([f"above {above:g}"] if above is not None
                 else [f"at least {low:g}"] if low is not None else [])
        if high is not None:
            parts.append(f"at most {high:g}")
        unit = "" if self.unit == "-" else f" {self.unit}"
        return f"must be {' and '.join(parts)}{unit}"


class ComponentPreset(BaseModel):
    """Named parameter values for a component, e.g. a competition's limits,
    with a note on where they come from."""

    name: str
    values: dict[str, ParamValue]
    note: str = ""


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
    # Sets of parameter values the Properties panel applies in one step.
    presets: list[ComponentPreset] = Field(default_factory=list)


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
    # Per-instance "outside the data" settings: table key → one per axis,
    # overriding the library's (AxisDef.outside). Not a parameter, so case
    # overrides and sweeps cannot change it.
    tableOutside: dict[str, list[OutsidePolicy]] = Field(default_factory=dict)
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


# a reference value's grade (VAL-35): green, amber, red, grey
Grade = Literal["within", "near", "outside", "missing", "not valid"]


class ReferenceValue(BaseModel):
    """An expected value of a case: a summary value's label, the number,
    a tolerance (in the value's unit, or % of the reference) and a source."""

    model_config = ConfigDict(extra="allow")

    kpi: str
    value: float
    tolerance: float = 5.0
    tolerancePct: bool = True
    source: str = ""


class ReferenceCheck(BaseModel):
    """How far a run's value is from a reference, or from a hand calculation."""

    label: str
    value: Optional[float] = None
    reference: float
    unit: str = ""
    difference: Optional[float] = None
    differencePct: Optional[float] = None
    tolerance: float = 0.0  # absolute, in the unit
    grade: Grade
    source: str = ""
    automatic: bool = False
    # "two-sided": |gap| ≤ tolerance; "at most"/"at least": a bound the
    # value must keep to (hand calculations)
    bound: Literal["two-sided", "at most", "at least"] = "two-sided"
    note: Optional[str] = None


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
    # "performance": the Driver holds full throttle until the car reaches its
    # target (its PI holds the target after that), and the run reports the
    # time to the target and the maximum speed instead of judging the speed
    # trace (a 0-100 km/h or top-speed test). "acceleration": the Driver holds
    # full throttle for the whole run (its target is not read) and the run is
    # timed from the start line to endDistance past it, with the duration as
    # its time limit (a Formula Student 75 m acceleration run). "lap": the
    # model's Race Track sets the run (its layout and laps, per case through
    # parameterOverrides): a quasi-steady-state lap solver finds the speed
    # along it and the motors and battery drive that trace, so duration,
    # timeStep and realtimeFactor do not apply
    kind: Literal["cycle", "performance", "acceleration", "lap"] = "cycle"
    # end the run when the vehicle has driven startLine + endDistance, m;
    # None or 0 = run the whole duration
    endDistance: Optional[float] = None
    # end the run after this many passes through the profile of the Driving
    # Task the Driver follows, when its Profile Axis is Distance (a lap is its
    # last point's distance less its first's), counted from startLine;
    # endDistance wins when both are set. None or 0 = no lap count
    endLaps: Optional[float] = None
    # run the cycle again from the charge it ended with until the battery's
    # stored energy changes by less than 1 % of the fuel's energy (at most 5
    # runs; solver/balance.py, ENG-33). None = on for a cycle case of a hybrid
    # (an engine, a battery and an E-Motor) that is not paced, False = off,
    # True = on
    chargeBalance: Optional[bool] = None
    # distance driven before the timer starts, m (FS Rules 2026 v1.1 (FSG) D 5.2.3
    # stages the car 0.30 m behind the start line)
    startLine: float = 0.0
    # a time to compare the acceleration test's time with, s (e.g. last
    # year's best run); None = none
    referenceTime: Optional[float] = None
    # build the run's energy report (RES-22): where the sources' energy went,
    # per part and as a Sankey chart
    energyReport: bool = True
    # a Formula Student dynamic event this case stands for (MOD-43): the run
    # then reports the event's time as the rules take it, an estimate of its
    # points against referenceTime (the fastest team's time, Tmin) and the
    # rule checks; "endurance" also stops the car for the driver change at
    # half distance and reports the efficiency. None = not an event
    fsEvent: Optional[Literal["acceleration", "skidpad", "autocross", "endurance"]] = None
    # the most efficient team's endurance energy, kWh, and its driving time,
    # s (None: referenceTime), for the efficiency points (FS Rules 2026 v1.1
    # (FSG) D 9.4: EFmin = T² · E of that team)
    referenceEnergy: Optional[float] = None
    referenceEnergyTime: Optional[float] = None
    # Per-case parameter overrides: {elementId: {paramKey: value}}. Layered on
    # top of each element's own parameterOverrides at model-build time, so a
    # case can tweak values — and a parameter sweep can vary one — without
    # editing the shared topology.
    parameterOverrides: dict[str, dict[str, ParamValue]] = Field(default_factory=dict)
    # expected values (VAL-35): a summary value's label, the number, its
    # tolerance and where it comes from; each run says how far it lands
    references: list[ReferenceValue] = Field(default_factory=list)


class StudyFactor(BaseModel):
    """One swept parameter of a study, with its labels as they were when it
    ran (the element may be renamed or removed since)."""

    model_config = PERSISTED

    elementId: str
    paramKey: str
    elementLabel: str = ""
    paramLabel: str = ""
    unit: str = ""
    values: list[float] = Field(default_factory=list)


class StudyKpi(BaseModel):
    """A column of a study's results table: a run summary value."""

    model_config = PERSISTED

    label: str
    unit: str = ""


class StudyPoint(BaseModel):
    """A row of a study's results table: one point and its answers."""

    model_config = PERSISTED

    values: list[float]  # the factor values, in factor order
    runId: Optional[str] = None  # the run may since have left the history
    status: Literal["success", "failed", "warning", "cancelled", "not run"]
    incomplete: Optional[str] = None  # why its run did not finish normally
    kpis: dict[str, float] = Field(default_factory=dict)  # KPI label → value
    notValid: dict[str, str] = Field(default_factory=dict)  # KPI label → why
    wallS: Optional[float] = None  # its run's wall time, s (ENG-05)


class Study(BaseModel):
    """A parameter study (STU-03): what was swept on which case, and its
    compact results table. Kept with the project's runs, not in the model
    file (PLT-34), so running a sweep never changes the model."""

    model_config = PERSISTED

    id: str
    startedAt: int  # epoch ms
    caseId: str
    caseName: str = ""
    factors: list[StudyFactor]
    kpis: list[StudyKpi] = Field(default_factory=list)
    points: list[StudyPoint] = Field(default_factory=list)
    # how many points ran at once and the study's wall time, s (ENG-05)
    workers: Optional[int] = None
    wallS: Optional[float] = None


class ExampleCard(BaseModel):
    """What an example answers and what to expect from it (CON-15): shown in
    the Project tab and the Open menu; its expected results are the cases'
    reference values (SimCase.references)."""

    model_config = PERSISTED

    question: str = ""
    tags: list[str] = Field(default_factory=list)
    difficulty: Literal["beginner", "intermediate", "advanced"] = "beginner"
    runTimeS: Optional[float] = None  # about how long its cases take to run, s
    learn: list[str] = Field(default_factory=list)  # what you will learn
    # demo: shows the workflow only; plausibility-checked: its results fall in
    # bands from real cars; validated: compared with measurements of that car
    status: Literal["demo", "plausibility-checked", "validated"] = "demo"
    features: list[str] = Field(default_factory=list)
    author: str = ""
    version: str = ""
    licence: str = ""
    narrative: list[str] = Field(default_factory=list)  # what happens when


class Attachment(BaseModel):
    """A file kept with the project (STD-02), in its resources folder. The
    hash is the file's when it was attached, so a changed file is noticed."""

    model_config = PERSISTED

    # where it is, relative to the project: "resources/<name>"
    path: str = Field(pattern=r"^resources/[^/\\.][^/\\]*$")
    sha256: str = Field(pattern=r"^[0-9a-f]{64}$")
    bytes: int = Field(ge=0)


class ProjectCycle(BaseModel):
    """A drive cycle of the user's own, kept in the project file (CON-11):
    a speed (km/h), and optionally a road grade (%), at each point of
    ``x``: the time in s, or with ``axis`` "distance" the distance driven
    in m (a speed or a grade, or both, against distance). A Driving Task or
    Road Profile names it by ``id`` in its ``cycle`` parameter, as it names
    a bundled one (app/cycles.py checks the fields together)."""

    model_config = PERSISTED

    # "own:" keeps it apart from the bundled ids, so a LightSim that does not
    # know the project's cycles says so instead of driving a bundled one
    id: str = Field(pattern=r"^own:[A-Za-z0-9._-]{1,64}$")
    name: str = Field(min_length=1, max_length=200)
    axis: Literal["time", "distance"] = "time"
    x: list[float] = Field(max_length=100_000)
    speed: Optional[list[float]] = Field(None, max_length=100_000)
    grade: Optional[list[float]] = Field(None, max_length=100_000)
    # where the data came from (the file it was imported from), and a note
    source: str = Field("", max_length=1000)
    note: str = Field("", max_length=2000)


def _project_schema_extra(schema: dict) -> None:
    # read from the file's extra fields, not a model field, so files that
    # leave it out stay byte-for-byte as they were (lightsim/ai_access.py)
    schema["properties"]["noAi"] = {
        "type": "boolean", "default": False,
        "description": "true hides the project from AI tools (the MCP server, an in-app "
                       "assistant), whatever folders they may see."}


class Project(BaseModel):
    model_config = ConfigDict(extra="allow", json_schema_extra=_project_schema_extra)

    id: str
    name: str
    # Project-file format version (app/migrations.py upgrades older files on
    # load; files written before versioning are version 1) and the LightSim
    # that saved the file; both are set on every save (PLT-07).
    schemaVersion: int = CURRENT_VERSION
    savedWith: Optional[str] = None
    # Short human-readable summary, shown in the Open menu so example projects
    # are self-describing. Optional — user-created projects usually omit it.
    description: str | None = None
    systems: list[SystemNode]
    dataBusConnections: list[DataBusConnection] = Field(default_factory=list)
    cases: list[SimCase] = Field(default_factory=list)
    # an example's card (CON-15); user projects may have one too
    card: Optional[ExampleCard] = None
    # files kept with the project (FMUs, AI models, measured data), STD-02
    attachments: list[Attachment] = Field(default_factory=list)
    # drive cycles of the user's own (CON-11); left out of the file when
    # there are none, so a project without stays as it was
    cycles: list[ProjectCycle] = Field(default_factory=list, exclude_if=lambda v: not v)


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
    # the lowest, highest and time-averaged value over the output interval
    # that ends at each point, taken at every solver step (ENG-16), so a peak
    # between two recorded points is kept; aligned with timeSeries (null
    # where it has no data). Absent when each interval is one solver step
    min: Optional[list[Optional[float]]] = None
    max: Optional[list[Optional[float]]] = None
    mean: Optional[list[Optional[float]]] = None


class SummaryValue(BaseModel):
    # stable name of the figure, for scripts, the CLI and studies (AI-07):
    # "<elementId>.<metric>" for a part's figure, "<metric>" for the run's
    # (docs/spec/results.md lists them). Never changes with a label or a
    # language; "" on runs stored before keys existed
    key: str = ""
    label: str
    value: float
    unit: str
    # why this number is not valid (the run verdict), e.g. "cycle not followed"
    notValid: Optional[str] = None
    # a check's limit, in the row's unit, and whether the value kept to it
    limit: Optional[float] = None
    passed: Optional[bool] = None


class PartEnergyFlow(BaseModel):
    """One part's energy over a run (MOD-10), in kWh, with the duty values
    of its throughput power in kW. For every part energyIn − energyOut −
    losses − stored = 0; energyIn and energyOut are both ≥ 0 (what entered
    and what left it at any port), and energyInReverse is the part of
    energyIn that came back from the road side (regeneration, a dragged
    engine)."""

    elementId: Optional[str] = None  # None for a driveline's rotating parts
    label: str
    part: str  # the component type ("motor.emotor"), or "driveline.inertia"
    energyIn: float
    energyOut: float
    losses: float
    stored: float  # change in the energy it stores (+ when it fills)
    energyInReverse: float = 0.0
    # its throughput power's largest magnitude, mean and root mean square,
    # kW (None for wheels and rotating parts, which keep none)
    peakPower: Optional[float] = None
    meanPower: Optional[float] = None
    rmsPower: Optional[float] = None
    # named parts of its losses or stored energy, kWh (the Vehicle's air
    # drag, rolling resistance, climbing and acceleration)
    terms: dict[str, float] = Field(default_factory=dict)
class EnergyPart(BaseModel):
    """One row of the energy table (RES-22), kWh: a part's own books
    (MOD-10, as in SimResult.partEnergy), so in − out − lost − stored is 0."""

    elementId: Optional[str] = None  # None for a row that is not one part
    label: str
    kind: str  # its component type ("driveline.inertia": a driveline's spinning parts)
    inKWh: float
    outKWh: float
    lostKWh: float  # lost, or used by a consumer
    storedKWh: float = 0.0  # the change of what it stores
    lostPct: float = 0.0  # its loss as a share of the sources' energy, %


class EnergyFlow(BaseModel):
    """A band of the energy Sankey chart: a source of the car's energy or
    a place it went."""

    label: str
    kWh: float
    # "source" or "released" (height or speed given up) on the left;
    # "road", "stored", "brakes", "losses", "loads", "recovered" on the right
    group: str
    elementId: Optional[str] = None


class EnergyReport(BaseModel):
    parts: list[EnergyPart] = Field(default_factory=list)
    sources: list[EnergyFlow] = Field(default_factory=list)
    sinks: list[EnergyFlow] = Field(default_factory=list)
    sourceKWh: float = 0.0  # the sources together
    # what the books do not explain: the sources less the sinks, where the
    # parts' books together do not close (the summary's Energy balance
    # residual, counted the other way round)
    remainderKWh: float = 0.0
    remainderPct: float = 0.0  # of the sources' energy
    # the run's electrical energy balance error, %, as its summary row
    balanceErrorPct: Optional[float] = None


class DutyRow(BaseModel):
    """A part's duty for one quantity (RES-39), over the run's solver
    steps: the highest and lowest value, the time mean and the RMS (the
    root-mean-square, the mean that sets heating)."""

    quantity: str
    unit: str
    max: float
    min: float
    mean: float
    rms: float


class DutyPart(BaseModel):
    elementId: str
    label: str
    kind: str
    rows: list[DutyRow]


class LimitLane(BaseModel):
    """What held one driveline back (RES-38): the state from each change
    on, [t in s, index into LimitReport.states], and the seconds in each."""

    label: str  # its motors and engines
    elementIds: list[str]
    changes: list[list[float]]
    seconds: dict[str, float]


class LimitReport(BaseModel):
    states: list[str]
    lanes: list[LimitLane]
    tEnd: float


class SimResult(BaseModel):
    caseId: str
    status: Literal["success", "failed", "warning", "cancelled"]
    messages: list[SimMessage]
    channels: list[Channel]
    summary: list[SummaryValue] = Field(default_factory=list)
    # where the energy went, part by part, from every part's own books (MOD-10)
    partEnergy: list[PartEnergyFlow] = Field(default_factory=list)
    # the run's reports (absent on runs from before 0.3 and failed runs)
    energy: Optional[EnergyReport] = None
    duty: list[DutyPart] = Field(default_factory=list)
    limits: Optional[LimitReport] = None
    # the case's expected values and the automatic hand calculations, each
    # with its gap and grade (VAL-35)
    references: list[ReferenceCheck] = Field(default_factory=list)


class LiveEdit(BaseModel):
    """A scalar parameter change sent to the engine while a run was going."""

    model_config = PERSISTED

    t: float  # simulated time the run had reached when it was sent, s
    elementId: str
    key: str
    value: ScalarValue


class RunSnapshot(BaseModel):
    """What made a run (RES-09): the project and case exactly as they were
    run (a sweep's value included), the app version, a fingerprint of the
    project, and the live parameter edits made while it ran."""

    model_config = PERSISTED

    project: Project
    case: SimCase
    appVersion: Optional[str] = None
    # SHA-256 of the project as canonical JSON (keys sorted), hex
    modelHash: Optional[str] = None
    liveEdits: list[LiveEdit] = Field(default_factory=list)


class StoredRun(BaseModel):
    """A finished run as the UI keeps it in its run history (``SimRun`` in
    frontend/src/types.ts); stored on disk by :mod:`app.run_store`."""

    id: str
    caseId: str
    caseName: str
    startedAt: int  # epoch ms
    status: Literal["success", "failed", "warning", "cancelled"]
    result: SimResult
    # sweep membership and the swept value (parameter sweeps only)
    sweepId: Optional[str] = None
    sweepParam: Optional[str] = None
    sweepValue: Optional[float] = None
    sweepUnit: Optional[str] = None
    # why the run is not a complete result (stopped, failed, connection lost)
    incomplete: Optional[str] = None
    # absent on runs stored before runs kept one
    snapshot: Optional[RunSnapshot] = None
    # its label (by default what changed since the previous run of its case)
    # and the user's note; both listed in the index (RES-10)
    name: Optional[str] = Field(None, max_length=120)
    note: Optional[str] = Field(None, max_length=4000)


class DataCheck(BaseModel):
    level: Literal["info", "warning", "error"]
    elementId: Optional[str] = None
    elementLabel: Optional[str] = None
    # every part the check is about (elementId first), for the Problems list
    # to select and frame; empty when it is about no part
    elementIds: list[str] = Field(default_factory=list)
    text: str
    # what to do about it, when the text does not say
    fix: Optional[str] = None
    # the case it is about (its own values or kind): it stops only that
    # case's runs. None: it is about the model, and stops every run
    caseId: Optional[str] = None


class SimulateRequest(BaseModel):
    project: Project
    caseId: str


class StudyPointRequest(BaseModel):
    """One point of a study: the case values it sets ({element id: {key:
    value}}), its factor values for the study table and its run's name."""

    overrides: dict[str, dict[str, ParamValue]] = Field(default_factory=dict)
    values: list[float] = Field(default_factory=list)
    label: str = Field("", max_length=200)


class StudyRequest(BaseModel):
    """A study to run on all cores (ENG-05, app/studies.py)."""

    project: Project
    caseId: str
    points: list[StudyPointRequest] = Field(min_length=1, max_length=2000)
    sweepId: str = Field("", max_length=100)
    sweepParam: Optional[str] = Field(None, max_length=200)
    sweepUnit: Optional[str] = Field(None, max_length=50)
    workers: Optional[int] = Field(None, ge=1, le=256)  # None: the default pool size
    store: bool = True  # store each point as a run of the project
class LabelEstimateRequest(BaseModel):
    """CON-32: a project, the case to base the two EPA runs on (None: its
    first Cycle case) and the model year that picks EPA's coefficients."""
    project: Project
    caseId: Optional[str] = None
    modelYear: int = 2017


class VehicleTestsRequest(BaseModel):
    """CON-06: a project and the tests to run on it (None: all)."""
    project: Project
    tests: Optional[list[str]] = None


class TemplateNewRequest(BaseModel):
    """CON-18: the form's values (by field index, or 'elementId.key') and
    the new project's name."""
    values: dict[str, ParamValue] = Field(default_factory=dict)
    name: Optional[str] = None


class TemplateSaveRequest(BaseModel):
    """CON-18: a model saved as a template, with the form it will ask."""
    project: Project
    name: str
    description: str = ""
    form: list[dict] = Field(default_factory=list)
    slots: dict[str, str] = Field(default_factory=dict)


class ErrorDetail(BaseModel):
    detail: str


class FmuRef(BaseModel):
    """An FMU block's file parameters (see app/fmu/store.locate)."""
    fmuPath: str = ""
    fmuSha256: str = ""
    fmuName: str = ""


class FmuImport(BaseModel):
    """An FMU file as the import dialog shows it."""
    found: bool = True
    #: why the file was not found (found = False)
    problem: str = ""
    sha256: str = ""
    path: str = ""
    name: str = ""
    #: allowed to run on this computer
    allowed: bool = False
    #: app/fmu/info.describe: variables, FMI version, kind, tool, platforms
    info: dict[str, Any] = Field(default_factory=dict)


class ValidateRequest(BaseModel):
    project: Project


class LapLogRequest(BaseModel):
    """A lap from a data logger or lap simulator to read (STD-35)."""

    text: str
    preset: str = "Generic"
    # {"time" | "distance" | "speed" | "lap": column name} over the preset's
    columns: dict[str, Optional[str]] = Field(default_factory=dict)
    speedUnit: Optional[str] = None
    lap: Optional[int] = None
    repeatToKm: float = 0.0
    driverChangeS: float = 0.0


class LoggedLapIn(BaseModel):
    """A logged lap for the lap mode calibration (VAL-38)."""

    text: str
    # {"time" | "distance" | "speed" | "lat_accel" | "power" | "lap": column}
    columns: dict[str, Optional[str]] = Field(default_factory=dict)
    lap: Optional[int] = None
    speedUnit: str = "km/h"


class CalibrateRequest(BaseModel):
    """Calibrate lap mode's grip and downforce on one logged lap and check
    the prediction on another (VAL-38)."""

    project: Project
    calibration: LoggedLapIn
    check: Optional[LoggedLapIn] = None


class OverviewRun(BaseModel):
    """The run *Copy for AI* summarises: its key results and messages only."""

    caseName: str = ""
    status: str = ""
    incomplete: Optional[str] = None
    summary: list[SummaryValue] = Field(default_factory=list)
    messages: list[SimMessage] = Field(default_factory=list)


class OverviewRequest(BaseModel):
    project: Project
    run: Optional[OverviewRun] = None
    hideValues: bool = False


class OverviewText(BaseModel):
    text: str
    bytes: int


class AiClientState(BaseModel):
    id: str
    title: str
    installed: bool
    configPath: str


class AiConnection(BaseModel):
    """What Help > Connect an AI assistant shows."""

    command: list[str]
    warning: Optional[str] = None
    clients: list[AiClientState]
    lastUsed: Optional[dict] = None
    # why AI access cannot be turned on here: the organisation's policy
    managed: Optional[str] = None


# ---- AI access settings (AI-01): the app's page over lightsim/ai_access.py ----


class AiAccessFolder(BaseModel):
    path: str
    exists: bool
    # the folder this app saves projects to
    projects: bool = False


class AiTrustedProject(BaseModel):
    """A project file whose Script blocks AI tools may run (after the user
    confirms each run)."""

    path: str
    exists: bool
    name: Optional[str] = None
    # False: its scripts changed since it was trusted, so the trust no
    # longer holds until it is trusted again
    current: bool = False


class AiAuditEntry(BaseModel):
    """One call an AI tool made, from the local audit logs."""

    time: float  # seconds since 1970
    tool: str
    outcome: str
    project: Optional[str] = None
    client: Optional[str] = None
    # "mcp": an AI app connected to LightSim; "python": the lightsim package
    via: str


class AiAccess(BaseModel):
    """What Connect AI → AI access shows (the settings `lightsim ai` changes)."""

    enabled: bool  # the user's switch
    on: bool  # enabled, and the organisation's policy does not turn it off
    managed: Optional[str] = None  # the organisation's policy turns it off
    folders: list[AiAccessFolder]
    projectsFolder: str
    examples: bool
    trusted: list[AiTrustedProject]
    maxRunSeconds: float
    settingsPath: str
    audit: list[AiAuditEntry]  # newest first


class AiAccessChange(BaseModel):
    """Change the AI access settings. Folders and trusted projects can only
    be taken off here: a folder is added by the desktop app's folder picker
    (POST /api/ai/access/folders), a project trusted with `lightsim ai trust`."""

    enabled: Optional[bool] = None
    examples: Optional[bool] = None
    maxRunSeconds: Optional[float] = Field(default=None, ge=1, le=86400)
    removeFolders: list[str] = Field(default_factory=list)
    untrust: list[str] = Field(default_factory=list)


class AiFolderRequest(BaseModel):
    path: str
