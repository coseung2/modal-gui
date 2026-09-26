"""Renderer plugin contract.

A renderer turns a `RenderRequest` (clips + audio + beat markers + text cues)
into a deliverable. Plugins differ in how much they can do unattended, so each
one declares an `execution` mode:

  batch            we spawn the tool, stream progress, and get a finished file
  project_handoff  we write a project the user opens in the GUI tool themselves

The GUI reads `describe()` to build its UI, so a plugin must never claim a
capability it cannot actually deliver. A `project_handoff` plugin reports
`prepared` instead of `completed`, and never reports a render percentage.
"""

from __future__ import annotations

import json
import sys
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Callable, Literal, Protocol, runtime_checkable

ExecutionMode = Literal["batch", "project_handoff"]


@dataclass
class TextCue:
    start: float
    end: float
    text: str
    size: int = 64


@dataclass
class RenderRequest:
    """Everything a renderer needs, independent of which tool runs it."""

    clips: list[Path]
    audio: Path
    output: Path
    shot_durations: list[float]
    # Optional in-point (seconds) per clip, parallel to `clips`. Empty means
    # every shot starts at the head of its clip, which is the old behaviour.
    clip_starts: list[float] = field(default_factory=list)
    beats: list[float] = field(default_factory=list)
    cues: list[TextCue] = field(default_factory=list)
    duration: float = 60.0
    width: int = 1920
    height: int = 1080
    fps: int = 24
    metadata_path: Path | None = None
    options: dict = field(default_factory=dict)


@dataclass
class Capability:
    """What a plugin can actually do. Used by the GUI to enable or grey out UI."""

    kinetic_typography: bool = False
    beat_reactive_cuts: bool = False
    beat_reactive_effects: bool = False
    audio_mux: bool = False
    expressions: bool = False
    max_resolution: str | None = None
    watermark: bool = False


@dataclass
class PluginInfo:
    id: str
    name: str
    execution: ExecutionMode
    available: bool
    capabilities: Capability
    executable: str | None = None
    version: str | None = None
    unavailable_reason: str | None = None
    notes: str | None = None


Emit = Callable[..., None]


def stdout_emitter(plugin_id: str) -> Emit:
    """JSON-per-line events on stdout, the format the Tauri layer already reads."""

    def emit(event: str, **fields: object) -> None:
        payload: dict[str, object] = {
            "type": event,
            "plugin": plugin_id,
            "ts": round(time.time(), 3),
        }
        payload.update(fields)
        sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
        sys.stdout.flush()

    return emit


@runtime_checkable
class RendererPlugin(Protocol):
    id: str
    name: str
    execution: ExecutionMode

    def describe(self) -> PluginInfo:
        """Probe the host for the tool and report real availability."""

    def render(self, request: RenderRequest, emit: Emit) -> Path:
        """Produce the deliverable, or the project file for handoff plugins."""


def info_to_dict(info: PluginInfo) -> dict:
    payload = asdict(info)
    return payload
