use crate::AppState;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::{collections::HashMap, fs, path::Path, process::Command};
use task_core::ManagedFile;
use task_store_sqlite::managed_files::{
    media_type, resolve_storage_key, restore_payloads, RestoredStorageKey, MAX_MANAGED_FILE_BYTES,
};
pub use task_store_sqlite::managed_files::{payloads_for_workspace, ManagedFilePayload};

#[tauri::command]
pub fn import_managed_file(
    state: tauri::State<'_, AppState>,
    task_id: String,
    source_path: String,
    kind: String,
) -> Result<ManagedFile, String> {
    state
        .store
        .import_task_file(&task_id, Path::new(&source_path), &kind)
}

#[tauri::command]
pub fn read_managed_image(
    state: tauri::State<'_, AppState>,
    storage_key: String,
) -> Result<String, String> {
    let path = resolve_storage_key(&state.managed_files_root, &storage_key)?;
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| "Image file is unavailable".to_string())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_MANAGED_FILE_BYTES
    {
        return Err("Image file is unavailable".to_string());
    }
    let mime = media_type(&path);
    if !mime.starts_with("image/") {
        return Err("Managed file is not a supported image".to_string());
    }
    Ok(format!(
        "data:{mime};base64,{}",
        STANDARD.encode(fs::read(path).map_err(|error| error.to_string())?)
    ))
}

#[tauri::command]
pub fn open_managed_file(
    state: tauri::State<'_, AppState>,
    storage_key: String,
) -> Result<(), String> {
    let path = resolve_storage_key(&state.managed_files_root, &storage_key)?;
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| "Managed file is unavailable".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Managed file is unavailable".to_string());
    }
    open_with_default_application(&path)
}

#[tauri::command]
pub fn restore_managed_files(
    state: tauri::State<'_, AppState>,
    files: Vec<ManagedFilePayload>,
) -> Result<Vec<RestoredStorageKey>, String> {
    let unique: HashMap<_, _> = files.iter().map(|file| (&file.storage_key, ())).collect();
    if unique.len() != files.len() {
        return Err("Backup contains duplicate managed file keys".to_string());
    }
    restore_payloads(&state.managed_files_root, &files)
}

#[cfg(target_os = "windows")]
fn open_with_default_application(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Process -FilePath $env:TODOLIST_OPEN_FILE -ErrorAction Stop",
        ])
        .env("TODOLIST_OPEN_FILE", path)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| error.to_string())
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err("Could not open the attachment with its default application".into())
            }
        })
}

#[cfg(target_os = "macos")]
fn open_with_default_application(path: &Path) -> Result<(), String> {
    Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_with_default_application(path: &Path) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}
