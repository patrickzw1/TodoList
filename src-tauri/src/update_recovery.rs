use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime},
};
use tauri::AppHandle;

const CACHE_OWNER: &str = "app.todolist.desktop.updater-cache.v1";
const STATE_FILE: &str = ".todolist-update-state.json";
const RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStateFile {
    owner: String,
    attempt_id: String,
    version: String,
    installer_file: String,
    installer_sha256: String,
    install_dir: String,
    state: String,
    reason: String,
    explorer_opened: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRecoveryStatus {
    attempt_id: String,
    version: String,
    state: String,
    reason: String,
    installer_directory: String,
    explorer_opened: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentBuildStatus {
    desktop_build: String,
    mcp_build: Option<String>,
    matches: bool,
    message: String,
}

#[derive(Clone, Debug)]
struct ValidAttempt {
    root: PathBuf,
    installer: PathBuf,
    state: UpdateStateFile,
}

fn build_identity() -> String {
    let channel = if cfg!(feature = "production") {
        "production"
    } else {
        "development"
    };
    format!("todolist/{}/{channel}", env!("CARGO_PKG_VERSION"))
}

fn current_install_dir(_app: &AppHandle) -> Result<PathBuf, String> {
    std::env::current_exe()
        .map_err(|error| error.to_string())?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Could not resolve the TodoList install directory".to_string())
}

fn path_equals(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

fn simple_component(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn sha256(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn validate_attempt(root: &Path, install_dir: &Path) -> Result<ValidAttempt, String> {
    let metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Update cache entry is not a regular directory".into());
    }
    let state_path = root.join(STATE_FILE);
    let state_metadata = fs::symlink_metadata(&state_path).map_err(|error| error.to_string())?;
    if state_metadata.file_type().is_symlink() || !state_metadata.is_file() {
        return Err("Update cache marker is not a regular file".into());
    }
    let state: UpdateStateFile =
        serde_json::from_slice(&fs::read(&state_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    if state.owner != CACHE_OWNER
        || !simple_component(&state.attempt_id)
        || !simple_component(&state.installer_file)
        || root.file_name().and_then(|name| name.to_str()) != Some(&state.attempt_id)
        || !state
            .attempt_id
            .starts_with(&format!("TodoList-{}-updater-", state.version))
        || state.installer_file != format!("TodoList-{}-installer.exe", state.version)
        || !path_equals(Path::new(&state.install_dir), install_dir)
    {
        return Err("Update cache ownership does not match this installation".into());
    }
    let installer = root.join(&state.installer_file);
    match fs::symlink_metadata(&installer) {
        Ok(installer_metadata) => {
            if installer_metadata.file_type().is_symlink() || !installer_metadata.is_file() {
                return Err("Retained update installer is unavailable".into());
            }
            if sha256(&installer)? != state.installer_sha256.to_ascii_lowercase() {
                return Err("Retained update installer failed its ownership hash check".into());
            }
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound && state.state == "installed" =>
        {
            // Cleanup deletes the verified installer before its marker. If the
            // process is interrupted between those operations, the owned
            // installed marker remains sufficient to finish deleting the now
            // non-retryable cache on the next startup.
        }
        Err(error) => return Err(error.to_string()),
    }
    Ok(ValidAttempt {
        root: root.to_path_buf(),
        installer,
        state,
    })
}

fn attempts_for(temp_root: &Path, install_dir: &Path) -> Vec<ValidAttempt> {
    let Ok(entries) = fs::read_dir(temp_root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| validate_attempt(&entry.path(), install_dir).ok())
        .collect()
}

fn latest_recovery(temp_root: &Path, install_dir: &Path) -> Option<ValidAttempt> {
    let mut attempts: Vec<_> = attempts_for(temp_root, install_dir)
        .into_iter()
        .filter(|attempt| matches!(attempt.state.state.as_str(), "failed" | "cancelled"))
        .collect();
    attempts.sort_by(|left, right| left.state.updated_at.cmp(&right.state.updated_at));
    attempts.pop()
}

fn remove_owned_attempt(attempt: ValidAttempt) -> Result<(), String> {
    let state_path = attempt.root.join(STATE_FILE);
    for entry in fs::read_dir(&attempt.root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        if name != std::ffi::OsStr::new(STATE_FILE)
            && name != std::ffi::OsStr::new(&attempt.state.installer_file)
        {
            return Err(format!(
                "Update cache contains unowned content and was retained: {}",
                entry.path().display()
            ));
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Update cache contains a non-regular entry and was retained".into());
        }
    }
    if attempt.installer.exists() {
        ensure_installer_not_running(&attempt.installer)?;
        fs::remove_file(&attempt.installer).map_err(|error| error.to_string())?;
    }
    fs::remove_file(state_path).map_err(|error| error.to_string())?;
    fs::remove_dir(attempt.root).map_err(|error| error.to_string())
}

fn cleanup_attempts(temp_root: &Path, install_dir: &Path, now: SystemTime) {
    for attempt in attempts_for(temp_root, install_dir) {
        let should_remove = if attempt.state.state == "installed" {
            true
        } else if matches!(attempt.state.state.as_str(), "failed" | "cancelled") {
            let modified = fs::metadata(attempt.root.join(STATE_FILE))
                .and_then(|metadata| metadata.modified())
                .unwrap_or(now);
            now.duration_since(modified).unwrap_or_default() >= RETENTION
        } else {
            false
        };
        if should_remove {
            let _ = remove_owned_attempt(attempt);
        }
    }
}

fn mcp_build_at(install_dir: &Path) -> Result<String, String> {
    let executable = install_dir.join(if cfg!(windows) {
        "todolist-mcp.exe"
    } else {
        "todolist-mcp"
    });
    if !executable.is_file() {
        return Err("TodoList MCP component is missing".into());
    }
    let mut command = Command::new(executable);
    command.arg("--print-build-identity");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("TodoList MCP component did not report its build identity".into());
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|_| "TodoList MCP build identity is not UTF-8".to_string())
}

#[tauri::command]
pub fn component_build_status(app: AppHandle) -> Result<ComponentBuildStatus, String> {
    let desktop_build = build_identity();
    let mcp_build = mcp_build_at(&current_install_dir(&app)?).ok();
    let matches = mcp_build.as_deref() == Some(desktop_build.as_str());
    let message = if matches {
        "主程序与 MCP 来自同一构建。".to_string()
    } else if mcp_build.is_some() {
        "主程序与 MCP 构建不一致，请重新运行保留的安装包。".to_string()
    } else {
        "无法读取 MCP 构建标识，请重新运行安装包。".to_string()
    };
    Ok(ComponentBuildStatus {
        desktop_build,
        mcp_build,
        matches,
        message,
    })
}

#[tauri::command]
pub fn update_recovery_status(app: AppHandle) -> Result<Option<UpdateRecoveryStatus>, String> {
    let install_dir = current_install_dir(&app)?;
    Ok(
        latest_recovery(&std::env::temp_dir(), &install_dir).map(|attempt| UpdateRecoveryStatus {
            attempt_id: attempt.state.attempt_id,
            version: attempt.state.version,
            state: attempt.state.state,
            reason: attempt.state.reason,
            installer_directory: attempt.root.to_string_lossy().into_owned(),
            explorer_opened: attempt.state.explorer_opened,
        }),
    )
}

fn selected_attempt(app: &AppHandle, attempt_id: &str) -> Result<ValidAttempt, String> {
    if !simple_component(attempt_id) {
        return Err("Invalid update attempt id".into());
    }
    let install_dir = current_install_dir(app)?;
    validate_attempt(&std::env::temp_dir().join(attempt_id), &install_dir)
}

#[cfg(windows)]
fn ensure_installer_not_running(path: &Path) -> Result<(), String> {
    use std::os::windows::fs::OpenOptionsExt;
    let result = fs::OpenOptions::new().write(true).share_mode(0).open(path);
    result
        .map(|_| ())
        .map_err(|_| "安装程序仍在运行，当前不能清理更新包。".to_string())
}

#[cfg(not(windows))]
fn ensure_installer_not_running(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn retry_update_installer(app: AppHandle, attempt_id: String) -> Result<(), String> {
    let attempt = selected_attempt(&app, &attempt_id)?;
    let mut command = Command::new(&attempt.installer);
    command.args(["/UPDATE", "/P", "/R", "/TODOLIST_AUTO_UPDATE=1"]);
    command.spawn().map_err(|error| error.to_string())?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn open_update_installer_location(app: AppHandle, attempt_id: String) -> Result<(), String> {
    let attempt = selected_attempt(&app, &attempt_id)?;
    #[cfg(windows)]
    {
        Command::new("explorer.exe")
            .arg(format!("/select,\"{}\"", attempt.installer.display()))
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(windows))]
    {
        Command::new("open")
            .arg(&attempt.root)
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn discard_update_installer(app: AppHandle, attempt_id: String) -> Result<(), String> {
    let attempt = selected_attempt(&app, &attempt_id)?;
    remove_owned_attempt(attempt)
}

pub fn cleanup_expired_update_attempts(app: &AppHandle) -> Result<(), String> {
    let install_dir = current_install_dir(app)?;
    cleanup_attempts(&std::env::temp_dir(), &install_dir, SystemTime::now());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_attempt(temp_root: &Path, install_dir: &Path, state: &str) -> PathBuf {
        let root = temp_root.join("TodoList-0.3.0-updater-test");
        fs::create_dir_all(&root).unwrap();
        let installer = root.join("TodoList-0.3.0-installer.exe");
        fs::write(&installer, b"signed fixture bytes").unwrap();
        let marker = UpdateStateFile {
            owner: CACHE_OWNER.into(),
            attempt_id: root.file_name().unwrap().to_string_lossy().into_owned(),
            version: "0.3.0".into(),
            installer_file: installer
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            installer_sha256: sha256(&installer).unwrap(),
            install_dir: install_dir.to_string_lossy().into_owned(),
            state: state.into(),
            reason: "fixture failure".into(),
            explorer_opened: true,
            created_at: "2026-09-06T00:00:00Z".into(),
            updated_at: "2026-09-06T00:01:00Z".into(),
        };
        fs::write(root.join(STATE_FILE), serde_json::to_vec(&marker).unwrap()).unwrap();
        root
    }

    #[test]
    fn recovery_requires_owned_marker_hash_and_matching_installation() {
        let temp = tempdir().unwrap();
        let install = tempdir().unwrap();
        let root = write_attempt(temp.path(), install.path(), "failed");
        assert!(latest_recovery(temp.path(), install.path()).is_some());
        assert!(latest_recovery(temp.path(), temp.path()).is_none());
        fs::write(root.join("TodoList-0.3.0-installer.exe"), b"tampered").unwrap();
        assert!(latest_recovery(temp.path(), install.path()).is_none());
    }

    #[test]
    fn cancelled_and_failed_are_recoverable_but_installing_is_not_reported() {
        let temp = tempdir().unwrap();
        let install = tempdir().unwrap();
        let root = write_attempt(temp.path(), install.path(), "cancelled");
        assert_eq!(
            latest_recovery(temp.path(), install.path())
                .unwrap()
                .state
                .state,
            "cancelled"
        );
        let mut marker: UpdateStateFile =
            serde_json::from_slice(&fs::read(root.join(STATE_FILE)).unwrap()).unwrap();
        marker.state = "installing".into();
        fs::write(root.join(STATE_FILE), serde_json::to_vec(&marker).unwrap()).unwrap();
        assert!(latest_recovery(temp.path(), install.path()).is_none());
    }

    #[test]
    fn installed_attempts_retry_cleanup_without_becoming_install_failures() {
        let temp = tempdir().unwrap();
        let install = tempdir().unwrap();
        let root = write_attempt(temp.path(), install.path(), "installed");
        cleanup_attempts(temp.path(), install.path(), SystemTime::now());
        assert!(!root.exists());
    }

    #[test]
    fn owned_cleanup_preserves_attempts_with_unrelated_content() {
        let temp = tempdir().unwrap();
        let install = tempdir().unwrap();
        let root = write_attempt(temp.path(), install.path(), "installed");
        fs::write(root.join("keep-me.txt"), b"unrelated").unwrap();
        cleanup_attempts(temp.path(), install.path(), SystemTime::now());
        assert!(root.join("keep-me.txt").exists());
        assert!(root.join("TodoList-0.3.0-installer.exe").exists());
        assert!(root.join(STATE_FILE).exists());
    }

    #[test]
    fn installed_marker_finishes_cleanup_after_installer_was_already_removed() {
        let temp = tempdir().unwrap();
        let install = tempdir().unwrap();
        let root = write_attempt(temp.path(), install.path(), "installed");
        fs::remove_file(root.join("TodoList-0.3.0-installer.exe")).unwrap();
        cleanup_attempts(temp.path(), install.path(), SystemTime::now());
        assert!(!root.exists());
    }
}
