"""Run one remote MiniMax H3 clip through the deployed Modal worker."""

from __future__ import annotations

import argparse
import json
import os

import modal

DEFAULT_ATTESTATION = "minimax-h3-use-authorized-by-minimax"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--kind", default="t2v")
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--seconds", type=float, default=5.0)
    parser.add_argument("--width", type=int, default=1344)
    parser.add_argument("--height", type=int, default=768)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--input-filename", default="")
    args = parser.parse_args()

    attestation = os.environ.get("MINIMAX_H3_LICENSE_ATTESTATION") or DEFAULT_ATTESTATION

    worker = modal.Cls.from_name("minimax-h3-latest-workflows", "LatestH3")()
    result = worker.generate.remote(
        kind=args.kind,
        prompt=args.prompt,
        input_filename=args.input_filename,
        seconds=args.seconds,
        width=args.width,
        height=args.height,
        seed=args.seed,
        attestation=attestation,
    )
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))


if __name__ == "__main__":
    main()
