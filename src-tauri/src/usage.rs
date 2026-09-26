//! Usage queries and bounded Modal billing synchronization.
use crate::database::{current_period, latest_billing_period, now_rfc3339, AppState};
use crate::diagnostics::{sanitize_detail, DETAIL_LIMIT};
use crate::paths::repo_root;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::thread;
use tauri::State;

// -------------------------------------------------------------- usage views
#[derive(Serialize)]
struct UsageJobRow {
    id: String,
    kind: Option<String>,
    prompt: String,
    status: String,
    stage: String,
    profile_id: Option<String>,
    profile_name: Option<String>,
    created_at: String,
    completed_at: Option<String>,
    runtime_seconds: Option<f64>,
    recorded_cost: Option<f64>,
}

/// Recent jobs with the cost actually recorded against them. Modal reports cost
/// per app and interval, so `recorded_cost` stays null until a per-job usage
/// record exists — it is never estimated from app totals.
pub(crate) fn usage_rows(conn: &Connection, limit: i64) -> Result<Value, String> {
    let period = latest_billing_period(conn);
    let mut statement = conn
        .prepare(
            "SELECT j.id, j.kind, j.prompt, j.status, j.stage, j.modal_profile_id, p.name, \
             j.created_at, j.completed_at, \
             CASE WHEN j.completed_at IS NOT NULL AND j.started_at IS NOT NULL \
                  THEN (julianday(j.completed_at) - julianday(j.started_at)) * 86400.0 END, \
             (SELECT SUM(u.amount) FROM usage_records u WHERE u.job_id = j.id) \
             FROM jobs j LEFT JOIN modal_profiles p ON p.id = j.modal_profile_id \
             ORDER BY COALESCE(j.completed_at, j.started_at, j.created_at) DESC \
             LIMIT ?1",
        )
        .map_err(|error| error.to_string())?;
    let jobs = statement
        .query_map(params![limit], |row| {
            Ok(UsageJobRow {
                id: row.get(0)?,
                kind: row.get(1)?,
                prompt: row.get(2)?,
                status: row.get(3)?,
                stage: row.get(4)?,
                profile_id: row.get(5)?,
                profile_name: row.get(6)?,
                created_at: row.get(7)?,
                completed_at: row.get(8)?,
                runtime_seconds: row.get(9)?,
                recorded_cost: row.get(10)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    let mut statement = conn
        .prepare(
            "SELECT u.modal_profile_id, p.name, u.object_id, u.label, SUM(u.amount), COUNT(*), \
             MAX(u.observed_at) \
             FROM usage_records u LEFT JOIN modal_profiles p ON p.id = u.modal_profile_id \
             WHERE u.source = 'modal_billing_report' AND u.period = ?1 \
             GROUP BY u.modal_profile_id, u.object_id, u.label \
             ORDER BY SUM(u.amount) DESC \
             LIMIT 40",
        )
        .map_err(|error| error.to_string())?;
    let objects = statement
        .query_map(params![period], |row| {
            Ok(json!({
                "profile_id": row.get::<_, Option<String>>(0)?,
                "profile_name": row.get::<_, Option<String>>(1)?,
                "object_id": row.get::<_, Option<String>>(2)?,
                "label": row.get::<_, Option<String>>(3)?,
                "cost": row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
                "intervals": row.get::<_, i64>(5)?,
                "observed_at": row.get::<_, Option<String>>(6)?,
            }))
        })
        .map_err(|error| error.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    // Counted so the UI can say plainly that no per-job cost exists yet instead
    // of implying the dashes are a display bug.
    let job_cost_records: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM usage_records WHERE job_id IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "period": period,
        "jobs": jobs,
        "objects": objects,
        "job_cost_records": job_cost_records,
    }))
}

#[tauri::command]
pub(crate) fn list_usage_rows(
    state: State<AppState>,
    job_limit: Option<i64>,
) -> Result<Value, String> {
    let limit = job_limit.unwrap_or(200).clamp(1, 1000);
    let conn = state.0.lock().map_err(|error| error.to_string())?;
    usage_rows(&conn, limit)
}

// ------------------------------------------------------------ billing sync

#[derive(Serialize)]
pub(crate) struct SyncOutcome {
    profile_id: String,
    name: String,
    ok: bool,
    period: String,
    total: Option<f64>,
    intervals: usize,
    objects: usize,
    message: Option<String>,
}

pub(crate) fn value_as_f64(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    }
}

/// Runs the Python sidecar, which owns the Modal CLI call and JSON parsing.
/// `MODAL_PROFILE` is set per process, so the global active profile never changes.
/// The report range is left to the CLI and the period comes back from the report
/// itself, so a local month that differs from the report never invents a period.
fn run_billing_sync(modal_profile: Option<&str>) -> Result<Value, String> {
    let mut command = Command::new("python");
    command
        .args(["-m", "worker.billing", "--for", "this month"])
        .current_dir(repo_root())
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUNBUFFERED", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(name) = modal_profile
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        command.env("MODAL_PROFILE", name);
    }
    let output = command
        .output()
        .map_err(|error| format!("billing 동기화를 실행하지 못했습니다: {error}"))?;
    if !output.status.success() {
        let detail = sanitize_detail(&String::from_utf8_lossy(&output.stderr), DETAIL_LIMIT);
        return Err(if detail.is_empty() {
            format!("billing 동기화 실패 (exit code {:?})", output.status.code())
        } else {
            detail
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .ok_or_else(|| String::from("billing 동기화 결과를 읽지 못했습니다."))?;
    serde_json::from_str(line.trim())
        .map_err(|error| format!("billing 동기화 JSON 파싱 실패: {error}"))
}

/// Bounded parallelism: Modal CLI calls are slow but a burst of them is rude, so
/// accounts are synced a few at a time on blocking threads.
const BILLING_SYNC_PARALLEL: usize = 3;

type SyncTarget = (String, String, Option<String>);
type SyncResult = (String, String, Result<Value, String>);

fn run_billing_batch(targets: Vec<SyncTarget>) -> Vec<SyncResult> {
    let mut results: Vec<SyncResult> = Vec::with_capacity(targets.len());
    for chunk in targets.chunks(BILLING_SYNC_PARALLEL.max(1)) {
        let chunk_results: Vec<SyncResult> = thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|(id, name, modal_profile)| {
                    scope.spawn(move || {
                        (
                            id.clone(),
                            name.clone(),
                            run_billing_sync(modal_profile.as_deref()),
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        (
                            String::new(),
                            String::new(),
                            Err(String::from("동기화 작업이 중단되었습니다.")),
                        )
                    })
                })
                .collect()
        });
        results.extend(chunk_results);
    }
    results
}

/// Stores one billing summary.
///
/// Only the periods the report actually covered are replaced, so a report for a
/// new month can never wipe an older month and repeated syncs stay duplicate-free.
/// Returns `(intervals, objects, periods)`.
pub(crate) fn write_sync_summary(
    conn: &Connection,
    profile_id: &str,
    summary: &Value,
    observed_at: &str,
) -> Result<(usize, usize, Vec<String>), String> {
    let rows = summary
        .get("rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let periods: Vec<String> = summary
        .get("periods")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let total = value_as_f64(summary.get("total")).unwrap_or(0.0);
    for period in &periods {
        conn.execute(
            "DELETE FROM usage_records WHERE modal_profile_id = ?1 \
             AND source = 'modal_billing_report' AND period = ?2",
            params![profile_id, period],
        )
        .map_err(|error| error.to_string())?;
    }
    for row in &rows {
        conn.execute(
            "INSERT INTO usage_records (modal_profile_id, job_id, source, amount, \
             period, label, object_id, raw_json, observed_at) \
             VALUES (?1, NULL, 'modal_billing_report', ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                profile_id,
                value_as_f64(row.get("cost")).unwrap_or(0.0),
                row.get("period").and_then(Value::as_str),
                row.get("description").and_then(Value::as_str),
                row.get("object_id").and_then(Value::as_str),
                sanitize_detail(&row.to_string(), 1000),
                observed_at
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    conn.execute(
        "UPDATE modal_profiles SET budget_used = ?2, last_synced_at = ?3, \
         last_sync_error = NULL, updated_at = ?3 WHERE id = ?1",
        params![profile_id, total, observed_at],
    )
    .map_err(|error| error.to_string())?;
    let objects = summary
        .get("apps")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    Ok((rows.len(), objects, periods))
}

/// Async on purpose: the Modal CLI call takes seconds, so it runs on blocking
/// worker threads and the database lock is only taken to read the target list and
/// to write results back.
#[tauri::command]
pub(crate) async fn sync_modal_billing(
    state: State<'_, AppState>,
    profile_ids: Option<Vec<String>>,
) -> Result<Vec<SyncOutcome>, String> {
    let targets: Vec<SyncTarget> = {
        let conn = state.0.lock().map_err(|error| error.to_string())?;
        let mut statement = conn
            .prepare(
                "SELECT id, name, modal_profile_name FROM modal_profiles \
                 WHERE archived_at IS NULL \
                 ORDER BY enabled DESC, priority DESC, name COLLATE NOCASE",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?;
        rows
    };
    let wanted = profile_ids.unwrap_or_default();
    let selected: Vec<SyncTarget> = if wanted.is_empty() {
        targets
    } else {
        targets
            .into_iter()
            .filter(|(id, _, _)| wanted.contains(id))
            .collect()
    };
    if selected.is_empty() {
        return Err(String::from("동기화할 Modal 계정이 없습니다."));
    }

    // No database lock is held while the CLI subprocesses run.
    let results: Vec<SyncResult> =
        tauri::async_runtime::spawn_blocking(move || run_billing_batch(selected))
            .await
            .map_err(|error| error.to_string())?;

    let now = now_rfc3339();
    let mut outcomes = Vec::new();
    let mut conn = state.0.lock().map_err(|error| error.to_string())?;
    let transaction = conn.transaction().map_err(|error| error.to_string())?;
    for (id, name, result) in results {
        match result {
            Ok(summary) => {
                let (intervals, objects, periods) =
                    write_sync_summary(&transaction, &id, &summary, &now)?;
                let total = value_as_f64(summary.get("total")).unwrap_or(0.0);
                let period = summary
                    .get("period")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(current_period);
                outcomes.push(SyncOutcome {
                    profile_id: id,
                    name,
                    ok: true,
                    period,
                    total: Some(total),
                    intervals,
                    objects,
                    message: if periods.is_empty() {
                        Some(String::from(
                            "리포트에 집계 구간이 없어 저장된 기록을 변경하지 않았습니다.",
                        ))
                    } else {
                        None
                    },
                });
            }
            Err(message) => {
                let message = sanitize_detail(&message, DETAIL_LIMIT);
                transaction
                    .execute(
                        "UPDATE modal_profiles SET last_sync_error = ?2, updated_at = ?3 WHERE id = ?1",
                        params![id, message, now],
                    )
                    .map_err(|error| error.to_string())?;
                outcomes.push(SyncOutcome {
                    profile_id: id,
                    name,
                    ok: false,
                    period: current_period(),
                    total: None,
                    intervals: 0,
                    objects: 0,
                    message: Some(message),
                });
            }
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(outcomes)
}
