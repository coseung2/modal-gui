"""JSONL Modal adapter. stdout is protocol-only; diagnostics go to stderr."""
import sys, time, traceback
from .protocol import decode, encode
from .job_runner import JobRunner

def emit(message: dict):
    print(encode(message), flush=True)

def main():
    runner = JobRunner(emit)
    while True:
        line = sys.stdin.readline()
        if not line:
            runner.wait_for_jobs()
            break
        if not line.strip(): continue
        try:
            message = decode(line)
            runner.handle(message)
        except Exception as exc:
            print(f"sidecar error: {exc}", file=sys.stderr, flush=True)
            job_id = locals().get("message", {}).get("job_id")
            if job_id: emit({"type":"failed","job_id":job_id,"code":"SIDECAR_CRASHED","message":str(exc),"retryable":False})

if __name__ == "__main__": main()
