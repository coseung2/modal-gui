import json
from typing import Any

REQUIRED = {
    "start_job": ("job_id", "profile_id", "prompt"),
    "start_music": ("job_id", "profile_id", "style", "lyrics"),
    "attach_job": ("job_id", "profile_id", "function_call_id"),
    "cancel_job": ("job_id",),
}

def decode(line: str) -> dict[str, Any]:
    message = json.loads(line)
    if not isinstance(message, dict) or not isinstance(message.get("type"), str):
        raise ValueError("message must be an object with a type")
    for key in REQUIRED.get(message["type"], ()):
        if not message.get(key):
            raise ValueError(f"missing required field: {key}")
    return message

def encode(message: dict[str, Any]) -> str:
    if not isinstance(message.get("type"), str):
        raise ValueError("outbound message requires type")
    if "job_id" in message and not message["job_id"]:
        raise ValueError("job_id cannot be empty")
    return json.dumps(message, ensure_ascii=False, separators=(",", ":"))
