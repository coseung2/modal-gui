"""Submit one YuE2 music generation job to Modal."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import modal


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--request", type=Path)
    parser.add_argument("--job-id")
    parser.add_argument("--style")
    parser.add_argument("--lyrics")
    parser.add_argument("--seed", type=int, default=4301)
    args = parser.parse_args()

    request = json.loads(args.request.read_text(encoding="utf-8")) if args.request else {}
    job_id = request.get("job_id", args.job_id)
    style = request.get("style", args.style)
    lyrics = request.get("lyrics", args.lyrics)
    seed = int(request.get("seed", args.seed))
    if not job_id or not style or not lyrics:
        parser.error("provide --request or --job-id, --style, and --lyrics")

    function = modal.Function.from_name("yue2-music", "generate_music")
    result = function.remote(job_id, style, lyrics, seed)
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))


if __name__ == "__main__":
    main()
