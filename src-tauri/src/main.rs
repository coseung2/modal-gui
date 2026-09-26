mod accounts;
mod database;
mod diagnostics;
mod jobs;
mod media;
mod paths;
mod pipeline;
mod usage;

use database::{init_db, AppState};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;
use tauri::{Manager, State};

#[derive(Serialize)]
struct Health {
    database: bool,
    sidecar: bool,
}

#[tauri::command]
fn health(state: State<AppState>) -> Result<Health, String> {
    let database = state.0.lock().map_err(|error| error.to_string()).is_ok();
    Ok(Health {
        database,
        sidecar: true,
    })
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
            jobs::create_job,
            jobs::start_job,
            jobs::start_music,
            accounts::list_modal_profiles,
            accounts::save_modal_profile,
            accounts::set_modal_profile_enabled,
            accounts::archive_modal_profile,
            usage::list_usage_rows,
            usage::sync_modal_billing,
            media::list_pipeline_inputs,
            media::read_json_file,
            media::make_thumbnail,
            media::write_json_file,
            media::reveal_in_explorer,
            pipeline::analyze_audio,
            pipeline::render_trailer,
            pipeline::render_spec,
            pipeline::build_storyboard,
            pipeline::list_renderers,
            media::open_with_default
        ])
        .run(tauri::generate_context!())
        .expect("error while running Modal GUI");
}

#[cfg(test)]
mod tests;
