use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use tauri::{Emitter, Manager, State};

struct AppState(Mutex<Connection>);

#[derive(Serialize)]
struct Health {
    database: bool,
    sidecar: bool,
}

#[derive(Deserialize, Clone)]
struct NewJob {
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

fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS modal_profiles (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          workspace_label TEXT,
          enabled INTEGER NOT NULL DEFAULT 1,
          keychain_ref TEXT NOT NULL,
          budget_limit REAL,
          budget_used REAL NOT NULL DEFAULT 0,
          reserve_amount REAL NOT NULL DEFAULT 0,
          max_concurrency INTEGER NOT NULL DEFAULT 1,
          priority INTEGER NOT NULL DEFAULT 0,
          cooldown_until TEXT,
          last_error TEXT,
          last_used_at TEXT,
          modal_profile_name TEXT,
          last_synced_at TEXT,
          last_sync_error TEXT,
          archived_at TEXT,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS jobs (
          id TEXT PRIMARY KEY,
          modal_profile_id TEXT,
          function_call_id TEXT,
          status TEXT NOT NULL,
          stage TEXT NOT NULL,
          kind TEXT,
          prompt TEXT NOT NULL,
          input_path TEXT NOT NULL,
          output_path TEXT,
          thumbnail_path TEXT,
          duration INTEGER,
          resolution TEXT,
          seed INTEGER,
          progress REAL,
          error_code TEXT,
          error_message TEXT,
          retry_count INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          started_at TEXT,
          completed_at TEXT,
          FOREIGN KEY (modal_profile_id) REFERENCES modal_profiles(id)
        );
        CREATE TABLE IF NOT EXISTS job_events (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          job_id TEXT NOT NULL,
          event_type TEXT NOT NULL,
          level TEXT,
          stage TEXT,
          message TEXT,
          payload_json TEXT,
          created_at TEXT NOT NULL,
          FOREIGN KEY (job_id) REFERENCES jobs(id)
        );
        CREATE TABLE IF NOT EXISTS usage_records (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          modal_profile_id TEXT NOT NULL,
          job_id TEXT,
          source TEXT NOT NULL,
          amount REAL,
          period TEXT,
          label TEXT,
          object_id TEXT,
          raw_json TEXT,
          observed_at TEXT NOT NULL,
          FOREIGN KEY (modal_profile_id) REFERENCES modal_profiles(id)
        );
        "#,
    )?;
    // Columns added after the first release; CREATE TABLE IF NOT EXISTS cannot add them.
    ensure_column(conn, "modal_profiles", "modal_profile_name", "TEXT")?;
    ensure_column(conn, "modal_profiles", "last_synced_at", "TEXT")?;
    ensure_column(conn, "modal_profiles", "last_sync_error", "TEXT")?;
    ensure_column(conn, "modal_profiles", "archived_at", "TEXT")?;
    ensure_column(conn, "jobs", "kind", "TEXT")?;
    ensure_column(conn, "usage_records", "period", "TEXT")?;
    ensure_column(conn, "usage_records", "label", "TEXT")?;
    ensure_column(conn, "usage_records", "object_id", "TEXT")?;
    // The index touches a new column, so it must be created after the migrations.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS usage_records_profile_period
           ON usage_records (modal_profile_id, source, period);
         CREATE INDEX IF NOT EXISTS usage_records_job_id
           ON usage_records (job_id);",
    )?;
    seed_default_profile(conn)
}

fn ensure_column(conn: &Connection, table: &str, column: &str, ddl: &str) -> rusqlite::Result<()> {
    let existing: Vec<String> = conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !existing.iter().any(|name| name == column) {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"))?;
    }
    Ok(())
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Month tag (UTC) that keeps one billing period per profile.
fn current_period() -> String {
    chrono::Utc::now().format("%Y-%m").to_string()
}

/// Longest error text we keep in the database.
const DETAIL_LIMIT: usize = 400;

const REDACTED: &str = "<redacted>";

/// Credential prefixes whose value follows the prefix directly.
const SECRET_PREFIXES: [&str; 5] = ["ak-", "as-", "sk-", "ghp_", "Bearer "];

/// `WORD = value`, `WORD: value`, `"WORD": "value"` style assignments.
const SECRET_ASSIGNMENTS: [&str; 2] = ["MODAL_TOKEN_SECRET", "token"];

/// Characters that can appear inside a credential value.
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '-' | '_' | '.' | '+' | '/' | '=' | ':' | '@' | '~' | '%' | '&' | '#' | '!' | '*' | '$'
        )
}

/// Length of the credential value starting at `tail`, including any surrounding
/// quotes. Returns 0 when nothing value-like follows, so a marker is never added
/// without a value.
fn value_span(tail: &str) -> usize {
    let mut start = 0;
    for (index, c) in tail.char_indices() {
        if c.is_whitespace() || matches!(c, '=' | ':' | ',' | ';') {
            start = index + c.len_utf8();
        } else {
            break;
        }
    }
    let rest = &tail[start..];
    let Some(first) = rest.chars().next() else {
        return 0;
    };
    if matches!(first, '"' | '\'' | '`') {
        // A quoted value runs to its closing quote, honouring backslash escapes,
        // and stops at the line end when the quote is never closed.
        let after = &rest[first.len_utf8()..];
        let mut end = after.len();
        let mut escaped = false;
        for (offset, c) in after.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if c == '\\' {
                escaped = true;
                continue;
            }
            if c == first {
                end = offset + c.len_utf8();
                break;
            }
            if c == '\n' {
                end = offset;
                break;
            }
        }
        return start + first.len_utf8() + end;
    }
    let run = rest.find(|c: char| !is_token_char(c)).unwrap_or(rest.len());
    start + run
}

/// Replaces the credential after every `prefix` occurrence. With
/// `require_separator` the word only counts as a marker when whitespace and a
/// `=` or `:` separator follow it, which keeps prose like "token expired" intact.
fn redact_after(text: &str, prefix: &str, require_separator: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(prefix) {
        let (head, tail) = rest.split_at(index + prefix.len());
        out.push_str(head);
        let mut value_start = 0;
        if require_separator {
            let mut separated = false;
            for (offset, c) in tail.char_indices() {
                if matches!(c, '=' | ':') {
                    separated = true;
                    value_start = offset + c.len_utf8();
                    break;
                }
                // JSON writes the separator after the quoted key: "token":"value"
                if c.is_whitespace() || matches!(c, '"' | '\'' | '`') {
                    value_start = offset + c.len_utf8();
                    continue;
                }
                break;
            }
            if !separated {
                rest = tail;
                continue;
            }
        }
        let span = value_span(&tail[value_start..]);
        if span == 0 {
            // Nothing value-like follows, so leave the text exactly as it was.
            rest = tail;
            continue;
        }
        if require_separator {
            out.push_str(&tail[..value_start]);
        }
        out.push_str(REDACTED);
        rest = &tail[value_start + span..];
    }
    out.push_str(rest);
    out
}

/// One marker per secret: nested markers and repeated redactions collapse.
fn collapse_redactions(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(REDACTED) {
        out.push_str(&rest[..index]);
        out.push_str(REDACTED);
        let mut tail = &rest[index + REDACTED.len()..];
        loop {
            let trimmed = tail.trim_start_matches(char::is_whitespace);
            if trimmed.starts_with(REDACTED) {
                tail = &trimmed[REDACTED.len()..];
            } else {
                break;
            }
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Makes CLI and worker output safe to persist and to show: credentials are
/// redacted, whitespace collapsed, and the length capped.
fn sanitize_detail(raw: &str, max: usize) -> String {
    let mut text = raw.trim().to_string();
    for prefix in SECRET_PREFIXES {
        if text.contains(prefix) {
            text = redact_after(&text, prefix, false);
        }
    }
    for word in SECRET_ASSIGNMENTS {
        if text.contains(word) {
            text = redact_after(&text, word, true);
        }
    }
    text = collapse_redactions(&text);
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut clipped: String = flat.chars().take(max).collect();
    clipped.push('…');
    clipped
}

/// Finds which local Modal profile is active by reading `~/.modal.toml`.
///
/// The file has to be read as a whole to find the `active` flag, so token lines
/// pass through memory here. They are never returned, stored, logged, or sent
/// anywhere: the only value that leaves this function is one section name.
fn detect_active_modal_profile() -> Option<String> {
    let path = std::env::var("MODAL_CONFIG_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .map(|home| std::path::PathBuf::from(home).join(".modal.toml"))
        })?;
    read_active_modal_profile(&path)
}

fn read_active_modal_profile(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut section: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = Some(
                trimmed
                    .trim_matches(|c| c == '[' || c == ']')
                    .trim()
                    .to_string(),
            );
            continue;
        }
        let mut parts = trimmed.splitn(2, '=');
        if parts.next().map(|key| key.trim()) == Some("active")
            && parts.next().map(|value| value.trim()) == Some("true")
        {
            return section;
        }
    }
    None
}

/// A Modal profile name ends up as the `MODAL_PROFILE` value, so keep it safe.
fn is_valid_modal_profile_name(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | '+'))
}

/// Keeps the first run usable: one account row pointing at whatever profile the
/// local Modal CLI currently uses. Created only when no account exists yet.
fn seed_default_profile(conn: &Connection) -> rusqlite::Result<()> {
    let existing: i64 = conn.query_row("SELECT COUNT(*) FROM modal_profiles", [], |row| row.get(0))?;
    if existing > 0 {
        return Ok(());
    }
    let now = now_rfc3339();
    let detected = detect_active_modal_profile();
    conn.execute(
        "INSERT INTO modal_profiles (id, name, workspace_label, enabled, keychain_ref, budget_limit, \
         budget_used, reserve_amount, max_concurrency, priority, modal_profile_name, created_at, updated_at) \
         VALUES (?1, ?2, ?3, 1, '', NULL, 0, 0, 1, 0, ?4, ?5, ?5)",
        params!["modal_01", "기본 Modal 계정", detected.clone(), detected, now],
    )?;
    Ok(())
}

#[tauri::command]
fn health(state: State<AppState>) -> Result<Health, String> {
    let database = state
        .0
        .lock()
        .map_err(|error| error.to_string())
        .is_ok();
    Ok(Health {
        database,
        sidecar: true,
    })
}

#[tauri::command]
fn create_job(state: State<AppState>, job: NewJob) -> Result<(), String> {
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

/// Resolves the Modal account for a job.
///
/// An explicit account is honored only when it exists and is enabled: a stale or
/// stopped selection fails loudly instead of silently running on another
/// account. Only an empty request falls back to the first enabled account.
/// The second value is the local Modal CLI profile name (`MODAL_PROFILE`).
fn resolve_profile(
    conn: &Connection,
    profile_id: &Option<String>,
) -> Result<(String, Option<String>), String> {
    let explicit = profile_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let row = match explicit {
        Some(id) => conn
            .query_row(
                "SELECT id, name, enabled, modal_profile_name FROM modal_profiles \
                 WHERE id = ?1 AND archived_at IS NULL",
                params![id],
                read_profile_choice,
            )
            .map_err(|_| format!("요청한 Modal 계정을 찾을 수 없습니다: {id}"))?,
        None => conn
            .query_row(
                "SELECT id, name, enabled, modal_profile_name FROM modal_profiles \
                 WHERE enabled = 1 AND archived_at IS NULL \
                 ORDER BY priority DESC, name COLLATE NOCASE LIMIT 1",
                [],
                read_profile_choice,
            )
            .map_err(|_| NO_PROFILE_MESSAGE.to_string())?,
    };
    let (id, name, enabled, modal_profile) = row;
    if enabled == 0 {
        return Err(format!(
            "중지된 Modal 계정입니다: {name}. 사용량 탭에서 '사용'으로 전환하세요."
        ));
    }
    conn.execute(
        "UPDATE modal_profiles SET last_used_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![id, now_rfc3339()],
    )
    .map_err(|error| error.to_string())?;
    Ok((id, modal_profile))
}

fn read_profile_choice(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, i64, Option<String>)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

/// One worker process per job, so the selected Modal CLI profile can be passed
/// through the child environment without touching the global active profile.
fn spawn_worker(
    app: tauri::AppHandle,
    message: Value,
    modal_profile: Option<&str>,
) -> Result<(), String> {
    let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut command = Command::new("python");
    command
        .args(["-m", "worker.main"])
        .current_dir(repo_root)
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
    if let Some(name) = modal_profile.map(str::trim).filter(|value| !value.is_empty()) {
        command.env("MODAL_PROFILE", name);
    }
    let mut child = command
        .spawn()
        .map_err(|error| error.to_string())?;

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

fn mark_worker_exit_row(conn: &Connection, job_id: &str, code: Option<i32>) {
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

fn mark_spawn_failure_row(conn: &Connection, job_id: &str, detail: &str) {
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
fn apply_worker_event(conn: &Connection, event: &Value) -> Result<(), String> {
    let job_id = event
        .get("job_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(job_id) = job_id else {
        return Ok(());
    };
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or_default();
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
        "failed" => (Some("FAILED"), None, progress, true, true, true, detail.as_deref()),
        "cancelled" => (Some("CANCELLED"), None, progress, true, true, true, None),
        "remote_attached" => (Some("RUNNING"), None, progress, true, false, false, None),
        "stage" => {
            let stage_value = stage.unwrap_or_default();
            let status = if stage_value == "RESULT_DOWNLOADING" || stage_value == "AUDIO_DOWNLOADING" {
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
fn start_job(app: tauri::AppHandle, state: State<AppState>, job: NewJob) -> Result<(), String> {
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
fn start_music(app: tauri::AppHandle, state: State<AppState>, job: NewJob) -> Result<(), String> {
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

const NO_PROFILE_MESSAGE: &str =
    "사용 중인 Modal 계정이 없습니다. 사용량 탭에서 계정을 추가하거나 사용으로 전환하세요.";

// ------------------------------------------------------------------ accounts

#[derive(Serialize)]
struct ModalProfileRow {
    id: String,
    name: String,
    modal_profile_name: Option<String>,
    workspace_label: Option<String>,
    enabled: bool,
    keychain_ref: String,
    budget_limit: Option<f64>,
    reserve_amount: f64,
    max_concurrency: i64,
    priority: i64,
    last_used_at: Option<String>,
    last_synced_at: Option<String>,
    last_sync_error: Option<String>,
    period: String,
    month_cost: f64,
    month_intervals: i64,
}

#[derive(Deserialize)]
struct ProfileInput {
    id: Option<String>,
    name: String,
    modal_profile_name: Option<String>,
    workspace_label: Option<String>,
    keychain_ref: Option<String>,
    budget_limit: Option<f64>,
    max_concurrency: Option<i64>,
    priority: Option<i64>,
}

fn trimmed(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

/// The newest billing period actually stored, not the local month. A report whose
/// period differs from the local month (KST month start versus UTC) must not make
/// every account read as zero.
fn latest_billing_period(conn: &Connection) -> String {
    conn.query_row(
        "SELECT MAX(period) FROM usage_records \
         WHERE source = 'modal_billing_report' AND period IS NOT NULL",
        [],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
    .unwrap_or_else(current_period)
}

fn list_profiles(conn: &Connection) -> Result<Vec<ModalProfileRow>, String> {
    let period = latest_billing_period(conn);
    let mut statement = conn
        .prepare(
            "SELECT p.id, p.name, p.modal_profile_name, p.workspace_label, p.enabled, p.keychain_ref, \
             p.budget_limit, p.reserve_amount, p.max_concurrency, p.priority, p.last_used_at, \
             p.last_synced_at, p.last_sync_error, \
             (SELECT COALESCE(SUM(u.amount), 0) FROM usage_records u WHERE u.modal_profile_id = p.id \
               AND u.source = 'modal_billing_report' AND u.period = ?1), \
             (SELECT COUNT(*) FROM usage_records u WHERE u.modal_profile_id = p.id \
               AND u.source = 'modal_billing_report' AND u.period = ?1) \
             FROM modal_profiles p \
             WHERE p.archived_at IS NULL \
             ORDER BY p.enabled DESC, p.priority DESC, p.name COLLATE NOCASE",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![period], |row| {
            Ok(ModalProfileRow {
                id: row.get(0)?,
                name: row.get(1)?,
                modal_profile_name: row.get(2)?,
                workspace_label: row.get(3)?,
                enabled: row.get::<_, i64>(4)? != 0,
                keychain_ref: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                budget_limit: row.get(6)?,
                reserve_amount: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
                max_concurrency: row.get(8)?,
                priority: row.get(9)?,
                last_used_at: row.get(10)?,
                last_synced_at: row.get(11)?,
                last_sync_error: row.get(12)?,
                period: period.clone(),
                month_cost: row.get::<_, Option<f64>>(13)?.unwrap_or(0.0),
                month_intervals: row.get(14)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_modal_profiles(state: State<AppState>) -> Result<Vec<ModalProfileRow>, String> {
    let conn = state.0.lock().map_err(|error| error.to_string())?;
    list_profiles(&conn)
}

/// Full replace of one account. `budget_limit` is the optional allocated credit
/// declared by the user; an empty value means "unknown", never a guessed balance.
fn save_profile(conn: &Connection, profile: &ProfileInput) -> Result<Vec<ModalProfileRow>, String> {
    let name = profile.name.trim().to_string();
    if name.is_empty() {
        return Err(String::from("계정 이름을 입력하세요."));
    }
    if name.chars().count() > 60 {
        return Err(String::from("계정 이름은 60자 이하로 입력하세요."));
    }
    let modal_profile_name = trimmed(&profile.modal_profile_name);
    if let Some(value) = &modal_profile_name {
        if !is_valid_modal_profile_name(value) {
            return Err(String::from(
                "Modal 프로필 이름에는 영문/숫자와 . _ - @ + 만 사용할 수 있습니다.",
            ));
        }
    }
    let workspace_label = trimmed(&profile.workspace_label);
    // A reference only. Token values are never accepted or stored here.
    let keychain_ref = trimmed(&profile.keychain_ref).unwrap_or_default();
    if keychain_ref.chars().count() > 200 {
        return Err(String::from("자격증명 참조는 200자 이하로 입력하세요."));
    }
    let budget_limit = profile
        .budget_limit
        .filter(|value| value.is_finite() && *value >= 0.0);
    let max_concurrency = profile.max_concurrency.unwrap_or(1).clamp(1, 32);
    let priority = profile.priority.unwrap_or(0).clamp(-100, 100);
    let now = now_rfc3339();
    let id = trimmed(&profile.id)
        .unwrap_or_else(|| format!("modal_{}", chrono::Utc::now().timestamp_millis()));
    conn.execute(
        "INSERT INTO modal_profiles (id, name, workspace_label, enabled, keychain_ref, budget_limit, \
         budget_used, reserve_amount, max_concurrency, priority, modal_profile_name, created_at, updated_at) \
         VALUES (?1, ?2, ?3, 1, ?4, ?5, 0, 0, ?6, ?7, ?8, ?9, ?9) \
         ON CONFLICT(id) DO UPDATE SET name = ?2, workspace_label = ?3, keychain_ref = ?4, \
           budget_limit = ?5, max_concurrency = ?6, priority = ?7, modal_profile_name = ?8, \
           archived_at = NULL, updated_at = ?9",
        params![
            id,
            name,
            workspace_label,
            keychain_ref,
            budget_limit,
            max_concurrency,
            priority,
            modal_profile_name,
            now
        ],
    )
    .map_err(|error| error.to_string())?;
    list_profiles(conn)
}

#[tauri::command]
fn save_modal_profile(
    state: State<AppState>,
    profile: ProfileInput,
) -> Result<Vec<ModalProfileRow>, String> {
    let conn = state.0.lock().map_err(|error| error.to_string())?;
    save_profile(&conn, &profile)
}

fn set_profile_enabled(
    conn: &Connection,
    id: &str,
    enabled: bool,
) -> Result<Vec<ModalProfileRow>, String> {
    let changed = conn
        .execute(
            "UPDATE modal_profiles SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, if enabled { 1 } else { 0 }, now_rfc3339()],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err(String::from("계정을 찾지 못했습니다."));
    }
    list_profiles(conn)
}

#[tauri::command]
fn set_modal_profile_enabled(
    state: State<AppState>,
    id: String,
    enabled: bool,
) -> Result<Vec<ModalProfileRow>, String> {
    let conn = state.0.lock().map_err(|error| error.to_string())?;
    set_profile_enabled(&conn, &id, enabled)
}

/// Retires an account instead of deleting it. The row stays as a tombstone so
/// past jobs and cost records keep their account name, and nothing about the
/// user's usage history is destroyed.
fn archive_profile(conn: &Connection, id: &str) -> Result<Vec<ModalProfileRow>, String> {
    let changed = conn
        .execute(
            "UPDATE modal_profiles SET enabled = 0, archived_at = COALESCE(archived_at, ?2), \
             updated_at = ?2 WHERE id = ?1 AND archived_at IS NULL",
            params![id, now_rfc3339()],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err(String::from("보관할 Modal 계정을 찾지 못했습니다."));
    }
    list_profiles(conn)
}

#[tauri::command]
fn archive_modal_profile(state: State<AppState>, id: String) -> Result<Vec<ModalProfileRow>, String> {
    let conn = state.0.lock().map_err(|error| error.to_string())?;
    archive_profile(&conn, &id)
}

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
fn usage_rows(conn: &Connection, limit: i64) -> Result<Value, String> {
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
fn list_usage_rows(state: State<AppState>, job_limit: Option<i64>) -> Result<Value, String> {
    let limit = job_limit.unwrap_or(200).clamp(1, 1000);
    let conn = state.0.lock().map_err(|error| error.to_string())?;
    usage_rows(&conn, limit)
}

// ------------------------------------------------------------ billing sync

#[derive(Serialize)]
struct SyncOutcome {
    profile_id: String,
    name: String,
    ok: bool,
    period: String,
    total: Option<f64>,
    intervals: usize,
    objects: usize,
    message: Option<String>,
}

fn value_as_f64(value: Option<&Value>) -> Option<f64> {
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
    if let Some(name) = modal_profile.map(str::trim).filter(|value| !value.is_empty()) {
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
    serde_json::from_str(line.trim()).map_err(|error| format!("billing 동기화 JSON 파싱 실패: {error}"))
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
fn write_sync_summary(
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
async fn sync_modal_billing(
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
    let results: Vec<SyncResult> = tauri::async_runtime::spawn_blocking(move || run_billing_batch(selected))
        .await
        .map_err(|error| error.to_string())?;

    let now = now_rfc3339();
    let mut outcomes = Vec::new();
    let mut conn = state.0.lock().map_err(|error| error.to_string())?;
    let transaction = conn.transaction().map_err(|error| error.to_string())?;
    for (id, name, result) in results {
        match result {
            Ok(summary) => {
                let (intervals, objects, periods) = write_sync_summary(&transaction, &id, &summary, &now)?;
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
                        Some(String::from("리포트에 집계 구간이 없어 저장된 기록을 변경하지 않았습니다."))
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

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn deliverables_root() -> std::path::PathBuf {
    std::path::PathBuf::from(r"F:\modal-gui\deliverables")
}

fn thumbs_root() -> std::path::PathBuf {
    std::path::PathBuf::from(r"F:\modal-gui\thumbs")
}

fn default_clips_root() -> std::path::PathBuf {
    std::path::PathBuf::from(r"F:\modal-gui\h3-clips\generated")
}

fn default_music_root() -> std::path::PathBuf {
    std::path::PathBuf::from(r"F:\modal-gui\music")
}

fn edits_root() -> std::path::PathBuf {
    std::path::PathBuf::from(r"F:\modal-gui\edits")
}

/// Stable short tag for a full path, used to keep thumbnail caches distinct.
fn path_tag(path: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", ((hash >> 32) as u32) ^ (hash as u32))
}

#[derive(Serialize)]
struct MediaEntry {
    name: String,
    path: String,
    size_bytes: u64,
    modified: Option<String>,
}

fn scan_media(root: &std::path::Path, extensions: &[&str]) -> Vec<MediaEntry> {
    let mut entries = Vec::new();
    let read = match std::fs::read_dir(root) {
        Ok(read) => read,
        Err(_) => return entries,
    };
    for item in read.flatten() {
        let path = item.path();
        let matches = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| extensions.contains(&value.to_ascii_lowercase().as_str()))
            .unwrap_or(false);
        if !matches {
            continue;
        }
        let metadata = match item.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let modified = metadata
            .modified()
            .ok()
            .map(|time| chrono::DateTime::<chrono::Local>::from(time).to_rfc3339());
        entries.push(MediaEntry {
            name: path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string(),
            path: path.to_string_lossy().to_string(),
            size_bytes: metadata.len(),
            modified,
        });
    }
    entries.sort_by(|left, right| right.modified.cmp(&left.modified));
    entries
}

#[tauri::command]
fn list_pipeline_inputs(clips_root: Option<String>) -> Result<Value, String> {
    let clips_dir = clips_root.unwrap_or_else(|| default_clips_root().to_string_lossy().to_string());
    let audio_dir = default_music_root();
    let mut audio = scan_media(&audio_dir, &["flac", "wav", "mp3", "m4a"]);
    for depth in std::fs::read_dir(&audio_dir).into_iter().flatten().flatten() {
        if depth.path().is_dir() {
            audio.extend(scan_media(&depth.path(), &["flac", "wav", "mp3", "m4a"]));
            for nested in std::fs::read_dir(depth.path()).into_iter().flatten().flatten() {
                if nested.path().is_dir() {
                    audio.extend(scan_media(&nested.path(), &["flac", "wav", "mp3", "m4a"]));
                }
            }
        }
    }
    Ok(json!({
        "clips_root": clips_dir.clone(),
        "clips": scan_media(&std::path::PathBuf::from(&clips_dir), &["mp4", "mov", "mkv"]),
        "audio": audio,
        "renders": scan_media(&deliverables_root(), &["mp4", "mov"]),
        "edits": scan_media(&edits_root(), &["json"]),
        "markers": scan_media(&deliverables_root(), &["json"])
            .into_iter()
            .filter(|entry| entry.name.contains("beat"))
            .collect::<Vec<_>>(),
    }))
}

#[tauri::command]
fn read_json_file(path: String) -> Result<Value, String> {
    let raw = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

/// Extracts one frame so the result grid can show real thumbnails. Cached by
/// file name plus a path tag under `F:\modal-gui\thumbs`, so two files with the
/// same name in different folders never share a thumbnail. The caller falls
/// back to a video poster frame when ffmpeg is unavailable.
#[tauri::command]
fn make_thumbnail(path: String, time: Option<f64>) -> Result<String, String> {
    let source = std::path::PathBuf::from(&path);
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| String::from("파일 이름을 읽지 못했습니다."))?;
    let root = thumbs_root();
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let target = root.join(format!("{stem}-{}.jpg", path_tag(&path)));
    if target.exists() {
        return Ok(target.to_string_lossy().to_string());
    }

    let mut last_error = String::new();
    for attempt in [time.unwrap_or(1.5), 0.0] {
        let seek = format!("{attempt:.3}");
        let output = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-ss",
                seek.as_str(),
                "-i",
                path.as_str(),
                "-frames:v",
                "1",
                "-vf",
                "scale=480:-2",
                "-y",
            ])
            .arg(&target)
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() && target.exists() {
            return Ok(target.to_string_lossy().to_string());
        }
        last_error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let _ = std::fs::remove_file(&target);
        if last_error.is_empty() {
            return Err(String::from("ffmpeg가 프레임을 만들지 못했습니다."));
        }
    }
    Err(last_error)
}

#[tauri::command]
fn reveal_in_explorer(path: String) -> Result<(), String> {
    Command::new("explorer")
        .arg("/select,")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[derive(Deserialize)]
struct AnalyzeRequest {
    run_id: String,
    audio: String,
    output: String,
    duration: Option<f64>,
    sensitivity: Option<f64>,
    min_gap: Option<f64>,
    max_beats: Option<i64>,
    bins: Option<i64>,
}

#[derive(Deserialize)]
struct RenderRequest {
    run_id: String,
    mode: String,
    audio: String,
    clips_root: String,
    markers: Option<String>,
    cues: Option<String>,
    output: String,
    metadata: Option<String>,
    duration: Option<f64>,
    shot_seconds: Option<f64>,
    snap_cuts: Option<bool>,
    renderer: Option<String>,
    renderer_options: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct EditSpecRequest {
    run_id: String,
    spec: String,
    output: String,
    metadata: Option<String>,
    mode: Option<String>,
    renderer: Option<String>,
}

#[derive(Deserialize)]
struct StoryboardRequest {
    audio: String,
    output: String,
    clips_root: Option<String>,
    markers: Option<String>,
    duration: Option<f64>,
    shot_seconds: Option<f64>,
    snap_cuts: Option<bool>,
}

/// Runs the pipeline CLI and forwards each JSON line to the frontend as a
/// `pipeline-event` tagged with the run id.
fn spawn_pipeline(app: tauri::AppHandle, run_id: String, args: Vec<String>) -> Result<(), String> {
    let mut child = Command::new("python")
        .arg("tools/motion_graphics_pipeline.py")
        .args(&args)
        .current_dir(repo_root())
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUNBUFFERED", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_app = app.clone();
    let stderr_app = app.clone();
    let stdout_run = run_id.clone();
    let stderr_run = run_id.clone();

    thread::spawn(move || {
        if let Some(stdout) = stdout {
            for line in BufReader::new(stdout).lines().flatten() {
                match serde_json::from_str::<Value>(&line) {
                    Ok(mut value) => {
                        if let Some(object) = value.as_object_mut() {
                            object.insert("run_id".into(), Value::String(stdout_run.clone()));
                        }
                        let _ = stdout_app.emit("pipeline-event", value);
                    }
                    Err(_) => {
                        let _ = stdout_app.emit(
                            "pipeline-event",
                            json!({"type": "log", "run_id": stdout_run, "message": line}),
                        );
                    }
                }
            }
        }
        let mut error_tail: Vec<String> = Vec::new();
        if let Some(stderr) = stderr {
            for line in BufReader::new(stderr).lines().flatten() {
                error_tail.push(line.clone());
                if error_tail.len() > 20 {
                    error_tail.remove(0);
                }
                let _ = stderr_app.emit(
                    "pipeline-event",
                    json!({"type": "log", "level": "error", "run_id": stderr_run, "message": line}),
                );
            }
        }
        let status = child.wait();
        let ok = status.map(|value| value.success()).unwrap_or(false);
        if !ok {
            let _ = stderr_app.emit(
                "pipeline-event",
                json!({
                    "type": "failed",
                    "run_id": stderr_run,
                    "message": "파이프라인 프로세스가 비정상 종료되었습니다.",
                    "detail": error_tail.join("\n"),
                }),
            );
        }
        let _ = stderr_app.emit(
            "pipeline-event",
            json!({"type": "process_exit", "run_id": stderr_run, "success": ok}),
        );
    });

    Ok(())
}

#[tauri::command]
fn analyze_audio(app: tauri::AppHandle, request: AnalyzeRequest) -> Result<(), String> {
    let mut args = vec![
        String::from("analyze"),
        String::from("--audio"),
        request.audio,
        String::from("--output"),
        request.output,
        String::from("--duration"),
        request.duration.unwrap_or(60.0).to_string(),
        String::from("--sensitivity"),
        request.sensitivity.unwrap_or(1.1).to_string(),
        String::from("--min-gap"),
        request.min_gap.unwrap_or(0.3).to_string(),
    ];
    args.push(String::from("--max-beats"));
    args.push(request.max_beats.unwrap_or(64).to_string());
    args.push(String::from("--bins"));
    args.push(request.bins.unwrap_or(600).to_string());
    spawn_pipeline(app, request.run_id, args)
}

#[tauri::command]
fn render_trailer(app: tauri::AppHandle, request: RenderRequest) -> Result<(), String> {
    let mut args = vec![
        String::from("render"),
        String::from("--mode"),
        request.mode,
        String::from("--audio"),
        request.audio,
        String::from("--clips-root"),
        request.clips_root,
        String::from("--output"),
        request.output,
        String::from("--duration"),
        request.duration.unwrap_or(60.0).to_string(),
        String::from("--shot-seconds"),
        request.shot_seconds.unwrap_or(6.0).to_string(),
    ];
    if let Some(markers) = request.markers {
        args.push(String::from("--markers"));
        args.push(markers);
    }
    if let Some(cues) = request.cues {
        args.push(String::from("--cues"));
        args.push(cues);
    }
    if let Some(metadata) = request.metadata {
        args.push(String::from("--metadata"));
        args.push(metadata);
    }
    if request.snap_cuts.unwrap_or(false) {
        args.push(String::from("--snap-cuts"));
    }
    args.push(String::from("--renderer"));
    args.push(request.renderer.unwrap_or_else(|| String::from("ffmpeg")));
    for option in request.renderer_options.unwrap_or_default() {
        args.push(String::from("--renderer-option"));
        args.push(option);
    }
    spawn_pipeline(app, request.run_id, args)
}

/// Renders exactly the shots a storyboard file lists, trims included.
#[tauri::command]
fn render_spec(app: tauri::AppHandle, request: EditSpecRequest) -> Result<(), String> {
    let mut args = vec![
        String::from("edit"),
        String::from("--spec"),
        request.spec,
        String::from("--output"),
        request.output,
        String::from("--mode"),
        request.mode.unwrap_or_else(|| String::from("graphics")),
        String::from("--renderer"),
        request.renderer.unwrap_or_else(|| String::from("ffmpeg")),
    ];
    if let Some(metadata) = request.metadata {
        args.push(String::from("--metadata"));
        args.push(metadata);
    }
    spawn_pipeline(app, request.run_id, args)
}

/// Builds an editable shot list from the current material and returns it, so
/// the GUI (or an agent) has a starting point instead of a blank timeline.
#[tauri::command]
fn build_storyboard(request: StoryboardRequest) -> Result<Value, String> {
    let mut args = vec![
        String::from("storyboard"),
        String::from("--audio"),
        request.audio,
        String::from("--output"),
        request.output.clone(),
        String::from("--duration"),
        request.duration.unwrap_or(60.0).to_string(),
        String::from("--shot-seconds"),
        request.shot_seconds.unwrap_or(6.0).to_string(),
    ];
    if let Some(root) = request.clips_root {
        args.push(String::from("--clips-root"));
        args.push(root);
    }
    if let Some(markers) = request.markers {
        args.push(String::from("--markers"));
        args.push(markers);
    }
    if request.snap_cuts.unwrap_or(false) {
        args.push(String::from("--snap-cuts"));
    }
    let result = Command::new("python")
        .arg("tools/motion_graphics_pipeline.py")
        .args(&args)
        .current_dir(repo_root())
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .map_err(|error| error.to_string())?;
    if !result.status.success() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        let failure = stdout
            .lines()
            .rev()
            .find(|line| line.contains("\"failed\""))
            .map(|line| line.to_string());
        return Err(failure
            .unwrap_or_else(|| String::from_utf8_lossy(&result.stderr).to_string()));
    }
    let raw = std::fs::read_to_string(&request.output).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

/// Probes each renderer plugin on the host and returns availability plus
/// capabilities, so the GUI can reflect what is actually installed.
#[tauri::command]
fn list_renderers() -> Result<Value, String> {
    let output = Command::new("python")
        .arg("tools/motion_graphics_pipeline.py")
        .arg("plugins")
        .current_dir(repo_root())
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .rev()
        .find(|line| line.contains("\"renderers\""))
        .ok_or_else(|| "플러그인 목록을 읽지 못했습니다.".to_string())?;
    serde_json::from_str(line).map_err(|error| error.to_string())
}

#[tauri::command]
fn open_with_default(path: String) -> Result<(), String> {
    Command::new("cmd")
        .args(["/C", "start", "", &path])
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn write_json_file(path: String, value: Value) -> Result<(), String> {
    let target = std::path::PathBuf::from(&path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let body = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    std::fs::write(target, body + "\n").map_err(|error| error.to_string())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let path = app.path().app_data_dir()?.join("database");
            std::fs::create_dir_all(&path)?;
            let conn = Connection::open(path.join("app.db"))?;
            init_db(&conn)?;
            app.manage(AppState(Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health,
            create_job,
            start_job,
            start_music,
            list_modal_profiles,
            save_modal_profile,
            set_modal_profile_enabled,
            archive_modal_profile,
            list_usage_rows,
            sync_modal_billing,
            list_pipeline_inputs,
            read_json_file,
            make_thumbnail,
            write_json_file,
            reveal_in_explorer,
            analyze_audio,
            render_trailer,
            render_spec,
            build_storyboard,
            list_renderers,
            open_with_default
        ])
        .run(tauri::generate_context!())
        .expect("error while running Modal GUI");
}

#[cfg(test)]
mod tests;
