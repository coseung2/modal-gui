"""Build an evidence-backed H3/YuE2 usage report for F:."""

from __future__ import annotations

import argparse
import html
import json
import subprocess
from datetime import datetime
from pathlib import Path


def duration(path: Path) -> float:
    command = [
        "ffprobe", "-v", "error", "-show_entries", "format=duration",
        "-of", "default=noprint_wrappers=1:nokey=1", str(path),
    ]
    result = subprocess.run(command, check=True, capture_output=True, text=True)
    return float(result.stdout.strip())


def bar(label: str, value: float, maximum: float, color: str) -> str:
    width = 0 if maximum <= 0 else max(2, round(value / maximum * 100))
    return f'<div class="bar-row"><span>{html.escape(label)}</span><div class="track"><div class="fill" style="width:{width}%;background:{color}"></div></div><b>{value:.6f}</b></div>'


REMOTE_CREATED_AT = {
    "fl2v_00001-audio.mp4": "2026-09-25T08:53+09:00",
    "fl2v_00002-audio.mp4": "2026-09-25T08:59+09:00",
    "fl2v_00003-audio.mp4": "2026-09-25T09:01+09:00",
    "fl2v_00004-audio.mp4": "2026-09-25T09:02+09:00",
    "fl2v_00005-audio.mp4": "2026-09-25T09:04+09:00",
    "fl2v_00006-audio.mp4": "2026-09-25T09:05+09:00",
    "fl2v_00007-audio.mp4": "2026-09-25T09:07+09:00",
    "fl2v_00008-audio.mp4": "2026-09-25T09:08+09:00",
    "fl2v_00009-audio.mp4": "2026-09-25T09:10+09:00",
    "ref2v_00001-audio.mp4": "2026-09-25T09:25+09:00",
}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, default=Path(r"F:\modal-gui\reports"))
    parser.add_argument("--h3-app-cost", type=float, required=True, help="H3 cost for the 08:00-10:00 work window")
    parser.add_argument("--yue2-app-cost", type=float, required=True, help="YuE2 cost for the 08:00-10:00 work window")
    args = parser.parse_args()

    clips_root = Path(r"F:\modal-gui\h3-clips")
    clips = sorted(clips_root.glob("generated/*.mp4")) + sorted(clips_root.glob("smoke/*.mp4"))
    rows = []
    for path in clips:
        rows.append({
            "file": str(path),
            "kind": "smoke" if path.parent.name == "smoke" else "variant",
            "workflow": "Ref2V" if path.name.startswith("ref2v") else "FL2V",
            "used_in_final": path.parent.name == "generated",
            "duration_seconds": round(duration(path), 6),
            "size_bytes": path.stat().st_size,
            "modal_created_at_local": REMOTE_CREATED_AT.get(path.name),
            "modal_created_at_precision": "minute",
            "downloaded_at_local": datetime.fromtimestamp(path.stat().st_mtime).isoformat(timespec="seconds"),
            "generation_elapsed_seconds": None,
            "generation_elapsed_note": "The original batch wrapper did not capture per-invocation wall time.",
            "per_clip_credit": None,
            "per_clip_credit_note": "Modal billing report exposes app/hour resources, not per-invocation credits.",
        })

    final_path = Path(r"F:\modal-gui\deliverables\pubg-update43-1-60s-yue2-v1.mp4")
    yue_path = Path(r"F:\modal-gui\music\yue2\pubg-update43-1-yue2-4301\audio.flac")
    variant_rows = [row for row in rows if row["kind"] == "variant"]
    smoke_rows = [row for row in rows if row["kind"] == "smoke"]
    work_total = args.h3_app_cost + args.yue2_app_cost
    data = {
        "generated_at_local": datetime.now().isoformat(timespec="seconds"),
        "clips": rows,
        "summary": {
            "distinct_fl2v_variants": len(variant_rows),
            "smoke_clips": len(smoke_rows),
            "all_successful_h3_outputs": len(rows),
            "distinct_fl2v_media_seconds": round(sum(row["duration_seconds"] for row in variant_rows), 6),
            "all_h3_media_seconds": round(sum(row["duration_seconds"] for row in rows), 6),
            "final_video_seconds": round(duration(final_path), 6),
            "yue2_raw_audio_seconds": round(duration(yue_path), 6),
            "yue2_audio_used_seconds": 60.0,
            "billing_window": "2026-09-25 08:00-10:00 Asia/Seoul",
            "h3_work_window_app_cost": args.h3_app_cost,
            "yue2_work_window_app_cost": args.yue2_app_cost,
            "work_window_app_cost_total": round(work_total, 8),
            "per_clip_credit": None,
            "generation_elapsed_per_clip": None,
            "billing_unit_note": "Modal billing report exposes app/hour resource costs, not per-call credits or per-call elapsed time.",
            "credit_exhaustion_verified": False,
            "credit_exhaustion_note": "이번 실행은 한도 소진까지 반복하지 않았고 잔여 크레딧 API도 제공되지 않았습니다.",
        },
    }

    args.output_dir.mkdir(parents=True, exist_ok=True)
    json_path = args.output_dir / "h3-yue2-usage-report.json"
    html_path = args.output_dir / "h3-yue2-usage-report.html"
    json_path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    max_cost = max(args.h3_app_cost, args.yue2_app_cost, 1e-9)
    clip_rows = "".join(
        f"<tr><td>{html.escape(Path(row['file']).name)}</td><td>{row['workflow']}</td><td>{'최종 사용' if row['used_in_final'] else 'smoke'}</td><td>{row['duration_seconds']:.3f}</td><td>{row['size_bytes']:,}</td><td>{row['modal_created_at_local'] or '미확인'}</td><td>미수집</td><td>미노출</td></tr>"
        for row in rows
    )
    report_html = f"""<!doctype html>
<html lang="ko"><meta charset="utf-8"><title>H3 / YuE2 usage report</title>
<style>
body{{font-family:Segoe UI,Arial,sans-serif;background:#10141c;color:#eef2f7;max-width:1200px;margin:32px auto;padding:0 24px}}
h1,h2{{color:#fff}} .note{{color:#aeb9c8}} .card{{background:#19212d;border:1px solid #2b394b;border-radius:12px;padding:18px;margin:18px 0}}
.bar-row{{display:grid;grid-template-columns:260px 1fr 110px;gap:12px;align-items:center;margin:12px 0}}.track{{height:18px;background:#283447;border-radius:9px;overflow:hidden}}.fill{{height:100%;border-radius:9px}}
table{{width:100%;border-collapse:collapse;font-size:13px}}th,td{{padding:9px;border-bottom:1px solid #2b394b;text-align:left}}th{{color:#aeb9c8}} .metric{{display:inline-block;margin:8px 24px 8px 0}}.metric b{{display:block;font-size:24px;color:#ffca55}}
</style><body>
<h1>H3 / YuE2 제작 사용량 리포트</h1>
<p class="note">생성 시각: {html.escape(data["generated_at_local"])}</p>
<div class="card"><h2>요약</h2>
<span class="metric"><b>{len(variant_rows)}</b>서로 다른 FL2V 후보</span>
<span class="metric"><b>{len(smoke_rows)}</b>smoke 테스트</span>
<span class="metric"><b>{data["summary"]["distinct_fl2v_media_seconds"]:.2f}s</b>후보 원본 미디어</span>
<span class="metric"><b>{data["summary"]["final_video_seconds"]:.3f}s</b>최종 영상</span>
<span class="metric"><b>{data["summary"]["yue2_raw_audio_seconds"]:.2f}s</b>YuE2 원본 음악</span>
</div>
<div class="card"><h2>Modal 작업 시간대 비용 시각화</h2>
{bar("H3 latest workflows 앱", args.h3_app_cost, max_cost, "#ff5e57")}
{bar("YuE2 music 앱", args.yue2_app_cost, max_cost, "#55c7ff")}
<p><b>작업 시간대 합계: {work_total:.8f}</b></p>
<p class="note">범위: 2026-09-25 08:00-10:00 Asia/Seoul. 앱·시간대·리소스 비용이며, 클립별 크레딧이나 호출별 과금은 Modal CLI에서 제공되지 않습니다. 한도 소진도 확인하지 않았습니다.</p>
</div>
<div class="card"><h2>클립별 기록</h2><table><thead><tr><th>파일</th><th>워크플로우</th><th>최종 사용</th><th>미디어 길이(s)</th><th>크기(bytes)</th><th>Modal 생성 시각</th><th>처리시간</th><th>크레딧</th></tr></thead><tbody>{clip_rows}</tbody></table><p class="note">Modal 생성 시각은 볼륨 목록의 분 단위 증거입니다. 개별 처리시간과 개별 크레딧은 당시 배치 래퍼가 기록하지 않아 미수집/미노출입니다.</p></div>
<div class="card"><h2>산출물</h2><p><code>{html.escape(str(final_path))}</code></p><p><code>{html.escape(str(yue_path))}</code></p></div>
<div class="card"><h2>요청 범위 대조</h2><p>서로 다른 장면 후보: <b>{len(variant_rows)}/요청 크레딧 한도 소진 확인 안 됨</b></p><p class="note">이 생성 배치는 8개 FL2V 변주와 FL2V·Ref2V smoke 테스트에서 종료했습니다. Modal 계정의 남은 잔액/한도와 연결해 반복하지 않았기 때문에 “크레딧을 다 썼다”고 볼 수 없습니다.</p></div>
</body></html>"""
    html_path.write_text(report_html, encoding="utf-8")
    print(json.dumps({"json": str(json_path), "html": str(html_path), "variants": len(variant_rows), "smoke": len(smoke_rows), "work_window_cost": work_total}, ensure_ascii=False))


if __name__ == "__main__":
    main()
