//! Local media discovery, thumbnails, JSON files and shell integration.
use crate::paths::{
    default_clips_root, default_music_root, deliverables_root, edits_root, thumbs_root,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::process::Command;

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
pub(crate) fn list_pipeline_inputs(clips_root: Option<String>) -> Result<Value, String> {
    let clips_dir =
        clips_root.unwrap_or_else(|| default_clips_root().to_string_lossy().to_string());
    let audio_dir = default_music_root();
    let mut audio = scan_media(&audio_dir, &["flac", "wav", "mp3", "m4a"]);
    for depth in std::fs::read_dir(&audio_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        if depth.path().is_dir() {
            audio.extend(scan_media(&depth.path(), &["flac", "wav", "mp3", "m4a"]));
            for nested in std::fs::read_dir(depth.path())
                .into_iter()
                .flatten()
                .flatten()
            {
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
pub(crate) fn read_json_file(path: String) -> Result<Value, String> {
    let raw = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

/// Extracts one frame so the result grid can show real thumbnails. Cached by
/// file name plus a path tag under `F:\modal-gui\thumbs`, so two files with the
/// same name in different folders never share a thumbnail. The caller falls
/// back to a video poster frame when ffmpeg is unavailable.
#[tauri::command]
pub(crate) fn make_thumbnail(path: String, time: Option<f64>) -> Result<String, String> {
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
pub(crate) fn reveal_in_explorer(path: String) -> Result<(), String> {
    Command::new("explorer")
        .arg("/select,")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn open_with_default(path: String) -> Result<(), String> {
    Command::new("cmd")
        .args(["/C", "start", "", &path])
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn write_json_file(path: String, value: Value) -> Result<(), String> {
    let target = std::path::PathBuf::from(&path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let body = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    std::fs::write(target, body + "\n").map_err(|error| error.to_string())
}
