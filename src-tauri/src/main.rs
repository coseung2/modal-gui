use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use tauri::{Manager, State};

struct AppState(Mutex<Connection>);
#[derive(Serialize)] struct Health { database: bool, sidecar: bool }
#[derive(Deserialize)] struct NewJob { id: String, prompt: String, input_path: String, duration: i64, resolution: String }

fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(r#"
      PRAGMA foreign_keys=ON;
      CREATE TABLE IF NOT EXISTS modal_profiles (id TEXT PRIMARY KEY, name TEXT NOT NULL, workspace_label TEXT, enabled INTEGER NOT NULL DEFAULT 1, keychain_ref TEXT NOT NULL, budget_limit REAL, budget_used REAL NOT NULL DEFAULT 0, reserve_amount REAL NOT NULL DEFAULT 0, max_concurrency INTEGER NOT NULL DEFAULT 1, priority INTEGER NOT NULL DEFAULT 0, cooldown_until TEXT, last_error TEXT, last_used_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS jobs (id TEXT PRIMARY KEY, modal_profile_id TEXT, function_call_id TEXT, status TEXT NOT NULL, stage TEXT NOT NULL, prompt TEXT NOT NULL, input_path TEXT NOT NULL, output_path TEXT, thumbnail_path TEXT, duration INTEGER, resolution TEXT, seed INTEGER, progress REAL, error_code TEXT, error_message TEXT, retry_count INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, started_at TEXT, completed_at TEXT, FOREIGN KEY (modal_profile_id) REFERENCES modal_profiles(id));
      CREATE TABLE IF NOT EXISTS job_events (id INTEGER PRIMARY KEY AUTOINCREMENT, job_id TEXT NOT NULL, event_type TEXT NOT NULL, level TEXT, stage TEXT, message TEXT, payload_json TEXT, created_at TEXT NOT NULL, FOREIGN KEY (job_id) REFERENCES jobs(id));
      CREATE TABLE IF NOT EXISTS usage_records (id INTEGER PRIMARY KEY AUTOINCREMENT, modal_profile_id TEXT NOT NULL, job_id TEXT, source TEXT NOT NULL, amount REAL, raw_json TEXT, observed_at TEXT NOT NULL, FOREIGN KEY (modal_profile_id) REFERENCES modal_profiles(id));
    "#)
}

#[tauri::command] fn health(state: State<AppState>) -> Result<Health, String> { Ok(Health { database: state.0.lock().map_err(|e| e.to_string()).is_ok(), sidecar: false }) }
#[tauri::command] fn create_job(state: State<AppState>, job: NewJob) -> Result<(), String> { let now = chrono::Utc::now().to_rfc3339(); state.0.lock().map_err(|e| e.to_string())?.execute("INSERT INTO jobs (id,status,stage,prompt,input_path,duration,resolution,created_at) VALUES (?1,'QUEUED','JOB_CREATED',?2,?3,?4,?5,?6)", params![job.id,job.prompt,job.input_path,job.duration,job.resolution,now]).map_err(|e| e.to_string())?; Ok(()) }

#[tauri::command]
fn start_job(job: NewJob) -> Result<(), String> {
    let worker = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../worker/main.py");
    let mut child = Command::new("python")
        .arg(worker)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let message = serde_json::json!({
        "type": "start_job",
        "job_id": job.id,
        "profile_id": "modal_01",
        "input_path": job.input_path,
        "prompt": job.prompt,
        "options": {"duration": job.duration, "resolution": job.resolution}
    });
    let stdin = child.stdin.as_mut().ok_or_else(|| "worker stdin unavailable".to_string())?;
    writeln!(stdin, "{}", message).map_err(|e| e.to_string())?;
    Ok(())
}

fn main() { tauri::Builder::default().setup(|app| { let path = app.path().app_data_dir()?.join("database"); std::fs::create_dir_all(&path)?; let conn = Connection::open(path.join("app.db"))?; init_db(&conn)?; app.manage(AppState(Mutex::new(conn))); Ok(()) }).invoke_handler(tauri::generate_handler![health, create_job, start_job]).run(tauri::generate_context!()).expect("error while running Modal GUI"); }
