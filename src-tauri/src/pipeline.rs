//! Local editing/rendering requests and pipeline event forwarding.
use crate::paths::repo_root;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::thread;
use tauri::Emitter;

#[derive(Deserialize)]
pub(crate) struct AnalyzeRequest {
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
pub(crate) struct RenderRequest {
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
pub(crate) struct EditSpecRequest {
    run_id: String,
    spec: String,
    output: String,
    metadata: Option<String>,
    mode: Option<String>,
    renderer: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct StoryboardRequest {
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
pub(crate) fn analyze_audio(app: tauri::AppHandle, request: AnalyzeRequest) -> Result<(), String> {
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
pub(crate) fn render_trailer(app: tauri::AppHandle, request: RenderRequest) -> Result<(), String> {
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
pub(crate) fn render_spec(app: tauri::AppHandle, request: EditSpecRequest) -> Result<(), String> {
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
pub(crate) fn build_storyboard(request: StoryboardRequest) -> Result<Value, String> {
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
        return Err(failure.unwrap_or_else(|| String::from_utf8_lossy(&result.stderr).to_string()));
    }
    let raw = std::fs::read_to_string(&request.output).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

/// Probes each renderer plugin on the host and returns availability plus
/// capabilities, so the GUI can reflect what is actually installed.
#[tauri::command]
pub(crate) fn list_renderers() -> Result<Value, String> {
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
