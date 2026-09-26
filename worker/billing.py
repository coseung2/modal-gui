"""Workspace billing sync for the Modal GUI.

`modal billing report --json` returns metered cost per Modal object and
interval. Neither the CLI nor the SDK exposes a remaining credit balance, so
this module only reports cost that Modal actually returned — never a guessed
balance.

Usage (from the Tauri backend):

    MODAL_PROFILE=<name> python -m worker.billing --for "this month"

stdout is a single JSON object; diagnostics go to stderr.
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from decimal import Decimal, InvalidOperation
from typing import Any

DEFAULT_RANGE = "this month"
DEFAULT_TIMEOUT = 180

_PERIOD_RE = re.compile(r"^(\d{4}-\d{2})")


def command_candidates() -> list[list[str]]:
    """Prefer the `modal` executable, then the interpreter running this worker."""
    found = shutil.which("modal")
    candidates: list[list[str]] = []
    if found:
        candidates.append([found])
    candidates.append([sys.executable or "python", "-m", "modal"])
    return candidates


def run_report(range_text: str = DEFAULT_RANGE, timeout: int = DEFAULT_TIMEOUT) -> str:
    """Run `modal billing report --json` and return its stdout."""
    errors: list[str] = []
    candidates = command_candidates()
    for index, candidate in enumerate(candidates):
        args = [*candidate, "billing", "report", "--for", range_text, "--json"]
        try:
            completed = subprocess.run(
                args,
                capture_output=True,
                encoding="utf-8",
                errors="replace",
                timeout=timeout,
            )
        except FileNotFoundError as exc:
            errors.append(f"{candidate[0]}: {exc}")
            continue
        except subprocess.TimeoutExpired:
            raise RuntimeError(f"billing report가 {timeout}초 안에 끝나지 않았습니다.") from None
        if completed.returncode == 0:
            return completed.stdout
        detail = (completed.stderr or completed.stdout or "").strip()
        errors.append(f"{' '.join(candidate)} (exit {completed.returncode}): {detail[:400]}")
        # A failed CLI run is a real answer; only try the next runner when this one
        # could not even load Modal.
        if index + 1 < len(candidates) and "No module named" not in detail:
            break
    raise RuntimeError("Modal billing report를 실행하지 못했습니다.\n" + "\n".join(errors))


def period_of(interval_start: str | None) -> str:
    """Month tag (YYYY-MM) for a report interval, defaulting to the current UTC month."""
    match = _PERIOD_RE.match(str(interval_start or ""))
    if match:
        return match.group(1)
    return datetime.now(timezone.utc).strftime("%Y-%m")


def parse_report(text: str) -> list[dict[str, Any]]:
    """Read the JSON array printed by `modal billing report --json`."""
    payload = (text or "").strip()
    start, end = payload.find("["), payload.rfind("]")
    if not payload or start == -1 or end < start:
        raise ValueError("billing report JSON 배열을 찾지 못했습니다.")
    try:
        rows = json.loads(payload[start : end + 1])
    except json.JSONDecodeError as exc:
        raise ValueError(f"billing report JSON을 해석하지 못했습니다: {exc}") from exc
    if not isinstance(rows, list):
        raise ValueError("billing report는 배열이어야 합니다.")
    parsed: list[dict[str, Any]] = []
    for row in rows:
        if not isinstance(row, dict):
            continue
        interval_start = str(row.get("interval_start") or "")
        parsed.append(
            {
                "object_id": str(row.get("object_id") or ""),
                "description": str(row.get("description") or ""),
                "environment": str(row.get("environment") or row.get("environment_name") or ""),
                "interval_start": interval_start,
                "period": period_of(interval_start),
                "cost": str(row.get("cost") or "0"),
            }
        )
    return parsed


def _decimal(value: Any) -> Decimal:
    try:
        return Decimal(str(value))
    except (InvalidOperation, ValueError):
        return Decimal("0")


def summarize(rows: list[dict[str, Any]], requested_period: str | None = None) -> dict[str, Any]:
    """Total cost plus a per-object breakdown for the newest period in the report.

    The period comes from the report itself, so a caller whose local month differs
    from the report range (for example a KST month start against UTC buckets) never
    replaces a period the report did not cover.
    """
    period = requested_period or (
        max((row["period"] for row in rows), default=None) or period_of(None)
    )
    in_period = [row for row in rows if row["period"] == period]
    total = Decimal("0")
    objects: dict[tuple[str, str], Decimal] = {}
    normalized: list[dict[str, Any]] = []
    for row in in_period:
        cost = _decimal(row["cost"])
        total += cost
        key = (row["object_id"], row["description"])
        objects[key] = objects.get(key, Decimal("0")) + cost
        normalized.append({**row, "cost": str(cost)})
    apps = [
        {
            "object_id": object_id,
            "description": description,
            "cost": str(cost),
            "intervals": sum(
                1 for row in in_period if row["object_id"] == object_id and row["description"] == description
            ),
        }
        for (object_id, description), cost in objects.items()
    ]
    apps.sort(key=lambda item: _decimal(item["cost"]), reverse=True)
    return {
        "period": period,
        "periods": sorted({row["period"] for row in in_period}),
        "total": str(total),
        "apps": apps,
        "rows": normalized,
        "intervals": len(normalized),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="worker.billing", description="Sync Modal workspace billing")
    parser.add_argument("--for", dest="range_text", default=DEFAULT_RANGE, help="report range, e.g. 'this month'")
    parser.add_argument("--period", default=None, help="month tag to keep (default: newest in the report)")
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT)
    parser.add_argument("--stdin-file", default=None, help="read report JSON from a file instead of Modal")
    args = parser.parse_args(argv)
    try:
        if args.stdin_file:
            with open(args.stdin_file, encoding="utf-8") as handle:
                text = handle.read()
        else:
            text = run_report(args.range_text, args.timeout)
        summary = summarize(parse_report(text), args.period)
    except (RuntimeError, ValueError, OSError) as exc:
        print(str(exc), file=sys.stderr, flush=True)
        return 2
    print(json.dumps(summary, ensure_ascii=False, separators=(",", ":")), flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
