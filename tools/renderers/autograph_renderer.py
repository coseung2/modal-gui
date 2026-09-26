"""Autograph renderer plugin.

Autograph ships a dedicated command-line renderer, so this is a `batch` plugin
like FFmpeg. The syntax below is taken from `AutographRenderer.exe --help` on
the installed build, not guessed:

  AutographRenderer.exe project.agp --background \
      --render Main output_file=out.mov format=1920x1080:1.0 \
      framerate=24.0 range=0:00:00:00;0:00:01:00 \
      container=mov video_codec=prores

Note the details that differ from the marketing docs: the binary is
`AutographRenderer.exe` (not `Autograph.exe`), `range` takes two timecodes
separated by `;`, and the install path is versioned
(`C:\\Program Files\\Maxon Autograph 2026`).

Autograph is not in the Maxon App catalogue -- it ships as a separate ~1GB
installer from maxon.net that requires elevation. Command-line rendering also
needs a licence that covers it. We probe for the executable and report a precise
reason when it is missing rather than pretending it can run.

Because Autograph drives its own composition, this plugin does not synthesise
typography from scratch. It expects an `.agp` template whose composition
parameters are named, then overrides those parameters per render. The template
path is supplied through `request.options["template"]`.

Without a template we cannot render unattended, but the data is still useful.
In that case the plugin falls back to writing the sidecar JSON plus a short
README and reports `prepared`, so the user can wire it into a project by hand
instead of getting a hard failure.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import time
from pathlib import Path

from .base import Capability, Emit, PluginInfo, RenderRequest

# Autograph installs into a version-stamped folder, so glob the parents rather
# than hard-coding one path.
SEARCH_PARENTS = (
    r"C:\Program Files",
    r"C:\Program Files (x86)",
    os.path.expandvars(r"%LOCALAPPDATA%\Programs"),
)

# The dedicated renderer first: it is the documented batch entry point.
BINARY_NAMES = ("AutographRenderer.exe", "Autograph.exe")


def find_autograph() -> str | None:
    """Locate the Autograph batch renderer, preferring AutographRenderer.exe."""
    for name in BINARY_NAMES:
        direct = shutil.which(Path(name).stem)
        if direct:
            return direct
    for parent in SEARCH_PARENTS:
        base = Path(parent)
        if not base.exists():
            continue
        for folder in sorted(base.glob("Maxon Autograph*"), reverse=True):
            for name in BINARY_NAMES:
                candidate = folder / "bin" / name
                if candidate.exists():
                    return str(candidate)
    return None


def timecode(seconds: float, fps: int) -> str:
    """Format seconds as the H:MM:SS:FF timecode Autograph's `range` expects."""
    total_frames = int(round(seconds * fps))
    frames = total_frames % fps
    total_seconds = total_frames // fps
    return (
        f"{total_seconds // 3600}:"
        f"{(total_seconds // 60) % 60:02d}:"
        f"{total_seconds % 60:02d}:"
        f"{frames:02d}"
    )


class AutographRenderer:
    id = "autograph"
    name = "Autograph"
    execution = "batch"

    def describe(self) -> PluginInfo:
        executable = find_autograph()
        version = None
        if executable:
            match = re.search(r"Maxon Autograph[ _-]?(\d+(?:\.\d+)*)", executable)
            if match:
                version = match.group(1)
        return PluginInfo(
            id=self.id,
            name=self.name,
            execution="batch",
            available=bool(executable),
            executable=executable,
            version=version,
            unavailable_reason=None if executable else (
                "Autograph이 설치되지 않았습니다. Maxon App 카탈로그에 없으므로 maxon.net의 "
                "별도 인스톨러가 필요합니다. tools\\install_autograph.ps1을 실행하세요. "
                "커맨드라인 렌더에는 Autograph Commandline 라이선스가 추가로 필요합니다."
            ),
            capabilities=Capability(
                kinetic_typography=True,
                beat_reactive_cuts=True,
                beat_reactive_effects=True,
                audio_mux=True,
                expressions=True,
                max_resolution=None,
                watermark=False,
            ),
            notes=(
                "AutographRenderer.exe --background --render 로 무인 렌더가 됩니다. "
                "Render Manager의 컴포지션 파라미터를 렌더마다 덮어쓰는 방식이라 "
                ".agp 템플릿이 필요합니다. 템플릿이 없으면 소재 데이터만 내보냅니다."
            ),
        )

    def render(self, request: RenderRequest, emit: Emit) -> Path:
        executable = find_autograph()
        if not executable:
            emit("failed", message=self.describe().unavailable_reason)
            raise SystemExit(3)

        composition = request.options.get("composition", "Main")
        container = request.options.get("container", "mov")
        codec = request.options.get("video_codec", "prores")

        # Per-render values go to a sidecar the template reads through a named
        # parameter, so cue text and beats survive without editing the project.
        sidecar = request.output.with_suffix(".autograph-input.json")
        sidecar.parent.mkdir(parents=True, exist_ok=True)
        sidecar.write_text(json.dumps({
            "clips": [str(path) for path in request.clips],
            "audio": str(request.audio),
            "shot_durations": request.shot_durations,
            "beats": request.beats,
            "cues": [
                {"start": cue.start, "end": cue.end, "text": cue.text, "size": cue.size}
                for cue in request.cues
            ],
        }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

        template = request.options.get("template")
        if not template or not Path(template).exists():
            # No template means no unattended render. Hand the data over instead
            # of failing, and say plainly what is missing.
            emit(
                "prepared",
                payload=str(sidecar),
                executable=executable,
                message=(
                    "Autograph .agp 템플릿이 없어 무인 렌더를 건너뛰었습니다. 소재 데이터를 "
                    f"{sidecar.name}에 저장했습니다. 템플릿을 만든 뒤 "
                    "--renderer-option template=<경로> 로 다시 실행하세요."
                ),
            )
            return sidecar

        command = [
            executable, str(template),
            "--background",
            "--no-splashscreen",
            "--render", composition,
            f"output_file={request.output}",
            f"format={request.width}x{request.height}:1.0",
            f"framerate={float(request.fps)}",
            # Autograph wants two H:MM:SS:FF timecodes separated by ';'.
            f"range={timecode(0, request.fps)};{timecode(request.duration, request.fps)}",
            f"container={container}",
            f"video_codec={codec}",
            f"input_json={sidecar}",
        ]
        emit(
            "render_started",
            mode="autograph",
            output=str(request.output),
            composition=composition,
            shots=len(request.shot_durations),
            beat_count=len(request.beats),
        )
        process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        tail: list[str] = []
        assert process.stdout is not None
        # Autograph blocks indefinitely when the composition name does not exist
        # in the project's Render Manager, so cap the wait instead of hanging the
        # GUI. The budget scales with clip length plus a fixed startup allowance.
        timeout = float(request.options.get("timeout", 300 + request.duration * 30))
        deadline = time.monotonic() + timeout
        for line in process.stdout:
            line = line.strip()
            if line:
                tail.append(line)
                del tail[:-40]
                percent = re.search(r"(\d{1,3})\s*%", line)
                if percent:
                    emit("render_progress", percent=float(percent.group(1)))
                else:
                    emit("log", message=line)
            if time.monotonic() > deadline:
                process.kill()
                emit(
                    "failed",
                    message=(
                        f"Autograph이 {int(timeout)}초 안에 응답하지 않아 중단했습니다. "
                        f"템플릿에 '{composition}' 컴포지션이 Render Manager에 있는지 확인하세요."
                    ),
                    detail="\n".join(tail[-12:]),
                )
                raise SystemExit(5)
        code = process.wait()
        if code != 0:
            emit("failed", message=f"Autograph exited with code {code}", detail="\n".join(tail[-12:]))
            raise SystemExit(code)
        return request.output
