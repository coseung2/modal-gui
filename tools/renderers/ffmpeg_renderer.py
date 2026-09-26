"""FFmpeg renderer plugin: the batch baseline that ships working today.

Typography here is beat-reactive rather than decorative. Each cue gets an
entry animation, and every detected beat drives a scale punch on the footage
plus a size kick on whatever text is on screen. The vocabulary is limited to
what FFmpeg expressions actually support, verified against this build:

  fontsize   accepts expressions  -> per-beat size kick
  x / y      accept expressions   -> slide, bounce, drift
  alpha      accepts expressions  -> fade, flicker
  scale w/h  accept expressions   -> beat punch on the footage
  drawbox w  accepts expressions  -> mask wipe reveal, progress bar
  rgbashift  does NOT             -> chroma split is done with blend instead

One trap is worth spelling out: `fontsize` parses an expression, and it works on
small synthetic inputs, but on a real 1080p clip a per-frame size change stalls
drawtext and the render produces zero frames with no error message. Verified on
ffmpeg 8.1.1. So the beat reaction on type is carried by position, opacity and a
scale punch on the footage, and font size stays constant.
"""

from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path

from .base import Capability, Emit, PluginInfo, RenderRequest


# Cue entry animations. Each returns the x/y/alpha/fontsize expression pieces
# for a cue running from `start` to `end`, given the composition size.
ENTRY_STYLES = ("slide", "rise", "wipe", "punch")


def escape_text(value: str) -> str:
    return value.replace("\\", "\\\\").replace(":", "\\:").replace("'", "\\'").replace("%", "\\%")


def _font_path() -> str:
    for candidate in (r"C:/Windows/Fonts/arialbd.ttf", r"C:/Windows/Fonts/segoeuib.ttf"):
        if Path(candidate).exists():
            return candidate
    return r"C:/Windows/Fonts/arial.ttf"


def _beat_kick(beats: list[float], window: float = 0.18, time_var: str = "t") -> str:
    """An expression that spikes to 1 at each beat and decays to 0.

    A flat sum of one pulse per beat does not scale: 47 beats produced a 41 KB
    graph that burned 37 CPU-minutes without emitting a single frame. This emits
    a balanced `if` chain keyed on time instead, so evaluation descends a tree
    and touches log2(n) comparisons rather than every beat.

    `time_var` exists because filters disagree on the timestamp name: drawtext
    and scale use `t`, while blend only accepts `T` and rejects `t` outright.
    """
    if not beats:
        return "0"

    slope = 1.0 / window

    def build(subset: list[float]) -> str:
        if len(subset) == 1:
            return f"max(0,1-{slope:.3f}*abs({time_var}-{subset[0]:.3f}))"
        middle = len(subset) // 2
        return (
            f"if(lt({time_var},{subset[middle]:.3f}),"
            f"{build(subset[:middle])},{build(subset[middle:])})"
        )

    return build(sorted(beats))


class FfmpegRenderer:
    id = "ffmpeg"
    name = "FFmpeg"
    execution = "batch"

    def describe(self) -> PluginInfo:
        executable = shutil.which("ffmpeg")
        version = None
        if executable:
            first = subprocess.run(
                [executable, "-version"], capture_output=True, text=True, check=False
            ).stdout.splitlines()
            if first:
                version = first[0].split(" ")[2] if len(first[0].split(" ")) > 2 else first[0]
        return PluginInfo(
            id=self.id,
            name=self.name,
            execution="batch",
            available=bool(executable),
            executable=executable,
            version=version,
            unavailable_reason=None if executable else "ffmpeg이 PATH에 없습니다.",
            capabilities=Capability(
                kinetic_typography=True,
                beat_reactive_cuts=True,
                beat_reactive_effects=True,
                audio_mux=True,
                expressions=False,
                max_resolution=None,
                watermark=False,
            ),
            notes="drawtext/drawbox 기반. 타이포는 단순한 sin 흔들림과 페이드까지만 가능합니다.",
        )

    def build_filter(self, request: RenderRequest, graphics: bool) -> str:
        parts: list[str] = []
        width, height = request.width, request.height
        for index, seconds in enumerate(request.shot_durations):
            zoom = 1.04 + (index % 4) * 0.012
            # An explicit in-point only appears when the storyboard asked for
            # one, so untouched shots keep producing the exact same graph.
            start = request.clip_starts[index] if index < len(request.clip_starts) else 0.0
            head = f"start={start:.3f}:" if start > 0 else ""
            parts.append(
                f"[{index}:v]trim={head}duration={seconds:.3f},setpts=PTS-STARTPTS,"
                f"scale={width}:{height}:force_original_aspect_ratio=increase,"
                f"crop={width}:{height},setsar=1,"
                f"zoompan=z={zoom:.3f}:x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':"
                f"d=1:s={width}x{height}:fps={request.fps}[v{index}]"
            )
        parts.append(
            "".join(f"[v{index}]" for index in range(len(request.shot_durations)))
            + f"concat=n={len(request.shot_durations)}:v=1:a=0[base]"
        )
        if not graphics:
            parts.append("[base]null[video]")
            return ";".join(parts)

        kick = _beat_kick(request.beats)
        font = _font_path().replace(":", chr(92) + ":")
        current = "[base]"
        step = 0

        # Beat punch on the footage itself: a short scale-up on every beat, so
        # the image moves with the music instead of only the overlays.
        punch = request.options.get("beat_punch", 0.045)
        if punch:
            parts.append(
                f"{current}scale=w='{width}*(1+{punch}*({kick}))':h=-2:eval=frame,"
                f"crop={width}:{height},setsar=1[punch]"
            )
            current = "[punch]"

        # A chroma split was tried here with blend's all_expr, but that expression
        # runs per pixel and made the render never finish. Beat accents are
        # carried by the flashes and type motion below, all per-frame.

        # Beat flashes, kept short so they read as accents.
        for index, beat in enumerate(request.beats):
            out = f"[flash{index}]"
            parts.append(
                f"{current}drawbox=x=0:y=0:w={width}:h={height}:color=white@0.10:t=fill:"
                f"enable='between(t,{beat:.3f},{beat + 0.05:.3f})'{out}"
            )
            current = out

        for index, cue in enumerate(request.cues):
            current = self._draw_cue(parts, current, index, cue, request, font, kick)

        parts.append(
            f"{current}drawbox=x=0:y=0:w='{width}*(mod(t*1.8\\,1))':h=8:"
            "color=0xF2A900@0.9:t=fill[video]"
        )
        return ";".join(parts)

    def _draw_cue(
        self,
        parts: list[str],
        current: str,
        index: int,
        cue,
        request: RenderRequest,
        font: str,
        kick: str,
    ) -> str:
        """Append one kinetic-typography cue and return the new filter label."""
        width, height = request.width, request.height
        start, end = cue.start, cue.end
        span = max(0.2, end - start)
        entry = min(0.45, span / 3)
        exit_fade = min(0.25, span / 5)
        style = ENTRY_STYLES[index % len(ENTRY_STYLES)]

        # Normalised entry progress, eased so motion settles instead of stopping.
        progress = f"min(1,(t-{start:.3f})/{entry:.3f})"
        eased = f"(1-pow(1-{progress},3))"
        baseline = height * 0.34

        if style == "slide":
            x_expr = f"'{width * 0.06:.0f}+{width * 0.18:.0f}*(1-{eased})'"
            y_expr = f"'{baseline:.0f}'"
        elif style == "rise":
            x_expr = f"'{width * 0.06:.0f}'"
            y_expr = f"'{baseline:.0f}+{height * 0.10:.0f}*(1-{eased})'"
        elif style == "wipe":
            x_expr = f"'{width * 0.06:.0f}'"
            y_expr = f"'{baseline:.0f}'"
        else:  # punch: centred, drops in from above and settles
            x_expr = "'(w-text_w)/2'"
            y_expr = f"'{baseline:.0f}-{height * 0.06:.0f}*(1-{eased})'"

        # Font size must stay constant (see module docstring), so the beat lands
        # as a small vertical jolt plus an opacity lift instead.
        jolt = f"-{height * 0.012:.1f}*({kick})"
        y_expr = y_expr[:-1] + jolt + "'"

        fade_in = f"(t-{start:.3f})/{entry * 0.5:.3f}"
        fade_out = f"({end:.3f}-t)/{exit_fade:.3f}"
        alpha = (
            f"'min(1,if(lt(t,{start + entry * 0.5:.3f}),{fade_in},"
            f"if(gt(t,{end - exit_fade:.3f}),{fade_out},0.88+0.12*({kick}))))'"
        )

        out = f"[type{index}]"
        parts.append(
            f"{current}drawtext=fontfile='{font}':"
            f"text='{escape_text(cue.text)}':fontcolor=white:fontsize={cue.size}:"
            f"x={x_expr}:y={y_expr}:borderw=3:bordercolor=black@0.85:"
            f"shadowx=2:shadowy=2:shadowcolor=black@0.5:"
            f"alpha={alpha}:enable='between(t,{start:.3f},{end:.3f})'{out}"
        )
        current = out

        # The wipe style needs a mask that retracts to uncover the text.
        if style == "wipe":
            out = f"[wipe{index}]"
            mask_w = f"'{width:.0f}*(1-{eased})'"
            parts.append(
                f"{current}drawbox=x='{width * 0.05:.0f}':y='{baseline - cue.size * 0.25:.0f}':"
                f"w={mask_w}:h='{cue.size * 1.6:.0f}':color=black@0.92:t=fill:"
                f"enable='between(t,{start:.3f},{start + entry:.3f})'{out}"
            )
            current = out
        return current

    def render(self, request: RenderRequest, emit: Emit) -> Path:
        graphics = bool(request.cues)
        emit(
            "render_started",
            mode="graphics" if graphics else "base",
            output=str(request.output),
            shots=len(request.shot_durations),
            beat_count=len(request.beats),
        )
        command = ["ffmpeg", "-y"]
        for clip in request.clips:
            command.extend(["-stream_loop", "-1", "-i", str(clip)])
        # The beat-reactive graph runs to tens of kilobytes with ~47 beats, past
        # the Windows command-line limit, so pass it as a script file.
        graph_path = request.output.with_suffix(".filter.txt")
        graph_path.parent.mkdir(parents=True, exist_ok=True)
        graph_path.write_text(self.build_filter(request, graphics), encoding="utf-8")
        command.extend([
            "-stream_loop", "-1", "-i", str(request.audio),
            "-filter_complex_script", str(graph_path),
            "-map", "[video]", "-map", f"{len(request.clips)}:a",
            "-t", f"{request.duration:.3f}",
            "-c:v", "libx264", "-preset", "medium", "-crf", "18",
            "-pix_fmt", "yuv420p", "-r", str(request.fps),
            "-c:a", "aac", "-b:a", "256k",
            "-af", f"afade=t=out:st={max(0.0, request.duration - 2):.3f}:d=2",
            "-movflags", "+faststart", str(request.output),
        ])
        request.output.parent.mkdir(parents=True, exist_ok=True)
        try:
            self._run(command, request.duration, emit)
        finally:
            # Remove the script even when ffmpeg fails, so a broken run does not
            # leave a 40 KB artefact next to the deliverables.
            graph_path.unlink(missing_ok=True)

        if request.metadata_path:
            request.metadata_path.parent.mkdir(parents=True, exist_ok=True)
            request.metadata_path.write_text(json.dumps({
                "renderer": self.id,
                "mode": "graphics" if graphics else "base",
                "duration_seconds": request.duration,
                "shot_durations": request.shot_durations,
                "clip_starts": request.clip_starts,
                "clips_used": [str(path) for path in request.clips],
                "beats_used": request.beats,
                "text_cues": [
                    {"start": cue.start, "end": cue.end, "text": cue.text, "size": cue.size}
                    for cue in request.cues
                ],
                "audio": str(request.audio),
            }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        return request.output

    def _run(self, command: list[str], total: float, emit: Emit) -> None:
        process = subprocess.Popen(
            command + ["-progress", "pipe:2", "-nostats"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        tail: list[str] = []
        assert process.stderr is not None
        for line in process.stderr:
            line = line.strip()
            tail.append(line)
            del tail[:-40]
            if line.startswith("out_time_ms=") and total > 0:
                try:
                    seconds = int(line.split("=", 1)[1]) / 1_000_000
                except ValueError:
                    continue
                emit(
                    "render_progress",
                    percent=round(min(100.0, seconds / total * 100), 1),
                    seconds=round(seconds, 2),
                )
        code = process.wait()
        if code != 0:
            emit("failed", message=f"ffmpeg exited with code {code}", detail="\n".join(tail[-12:]))
            raise SystemExit(code)
