"""Modal MiniMax H3 worker with a deterministic ComfyUI API conversion path."""

from __future__ import annotations

import json
import os
import subprocess
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

import modal


app = modal.App("minimax-h3-latest-workflows")
image = modal.Image.from_id("im-AYSPVNRooQYXy8IgQlPWOJ").run_commands(
    "git clone https://github.com/Kosinkadink/ComfyUI-VideoHelperSuite.git /root/ComfyUI/custom_nodes/ComfyUI-VideoHelperSuite",
    "if [ -f /root/ComfyUI/custom_nodes/ComfyUI-VideoHelperSuite/requirements.txt ]; then python -m pip install -r /root/ComfyUI/custom_nodes/ComfyUI-VideoHelperSuite/requirements.txt; fi",
    "git clone https://github.com/LBH-123-AI/Comfyui_Minimax_h3_latent_Upscaler.git /root/ComfyUI/custom_nodes/Comfyui_Minimax_h3_latent_Upscaler",
    "if [ -f /root/ComfyUI/custom_nodes/Comfyui_Minimax_h3_latent_Upscaler/requirements.txt ]; then python -m pip install -r /root/ComfyUI/custom_nodes/Comfyui_Minimax_h3_latent_Upscaler/requirements.txt; fi",
    "git clone https://github.com/Zironic/H3-Optimizations.git /root/ComfyUI/custom_nodes/H3-Optimizations",
    "if [ -f /root/ComfyUI/custom_nodes/H3-Optimizations/requirements.txt ]; then python -m pip install -r /root/ComfyUI/custom_nodes/H3-Optimizations/requirements.txt; fi",
    "git clone https://github.com/Deno2026/comfyui-deno-custom-nodes.git /root/ComfyUI/custom_nodes/comfyui-deno-custom-nodes",
    "if [ -f /root/ComfyUI/custom_nodes/comfyui-deno-custom-nodes/requirements.txt ]; then python -m pip install -r /root/ComfyUI/custom_nodes/comfyui-deno-custom-nodes/requirements.txt; fi",
)
data = modal.Volume.from_name("minimax-h3-comfyui-data")
models = modal.Volume.from_name("minimax-h3-models")

ROOT = Path("/data")
PORT = 8188
SKIP = {
    "Note",
    "MarkdownNote",
    "FancyTimerNode",
    "DenoTextEncoderUnload",
    "Seed (rgthree)",
    "BlockSparseAttention",
    "MiniMaxH3MemoryEfficientSageAttentionPatch",
    "MiniMaxChunkFeedForward",
}
WIDGET_MAP = {
    "CLIPLoader": ["clip_name", "type", "device"],
    "UNETLoader": ["unet_name", "weight_dtype"],
    "VAELoader": ["vae_name"],
    "KSamplerSelect": ["sampler_name"],
    "BasicScheduler": ["scheduler", "steps", "denoise"],
    "LoraLoaderModelOnly": ["lora_name", "strength_model"],
    "MiniMaxH3SigmaShift": ["shift_video", "shift_audio"],
    "MiniMaxChunkFeedForward": ["chunks", "seq_threshold"],
    "MinimaxH3LatentUpscaler3D": [
        "model_name", "mode", "mode.width", "mode.height", "align",
        "enable_temporal_chunking", "force_unload", "device", "precision",
    ],
}


def _link(src: Path, dst: Path) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    if dst.exists() or dst.is_symlink():
        dst.unlink()
    os.symlink(src, dst)


def _prepare_models() -> None:
    base = Path("/models")
    comfy = Path("/root/ComfyUI/models")
    mappings = {
        "diffusion_models/minimax_h3_fl2va_pruned_int8_convrot.safetensors": "diffusion_models/minimax_h3_fl2va_pruned_int8_convrot.safetensors",
        "diffusion_models/minimax_h3_ref2va_pruned_int8_convrot.safetensors": "diffusion_models/minimax_h3_ref2va_pruned_int8_convrot.safetensors",
        "text_encoders/qwen3vl_32b_minimax_h3_int8_convrot.safetensors": "text_encoders/qwen3vl_32b_minimax_h3_int8_convrot.safetensors",
        "vae/minimax_h3_video_vae_int8_convrot.safetensors": "vae/minimax_h3_video_vae_int8_convrot.safetensors",
        "vae/minimax_h3_audio_vae_fp32.safetensors": "vae/minimax_h3_audio_vae_fp32.safetensors",
        "latent_upscale_models/minimax_h3_latent_upscaler_3d_conv_v1_fp32.pth": "latent_upscale_models/minimax_h3_latent_upscaler_3d_conv_v1_fp32.pth",
        "loras/lightx2v_hybrid-4to8step-full-fusion_Turbo_pruned.safetensors": "loras/H3/lightx2v_hybrid-4to8step-full-fusion_Turbo_pruned.safetensors",
    }
    for source, target in mappings.items():
        _link(base / source, comfy / target)


def _normalise_input_filename(value: str) -> str:
    return value.replace("\\", "/").removeprefix("input/")


def _convert(workflow: dict, prompt: str, input_filename: str, output_prefix: str, seconds: float, width: int, height: int, seed: int) -> dict:
    nodes = {str(n["id"]): n for n in workflow.get("nodes", []) if n.get("type") not in SKIP and n.get("mode", 0) != 4}
    links = {str(row[0]): row for row in workflow.get("links", [])}
    passthrough = {
        "48": {0: links.get("83"), 1: links.get("176")},
        "190": {0: links.get("741")},
        "181": {0: links.get("738")},
        "257": {0: links.get("862"), 1: links.get("863")},
        "261": {0: links.get("801")},
        "286": {0: links.get("901")},
        "295": {0: links.get("822")},
        "332": {0: links.get("894")},
        "272": {0: links.get("890")},
        "287": {0: links.get("823")},
    }
    api = {node_id: {"class_type": node["type"], "inputs": {}} for node_id, node in nodes.items()}
    for node_id, node in nodes.items():
        inputs = node.get("inputs") or []
        for item in inputs:
            name, link_id = item.get("name"), item.get("link")
            if link_id is None or str(link_id) not in links:
                continue
            link = links[str(link_id)]
            origin_id, origin_slot = str(link[1]), link[2]
            for _ in range(8):
                replacement = passthrough.get(origin_id, {}).get(origin_slot)
                if replacement is None:
                    break
                origin_id, origin_slot = str(replacement[1]), replacement[2]
            if origin_id in nodes:
                api[node_id]["inputs"][name] = [origin_id, origin_slot]

        named = node.get("widgets_values_named")
        if isinstance(named, dict):
            for name, value in named.items():
                api[node_id]["inputs"].setdefault(name, value)
        else:
            widgets = node.get("widgets_values")
            if isinstance(widgets, dict):
                for name, value in widgets.items():
                    if name != "videopreview":
                        api[node_id]["inputs"].setdefault(name, value)
            elif isinstance(widgets, list):
                names = [i["name"] for i in inputs if i.get("widget") and i.get("link") is None]
                for name, value in zip(WIDGET_MAP.get(node["type"], names), widgets):
                    api[node_id]["inputs"].setdefault(name, value)

        if node["type"] == "PrimitiveStringMultiline":
            api[node_id]["inputs"]["value"] = prompt
        if node["type"] == "LoadImage":
            api[node_id]["inputs"]["image"] = _normalise_input_filename(input_filename)
        if node["type"] == "DenoMiniMaxH3ReferenceImageLoader":
            api[node_id]["inputs"]["image_paths"] = _normalise_input_filename(input_filename)
        if node["type"] in {"MiniMaxH3ImageToVideo", "DenoMiniMaxH3ReferenceToVideo"}:
            api[node_id]["inputs"]["width"] = width
            api[node_id]["inputs"]["height"] = height
            frames = max(5, round(seconds * 24))
            api[node_id]["inputs"]["length"] = frames + (5 - (frames % 17)) % 17
        if node["type"] == "VHS_VideoCombine":
            api[node_id]["inputs"].update(filename_prefix=output_prefix, frame_rate=24, format="video/h264-mp4", crf=19)
        if node["type"] == "RandomNoise":
            api[node_id]["inputs"]["noise_seed"] = seed
        if node["type"] == "MinimaxH3LatentUpscaler3D":
            api[node_id]["inputs"]["align"] = 1
        if node["type"] == "LoraLoaderModelOnly":
            api[node_id]["inputs"]["lora_name"] = "H3/lightx2v_hybrid-4to8step-full-fusion_Turbo_pruned.safetensors"
    return api


def _post_prompt(api: dict) -> str:
    prompt_id = str(uuid.uuid4())
    payload = json.dumps({"prompt": api, "prompt_id": prompt_id, "client_id": "minimax-h3-latest"}).encode()
    request = urllib.request.Request(f"http://127.0.0.1:{PORT}/prompt", data=payload, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            returned = json.load(response)
    except urllib.error.HTTPError as exc:
        raise RuntimeError(exc.read().decode("utf-8", "replace")) from exc
    return returned.get("prompt_id", prompt_id)


def _wait(prompt_id: str) -> dict:
    deadline = time.time() + 3600
    while time.time() < deadline:
        with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/history/{prompt_id}", timeout=30) as response:
            history = json.load(response)
        entry = history.get(prompt_id)
        if entry:
            status = entry.get("status", {})
            if status.get("completed"):
                return entry
            if status.get("status_str") == "error":
                messages = status.get("messages", [])
                detail = messages[-1][1] if messages else status
                raise RuntimeError(f"ComfyUI prompt failed: {detail}")
        time.sleep(5)
    raise TimeoutError(prompt_id)


@app.cls(image=image, gpu="L40S", volumes={"/data": data, "/models": models}, timeout=3600, startup_timeout=1800, scaledown_window=30, max_containers=1)
class LatestH3:
    @modal.enter()
    def start(self):
        _prepare_models()
        self.server = subprocess.Popen([
            "python", "main.py", "--listen", "127.0.0.1", "--port", str(PORT),
            "--input-directory", "/data/input", "--output-directory", "/data/output",
            "--user-directory", "/data/user", "--database-url", "sqlite:////tmp/h3.db",
        ], cwd="/root/ComfyUI", stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT)
        deadline = time.time() + 600
        while time.time() < deadline:
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/system_stats", timeout=5):
                    return
            except Exception:
                time.sleep(2)
        raise RuntimeError("ComfyUI did not become ready")

    @modal.exit()
    def stop(self):
        self.server.terminate()

    @modal.method()
    def generate(self, kind: str, prompt: str, input_filename: str, seconds: float = 5, width: int = 1344, height: int = 768, seed: int = 42, attestation: str | None = None) -> dict:
        import sys
        sys.path.insert(0, "/root/h3_service")
        from h3_service.license_gate import require_attestation
        require_attestation(attestation, purpose="latest H3 generation")
        workflow_name = {"t2v": "video_minimax_h3_t2v.json", "ref2v": "video_minimax_h3_r2v.json"}.get(kind, "video_minimax_h3_i2v.json")
        workflow = json.loads((ROOT / "user/default/workflows" / workflow_name).read_text(encoding="utf-8"))
        api = _convert(workflow, prompt, input_filename, f"PUBG/{kind}", seconds, width, height, seed)
        prompt_id = _post_prompt(api)
        entry = _wait(prompt_id)
        outputs = []
        for node in entry.get("outputs", {}).values():
            for key in ("gifs", "videos", "images"):
                outputs.extend(node.get(key, []) or [])
        if not outputs:
            raise RuntimeError(f"no output: {entry}")
        record = outputs[0]
        relative = str(Path(record.get("subfolder", "")) / record["filename"])
        data.commit()
        return {"status": "success", "prompt_id": prompt_id, "relative_path": relative, "kind": kind}


if __name__ == "__main__":
    with app.run():
        print("deployed local entrypoint")
