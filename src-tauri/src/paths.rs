//! Existing local workspace locations shared by native commands.

pub(crate) fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

pub(crate) fn deliverables_root() -> std::path::PathBuf {
    std::path::PathBuf::from(r"F:\modal-gui\deliverables")
}

pub(crate) fn thumbs_root() -> std::path::PathBuf {
    std::path::PathBuf::from(r"F:\modal-gui\thumbs")
}

pub(crate) fn default_clips_root() -> std::path::PathBuf {
    std::path::PathBuf::from(r"F:\modal-gui\h3-clips\generated")
}

pub(crate) fn default_music_root() -> std::path::PathBuf {
    std::path::PathBuf::from(r"F:\modal-gui\music")
}

pub(crate) fn edits_root() -> std::path::PathBuf {
    std::path::PathBuf::from(r"F:\modal-gui\edits")
}
