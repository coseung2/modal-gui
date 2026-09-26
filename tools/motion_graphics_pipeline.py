"""Audio analysis and motion-graphics rendering for the 60s H3 trailer.

Subcommands
  analyze  read an audio file, emit waveform peaks and beat markers as JSON
  render   build a footage-only base cut or a beat-aware motion-graphics cut
  storyboard  write an editable shot list (per-shot clip, in, out) as JSON
  edit     render exactly the shots a storyboard lists
  plugins  report which renderer plugins are installed and what they can do

Both subcommands write newline-delimited JSON progress events to stdout so the
desktop GUI can show live state instead of guessing.

Rendering itself is delegated to a renderer plugin (see tools/renderers). The
FFmpeg plugin is the always-available baseline; others may be batch tools or
project handoffs, and the pipeline reports whichever mode actually applies.
"""

from __future__ import annotations

import argparse
import json
import math
import struct
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from tools.renderers import (  # noqa: E402
    RenderRequest,
    TextCue,
    describe_all,
    get_plugin,
)

DEFAULT_TEXT_CUES = [
    {"start": 0.0, "end": 5.0, "text": "PUBG: BATTLEGROUNDS", "size": 86},
    {"start": 6.0, "end": 12.0, "text": "UPDATE 43.1", "size": 112},
    {"start": 12.0, "end": 18.0, "text": "KICK. DIVE. DOMINATE.", "size": 64},
    {"start": 18.0, "end": 24.0, "text": "LMG BALANCE // NEW PRESSURE", "size": 48},
    {"start": 24.0, "end": 30.0, "text": "SURFACE + UNDERWATER COMBAT", "size": 45},
    {"start": 30.0, "end": 36.0, "text": "CARRY. THROW. MOVE.", "size": 58},
    {"start": 36.0, "end": 42.0, "text": "INSTANT ITEM USE", "size": 72},
    {"start": 42.0, "end": 48.0, "text": "RANKED // SEASON 43", "size": 60},
    {"start": 48.0, "end": 54.0, "text": "PRESETS 5  ->  10", "size": 70},
    {"start": 54.0, "end": 60.0, "text": "DROP IN. TAKE OVER.", "size": 76},
]

FRAME_SECONDS = 0.05


def emit(event: str, **fields: object) -> None:
    payload: dict[str, object] = {"type": event, "ts": round(time.time(), 3)}
    payload.update(fields)
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def read_mono_pcm(audio: Path, seconds: float) -> list[int]:
    raw = subprocess.run(
        [
            "ffmpeg", "-v", "error", "-i", str(audio), "-t", f"{seconds:.3f}",
            "-ac", "1", "-ar", "16000", "-f", "s16le", "-",
        ],
        check=True,
        capture_output=True,
    ).stdout
    count = len(raw) // 2
    return list(struct.unpack("<" + "h" * count, raw[: count * 2]))


def probe_duration(media: Path) -> float:
    out = subprocess.run(
        [
            "ffprobe", "-v", "error", "-show_entries", "format=duration",
            "-of", "default=nw=1:nk=1", str(media),
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    try:
        return float(out)
    except ValueError:
        return 0.0


def frame_energies(samples: list[int]) -> list[float]:
    frame = int(16000 * FRAME_SECONDS)
    energies: list[float] = []
    for start in range(0, max(0, len(samples) - frame), frame):
        window = samples[start : start + frame]
        energies.append(math.sqrt(sum(value * value for value in window) / frame))
    return energies


def detect_beats(energies: list[float], sensitivity: float, min_gap: float, limit: int) -> list[float]:
    """Pick onsets against a sliding local baseline so quiet intros are not skipped."""
    if not energies:
        return []
    window = int(1.6 / FRAME_SECONDS)
    picks: list[tuple[float, float]] = []
    for index in range(1, len(energies) - 1):
        value = energies[index]
        low = max(0, index - window)
        high = min(len(energies), index + window + 1)
        local = energies[low:high]
        mean = sum(local) / len(local)
        spread = math.sqrt(sum((item - mean) ** 2 for item in local) / len(local))
        threshold = mean + spread * sensitivity
        if value < threshold or value < energies[index - 1] or value < energies[index + 1]:
            continue
        strength = 0.0 if mean <= 0 else (value - mean) / mean
        picks.append((index * FRAME_SECONDS, strength))
    picks.sort(key=lambda item: item[1], reverse=True)
    chosen: list[tuple[float, float]] = []
    for timestamp, strength in picks:
        if all(abs(timestamp - existing) >= min_gap for existing, _ in chosen):
            chosen.append((timestamp, strength))
        if len(chosen) >= limit:
            break
    chosen.sort()
    return [round(timestamp, 3) for timestamp, _ in chosen]


def waveform_peaks(energies: list[float], bins: int) -> list[float]:
    if not energies or bins <= 0:
        return []
    peaks: list[float] = []
    for index in range(bins):
        low = int(index * len(energies) / bins)
        high = max(low + 1, int((index + 1) * len(energies) / bins))
        peaks.append(max(energies[low:high]))
    ceiling = max(peaks) or 1.0
    return [round(value / ceiling, 4) for value in peaks]


def snap_segments(beats: list[float], total: float, target: float) -> list[float]:
    """Return cut lengths aligned to beats, keeping each shot near `target` seconds."""
    if not beats:
        count = max(1, round(total / target))
        step = total / count
        return [round(step, 3)] * count
    boundaries = [0.0]
    cursor = 0.0
    while total - cursor > target * 1.4:
        ideal = cursor + target
        options = [beat for beat in beats if cursor + target * 0.45 <= beat <= cursor + target * 1.55]
        pick = min(options, key=lambda beat: abs(beat - ideal)) if options else ideal
        cursor = round(pick, 3)
        boundaries.append(cursor)
    boundaries.append(round(total, 3))
    return [round(boundaries[index + 1] - boundaries[index], 3) for index in range(len(boundaries) - 1)]


def command_analyze(args: argparse.Namespace) -> None:
    emit("analyze_started", audio=str(args.audio))
    audio_duration = probe_duration(args.audio)
    window = min(args.duration, audio_duration) if audio_duration else args.duration
    energies = frame_energies(read_mono_pcm(args.audio, window))
    beats = detect_beats(energies, args.sensitivity, args.min_gap, args.max_beats)
    peaks = waveform_peaks(energies, args.bins)
    payload = {
        "audio": str(args.audio),
        "audio_duration_seconds": round(audio_duration, 3),
        "analyzed_seconds": round(window, 3),
        "sensitivity": args.sensitivity,
        "min_gap_seconds": args.min_gap,
        "beats": beats,
        "waveform": peaks,
        "waveform_bin_seconds": round(window / max(1, len(peaks)), 4) if peaks else 0,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    emit(
        "analyze_completed",
        markers=str(args.output),
        beat_count=len(beats),
        waveform_bins=len(peaks),
        analyzed_seconds=round(window, 3),
    )


def command_render(args: argparse.Namespace) -> None:
    clips = sorted(args.clips_root.glob("*.mp4"))
    if not clips:
        emit("failed", message=f"No clips found under {args.clips_root}")
        raise SystemExit(2)

    try:
        plugin = get_plugin(args.renderer)
    except KeyError as error:
        emit("failed", message=str(error))
        raise SystemExit(2) from error
    info = plugin.describe()
    if not info.available:
        emit("failed", message=info.unavailable_reason or f"{info.name}을 사용할 수 없습니다.")
        raise SystemExit(3)

    markers: dict = {}
    if args.markers and args.markers.exists():
        markers = json.loads(args.markers.read_text(encoding="utf-8"))
    beats = [float(value) for value in markers.get("beats", []) if float(value) < args.duration]
    if not beats:
        emit("analyze_started", audio=str(args.audio))
        energies = frame_energies(read_mono_pcm(args.audio, args.duration))
        beats = detect_beats(energies, 1.1, 0.3, 48)
        emit("analyze_completed", beat_count=len(beats))

    if args.snap_cuts:
        durations = snap_segments(beats, args.duration, args.shot_seconds)
    else:
        count = max(1, round(args.duration / args.shot_seconds))
        durations = [round(args.duration / count, 3)] * count

    sequence = [clips[index % len(clips)] for index in range(len(durations))]
    cues: list[dict] = DEFAULT_TEXT_CUES
    if args.cues and args.cues.exists():
        loaded = json.loads(args.cues.read_text(encoding="utf-8"))
        cues = loaded.get("cues", []) if isinstance(loaded, dict) else loaded
    if args.mode == "base":
        cues = []

    options = dict(args.renderer_option or [])
    request = RenderRequest(
        clips=sequence,
        audio=args.audio,
        output=args.output,
        shot_durations=durations,
        beats=beats,
        cues=[
            TextCue(
                start=float(cue["start"]),
                end=float(cue["end"]),
                text=str(cue["text"]),
                size=int(cue.get("size", 64)),
            )
            for cue in cues
        ],
        duration=args.duration,
        fps=24,
        metadata_path=args.metadata,
        options=options,
    )
    result = plugin.render(request, emit)

    # A batch plugin can still fall back to handoff (for example Autograph
    # without an .agp template), so trust the artifact rather than the mode.
    is_video = result.suffix.lower() in {".mp4", ".mov", ".mkv", ".webm"}
    if plugin.execution == "project_handoff" or not is_video:
        emit("prepare_completed", artifact=str(result), renderer=plugin.id, shots=len(durations))
        return
    emit(
        "render_completed",
        output=str(result),
        renderer=plugin.id,
        metadata=str(args.metadata) if args.metadata else None,
        duration_seconds=round(probe_duration(result), 3),
        shots=len(durations),
    )


def command_plugins(_: argparse.Namespace) -> None:
    emit("plugins", renderers=describe_all())


def cue_dicts(cues: list[TextCue]) -> list[dict]:
    return [
        {"start": cue.start, "end": cue.end, "text": cue.text, "size": cue.size}
        for cue in cues
    ]


def cues_from(raw: object, fallback: list[dict]) -> list[dict]:
    """Accept either a bare cue list or a storyboard object holding one."""
    if isinstance(raw, dict):
        raw = raw.get("cues")
    if not isinstance(raw, list) or not raw:
        return fallback
    return [
        {
            "start": float(cue["start"]),
            "end": float(cue["end"]),
            "text": str(cue["text"]),
            "size": int(cue.get("size", 64)),
        }
        for cue in raw
    ]


def detect_beats_for(audio: Path, duration: float) -> list[float]:
    """Beats for a spec that has no marker file of its own."""
    emit("analyze_started", audio=str(audio))
    energies = frame_energies(read_mono_pcm(audio, duration))
    beats = detect_beats(energies, 1.1, 0.3, 48)
    emit("analyze_completed", beat_count=len(beats))
    return beats


def command_storyboard(args: argparse.Namespace) -> None:
    """Write the editable shot list the GUI shows and the agent can rewrite."""
    clips = sorted(args.clips_root.glob("*.mp4"))
    if not clips:
        emit("failed", message=f"No clips found under {args.clips_root}")
        raise SystemExit(2)

    beats: list[float] = []
    if args.markers and args.markers.exists():
        beats = [
            float(value)
            for value in json.loads(args.markers.read_text(encoding="utf-8")).get("beats", [])
            if float(value) < args.duration
        ]
    if not beats:
        beats = detect_beats_for(args.audio, args.duration)

    if args.snap_cuts:
        durations = snap_segments(beats, args.duration, args.shot_seconds)
    else:
        count = max(1, round(args.duration / args.shot_seconds))
        durations = [round(args.duration / count, 3)] * count

    shots = [
        {"clip": str(clips[index % len(clips)]), "in": 0.0, "out": round(seconds, 3)}
        for index, seconds in enumerate(durations)
    ]
    spec = {
        "name": args.output.stem,
        "audio": str(args.audio),
        "duration": round(sum(durations), 3),
        "fps": args.fps,
        "width": args.width,
        "height": args.height,
        "shots": shots,
        "cues": cues_from(args.cues, DEFAULT_TEXT_CUES),
        "beats": beats,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(spec, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    emit(
        "storyboard_written",
        spec=str(args.output),
        shots=len(shots),
        duration=spec["duration"],
        beat_count=len(beats),
    )


def command_edit(args: argparse.Namespace) -> None:
    """Render exactly the shots the spec lists, trims included."""
    spec = json.loads(args.spec.read_text(encoding="utf-8"))
    shots = [shot for shot in spec.get("shots", []) if shot.get("clip")]
    if not shots:
        emit("failed", message=f"스토리보드에 샷이 없습니다: {args.spec}")
        raise SystemExit(2)

    missing = [str(shot["clip"]) for shot in shots if not Path(shot["clip"]).is_file()]
    if missing:
        emit(
            "failed",
            message=f"클립 {len(missing)}개를 찾지 못했습니다.",
            detail="\n".join(missing),
        )
        raise SystemExit(2)

    clips = [Path(str(shot["clip"])) for shot in shots]
    starts = [max(0.0, float(shot.get("in", 0.0))) for shot in shots]
    durations = [
        max(0.2, float(shot["out"]) - start)
        for shot, start in zip(shots, starts)
    ]
    audio = Path(str(spec["audio"]))
    duration = float(spec.get("duration") or sum(durations))

    try:
        plugin = get_plugin(args.renderer)
    except KeyError as error:
        emit("failed", message=str(error))
        raise SystemExit(2) from error
    info = plugin.describe()
    if not info.available:
        emit("failed", message=info.unavailable_reason or f"{info.name}을 사용할 수 없습니다.")
        raise SystemExit(3)

    beats = [float(value) for value in spec.get("beats", []) if float(value) < duration]
    if not beats and args.mode == "graphics":
        beats = detect_beats_for(audio, duration)

    cues = cues_from(spec, DEFAULT_TEXT_CUES) if args.mode == "graphics" else []
    options = dict(args.renderer_option or [])
    request = RenderRequest(
        clips=clips,
        audio=audio,
        output=args.output,
        shot_durations=durations,
        clip_starts=starts,
        beats=beats,
        cues=[TextCue(start=float(c["start"]), end=float(c["end"]), text=str(c["text"]), size=int(c.get("size", 64))) for c in cues],
        duration=duration,
        fps=int(spec.get("fps", 24)),
        width=int(spec.get("width", 1920)),
        height=int(spec.get("height", 1080)),
        metadata_path=args.metadata,
        options=options,
    )
    result = plugin.render(request, emit)

    is_video = result.suffix.lower() in {".mp4", ".mov", ".mkv", ".webm"}
    if plugin.execution == "project_handoff" or not is_video:
        emit("prepare_completed", artifact=str(result), renderer=plugin.id, shots=len(durations))
        return
    emit(
        "render_completed",
        output=str(result),
        renderer=plugin.id,
        metadata=str(args.metadata) if args.metadata else None,
        duration_seconds=round(probe_duration(result), 3),
        shots=len(durations),
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="H3 trailer motion-graphics pipeline")
    sub = parser.add_subparsers(dest="command", required=True)

    analyze = sub.add_parser("analyze", help="detect beats and waveform peaks")
    analyze.add_argument("--audio", type=Path, required=True)
    analyze.add_argument("--output", type=Path, required=True)
    analyze.add_argument("--duration", type=float, default=60.0)
    analyze.add_argument("--sensitivity", type=float, default=1.1)
    analyze.add_argument("--min-gap", type=float, default=0.3)
    analyze.add_argument("--max-beats", type=int, default=64)
    analyze.add_argument("--bins", type=int, default=600)
    analyze.set_defaults(handler=command_analyze)

    render = sub.add_parser("render", help="render a base or motion-graphics cut")
    render.add_argument("--clips-root", type=Path, default=Path(r"F:\modal-gui\h3-clips\generated"))
    render.add_argument("--audio", type=Path, required=True)
    render.add_argument("--markers", type=Path, default=None)
    render.add_argument("--cues", type=Path, default=None)
    render.add_argument("--output", type=Path, required=True)
    render.add_argument("--metadata", type=Path, default=None)
    render.add_argument("--mode", choices=("base", "graphics"), required=True)
    render.add_argument("--duration", type=float, default=60.0)
    render.add_argument("--shot-seconds", type=float, default=6.0)
    render.add_argument("--snap-cuts", action="store_true")
    render.add_argument("--renderer", default="ffmpeg", help="renderer plugin id")
    render.add_argument(
        "--renderer-option",
        action="append",
        type=lambda value: tuple(value.split("=", 1)),
        metavar="KEY=VALUE",
        help="plugin-specific option, repeatable",
    )
    render.set_defaults(handler=command_render)

    plugins = sub.add_parser("plugins", help="list renderer plugins and availability")
    plugins.set_defaults(handler=command_plugins)

    storyboard = sub.add_parser(
        "storyboard", help="write an editable shot list from the current material"
    )
    storyboard.add_argument("--audio", type=Path, required=True)
    storyboard.add_argument(
        "--clips-root", type=Path, default=Path(r"F:\modal-gui\h3-clips\generated")
    )
    storyboard.add_argument("--markers", type=Path, default=None)
    storyboard.add_argument("--cues", type=Path, default=None)
    storyboard.add_argument("--output", type=Path, required=True)
    storyboard.add_argument("--duration", type=float, default=60.0)
    storyboard.add_argument("--shot-seconds", type=float, default=6.0)
    storyboard.add_argument("--snap-cuts", action="store_true")
    storyboard.add_argument("--fps", type=int, default=24)
    storyboard.add_argument("--width", type=int, default=1920)
    storyboard.add_argument("--height", type=int, default=1080)
    storyboard.set_defaults(handler=command_storyboard)

    edit = sub.add_parser("edit", help="render the shots a storyboard lists")
    edit.add_argument("--spec", type=Path, required=True)
    edit.add_argument("--output", type=Path, required=True)
    edit.add_argument("--metadata", type=Path, default=None)
    edit.add_argument("--mode", choices=("base", "graphics"), default="graphics")
    edit.add_argument("--renderer", default="ffmpeg", help="renderer plugin id")
    edit.add_argument(
        "--renderer-option",
        action="append",
        type=lambda value: tuple(value.split("=", 1)),
        metavar="KEY=VALUE",
        help="plugin-specific option, repeatable",
    )
    edit.set_defaults(handler=command_edit)

    args = parser.parse_args()
    args.handler(args)


if __name__ == "__main__":
    main()
