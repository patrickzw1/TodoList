use std::fs;
use task_core::Workspace;
use task_store_sqlite::{storage_path, SqliteTaskStore};
use tauri::Manager;

mod backup;
mod codex_integration;
mod managed_files;
mod update_recovery;
pub mod window_actions;

pub(crate) struct AppState {
    pub(crate) store: SqliteTaskStore,
    pub(crate) managed_files_root: std::path::PathBuf,
}

#[tauri::command]
fn load_workspace(state: tauri::State<'_, AppState>) -> Result<Option<Workspace>, String> {
    state.store.load_workspace()
}

#[tauri::command]
fn load_workspace_version(state: tauri::State<'_, AppState>) -> Result<Option<u64>, String> {
    state.store.load_workspace_version()
}

#[tauri::command]
fn save_workspace(state: tauri::State<'_, AppState>, workspace: Workspace) -> Result<(), String> {
    state.store.save_workspace(&workspace)?;
    if let Err(error) = state.store.cleanup_managed_files() {
        eprintln!("Could not reconcile TodoList managed files after save: {error}");
    }
    Ok(())
}

#[tauri::command]
fn restore_workspace(
    state: tauri::State<'_, AppState>,
    workspace: Workspace,
) -> Result<Workspace, String> {
    let restored = state.store.restore_workspace(&workspace)?;
    if let Err(error) = state.store.cleanup_managed_files() {
        eprintln!("Could not reconcile TodoList managed files after restore: {error}");
    }
    Ok(restored)
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
            if app.config().identifier != storage_path::APP_IDENTIFIER {
                return Err(std::io::Error::other(
                    "TodoList build channel and application identifier do not match",
                )
                .into());
            }
            let database_path = storage_path::database_path().map_err(std::io::Error::other)?;
            if let Some(parent) = database_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let store = SqliteTaskStore::open(database_path).map_err(std::io::Error::other)?;
            let managed_files_root =
                storage_path::managed_files_path().map_err(std::io::Error::other)?;
            fs::create_dir_all(&managed_files_root)?;
            if let Err(error) = store.cleanup_managed_files() {
                eprintln!("Could not finish pending managed file cleanup: {error}");
            }
            if let Err(error) = update_recovery::cleanup_expired_update_attempts(app.handle()) {
                eprintln!("Could not clean expired TodoList update attempts: {error}");
            }
            app.manage(AppState {
                store,
                managed_files_root,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_workspace,
            load_workspace_version,
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
            update_recovery::component_build_status,
            update_recovery::update_recovery_status,
            update_recovery::retry_update_installer,
            update_recovery::open_update_installer_location,
            update_recovery::discard_update_installer
        ])
        .run(tauri::generate_context!())
        .expect("failed to run TodoList");
}
