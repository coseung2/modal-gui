//! SQLite lifecycle, migrations and shared time/period queries.
use rusqlite::{params, Connection};
use std::sync::Mutex;

pub(crate) struct AppState(pub(crate) Mutex<Connection>);

pub(crate) fn init_db(conn: &Connection) -> rusqlite::Result<()> {
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

pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Month tag (UTC) that keeps one billing period per profile.
pub(crate) fn current_period() -> String {
    chrono::Utc::now().format("%Y-%m").to_string()
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

pub(crate) fn read_active_modal_profile(path: &std::path::Path) -> Option<String> {
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

/// Keeps the first run usable: one account row pointing at whatever profile the
/// local Modal CLI currently uses. Created only when no account exists yet.
fn seed_default_profile(conn: &Connection) -> rusqlite::Result<()> {
    let existing: i64 =
        conn.query_row("SELECT COUNT(*) FROM modal_profiles", [], |row| row.get(0))?;
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

/// The newest billing period actually stored, not the local month. A report whose
/// period differs from the local month (KST month start versus UTC) must not make
/// every account read as zero.
pub(crate) fn latest_billing_period(conn: &Connection) -> String {
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
