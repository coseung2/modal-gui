"""Generate a bounded set of varied H3 FL2V trailer clips sequentially."""

from __future__ import annotations

import json
import os

import modal


ATTESTATION = os.environ.get("MINIMAX_H3_LICENSE_ATTESTATION") or "minimax-h3-use-authorized-by-minimax"
INPUT_FILENAME = "pubg-test/exec-055debbd-ffb7-486e-9327-5d1b348a5d4f.png"
VARIANTS = [
    (4301, "A supply crate ignites in a volcanic battlefield as red flare smoke rises, sparks and dust, slow cinematic push-in, dramatic black and red lighting, realistic game cinematic, no logo, no text"),
    (4302, "A battle royale squad sprints from a smoking supply drop across black volcanic terrain, camera tracks low beside them, tactical urgency, dust and embers, realistic premium game trailer, no logo, no text"),
    (4303, "Extreme close-up of a modern light machine gun being raised and fired from behind volcanic rocks, muzzle flash, brass casings, red smoke in the distance, kinetic commercial camera, realistic game cinematic, no logo, no text"),
    (4304, "An armored off-road vehicle drifts around a volcanic ridge beside a burning supply crate, wheels throw ash, sparks trail through the air, aggressive tracking camera, realistic battle royale trailer, no logo, no text"),
    (4305, "A battle royale operator dives from the shoreline into dark water, bubbles and surface spray, red flare reflections above, dramatic underwater camera transition, realistic cinematic game trailer, no logo, no text"),
    (4306, "A lone competitor stands on a ridge as the blue zone closes across a volcanic island, distant squad silhouettes and supply smoke, wind and embers, slow heroic orbit, realistic game trailer, no logo, no text"),
    (4307, "Fast esports trailer shot of a squad crossing a damaged bridge under red flare light, debris and sparks, handheld impact camera, bold silhouettes, realistic battle royale action, no logo, no text"),
    (4308, "Final-circle battle royale moment: a competitor slides behind cover and throws a glowing tactical item toward a smoking ridge, sharp light streaks, tense close camera, realistic game cinematic, no logo, no text"),
]


def main() -> None:
    worker = modal.Cls.from_name("minimax-h3-latest-workflows", "LatestH3")()
    for seed, prompt in VARIANTS:
        result = worker.generate.remote(
            kind="fl2v",
            prompt=prompt,
            input_filename=INPUT_FILENAME,
            seconds=5,
            width=1344,
            height=768,
            seed=seed,
            attestation=ATTESTATION,
        )
        print(json.dumps({"seed": seed, "prompt": prompt, "result": result}, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
