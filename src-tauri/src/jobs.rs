//! Generation commands and worker-event persistence.
use crate::accounts::resolve_profile;
use crate::database::{now_rfc3339, AppState};
use crate::diagnostics::{sanitize_detail, DETAIL_LIMIT};
use crate::paths::{default_clips_root, default_music_root, repo_root};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::thread;
use tauri::{Emitter, Manager, State};

#[derive(Deserialize, Clone)]
pub(crate) struct NewJob {
    id: String,
    prompt: String,
    input_path: String,
    duration: i64,
    resolution: String,
    kind: Option<String>,
    profile_id: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    seed: Option<i64>,
    style: Option<String>,
    lyrics: Option<String>,
}

#[tauri::command]
pub(crate) fn create_job(state: State<AppState>, job: NewJob) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let conn = state.0.lock().map_err(|error| error.to_string())?;
    let (profile_id, _) = resolve_profile(&conn, &job.profile_id)?;
    let prompt = job_prompt(&job);
    conn.execute(
        "INSERT OR REPLACE INTO jobs (id,modal_profile_id,status,stage,kind,prompt,input_path,duration,resolution,seed,created_at) \
         VALUES (?1,?2,'QUEUED','JOB_CREATED',?3,?4,?5,?6,?7,?8,?9)",
        params![
            job.id,
            profile_id,
            job.kind,
            prompt,
            job.input_path,
            job.duration,
            job.resolution,
            job.seed,
            now
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// The description a job should appear under. Music jobs have no scene prompt,
/// so their style is what identifies the run in the usage table.
fn job_prompt(job: &NewJob) -> String {
    match job.kind.as_deref() {
        Some("music") => job
            .style
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| job.prompt.clone()),
        _ => job.prompt.clone(),
    }
}

/// One worker process per job, so the selected Modal CLI profile can be passed
/// through the child environment without touching the global active profile.
fn spawn_worker(
    app: tauri::AppHandle,
    message: Value,
    modal_profile: Option<&str>,
) -> Result<(), String> {
    let mut command = Command::new("python");
    command
        .args(["-m", "worker.main"])
        .current_dir(repo_root())
        .env(
            "MODAL_GUI_OUTPUT_ROOT",
            std::env::var("MODAL_GUI_OUTPUT_ROOT")
                .unwrap_or_else(|_| default_clips_root().to_string_lossy().to_string()),
        )
        .env(
            "MODAL_GUI_MUSIC_ROOT",
            std::env::var("MODAL_GUI_MUSIC_ROOT")
                .unwrap_or_else(|_| default_music_root().to_string_lossy().to_string()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(name) = modal_profile
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        command.env("MODAL_PROFILE", name);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;

    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| "worker stdin unavailable".to_string())?;
    writeln!(stdin, "{}", message).map_err(|error| error.to_string())?;
    drop(child.stdin.take());

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_app = app.clone();
    let stderr_app = app.clone();
    let watched_job = message
        .get("job_id")
        .and_then(Value::as_str)
        .map(str::to_string);

    thread::spawn(move || {
        if let Some(stdout) = stdout {
            for line in BufReader::new(stdout).lines().flatten() {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    // Same event stream as before; the database now mirrors it so
                    // the usage table does not stay at QUEUED forever.
                    persist_worker_event(&stdout_app, &value);
                    let _ = stdout_app.emit("worker-event", value);
                }
            }
        }
        if let Some(stderr) = stderr {
            for line in BufReader::new(stderr).lines().flatten() {
                let _ = stderr_app.emit(
                    "worker-log",
                    json!({"level": "error", "message": sanitize_detail(&line, DETAIL_LIMIT)}),
                );
            }
        }
        let exit = child.wait();
        // A worker that dies without a failed/completed event must not leave the
        // row running forever.
        if let Some(job_id) = watched_job {
            match exit {
                Ok(status) if status.success() => {}
                Ok(status) => mark_worker_exit(&stdout_app, &job_id, status.code()),
                Err(_) => mark_worker_exit(&stdout_app, &job_id, None),
            }
        }
    });

    Ok(())
}

fn mark_worker_exit(app: &tauri::AppHandle, job_id: &str, code: Option<i32>) {
    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(conn) = app_state.0.lock() else {
        return;
    };
    mark_worker_exit_row(&conn, job_id, code);
}

pub(crate) fn mark_worker_exit_row(conn: &Connection, job_id: &str, code: Option<i32>) {
    let _ = conn.execute(
        "UPDATE jobs SET status = 'FAILED', error_code = 'WORKER_EXITED', error_message = ?2, \
         completed_at = COALESCE(completed_at, ?3) \
         WHERE id = ?1 AND status NOT IN ('COMPLETED', 'CANCELLED', 'FAILED')",
        params![
            job_id,
            format!("worker가 비정상 종료했습니다 (exit code {:?})", code),
            now_rfc3339()
        ],
    );
}

/// A job that never reached a worker would otherwise stay QUEUED in the table.
fn mark_spawn_failure(state: &State<AppState>, job_id: &str, detail: &str) {
    let Ok(conn) = state.0.lock() else {
        return;
    };
    mark_spawn_failure_row(&conn, job_id, detail);
}

pub(crate) fn mark_spawn_failure_row(conn: &Connection, job_id: &str, detail: &str) {
    let _ = conn.execute(
        "UPDATE jobs SET status = 'FAILED', error_code = 'WORKER_SPAWN_FAILED', error_message = ?2, \
         completed_at = COALESCE(completed_at, ?3) WHERE id = ?1",
        params![job_id, sanitize_detail(detail, DETAIL_LIMIT), now_rfc3339()],
    );
}

fn persist_worker_event(app: &tauri::AppHandle, event: &Value) {
    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(conn) = app_state.0.lock() else {
        return;
    };
    let _ = apply_worker_event(&conn, event);
}

/// Mirrors one worker event into `jobs` (and keeps an audit row in `job_events`).
/// NULL-safe `COALESCE` updates mean an event only touches the fields it carries.
pub(crate) fn apply_worker_event(conn: &Connection, event: &Value) -> Result<(), String> {
    let job_id = event
        .get("job_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(job_id) = job_id else {
        return Ok(());
    };
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let stage = event.get("stage").and_then(Value::as_str);
    let progress = event.get("progress").and_then(Value::as_f64);
    let function_call_id = event.get("function_call_id").and_then(Value::as_str);
    let output_path = event.get("local_output_path").and_then(Value::as_str);
    let error_code = event.get("code").and_then(Value::as_str);
    let detail = event
        .get("message")
        .and_then(Value::as_str)
        .map(|raw| sanitize_detail(raw, DETAIL_LIMIT));
    let now = now_rfc3339();

    let (status, stage, progress, started, finished, error_set, error_value) = match event_type {
        "completed" => (
            Some("COMPLETED"),
            Some("COMPLETED"),
            Some(progress.unwrap_or(100.0)),
            true,
            true,
            true,
            None,
        ),
        "failed" => (
            Some("FAILED"),
            None,
            progress,
            true,
            true,
            true,
            detail.as_deref(),
        ),
        "cancelled" => (Some("CANCELLED"), None, progress, true, true, true, None),
        "remote_attached" => (Some("RUNNING"), None, progress, true, false, false, None),
        "stage" => {
            let stage_value = stage.unwrap_or_default();
            let status =
                if stage_value == "RESULT_DOWNLOADING" || stage_value == "AUDIO_DOWNLOADING" {
                    "DOWNLOADING"
                } else {
                    "RUNNING"
                };
            (Some(status), stage, progress, true, false, false, None)
        }
        _ => (None, None, progress, false, false, false, None),
    };
    if status.is_none() && stage.is_none() && progress.is_none() && !started && !finished {
        return Ok(());
    }

    let changed = conn
        .execute(
            "UPDATE jobs SET \
               status = COALESCE(?2, status), \
               stage = COALESCE(?3, stage), \
               progress = COALESCE(?4, progress), \
               started_at = CASE WHEN ?5 = 1 THEN COALESCE(started_at, ?6) ELSE started_at END, \
               completed_at = CASE WHEN ?7 = 1 THEN COALESCE(completed_at, ?6) ELSE completed_at END, \
               output_path = COALESCE(?8, output_path), \
               function_call_id = COALESCE(?9, function_call_id), \
               error_code = CASE WHEN ?10 = 1 THEN ?11 ELSE error_code END, \
               error_message = CASE WHEN ?10 = 1 THEN ?12 ELSE error_message END \
             WHERE id = ?1",
            params![
                job_id,
                status,
                stage,
                progress,
                if started { 1 } else { 0 },
                now,
                if finished { 1 } else { 0 },
                output_path,
                function_call_id,
                if error_set { 1 } else { 0 },
                error_code,
                error_value
            ],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO job_events (job_id, event_type, level, stage, message, payload_json, created_at) \
         SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7 WHERE EXISTS (SELECT 1 FROM jobs WHERE id = ?1)",
        params![
            job_id,
            event_type,
            if event_type == "failed" { Some("error") } else { None },
            event.get("stage").and_then(Value::as_str),
            detail,
            sanitize_detail(&event.to_string(), 1000),
            now
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) fn start_job(
    app: tauri::AppHandle,
    state: State<AppState>,
    job: NewJob,
) -> Result<(), String> {
    let kind = job.kind.clone().unwrap_or_else(|| "t2v".to_string());
    let (profile_id, modal_profile) = {
        let conn = state.0.lock().map_err(|error| error.to_string())?;
        resolve_profile(&conn, &job.profile_id)?
    };
    let modal_profile_env = modal_profile.clone();
    let job_id = job.id.clone();
    if let Err(error) = spawn_worker(
        app,
        json!({
            "type": "start_job",
            "job_id": job.id,
            "profile_id": profile_id,
            "modal_profile": modal_profile,
            "input_path": job.input_path,
            "prompt": job.prompt,
            "kind": kind,
            "duration": job.duration,
            "width": job.width.unwrap_or(1344),
            "height": job.height.unwrap_or(768),
            "seed": job.seed.unwrap_or(42)
        }),
        modal_profile_env.as_deref(),
    ) {
        mark_spawn_failure(&state, &job_id, &error);
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn start_music(
    app: tauri::AppHandle,
    state: State<AppState>,
    job: NewJob,
) -> Result<(), String> {
    let (profile_id, modal_profile) = {
        let conn = state.0.lock().map_err(|error| error.to_string())?;
        resolve_profile(&conn, &job.profile_id)?
    };
    let style = job_prompt(&job);
    let lyrics = job.lyrics.clone().unwrap_or_default();
    let modal_profile_env = modal_profile.clone();
    let job_id = job.id.clone();
    if let Err(error) = spawn_worker(
        app,
        json!({
            "type": "start_music",
            "job_id": job.id,
            "profile_id": profile_id,
            "modal_profile": modal_profile,
            "style": style,
            "lyrics": lyrics,
            "seed": job.seed.unwrap_or(4301)
        }),
        modal_profile_env.as_deref(),
    ) {
        mark_spawn_failure(&state, &job_id, &error);
        return Err(error);
    }
    Ok(())
}
