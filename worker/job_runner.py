import os, threading
from pathlib import Path
from typing import Callable, Any

DEFAULT_ATTESTATION = "minimax-h3-use-authorized-by-minimax"


def unique_path(path: Path) -> Path:
    """Keep new H3 clips from overwriting an earlier clip in the flat material folder."""
    if not path.exists():
        return path
    for index in range(2, 1000):
        candidate = path.with_name(f"{path.stem}-{index}{path.suffix}")
        if not candidate.exists():
            return candidate
    raise FileExistsError(str(path))

class JobRunner:
    def __init__(self, emit: Callable[[dict[str, Any]], None]): self.emit = emit; self.cancelled: set[str] = set(); self.active: list[threading.Thread] = []
    def handle(self, message: dict[str, Any]):
        kind, job_id = message["type"], message.get("job_id")
        if kind == "cancel_job": self.cancelled.add(job_id); self.emit({"type":"cancelled","job_id":job_id}); return
        if kind == "attach_job": self.emit({"type":"remote_attached","job_id":job_id,"function_call_id":message["function_call_id"]}); return
        if kind == "start_music":
            thread = threading.Thread(target=self._run_music, args=(message,), daemon=True)
            self.active.append(thread)
            thread.start()
            return
        if kind != "start_job": return
        thread = threading.Thread(target=self._run, args=(message,), daemon=True)
        self.active.append(thread)
        thread.start()

    def wait_for_jobs(self):
        for thread in self.active:
            thread.join()

    def _run_music(self, m: dict[str, Any]):
        job_id = m["job_id"]
        try:
            import modal
            self.emit({"type":"stage","job_id":job_id,"stage":"MUSIC_GENERATING"})
            function = modal.Function.from_name("yue2-music", "generate_music")
            result = function.remote(
                job_id,
                m["style"],
                m["lyrics"],
                int(m.get("seed", 4301) or 4301),
            )
            self.emit({"type":"stage","job_id":job_id,"stage":"AUDIO_DOWNLOADING"})
            volume = modal.Volume.from_name("yue2-outputs")
            relative = str(result["audio"])
            music_root = Path(os.environ.get("MODAL_GUI_MUSIC_ROOT", r"F:\modal-gui\music")).expanduser()
            output_path = music_root / "gui" / job_id / Path(relative).name
            output_path.parent.mkdir(parents=True, exist_ok=True)
            with output_path.open("wb") as handle:
                for chunk in volume.read_file(relative):
                    handle.write(chunk)
            self.emit({"type":"completed","job_id":job_id,"remote_output_path":relative,"local_output_path":str(output_path.resolve()),"kind":"music"})
        except Exception as exc:
            self.emit({"type":"failed","job_id":job_id,"code":"MUSIC_GENERATION_FAILED","message":str(exc),"retryable":True})
    def _run(self, m: dict):
        job_id=m["job_id"]
        try:
            import modal
            kind = str(m.get("kind", "t2v")).lower()
            attestation = os.environ.get("MINIMAX_H3_LICENSE_ATTESTATION") or DEFAULT_ATTESTATION
            input_path_value = m.get("input_path")
            input_path = Path(input_path_value).resolve() if input_path_value else None
            if kind != "t2v":
                if input_path is None or not input_path.is_file():
                    raise FileNotFoundError(f"입력 이미지가 없습니다: {input_path}")
                self.emit({"type":"stage","job_id":job_id,"stage":"INPUT_UPLOADING"})
            volume = modal.Volume.from_name("minimax-h3-comfyui-data")
            remote_input = ""
            if input_path is not None and input_path.is_file():
                remote_input = f"gui/{job_id}/{input_path.name}"
                with volume.batch_upload(force=True) as batch:
                    batch.put_file(input_path, remote_input)
            self.emit({"type":"stage","job_id":job_id,"stage":"CONTAINER_STARTING"})
            worker = modal.Cls.from_name("minimax-h3-latest-workflows", "LatestH3")()
            self.emit({"type":"remote_attached","job_id":job_id,"function_call_id":f"modal-{job_id}"})
            self.emit({"type":"stage","job_id":job_id,"stage":"GENERATING"})
            result = worker.generate.remote(
                kind=kind,
                prompt=m["prompt"],
                input_filename=remote_input,
                seconds=float(m.get("duration", 5)),
                width=int(m.get("width", 1344)),
                height=int(m.get("height", 768)),
                seed=int(m.get("seed", 42) or 42),
                attestation=attestation,
            )
            self.emit({"type":"stage","job_id":job_id,"stage":"RESULT_DOWNLOADING"})
            relative = result["relative_path"]
            output_root = Path(os.environ.get("MODAL_GUI_OUTPUT_ROOT", r"F:\modal-gui\h3-clips\generated")).expanduser()
            output_root.mkdir(parents=True, exist_ok=True)
            output_path = unique_path(output_root / Path(relative).name)
            with output_path.open("wb") as handle:
                for chunk in volume.read_file(f"output/{relative}"):
                    handle.write(chunk)
            self.emit({"type":"completed","job_id":job_id,"remote_output_path":relative,"local_output_path":str(output_path.resolve())})
        except Exception as exc:
            self.emit({"type":"failed","job_id":job_id,"code":"MODAL_GENERATION_FAILED","message":str(exc),"retryable":True})
