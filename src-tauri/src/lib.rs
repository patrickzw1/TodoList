use std::fs;
use task_core::Workspace;
use task_store_sqlite::SqliteTaskStore;
use tauri::Manager;

mod backup;
mod codex_integration;
pub mod window_actions;

struct AppState {
    store: SqliteTaskStore,
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
    state.store.save_workspace(&workspace)
}

#[tauri::command]
fn restore_workspace(
    state: tauri::State<'_, AppState>,
    workspace: Workspace,
) -> Result<Workspace, String> {
    state.store.restore_workspace(&workspace)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            fs::create_dir_all(&app_data)?;
            let store = SqliteTaskStore::open(app_data.join("todolist.sqlite"))
                .map_err(std::io::Error::other)?;
            app.manage(AppState { store });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_workspace,
            load_workspace_version,
            save_workspace,
            restore_workspace,
            backup::export_workspace_backup,
            backup::read_workspace_backup,
            window_actions::open_main_window,
            window_actions::open_sticky_window,
            codex_integration::codex_integration_status,
            codex_integration::configure_codex_integration,
            codex_integration::remove_codex_integration
        ])
        .run(tauri::generate_context!())
        .expect("failed to run TodoList");
}
