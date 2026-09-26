"""Assemble H3 clips into a 60-second typography-led trailer with FFmpeg."""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path


SEGMENT_DURATIONS = [6, 6, 6, 6, 6, 6, 6, 6, 6, 6]
TEXT_CUES = [
    (0, 5, "PUBG: BATTLEGROUNDS", 86, 92, 112),
    (6, 12, "UPDATE 43.1", 112, 92, 132),
    (12, 18, "KICK. DIVE. DOMINATE.", 64, 92, 148),
    (18, 24, "LMG BALANCE // NEW PRESSURE", 48, 92, 160),
    (24, 30, "SURFACE + UNDERWATER COMBAT", 45, 92, 172),
    (30, 36, "CARRY. THROW. MOVE.", 58, 92, 184),
    (36, 42, "INSTANT ITEM USE", 72, 92, 196),
    (42, 48, "RANKED // SEASON 43", 60, 92, 208),
    (48, 54, "PRESETS 5  →  10", 70, 92, 220),
    (54, 60, "DROP IN. TAKE OVER.", 76, 92, 232),
]


def ffmpeg_escape(value: str) -> str:
    return value.replace("\\", "\\\\").replace(":", "\\:").replace("'", "\\'")


def build_filter(clip_sequence: list[Path], fontfile: str) -> str:
    parts: list[str] = []
    for i in range(len(clip_sequence)):
        zoom = 1.04 + (i % 4) * 0.012
        parts.append(
            f"[{i}:v]trim=duration={SEGMENT_DURATIONS[i]}"
            f",setpts=PTS-STARTPTS,scale=1920:1080:force_original_aspect_ratio=increase"
            f",crop=1920:1080,setsar=1,zoompan=z={zoom:.3f}:x='iw/2-(iw/zoom/2)':"
            f"y='ih/2-(ih/zoom/2)':d=1:s=1920x1080:fps=24[v{i}]"
        )
    parts.append("".join(f"[v{i}]" for i in range(len(clip_sequence))) + f"concat=n={len(clip_sequence)}:v=1:a=0[base]")
    current = "[base]"
    for i, (start, end, text, size, y, hue) in enumerate(TEXT_CUES):
        out = f"[t{i}]"
        parts.append(
            f"{current}drawtext=fontfile='{fontfile}':text='{ffmpeg_escape(text)}':"
            f"fontcolor=white:fontsize={size}:x='92+18*sin(t*3.1)':y={y}:"
            f"borderw=2:bordercolor=black@0.75:enable='between(t,{start},{end})'{out}"
        )
        current = out
    parts.append(f"{current}drawbox=x=0:y=0:w='1920*(mod(t,6)/6)':h=8:color=0xF2{hue:02X}00@0.9:t=fill[video]")
    return ";".join(parts)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--clips-root", type=Path, default=Path(r"F:\modal-gui\h3-clips"))
    parser.add_argument("--output", type=Path, default=Path(r"F:\modal-gui\deliverables\pubg-update43-1-60s-draft.mp4"))
    parser.add_argument("--audio", type=Path)
    parser.add_argument("--metadata", type=Path)
    args = parser.parse_args()

    clips = sorted(
        p for p in args.clips_root.rglob("*.mp4")
        if "deliverables" not in p.parts and "draft" not in p.stem.lower()
    )
    if not clips:
        raise SystemExit(f"No clips found under {args.clips_root}")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    fontfile = r"C\:/Windows/Fonts/arialbd.ttf"
    clip_sequence = [clips[i % len(clips)] for i in range(len(SEGMENT_DURATIONS))]
    filter_graph = build_filter(clip_sequence, fontfile)
    command = ["ffmpeg", "-y"]
    for clip in clip_sequence:
        command.extend(["-stream_loop", "-1", "-i", str(clip)])
    if args.audio:
        command.extend(["-stream_loop", "-1", "-i", str(args.audio)])
    command.extend(["-filter_complex", filter_graph, "-map", "[video]"])
    if args.audio:
        command.extend(["-map", f"{len(clip_sequence)}:a", "-t", "60", "-c:a", "aac", "-b:a", "256k", "-af", "afade=t=out:st=58:d=2"])
    else:
        command.extend(["-t", "60"])
    command.extend(["-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p", "-r", "24", "-movflags", "+faststart", str(args.output)])
    subprocess.run(command, check=True)

    if args.metadata:
        args.metadata.parent.mkdir(parents=True, exist_ok=True)
        args.metadata.write_text(json.dumps({
            "duration_target_seconds": 60,
            "clips_used": [str(p) for p in clip_sequence],
            "patch_source": "https://www.pubg.com/en/news/11057",
            "audio": str(args.audio) if args.audio else None,
            "status": "draft" if not args.audio else "assembled",
        }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
