//! Unit tests for the SQLite layer behind the usage dashboard.
//!
//! The Modal CLI reports cost per app and interval, so most of the risk lives in
//! migrations and in the join/aggregate queries. These tests run against
//! in-memory databases and never touch the user's application database.

use crate::accounts::{
    archive_profile, is_valid_modal_profile_name, list_profiles, resolve_profile, save_profile,
    set_profile_enabled, ProfileInput,
};
use crate::database::{current_period, init_db, latest_billing_period, read_active_modal_profile};
use crate::diagnostics::{sanitize_detail, DETAIL_LIMIT};
use crate::jobs::{apply_worker_event, mark_spawn_failure_row, mark_worker_exit_row};
use crate::usage::{usage_rows, value_as_f64, write_sync_summary};
use rusqlite::params;
use rusqlite::Connection;
use serde_json::json;
use serde_json::Value;

/// The schema as shipped before accounts and usage gained their newer columns.
fn legacy_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE modal_profiles (
           id TEXT PRIMARY KEY, name TEXT NOT NULL, workspace_label TEXT,
           enabled INTEGER NOT NULL DEFAULT 1, keychain_ref TEXT NOT NULL, budget_limit REAL,
           budget_used REAL NOT NULL DEFAULT 0, reserve_amount REAL NOT NULL DEFAULT 0,
           max_concurrency INTEGER NOT NULL DEFAULT 1, priority INTEGER NOT NULL DEFAULT 0,
           cooldown_until TEXT, last_error TEXT, last_used_at TEXT,
           created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
         CREATE TABLE jobs (
           id TEXT PRIMARY KEY, modal_profile_id TEXT, function_call_id TEXT,
           status TEXT NOT NULL, stage TEXT NOT NULL, prompt TEXT NOT NULL, input_path TEXT NOT NULL,
           output_path TEXT, thumbnail_path TEXT, duration INTEGER, resolution TEXT, seed INTEGER,
           progress REAL, error_code TEXT, error_message TEXT,
           retry_count INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, started_at TEXT,
           completed_at TEXT);
         CREATE TABLE job_events (
           id INTEGER PRIMARY KEY AUTOINCREMENT, job_id TEXT NOT NULL, event_type TEXT NOT NULL,
           level TEXT, stage TEXT, message TEXT, payload_json TEXT, created_at TEXT NOT NULL);
         CREATE TABLE usage_records (
           id INTEGER PRIMARY KEY AUTOINCREMENT, modal_profile_id TEXT NOT NULL, job_id TEXT,
           source TEXT NOT NULL, amount REAL, raw_json TEXT, observed_at TEXT NOT NULL);",
    )
    .unwrap();
    conn
}

fn fresh_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn
}

fn draft(id: Option<&str>, name: &str) -> ProfileInput {
    ProfileInput {
        id: id.map(str::to_string),
        name: name.to_string(),
        modal_profile_name: Some(String::from("mallagaenge")),
        workspace_label: Some(String::from("main")),
        keychain_ref: Some(String::from("keychain:modal/mallagaenge")),
        budget_limit: Some(12.5),
        max_concurrency: None,
        priority: None,
    }
}

#[test]
fn migrations_upgrade_the_previous_schema_without_losing_rows() {
    let conn = legacy_db();
    conn.execute(
        "INSERT INTO modal_profiles (id, name, keychain_ref, created_at, updated_at) \
         VALUES ('modal_01', '기존 계정', '', 'x', 'x')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO jobs (id, status, stage, prompt, input_path, created_at) \
         VALUES ('job_1', 'COMPLETED', 'COMPLETED', 'p', 'C:/a.png', 'x')",
        [],
    )
    .unwrap();
    init_db(&conn).unwrap();

    let profiles = list_profiles(&conn).unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].name, "기존 계정");
    assert_eq!(profiles[0].modal_profile_name, None);
    assert_eq!(profiles[0].month_cost, 0.0);
    let kind: Option<String> = conn
        .query_row("SELECT kind FROM jobs WHERE id = 'job_1'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(kind, None);

    // Migrations are idempotent, so repeated startups stay harmless.
    init_db(&conn).unwrap();
    init_db(&conn).unwrap();
    assert_eq!(list_profiles(&conn).unwrap().len(), 1);
    let indexes: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'usage_records'",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(
        indexes.contains(&String::from("usage_records_profile_period")),
        "{indexes:?}"
    );
    assert!(
        indexes.contains(&String::from("usage_records_job_id")),
        "{indexes:?}"
    );
}

#[test]
fn a_fresh_database_seeds_exactly_one_usable_account() {
    let conn = fresh_db();
    let profiles = list_profiles(&conn).unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].id, "modal_01");
    assert!(profiles[0].enabled);
    assert_eq!(profiles[0].keychain_ref, "");
    assert_eq!(profiles[0].budget_limit, None);
}

#[test]
fn saving_an_account_keeps_its_id_and_enabled_state() {
    let conn = fresh_db();
    set_profile_enabled(&conn, "modal_01", false).unwrap();
    let updated = save_profile(&conn, &draft(Some("modal_01"), "H3 계정")).unwrap();
    let profile = updated.iter().find(|item| item.id == "modal_01").unwrap();
    assert_eq!(profile.name, "H3 계정");
    assert_eq!(profile.modal_profile_name.as_deref(), Some("mallagaenge"));
    assert_eq!(profile.workspace_label.as_deref(), Some("main"));
    assert_eq!(profile.budget_limit, Some(12.5));
    assert_eq!(profile.keychain_ref, "keychain:modal/mallagaenge");
    assert!(!profile.enabled);

    let created = save_profile(&conn, &draft(None, "두 번째")).unwrap();
    assert_eq!(created.len(), 2);
    assert_eq!(created.iter().filter(|item| item.enabled).count(), 1);
    assert!(created
        .iter()
        .any(|item| item.name == "두 번째" && item.id != "modal_01"));
}

#[test]
fn account_totals_follow_the_stored_period_not_the_local_month() {
    let conn = fresh_db();
    // Deliberately not the local month: the read side must not go blank just
    // because the report period and the local calendar month disagree.
    for (source, period_tag, amount) in [
        ("modal_billing_report", "2000-01", 1.25_f64),
        ("modal_billing_report", "2000-01", 0.75),
        ("modal_billing_report", "1999-12", 99.0),
        ("job_usage", "2000-01", 5.0),
    ] {
        conn.execute(
            "INSERT INTO usage_records (modal_profile_id, job_id, source, amount, period, observed_at) \
             VALUES ('modal_01', NULL, ?1, ?2, ?3, 'now')",
            params![source, amount, period_tag],
        )
        .unwrap();
    }
    assert_eq!(latest_billing_period(&conn), "2000-01");
    let profiles = list_profiles(&conn).unwrap();
    assert_eq!(profiles[0].period, "2000-01");
    assert_eq!(profiles[0].month_cost, 2.0);
    assert_eq!(profiles[0].month_intervals, 2);

    // With nothing stored, the local month is the only honest label.
    let empty = Connection::open_in_memory().unwrap();
    init_db(&empty).unwrap();
    assert_eq!(latest_billing_period(&empty), current_period());
}

#[test]
fn usage_rows_pair_jobs_with_recorded_cost_only() {
    let conn = fresh_db();
    conn.execute(
        "INSERT INTO jobs (id, modal_profile_id, status, stage, kind, prompt, input_path, created_at) \
         VALUES ('job_a', 'modal_01', 'COMPLETED', 'COMPLETED', 'fl2v', '프롬프트 A', 'C:/a.png', \
                 '2026-09-01T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO jobs (id, modal_profile_id, status, stage, kind, prompt, input_path, created_at) \
         VALUES ('job_b', 'modal_01', 'QUEUED', 'JOB_CREATED', 'music', '프롬프트 B', '', \
                 '2026-09-02T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO usage_records (modal_profile_id, job_id, source, amount, period, observed_at) \
         VALUES ('modal_01', 'job_a', 'job_usage', 0.42, ?1, 'now')",
        params![current_period()],
    )
    .unwrap();

    let value = usage_rows(&conn, 50).unwrap();
    let jobs = value.get("jobs").and_then(Value::as_array).unwrap();
    assert_eq!(jobs.len(), 2);
    let job_a = jobs.iter().find(|item| item["id"] == "job_a").unwrap();
    assert_eq!(job_a["recorded_cost"].as_f64(), Some(0.42));
    assert_eq!(job_a["profile_name"].as_str(), Some("기본 Modal 계정"));
    let job_b = jobs.iter().find(|item| item["id"] == "job_b").unwrap();
    assert!(job_b["recorded_cost"].is_null());
    assert_eq!(job_b["kind"].as_str(), Some("music"));
    // App-level billing rows stay out of the per-job table.
    assert!(value
        .get("objects")
        .and_then(Value::as_array)
        .unwrap()
        .is_empty());
}

#[test]
fn archiving_an_account_keeps_its_jobs_and_cost_history() {
    let conn = fresh_db();
    conn.execute(
        "INSERT INTO jobs (id, modal_profile_id, status, stage, prompt, input_path, created_at) \
         VALUES ('job_a', 'modal_01', 'COMPLETED', 'COMPLETED', 'p', 'i', 'x')",
        [],
    )
    .unwrap();
    for (job_id, source, amount) in [
        (Some("job_a"), "job_usage", 0.4_f64),
        (None, "modal_billing_report", 3.5),
    ] {
        conn.execute(
            "INSERT INTO usage_records (modal_profile_id, job_id, source, amount, period, observed_at) \
             VALUES ('modal_01', ?1, ?2, ?3, ?4, 'now')",
            params![job_id, source, amount, current_period()],
        )
        .unwrap();
    }

    // The account disappears from the active list but nothing is destroyed.
    assert!(archive_profile(&conn, "modal_01").unwrap().is_empty());
    let (enabled, archived): (i64, Option<String>) = conn
        .query_row(
            "SELECT enabled, archived_at FROM modal_profiles WHERE id = 'modal_01'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(enabled, 0);
    assert!(archived.is_some());
    let jobs: i64 = conn
        .query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))
        .unwrap();
    let records: i64 = conn
        .query_row("SELECT COUNT(*) FROM usage_records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(jobs, 1);
    assert_eq!(records, 2);

    // History still resolves the account name from the tombstone.
    let rows = usage_rows(&conn, 50).unwrap();
    assert_eq!(
        rows["jobs"][0]["profile_name"].as_str(),
        Some("기본 Modal 계정")
    );
    assert!(archive_profile(&conn, "modal_01").is_err());
    assert!(archive_profile(&conn, "ghost").is_err());
}

#[test]
fn account_validation_rejects_bad_input_without_storing_secrets() {
    let conn = fresh_db();
    let blank = ProfileInput {
        name: String::from("   "),
        ..draft(None, "x")
    };
    assert!(save_profile(&conn, &blank).is_err());
    let bad_profile = ProfileInput {
        modal_profile_name: Some(String::from("bad name!")),
        ..draft(None, "ok")
    };
    assert!(save_profile(&conn, &bad_profile).is_err());

    assert!(is_valid_modal_profile_name("mallagaenge"));
    assert!(is_valid_modal_profile_name("team-1@example.com"));
    assert!(!is_valid_modal_profile_name(""));
    assert!(!is_valid_modal_profile_name("has space"));
    assert!(!is_valid_modal_profile_name(&"a".repeat(65)));

    // An unusable credit value becomes "unknown" instead of a guessed balance.
    let nan = ProfileInput {
        budget_limit: Some(f64::NAN),
        ..draft(Some("modal_01"), "계정")
    };
    assert_eq!(save_profile(&conn, &nan).unwrap()[0].budget_limit, None);
    let negative = ProfileInput {
        budget_limit: Some(-3.0),
        ..draft(Some("modal_01"), "계정")
    };
    assert_eq!(
        save_profile(&conn, &negative).unwrap()[0].budget_limit,
        None
    );
}

#[test]
fn an_explicit_account_is_never_switched_or_stopped_silently() {
    let conn = fresh_db();
    let (id, modal_profile) = resolve_profile(&conn, &Some(String::from("modal_01"))).unwrap();
    assert_eq!(id, "modal_01");
    assert_eq!(modal_profile.as_deref(), Some("mallagaenge"));
    let used: Option<String> = conn
        .query_row(
            "SELECT last_used_at FROM modal_profiles WHERE id = 'modal_01'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(used.is_some());

    // A stale or archived id is an error, not a silent switch to another account.
    let stale = resolve_profile(&conn, &Some(String::from("ghost"))).unwrap_err();
    assert!(stale.contains("ghost"), "{stale}");

    // A higher-priority account is only chosen when the request is empty.
    let preferred = ProfileInput {
        priority: Some(50),
        ..draft(None, "우선 계정")
    };
    save_profile(&conn, &preferred).unwrap();
    assert_ne!(resolve_profile(&conn, &None).unwrap().0, "modal_01");
    assert_eq!(
        resolve_profile(&conn, &Some(String::from("modal_01")))
            .unwrap()
            .0,
        "modal_01"
    );

    // A stopped account is rejected even when it is named explicitly.
    set_profile_enabled(&conn, "modal_01", false).unwrap();
    let stopped = resolve_profile(&conn, &Some(String::from("modal_01"))).unwrap_err();
    assert!(stopped.contains("중지된"), "{stopped}");

    // No enabled account at all is rejected for both empty and named requests.
    let all: Vec<String> = list_profiles(&conn)
        .unwrap()
        .into_iter()
        .map(|item| item.id)
        .collect();
    assert_eq!(all.len(), 2);
    for id in all {
        set_profile_enabled(&conn, &id, false).unwrap();
    }
    assert!(resolve_profile(&conn, &None).is_err());
    assert!(resolve_profile(&conn, &Some(String::new())).is_err());

    // Archived accounts are not selectable either.
    archive_profile(&conn, "modal_01").unwrap();
    assert!(resolve_profile(&conn, &Some(String::from("modal_01"))).is_err());
}

#[test]
fn billing_totals_are_read_from_strings_or_numbers() {
    assert_eq!(value_as_f64(Some(&json!("7.24770579"))), Some(7.24770579));
    assert_eq!(value_as_f64(Some(&json!(2))), Some(2.0));
    assert_eq!(value_as_f64(Some(&json!("n/a"))), None);
    assert_eq!(value_as_f64(Some(&Value::Null)), None);
    assert_eq!(value_as_f64(None), None);
}

#[test]
fn worker_events_update_job_state_and_leave_an_audit_row() {
    let conn = fresh_db();
    conn.execute(
        "INSERT INTO jobs (id, modal_profile_id, status, stage, prompt, input_path, created_at) \
         VALUES ('job_1', 'modal_01', 'QUEUED', 'JOB_CREATED', 'p', 'C:/a.png', 'x')",
        [],
    )
    .unwrap();
    apply_worker_event(
        &conn,
        &json!({"type": "stage", "job_id": "job_1", "stage": "GENERATING"}),
    )
    .unwrap();
    let (status, stage, started): (String, String, Option<String>) = conn
        .query_row(
            "SELECT status, stage, started_at FROM jobs WHERE id = 'job_1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, "RUNNING");
    assert_eq!(stage, "GENERATING");
    assert!(started.is_some());

    apply_worker_event(
        &conn,
        &json!({"type": "stage", "job_id": "job_1", "stage": "RESULT_DOWNLOADING"}),
    )
    .unwrap();
    let status: String = conn
        .query_row("SELECT status FROM jobs WHERE id = 'job_1'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(status, "DOWNLOADING");

    apply_worker_event(
        &conn,
        &json!({"type": "completed", "job_id": "job_1", "local_output_path": "F:/out/clip.mp4"}),
    )
    .unwrap();
    let (status, stage, completed, output, progress): (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<f64>,
    ) = conn
        .query_row(
            "SELECT status, stage, completed_at, output_path, progress FROM jobs WHERE id = 'job_1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .unwrap();
    assert_eq!(status, "COMPLETED");
    assert_eq!(stage, "COMPLETED");
    assert!(completed.is_some());
    assert_eq!(output.as_deref(), Some("F:/out/clip.mp4"));
    assert_eq!(progress, Some(100.0));

    conn.execute(
        "INSERT INTO jobs (id, status, stage, prompt, input_path, created_at) \
         VALUES ('job_2', 'RUNNING', 'GENERATING', 'p', '', 'x')",
        [],
    )
    .unwrap();
    apply_worker_event(
        &conn,
        &json!({
            "type": "failed",
            "job_id": "job_2",
            "code": "MODAL_GENERATION_FAILED",
            "message": "boom ak-0123456789abcdef0123\nsecond line"
        }),
    )
    .unwrap();
    let (status, code, message): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT status, error_code, error_message FROM jobs WHERE id = 'job_2'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, "FAILED");
    assert_eq!(code.as_deref(), Some("MODAL_GENERATION_FAILED"));
    let message = message.unwrap();
    assert!(!message.contains("0123456789abcdef"), "{message}");
    assert!(message.contains("<redacted>"), "{message}");
    assert!(!message.contains('\n'), "{message}");

    // Events for a job we do not know about are ignored, not errors.
    apply_worker_event(
        &conn,
        &json!({"type": "stage", "job_id": "ghost", "stage": "GENERATING"}),
    )
    .unwrap();
    assert!(apply_worker_event(&conn, &json!({"type": "stage"})).is_ok());
    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM job_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(events, 4);
}

#[test]
fn repeated_syncs_stay_duplicate_free_and_keep_older_months() {
    let conn = fresh_db();
    conn.execute(
        "INSERT INTO usage_records (modal_profile_id, job_id, source, amount, period, label, observed_at) \
         VALUES ('modal_01', NULL, 'modal_billing_report', 4.5, '2026-09', 'minimax-h3-latest-workflows', 'old')",
        [],
    )
    .unwrap();
    let october = json!({
        "period": "2026-10",
        "periods": ["2026-10"],
        "total": "1.25",
        "apps": [{"object_id": "ap-1", "description": "minimax-h3-latest-workflows", "cost": "1.25", "intervals": 1}],
        "rows": [{"object_id": "ap-1", "description": "minimax-h3-latest-workflows", "period": "2026-10", "cost": "1.25"}],
        "intervals": 1
    });
    let (intervals, objects, periods) =
        write_sync_summary(&conn, "modal_01", &october, "now").unwrap();
    assert_eq!((intervals, objects), (1, 1));
    assert_eq!(periods, vec![String::from("2026-10")]);
    write_sync_summary(&conn, "modal_01", &october, "later").unwrap();

    let count = |period_tag: &str| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM usage_records WHERE period = ?1",
            params![period_tag],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(count("2026-09"), 1);
    assert_eq!(count("2026-10"), 1);
    assert_eq!(latest_billing_period(&conn), "2026-10");
    let profiles = list_profiles(&conn).unwrap();
    assert_eq!(profiles[0].month_cost, 1.25);

    // A report with no intervals changes nothing at all.
    let empty = json!({"period": "2026-11", "periods": [], "total": "0", "rows": [], "apps": []});
    let (intervals, objects, periods) =
        write_sync_summary(&conn, "modal_01", &empty, "now").unwrap();
    assert_eq!((intervals, objects), (0, 0));
    assert!(periods.is_empty());
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM usage_records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total, 2);
}

#[test]
fn sensitive_detail_is_redacted_and_clipped() {
    let message = sanitize_detail(
        "failed\nMODAL_TOKEN_SECRET=as-9876543210abcdef token=Bearer sk-abcdef0123456789 end",
        DETAIL_LIMIT,
    );
    assert!(!message.contains("9876543210abcdef"), "{message}");
    assert!(!message.contains("abcdef0123456789"), "{message}");
    assert!(!message.contains('\n'), "{message}");
    assert!(message.contains("<redacted>"), "{message}");

    let long = sanitize_detail(&"x".repeat(900), DETAIL_LIMIT);
    assert_eq!(long.chars().count(), DETAIL_LIMIT + 1);
    assert!(long.ends_with('…'));
    assert_eq!(
        sanitize_detail("  tidy   text  ", DETAIL_LIMIT),
        "tidy text"
    );
}

#[test]
fn quoted_and_nested_secrets_never_survive() {
    // The exact shapes reported by review: the quoted value used to survive.
    assert_eq!(
        sanitize_detail(r#"token="sess-abcdef123456""#, DETAIL_LIMIT),
        "token=<redacted>"
    );
    assert_eq!(
        sanitize_detail(r#"MODAL_TOKEN_SECRET="plainvalue123""#, DETAIL_LIMIT),
        "MODAL_TOKEN_SECRET=<redacted>"
    );
    // A quoted value used to stop at the quote and leave the secret in place.
    for (input, secret) in [
        (r#"token="sess-abcdef123456" done"#, "sess-abcdef123456"),
        (
            r#"MODAL_TOKEN_SECRET="plainvalue123" done"#,
            "plainvalue123",
        ),
        (
            r#"MODAL_TOKEN_SECRET = 'single-quoted-secret' end"#,
            "single-quoted-secret",
        ),
        (
            r#"MODAL_TOKEN_SECRET = "spaced-secret-321" end"#,
            "spaced-secret-321",
        ),
        (
            r#"token = `backtick-secret-222` end"#,
            "backtick-secret-222",
        ),
        (
            r#"Authorization: Bearer "quoted-bearer-secret" end"#,
            "quoted-bearer-secret",
        ),
        (
            r#"Authorization: Bearer bare-bearer-secret end"#,
            "bare-bearer-secret",
        ),
        (
            r#"{"token":"json-secret-987","other":1}"#,
            "json-secret-987",
        ),
        (
            r#"{"api":{"MODAL_TOKEN_SECRET":"nested-secret-654"}}"#,
            "nested-secret-654",
        ),
        (r#"token: colon-secret-777 end"#, "colon-secret-777"),
        (r#"token="esc\"aped-secret-111" end"#, "aped-secret-111"),
        (
            r#"token="unterminated-secret-999"#,
            "unterminated-secret-999",
        ),
        (
            r#"MODAL_TOKEN_SECRET=as-9876543210abcdef"#,
            "9876543210abcdef",
        ),
        (r#"key=ghp_abcdef0123456789"#, "abcdef0123456789"),
        (r#"token=ak-0123456789abcdef"#, "0123456789abcdef"),
        (r#"sk-abcdef0123456789"#, "abcdef0123456789"),
    ] {
        let cleaned = sanitize_detail(input, DETAIL_LIMIT);
        assert!(
            !cleaned.contains(secret),
            "leaked {secret} from {input} -> {cleaned}"
        );
        assert!(
            cleaned.contains("<redacted>"),
            "nothing redacted: {input} -> {cleaned}"
        );
    }
}

#[test]
fn redaction_markers_are_not_duplicated_or_added_without_a_value() {
    let collapsed = sanitize_detail("token=Bearer sk-abcdef0123456789 end", DETAIL_LIMIT);
    assert!(!collapsed.contains("abcdef0123456789"), "{collapsed}");
    assert_eq!(collapsed.matches("<redacted>").count(), 1, "{collapsed}");

    let already = sanitize_detail("a <redacted> <redacted> b", DETAIL_LIMIT);
    assert_eq!(already, "a <redacted> b");

    // A marker with nothing after it stays untouched and prose is preserved.
    assert_eq!(sanitize_detail("token=", DETAIL_LIMIT), "token=");
    assert_eq!(
        sanitize_detail("MODAL_TOKEN_SECRET= ", DETAIL_LIMIT),
        "MODAL_TOKEN_SECRET="
    );
    assert_eq!(
        sanitize_detail("MODAL_TOKEN_SECRET:   ", DETAIL_LIMIT),
        "MODAL_TOKEN_SECRET:"
    );
    assert_eq!(
        sanitize_detail("no token expired here", DETAIL_LIMIT),
        "no token expired here"
    );

    let clipped = sanitize_detail(&format!(r#"token="{}""#, "s".repeat(900)), DETAIL_LIMIT);
    assert!(!clipped.contains("ssss"), "{clipped}");
    assert_eq!(clipped, "token=<redacted>");
    assert_eq!(clipped.matches("<redacted>").count(), 1, "{clipped}");
}

#[test]
fn a_dead_or_unspawnable_worker_cannot_leave_a_job_running() {
    let conn = fresh_db();
    for id in ["job_running", "job_done", "job_queued"] {
        conn.execute(
            "INSERT INTO jobs (id, modal_profile_id, status, stage, prompt, input_path, created_at) \
             VALUES (?1, 'modal_01', 'RUNNING', 'GENERATING', 'p', '', 'x')",
            params![id],
        )
        .unwrap();
    }
    conn.execute(
        "UPDATE jobs SET status = 'COMPLETED', stage = 'COMPLETED' WHERE id = 'job_done'",
        [],
    )
    .unwrap();

    mark_worker_exit_row(&conn, "job_running", Some(1));
    mark_worker_exit_row(&conn, "job_done", Some(1));
    mark_spawn_failure_row(&conn, "job_queued", "python: not found");

    let row = |id: &str| -> (String, Option<String>, Option<String>) {
        conn.query_row(
            "SELECT status, error_code, completed_at FROM jobs WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
    };
    let (status, code, completed) = row("job_running");
    assert_eq!(status, "FAILED");
    assert_eq!(code.as_deref(), Some("WORKER_EXITED"));
    assert!(completed.is_some());
    // A finished job is never rewritten by a late exit report.
    assert_eq!(row("job_done").0, "COMPLETED");
    // A job that never reached a worker is reported instead of staying QUEUED.
    assert_eq!(row("job_queued").0, "FAILED");
}

#[test]
fn the_active_modal_profile_returns_only_a_section_name() {
    let dir = std::env::temp_dir().join("modal-gui-active-profile-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("modal.toml");
    std::fs::write(
        &path,
        "[first]\ntoken = \"a\"\ntoken = \"b\"\n\n[second]\ntoken = \"c\"\nactive = true\n",
    )
    .unwrap();
    // Only the section name comes back; token lines never leave the file read.
    assert_eq!(read_active_modal_profile(&path).as_deref(), Some("second"));
    assert!(!read_active_modal_profile(&path)
        .unwrap_or_default()
        .contains("token"));
    std::fs::write(&path, "[only]\ntoken = \"a\"\n").unwrap();
    assert_eq!(read_active_modal_profile(&path), None);
    assert_eq!(read_active_modal_profile(&dir.join("missing.toml")), None);
    let _ = std::fs::remove_dir_all(&dir);
}
