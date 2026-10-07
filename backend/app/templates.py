"""Vehicle templates with fixed slots (CON-18).

A template is a proven, pre-wired model with:

- named *slots*: which part plays which role (Battery, E-Drive 1, Engine,
  Transmission, Driveline, Chassis, Brakes, Driver, Accessories,
  Controller), after the slot set of Modelica's VehicleInterfaces (BSD-3;
  the idea only, no code);
- a short *form*: the few values a new project asks for (label, unit,
  default, limits, help), each written into one part's parameter;
- a version, recorded in every project made from it.

Built-in templates (``app/templates/*.json``) point at a shipped example;
templates the user saves (``Save as template…``) live in the projects
folder's ``templates/`` subfolder and carry their own copy of the model.
Swapping the part in a slot while keeping its wiring is UX-33.
"""
from __future__ import annotations

import json
import re
import uuid
from pathlib import Path

from pydantic import BaseModel, Field

from . import storage
from .paths import projects_dir
from .schemas import ParamValue, Project

BUILTIN_DIR = Path(__file__).parent / "templates"
SLOTS = ("Battery", "E-Drive 1", "E-Drive 2", "Engine", "Transmission", "Driveline", "Chassis",
         "Brakes", "Driver", "Accessories", "Controller", "Fuel Cell")


class FormField(BaseModel):
    elementId: str
    key: str
    label: str
    unit: str = ""
    default: ParamValue
    minimum: float | None = None
    maximum: float | None = None
    help: str = ""


class Template(BaseModel):
    id: str
    name: str
    description: str = ""
    version: int = 1
    builtin: bool = False
    # the model: a shipped example's id, or the model itself (user templates)
    example: str | None = None
    project: Project | None = None
    slots: dict[str, str] = Field(default_factory=dict)  # slot → element id
    form: list[FormField] = Field(default_factory=list)


class TemplateError(ValueError):
    pass


def user_dir() -> Path:
    return projects_dir() / "templates"


def _read(path: Path, builtin: bool) -> Template:
    t = Template.model_validate_json(path.read_bytes())
    t.builtin = builtin
    return t


def listing() -> list[dict]:
    """Built-in templates first, then the user's, without their models."""
    out = []
    for folder, builtin in ((BUILTIN_DIR, True), (user_dir(), False)):
        if not folder.is_dir():
            continue
        for f in sorted(folder.glob("*.json")):
            try:
                t = _read(f, builtin)
            except (ValueError, OSError):
                continue
            out.append(t.model_dump(mode="json", exclude={"project"}))
    return out


def get(template_id: str) -> Template:
    safe = storage.safe_id(template_id, "template")
    for folder, builtin in ((BUILTIN_DIR, True), (user_dir(), False)):
        path = folder / f"{safe}.json"
        if path.is_file():
            return _read(path, builtin)
    raise FileNotFoundError(template_id)


def model_of(t: Template) -> Project:
    if t.project is not None:
        return t.project.model_copy(deep=True)
    if t.example:
        return storage.load_example(t.example)
    raise TemplateError(f"Template '{t.name}' has no model.")


def check(t: Template, project: Project) -> None:
    """Every slot and form field names a part the model has, and a form
    field one of the part's parameters."""
    from .library import library_by_id
    defs = library_by_id()
    els = {e.id: e for s in project.systems for e in s.elements}
    for slot, el_id in t.slots.items():
        if slot not in SLOTS:
            raise TemplateError(f"Unknown slot '{slot}' (slots: {', '.join(SLOTS)}).")
        if el_id not in els:
            raise TemplateError(f"Slot '{slot}' names a part the model does not have ({el_id}).")
    for f in t.form:
        el = els.get(f.elementId)
        if el is None:
            raise TemplateError(f"Form field '{f.label}' names a part the model does not have.")
        cdef = defs.get(el.componentDefId)
        if cdef is None or not any(p.key == f.key for p in cdef.parameters):
            raise TemplateError(f"Form field '{f.label}': '{el.label}' has no parameter '{f.key}'.")


def instantiate(template_id: str, values: dict[str, ParamValue], name: str | None = None) -> Project:
    """A new project from a template: its model with the form's values
    (by field index as text, or 'elementId.key'), recording the template."""
    t = get(template_id)
    project = model_of(t)
    check(t, project)
    els = {e.id: e for s in project.systems for e in s.elements}
    for i, f in enumerate(t.form):
        value = values.get(str(i), values.get(f"{f.elementId}.{f.key}", f.default))
        if isinstance(value, (int, float)) and not isinstance(value, bool):
            if f.minimum is not None and value < f.minimum or f.maximum is not None and value > f.maximum:
                raise TemplateError(f"{f.label}: {value:g} {f.unit} is outside "
                                    f"{f.minimum:g} to {f.maximum:g} {f.unit}.")
        els[f.elementId].parameterOverrides[f.key] = value
    project.id = f"{t.id}-{uuid.uuid4().hex[:8]}"
    project.name = name or f"{t.name} (new)"
    # which template and version the project was made from (kept on save)
    setattr(project, "template", {"id": t.id, "name": t.name, "version": t.version})
    return project


def save_user_template(project: Project, name: str, description: str, form: list[FormField],
                       slots: dict[str, str]) -> Template:
    """Any model becomes a template: written to the projects folder."""
    base = re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")[:60] or "template"
    tid = f"user-{base}"
    folder = user_dir()
    folder.mkdir(parents=True, exist_ok=True)
    version = 1
    path = folder / f"{tid}.json"
    if path.is_file():  # saving over a template of the same name makes a new version
        try:
            version = _read(path, False).version + 1
        except (ValueError, OSError):
            pass
    t = Template(id=tid, name=name.strip() or "My template", description=description,
                 version=version, project=project.model_copy(deep=True), slots=slots, form=form)
    check(t, t.project)
    path.write_text(json.dumps(t.model_dump(mode="json", exclude={"builtin"}), indent=2,
                               ensure_ascii=False) + "\n", encoding="utf-8")
    return t


def delete_user_template(template_id: str) -> None:
    path = user_dir() / f"{storage.safe_id(template_id, 'template')}.json"
    if not path.is_file():
        raise FileNotFoundError(template_id)
    path.unlink()
