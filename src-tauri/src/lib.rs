use serde::{Deserialize, Serialize};
use std::fs;
use std::time::Instant;
use task_core::Workspace;
use task_diagnostics::{Component, DiagnosticLog, LogStatus, PathIdentity, Record};
use task_store_sqlite::{
    storage_path::{self, StorageClient},
    SqliteTaskStore,
};
use tauri::Manager;

mod backup;
mod codex_integration;
mod managed_files;
mod stable_update;
mod update_recovery;
pub mod window_actions;

pub(crate) struct AppState {
    pub(crate) store: SqliteTaskStore,
    pub(crate) database_path: std::path::PathBuf,
    pub(crate) managed_files_root: std::path::PathBuf,
    pub(crate) logger: DiagnosticLog,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageDiagnostics {
    build: String,
    database_path: String,
    read_error: Option<String>,
    version: Option<u64>,
    projects: usize,
    tasks: usize,
}

#[tauri::command]
fn storage_diagnostics(state: tauri::State<'_, AppState>) -> Result<StorageDiagnostics, String> {
    let (workspace, read_error) = match state.store.load_workspace() {
        Ok(workspace) => (workspace, None),
        Err(error) => {
            state
                .logger
                .record(Record::new("read_failed", "storage_diagnostics", "error").error(&error));
            (None, Some(error))
        }
    };
    Ok(StorageDiagnostics {
        build: format!(
            "todolist/{}/{}",
            env!("CARGO_PKG_VERSION"),
            if storage_path::IS_PRODUCTION {
                "production"
            } else {
                "development"
            }
        ),
        database_path: state
            .database_path
            .canonicalize()
            .unwrap_or_else(|_| state.database_path.clone())
            .display()
            .to_string(),
        read_error,
        version: workspace.as_ref().map(|value| value.version),
        projects: workspace.as_ref().map_or(0, |value| value.projects.len()),
        tasks: workspace.as_ref().map_or(0, |value| value.tasks.len()),
    })
}

#[tauri::command]
fn load_workspace(state: tauri::State<'_, AppState>) -> Result<Option<Workspace>, String> {
    let started = Instant::now();
    state
        .logger
        .record(Record::new("ipc_arrived", "load_workspace", "started"));
    let result = state.store.load_workspace();
    let record = match &result {
        Ok(Some(workspace)) => Record::new("read_completed", "load_workspace", "ok").workspace(
            workspace.version,
            workspace.projects.len(),
            workspace.tasks.len(),
        ),
        Ok(None) => Record::new("read_completed", "load_workspace", "empty"),
        Err(error) => Record::new("read_failed", "load_workspace", "error").error(error),
    };
    state.logger.record(record.duration(started.elapsed()));
    result
}

#[tauri::command]
fn load_workspace_version(state: tauri::State<'_, AppState>) -> Result<Option<u64>, String> {
    let started = Instant::now();
    let result = state.store.load_workspace_version();
    match &result {
        Err(error) => state.logger.record(
            Record::new("read_failed", "load_workspace_version", "error")
                .error(error)
                .duration(started.elapsed()),
        ),
        Ok(version) if state.logger.detailed() => {
            let mut record = Record::new("read_completed", "load_workspace_version", "ok")
                .duration(started.elapsed());
            record.workspace_version = *version;
            state.logger.record(record);
        }
        _ => {}
    }
    result
}

#[tauri::command]
fn save_workspace(state: tauri::State<'_, AppState>, workspace: Workspace) -> Result<(), String> {
    let started = Instant::now();
    state
        .logger
        .record(Record::new("ipc_arrived", "save_workspace", "started"));
    if let Err(error) = state.store.save_workspace(&workspace) {
        state.logger.record(
            Record::new("save_failed", "save_workspace", "error")
                .error(&error)
                .duration(started.elapsed()),
        );
        return Err(error);
    }
    state.logger.record(
        Record::new("save_completed", "save_workspace", "ok")
            .workspace(
                workspace.version,
                workspace.projects.len(),
                workspace.tasks.len(),
            )
            .duration(started.elapsed()),
    );
    if let Err(error) = state.store.cleanup_managed_files() {
        state
            .logger
            .record(Record::new("cleanup_failed", "managed_files", "error").error(&error));
        eprintln!("Could not reconcile TodoList managed files after save: {error}");
    }
    Ok(())
}

#[tauri::command]
fn restore_workspace(
    state: tauri::State<'_, AppState>,
    workspace: Workspace,
) -> Result<Workspace, String> {
    let started = Instant::now();
    state
        .logger
        .record(Record::new("ipc_arrived", "restore_workspace", "started"));
    let restored = match state.store.restore_workspace(&workspace) {
        Ok(restored) => restored,
        Err(error) => {
            state.logger.record(
                Record::new("save_failed", "restore_workspace", "error")
                    .error(&error)
                    .duration(started.elapsed()),
            );
            return Err(error);
        }
    };
    state.logger.record(
        Record::new("save_completed", "restore_workspace", "ok")
            .workspace(
                restored.version,
                restored.projects.len(),
                restored.tasks.len(),
            )
            .duration(started.elapsed()),
    );
    if let Err(error) = state.store.cleanup_managed_files() {
        state
            .logger
            .record(Record::new("cleanup_failed", "managed_files", "error").error(&error));
        eprintln!("Could not reconcile TodoList managed files after restore: {error}");
    }
    Ok(restored)
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum FrontendEvent {
    BridgeReady,
    InitialReadStarted,
    InitialReadTimedOut,
    InitialReadFailed,
    WorkspaceApplied,
    ExternalChange,
    PollFailed,
    PollRecovered,
    SaveFailed,
    SaveRecovered,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FrontendDiagnostic {
    event: FrontendEvent,
    duration_ms: Option<u64>,
    workspace_version: Option<u64>,
    projects: Option<usize>,
    tasks: Option<usize>,
}

#[tauri::command]
fn record_frontend_diagnostic(state: tauri::State<'_, AppState>, diagnostic: FrontendDiagnostic) {
    let (event, outcome) = match diagnostic.event {
        FrontendEvent::BridgeReady => ("bridge_ready", "ok"),
        FrontendEvent::InitialReadStarted => ("initial_read_started", "started"),
        FrontendEvent::InitialReadTimedOut => ("initial_read_timeout", "error"),
        FrontendEvent::InitialReadFailed => ("initial_read_failed", "error"),
        FrontendEvent::WorkspaceApplied => ("workspace_applied", "ok"),
        FrontendEvent::ExternalChange => ("external_change", "ok"),
        FrontendEvent::PollFailed => ("poll_failed", "error"),
        FrontendEvent::PollRecovered => ("poll_recovered", "ok"),
        FrontendEvent::SaveFailed => ("save_failed", "error"),
        FrontendEvent::SaveRecovered => ("save_recovered", "ok"),
    };
    let mut record = Record::new(event, "frontend", outcome);
    record.duration_ms = diagnostic.duration_ms.map(|value| value.min(60_000));
    record.workspace_version = diagnostic.workspace_version;
    record.projects = diagnostic.projects.map(|value| value.min(1_000_000));
    record.tasks = diagnostic.tasks.map(|value| value.min(1_000_000));
    state.logger.record(record);
}

#[tauri::command]
fn logging_status(state: tauri::State<'_, AppState>) -> LogStatus {
    state.logger.status()
}

#[tauri::command]
fn set_detailed_logging(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<LogStatus, String> {
    state.logger.set_detailed(enabled).map_err(str::to_string)
}

#[tauri::command]
fn open_logs_directory(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let directory = state.logger.directory().ok_or("日志目录不可用")?;
    if !directory.is_dir() {
        return Err("日志目录不存在".into());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(&directory)
            .spawn()
            .map_err(|_| "无法打开日志目录".to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("当前平台不支持打开日志目录".into())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_updater::Builder::new()
                .installer_arg("/TODOLIST_AUTO_UPDATE=1")
                .build(),
        )
        .setup(|app| {
            let logger = DiagnosticLog::for_current_exe(Component::Gui);
            let channel = if storage_path::IS_PRODUCTION {
                "production"
            } else {
                "development"
            };
            logger.record(
                Record::new("startup", "gui", "started").build(env!("CARGO_PKG_VERSION"), channel),
            );
            if app.config().identifier != storage_path::APP_IDENTIFIER {
                logger.record(Record::new("startup_failed", "channel", "error"));
                return Err(std::io::Error::other(
                    "TodoList build channel and application identifier do not match",
                )
                .into());
            }
            let database_path =
                storage_path::database_path(StorageClient::Desktop).map_err(|error| {
                    logger.record(
                        Record::new("startup_failed", "database_path", "error").error(&error),
                    );
                    std::io::Error::other(error)
                })?;
            logger.record(Record::new("database_selected", "startup", "ok").source(
                PathIdentity::database(
                    &database_path,
                    std::env::var_os("TODOLIST_DB_PATH").is_some(),
                ),
            ));
            if let Some(parent) = database_path.parent() {
                fs::create_dir_all(parent).inspect_err(|error| {
                    logger.record(
                        Record::new("startup_failed", "data_directory", "error")
                            .error(&error.to_string()),
                    )
                })?;
            }
            let store = SqliteTaskStore::open(&database_path).map_err(|error| {
                logger
                    .record(Record::new("startup_failed", "database_open", "error").error(&error));
                std::io::Error::other(error)
            })?;
            let managed_files_root =
                storage_path::managed_files_path(&database_path).map_err(|error| {
                    logger.record(
                        Record::new("startup_failed", "managed_path", "error").error(&error),
                    );
                    std::io::Error::other(error)
                })?;
            fs::create_dir_all(&managed_files_root).inspect_err(|error| {
                logger.record(
                    Record::new("startup_failed", "managed_directory", "error")
                        .error(&error.to_string()),
                )
            })?;
            if let Err(error) = store.cleanup_managed_files() {
                logger
                    .record(Record::new("cleanup_failed", "managed_files", "error").error(&error));
                eprintln!("Could not finish pending managed file cleanup: {error}");
            }
            if let Err(error) = update_recovery::cleanup_expired_update_attempts(app.handle()) {
                logger.record(
                    Record::new("cleanup_failed", "update_attempts", "error").error(&error),
                );
                eprintln!("Could not clean expired TodoList update attempts: {error}");
            }
            app.manage(AppState {
                store,
                database_path,
                managed_files_root,
                logger: logger.clone(),
            });
            logger.record(Record::new("startup", "gui", "ready"));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_workspace,
            load_workspace_version,
            storage_diagnostics,
            logging_status,
            set_detailed_logging,
            open_logs_directory,
            record_frontend_diagnostic,
            save_workspace,
            restore_workspace,
            managed_files::import_managed_file,
            managed_files::open_managed_file,
            managed_files::read_managed_image,
            managed_files::restore_managed_files,
            backup::export_workspace_backup,
            backup::read_workspace_backup,
            window_actions::open_main_window,
            window_actions::open_sticky_window,
            codex_integration::codex_integration_status,
            codex_integration::configure_codex_integration,
            codex_integration::remove_codex_integration,
            stable_update::check_stable_update,
            update_recovery::component_build_status,
            update_recovery::update_recovery_status,
            update_recovery::retry_update_installer,
            update_recovery::open_update_installer_location,
            update_recovery::discard_update_installer
        ])
        .run(tauri::generate_context!())
        .expect("failed to run TodoList");
}
