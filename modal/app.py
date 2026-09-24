"""Modal entrypoint. Keep this worker stateless; the desktop app owns job state."""
import modal

app = modal.App("modal-gui-h3")
image = modal.Image.debian_slim(python_version="3.12").pip_install("torch", "torchvision", "imageio", "imageio-ffmpeg")

@app.function(image=image, gpu="A10G", timeout=1800)
def generate_video(job_id: str, input_path: str, prompt: str, duration: int = 6, resolution: str = "1080p", seed: int | None = None) -> str:
    """Replace the model loader in this function with the pinned MiniMax H3 checkpoint."""
    print("@@STAGE:GPU_READY", flush=True)
    print("@@STAGE:MODEL_LOADING", flush=True)
    raise RuntimeError("MiniMax H3 checkpoint is not configured; set the pinned model loader before production generation")

@app.local_entrypoint()
def main():
    print("Deploy with: modal deploy modal/app.py")
