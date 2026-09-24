import os, threading
from pathlib import Path
from typing import Callable, Any

class JobRunner:
    def __init__(self, emit: Callable[[dict[str, Any]], None]): self.emit = emit; self.cancelled: set[str] = set()
    def handle(self, message: dict[str, Any]):
        kind, job_id = message["type"], message.get("job_id")
        if kind == "cancel_job": self.cancelled.add(job_id); self.emit({"type":"cancelled","job_id":job_id}); return
        if kind == "attach_job": self.emit({"type":"remote_attached","job_id":job_id,"function_call_id":message["function_call_id"]}); return
        if kind != "start_job": return
        threading.Thread(target=self._run, args=(message,), daemon=True).start()
    def _run(self, m: dict):
        job_id=m["job_id"]
        try:
            import modal
            input_path = Path(m["input_path"]).resolve()
            if not input_path.is_file():
                raise FileNotFoundError(f"입력 이미지가 없습니다: {input_path}")
            self.emit({"type":"stage","job_id":job_id,"stage":"INPUT_UPLOADING"})
            volume = modal.Volume.from_name("minimax-h3-comfyui-data")
            remote_input = f"input/gui/{job_id}/{input_path.name}"
            with volume.batch_upload(force=True) as batch:
                batch.put_file(input_path, remote_input)
            self.emit({"type":"stage","job_id":job_id,"stage":"CONTAINER_STARTING"})
            worker = modal.Cls.from_name("minimax-h3-latest-workflows", "LatestH3")()
            self.emit({"type":"remote_attached","job_id":job_id,"function_call_id":f"modal-{job_id}"})
            self.emit({"type":"stage","job_id":job_id,"stage":"GENERATING"})
            result = worker.generate.remote(
                kind="fl2v",
                prompt=m["prompt"],
                input_filename=remote_input,
                seconds=int(m.get("duration", 5)),
                width=1344,
                height=768,
                seed=int(m.get("seed", 42) or 42),
                attestation=os.environ.get("MINIMAX_H3_LICENSE_ATTESTATION"),
            )
            self.emit({"type":"stage","job_id":job_id,"stage":"RESULT_DOWNLOADING"})
            relative = result["relative_path"]
            output_path = Path("results") / job_id / Path(relative).name
            output_path.parent.mkdir(parents=True, exist_ok=True)
            with output_path.open("wb") as handle:
                for chunk in volume.read_file(f"output/{relative}"):
                    handle.write(chunk)
            self.emit({"type":"completed","job_id":job_id,"remote_output_path":relative,"local_output_path":str(output_path.resolve())})
        except Exception as exc:
            self.emit({"type":"failed","job_id":job_id,"code":"MODAL_GENERATION_FAILED","message":str(exc),"retryable":True})
