"""YuE2 music generation worker for the H3 trailer pipeline."""

from __future__ import annotations

import json
from pathlib import Path

import modal


app = modal.App("yue2-music")
model_volume = modal.Volume.from_name("yue2-models")
output_volume = modal.Volume.from_name("yue2-outputs")

image = (
    modal.Image.from_registry(
        "nvidia/cuda:12.8.1-cudnn-runtime-ubuntu22.04",
        add_python="3.12",
    )
    .apt_install("git", "ffmpeg")
    .pip_install(
        "torch==2.10.0",
        "transformers==4.57.6",
        "huggingface-hub==0.36.2",
        "safetensors==0.7.0",
        "tiktoken==0.12.0",
        "numpy==2.2.6",
        "soundfile==0.13.1",
        "accelerate==1.13.0",
    )
    .run_commands(
        "git clone --depth 1 https://github.com/multimodal-art-projection/YuE.git /root/YuE",
        "python -m pip install --no-deps /root/YuE",
    )
)


@app.function(
    image=image,
    gpu="L40S",
    volumes={"/models": model_volume, "/outputs": output_volume},
    timeout=3600,
    startup_timeout=1800,
    scaledown_window=60,
    max_containers=1,
)
def generate_music(
    job_id: str,
    style: str,
    lyrics: str,
    seed: int = 4301,
) -> dict[str, object]:
    from yue2 import YuE2Pipeline

    output_dir = Path("/outputs") / job_id
    output_dir.mkdir(parents=True, exist_ok=False)
    request = {
        "style": style,
        "lyrics": lyrics,
        "cot": "full",
        "seed": seed,
    }
    with YuE2Pipeline.from_pretrained(
        "m-a-p/YuE2-3B",
        vae="m-a-p/YuE2-Vae",
        device="cuda",
        cache_dir="/models/huggingface",
    ) as pipe:
        song = pipe(**request)
        song.save_artifacts(output_dir)
        truncated = dict(song.truncated)
    metadata = {
        "job_id": job_id,
        "model": "m-a-p/YuE2-3B",
        "vae": "m-a-p/YuE2-Vae",
        "request": request,
        "audio": f"{job_id}/audio.flac",
        "truncated": truncated,
        "license": "CC BY-NC 4.0 with additional creator permission",
    }
    (output_dir / "metadata.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    output_volume.commit()
    return metadata


if __name__ == "__main__":
    with app.run():
        print("deployed yue2-music")
