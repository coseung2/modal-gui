//! Account selection, validation and lifecycle commands.
use crate::database::{latest_billing_period, now_rfc3339, AppState};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

pub(crate) fn is_valid_modal_profile_name(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | '+'))
}

/// Resolves the Modal account for a job.
///
/// An explicit account is honored only when it exists and is enabled: a stale or
/// stopped selection fails loudly instead of silently running on another
/// account. Only an empty request falls back to the first enabled account.
/// The second value is the local Modal CLI profile name (`MODAL_PROFILE`).
pub(crate) fn resolve_profile(
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

fn read_profile_choice(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, String, i64, Option<String>)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

const NO_PROFILE_MESSAGE: &str =
    "사용 중인 Modal 계정이 없습니다. 사용량 탭에서 계정을 추가하거나 사용으로 전환하세요.";

// ------------------------------------------------------------------ accounts

#[derive(Serialize)]
pub(crate) struct ModalProfileRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) modal_profile_name: Option<String>,
    pub(crate) workspace_label: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) keychain_ref: String,
    pub(crate) budget_limit: Option<f64>,
    pub(crate) reserve_amount: f64,
    pub(crate) max_concurrency: i64,
    pub(crate) priority: i64,
    pub(crate) last_used_at: Option<String>,
    pub(crate) last_synced_at: Option<String>,
    pub(crate) last_sync_error: Option<String>,
    pub(crate) period: String,
    pub(crate) month_cost: f64,
    pub(crate) month_intervals: i64,
}

#[derive(Deserialize)]
pub(crate) struct ProfileInput {
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    pub(crate) modal_profile_name: Option<String>,
    pub(crate) workspace_label: Option<String>,
    pub(crate) keychain_ref: Option<String>,
    pub(crate) budget_limit: Option<f64>,
    pub(crate) max_concurrency: Option<i64>,
    pub(crate) priority: Option<i64>,
}

fn trimmed(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

pub(crate) fn list_profiles(conn: &Connection) -> Result<Vec<ModalProfileRow>, String> {
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
pub(crate) fn list_modal_profiles(state: State<AppState>) -> Result<Vec<ModalProfileRow>, String> {
    let conn = state.0.lock().map_err(|error| error.to_string())?;
    list_profiles(&conn)
}

/// Full replace of one account. `budget_limit` is the optional allocated credit
/// declared by the user; an empty value means "unknown", never a guessed balance.
pub(crate) fn save_profile(
    conn: &Connection,
    profile: &ProfileInput,
) -> Result<Vec<ModalProfileRow>, String> {
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
pub(crate) fn save_modal_profile(
    state: State<AppState>,
    profile: ProfileInput,
) -> Result<Vec<ModalProfileRow>, String> {
    let conn = state.0.lock().map_err(|error| error.to_string())?;
    save_profile(&conn, &profile)
}

pub(crate) fn set_profile_enabled(
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
pub(crate) fn set_modal_profile_enabled(
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
pub(crate) fn archive_profile(conn: &Connection, id: &str) -> Result<Vec<ModalProfileRow>, String> {
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
pub(crate) fn archive_modal_profile(
    state: State<AppState>,
    id: String,
) -> Result<Vec<ModalProfileRow>, String> {
    let conn = state.0.lock().map_err(|error| error.to_string())?;
    archive_profile(&conn, &id)
}
