"""Renderer plugin registry.

Adding a renderer means writing one module with `describe()` and `render()`,
then listing it here. The GUI discovers whatever this registry reports, so no
frontend change is needed to surface a new tool.
"""

from __future__ import annotations

from .autograph_renderer import AutographRenderer
from .base import (
    Capability,
    Emit,
    ExecutionMode,
    PluginInfo,
    RenderRequest,
    RendererPlugin,
    TextCue,
    info_to_dict,
    stdout_emitter,
)
from .cavalry_renderer import CavalryRenderer
from .ffmpeg_renderer import FfmpegRenderer

_PLUGINS: list[RendererPlugin] = [
    FfmpegRenderer(),
    AutographRenderer(),
    CavalryRenderer(),
]


def all_plugins() -> list[RendererPlugin]:
    return list(_PLUGINS)


def get_plugin(plugin_id: str) -> RendererPlugin:
    for plugin in _PLUGINS:
        if plugin.id == plugin_id:
            return plugin
    known = ", ".join(plugin.id for plugin in _PLUGINS)
    raise KeyError(f"unknown renderer '{plugin_id}'. available: {known}")


def describe_all() -> list[dict]:
    return [info_to_dict(plugin.describe()) for plugin in _PLUGINS]


__all__ = [
    "Capability",
    "Emit",
    "ExecutionMode",
    "PluginInfo",
    "RenderRequest",
    "RendererPlugin",
    "TextCue",
    "all_plugins",
    "describe_all",
    "get_plugin",
    "info_to_dict",
    "stdout_emitter",
]
